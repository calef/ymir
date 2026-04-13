//! Spatial, spectral, and identifier index for efficient catalog lookups.
//!
//! Builds an in-memory structure-of-arrays (SoA) layout over the Gaia
//! catalog joined against the NASA Exoplanet Archive. Exposes four
//! query shapes:
//!
//! * `lookup_by_gaia_id` — O(1) identifier resolution via a hash map.
//! * `cone_search` — brute-force dot-product scan against precomputed
//!   unit vectors. For the ~116k-row CAT-05 catalog this is a few ms per
//!   query, which is fine for the interactive GUI in Phase 4. If future
//!   catalogs (Gaia DR4, a wider distance cut) push row counts past a
//!   million, swap the inner loop for a proper k-d tree (see the
//!   `kiddo` crate).
//! * `filter_by_spectral` / `filter_by_distance` — linear scans over
//!   the cache-friendly SoA columns. Still fast at 116k rows.
//! * `filter_hz_hosts` — returns only stars with at least one known
//!   exoplanet whose semi-major axis falls inside the Kopparapu
//!   conservative habitable zone derived from the catalog luminosity.
//!
//! # Persistence
//!
//! Call [`CatalogIndex::save`] to serialize the SoA columns plus the
//! identifier map and HZ-presence flags via `bincode`. [`CatalogIndex::load`]
//! deserializes and rebuilds transient state (the Gaia-ID map and the
//! empty slice sentinel) — this keeps the on-disk footprint small and
//! avoids committing the bincode format of any particular tree crate.
//!
//! # Spectral classification
//!
//! Uses the Harvard system (O/B/A/F/G/K/M) derived from effective
//! temperature. Temperature bands follow the standard undergraduate
//! boundaries (Carroll & Ostlie, *An Introduction to Modern
//! Astrophysics*). Subclasses are a linear interpolation within each
//! band where `0` is the hottest edge and `9` is the coolest.
//!
//! # HZ heuristic for `filter_hz_hosts`
//!
//! Uses the simple optical-depth scaling
//! `hz_inner = sqrt(L / 1.1)` AU, `hz_outer = sqrt(L / 0.53)` AU
//! from Kopparapu et al. (2013) at the Sun's T_eff. This is
//! intentionally coarser than [`crate::star_context::StarContext`]'s
//! full T_eff-dependent polynomial — the visual overlay in Phase 4
//! only needs a rough yes/no filter, and the exoplanet archive's
//! `semi_major_axis_au` column itself has a few percent uncertainty.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::exoplanet::ExoplanetCatalog;
use crate::gaia::{GaiaError, GaiaReader};
use crate::star_context::SpectralClass;

/// Harvard spectral class plus integer subclass 0..=9 derived from
/// effective temperature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedSpectralType {
    /// Primary Harvard class letter.
    pub class: SpectralClass,
    /// Subclass digit, 0 (hottest) to 9 (coolest) within the class.
    pub subtype: u8,
}

impl IndexedSpectralType {
    /// Classify a star by effective temperature in Kelvin.
    ///
    /// Temperatures below 2400 K are clamped to M9 (the coolest
    /// representable class here) rather than erroring; the CAT-05
    /// catalog filters to Gaia DR3 rows with GSP-Phot T_eff in a
    /// realistic range so this branch is primarily defensive.
    pub fn from_teff(teff_k: f64) -> Self {
        #[allow(clippy::manual_range_contains)]
        let (class, hot, cool) = if teff_k >= 30_000.0 {
            // Treat anything hotter than the O class floor as O0.
            (SpectralClass::O, 60_000.0, 30_000.0)
        } else if teff_k >= 10_000.0 {
            (SpectralClass::B, 30_000.0, 10_000.0)
        } else if teff_k >= 7_500.0 {
            (SpectralClass::A, 10_000.0, 7_500.0)
        } else if teff_k >= 6_000.0 {
            (SpectralClass::F, 7_500.0, 6_000.0)
        } else if teff_k >= 5_200.0 {
            (SpectralClass::G, 6_000.0, 5_200.0)
        } else if teff_k >= 3_700.0 {
            (SpectralClass::K, 5_200.0, 3_700.0)
        } else if teff_k >= 2_400.0 {
            (SpectralClass::M, 3_700.0, 2_400.0)
        } else {
            // Below the M band floor: clamp to M9.
            return Self {
                class: SpectralClass::M,
                subtype: 9,
            };
        };
        // Linearly interpolate subtype: 0 at `hot`, 9 at `cool`.
        let frac = ((hot - teff_k) / (hot - cool)).clamp(0.0, 1.0);
        let subtype = (frac * 9.0).round() as u8;
        let subtype = subtype.min(9);
        Self { class, subtype }
    }
}

/// Summary row for a single star in the index. Returned by every
/// query method; callers that need more (e.g., `mass_sun`) should use
/// `row` to dereference the Gaia catalog separately.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StarInfo {
    /// Row index into the index's SoA columns.
    pub row: usize,
    /// Gaia DR3 source ID.
    pub gaia_id: u64,
    /// Right ascension in degrees (ICRS).
    pub ra_deg: f64,
    /// Declination in degrees (ICRS).
    pub dec_deg: f64,
    /// Inverse-parallax distance in parsecs.
    pub distance_pc: f64,
    /// Effective temperature in Kelvin.
    pub teff_k: f64,
    /// Luminosity in solar luminosities.
    pub luminosity_sun: f64,
    /// Spectral class + subtype derived from T_eff.
    pub spectral: IndexedSpectralType,
    /// Whether this star hosts at least one known HZ planet (per the
    /// NASA Exoplanet Archive join plus the Kopparapu heuristic).
    pub has_hz_planet: bool,
}

/// Errors returned when building, loading, or saving a [`CatalogIndex`].
#[derive(Debug, thiserror::Error)]
pub enum CatalogIndexError {
    /// Failed to ingest the underlying Gaia Parquet file.
    #[error("Gaia catalog error: {0}")]
    Gaia(#[from] GaiaError),

    /// Filesystem I/O error during save or load.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// Path that triggered the error.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// bincode serialization or deserialization failed.
    #[error("bincode error at {path}: {source}")]
    Bincode {
        /// Path that triggered the error.
        path: PathBuf,
        /// Underlying bincode error.
        #[source]
        source: Box<bincode::ErrorKind>,
    },
}

/// Persisted payload. Keeps the on-disk format tight: only the SoA
/// columns plus the HZ-host flag array and a serialized vector of
/// (gaia_id, row) pairs. The hash map and empty-slice sentinel rebuild
/// on load.
#[derive(Serialize, Deserialize)]
struct PersistedIndex {
    gaia_ids: Vec<u64>,
    ra_deg: Vec<f64>,
    dec_deg: Vec<f64>,
    distance_pc: Vec<f64>,
    teff_k: Vec<f64>,
    luminosity_sun: Vec<f64>,
    unit_vec: Vec<[f64; 3]>,
    spectral: Vec<IndexedSpectralType>,
    has_hz_planet: Vec<bool>,
}

/// In-memory spatial + spectral index over the Gaia + Exoplanet join.
///
/// Layout is structure-of-arrays so linear scans (the hot path for
/// every filter method here) stream through cache-friendly contiguous
/// memory. The unit vectors are precomputed once at build time so
/// cone searches only need a dot product per row.
#[derive(Debug, Clone)]
pub struct CatalogIndex {
    gaia_ids: Vec<u64>,
    ra_deg: Vec<f64>,
    dec_deg: Vec<f64>,
    distance_pc: Vec<f64>,
    teff_k: Vec<f64>,
    luminosity_sun: Vec<f64>,
    unit_vec: Vec<[f64; 3]>,
    spectral: Vec<IndexedSpectralType>,
    has_hz_planet: Vec<bool>,
    gaia_id_to_row: HashMap<u64, usize>,
}

/// Convert RA/Dec in degrees to a unit Cartesian vector on the
/// celestial sphere. The math is the standard spherical-to-Cartesian
/// transform with declination as the polar complement.
fn radec_to_unit(ra_deg: f64, dec_deg: f64) -> [f64; 3] {
    let ra = ra_deg.to_radians();
    let dec = dec_deg.to_radians();
    let cos_dec = dec.cos();
    [cos_dec * ra.cos(), cos_dec * ra.sin(), dec.sin()]
}

/// Kopparapu conservative HZ inner edge in AU, approximated as
/// `sqrt(L / 1.1)` for the T_eff-independent scaling used by
/// [`CatalogIndex::filter_hz_hosts`].
fn hz_inner_au(luminosity_sun: f64) -> f64 {
    (luminosity_sun / 1.1).sqrt()
}

/// Kopparapu conservative HZ outer edge in AU, approximated as
/// `sqrt(L / 0.53)`. Same caveats as [`hz_inner_au`].
fn hz_outer_au(luminosity_sun: f64) -> f64 {
    (luminosity_sun / 0.53).sqrt()
}

impl CatalogIndex {
    /// Build the index by streaming the Gaia reader and joining each
    /// row against the exoplanet catalog by `gaia_id`.
    pub fn build(gaia: &GaiaReader, exo: &ExoplanetCatalog) -> Result<Self, CatalogIndexError> {
        let mut gaia_ids = Vec::new();
        let mut ra_deg = Vec::new();
        let mut dec_deg = Vec::new();
        let mut distance_pc = Vec::new();
        let mut teff_k = Vec::new();
        let mut luminosity_sun = Vec::new();
        let mut unit_vec = Vec::new();
        let mut spectral = Vec::new();
        let mut has_hz_planet = Vec::new();
        let mut gaia_id_to_row: HashMap<u64, usize> = HashMap::new();

        for (row_idx, row) in gaia.rows().enumerate() {
            let row = row?;
            // Cross-reference the exoplanet archive. If any confirmed
            // planet's semi-major axis sits inside the Kopparapu
            // conservative HZ for the catalog luminosity, flag the
            // host as an HZ-bearing system.
            let inner = hz_inner_au(row.luminosity_sun);
            let outer = hz_outer_au(row.luminosity_sun);
            let planets = exo.lookup_by_gaia_id(row.gaia_id);
            let mut is_hz_host = false;
            for planet in planets {
                if let Some(a) = planet.semi_major_axis_au
                    && a >= inner
                    && a <= outer
                {
                    is_hz_host = true;
                    break;
                }
            }

            gaia_ids.push(row.gaia_id);
            ra_deg.push(row.ra_deg);
            dec_deg.push(row.dec_deg);
            distance_pc.push(row.distance_pc);
            teff_k.push(row.teff_k);
            luminosity_sun.push(row.luminosity_sun);
            unit_vec.push(radec_to_unit(row.ra_deg, row.dec_deg));
            spectral.push(IndexedSpectralType::from_teff(row.teff_k));
            has_hz_planet.push(is_hz_host);
            gaia_id_to_row.insert(row.gaia_id, row_idx);
        }

        Ok(Self {
            gaia_ids,
            ra_deg,
            dec_deg,
            distance_pc,
            teff_k,
            luminosity_sun,
            unit_vec,
            spectral,
            has_hz_planet,
            gaia_id_to_row,
        })
    }

    /// Number of stars indexed.
    pub fn len(&self) -> usize {
        self.gaia_ids.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.gaia_ids.is_empty()
    }

    /// Serialize the index to a binary file via bincode.
    pub fn save(&self, path: &Path) -> Result<(), CatalogIndexError> {
        let file = File::create(path).map_err(|source| CatalogIndexError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let writer = BufWriter::new(file);
        let persisted = PersistedIndex {
            gaia_ids: self.gaia_ids.clone(),
            ra_deg: self.ra_deg.clone(),
            dec_deg: self.dec_deg.clone(),
            distance_pc: self.distance_pc.clone(),
            teff_k: self.teff_k.clone(),
            luminosity_sun: self.luminosity_sun.clone(),
            unit_vec: self.unit_vec.clone(),
            spectral: self.spectral.clone(),
            has_hz_planet: self.has_hz_planet.clone(),
        };
        bincode::serialize_into(writer, &persisted).map_err(|source| CatalogIndexError::Bincode {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Load an index previously written by [`Self::save`]. Rebuilds
    /// the transient Gaia-ID hash map from the persisted row data.
    pub fn load(path: &Path) -> Result<Self, CatalogIndexError> {
        let file = File::open(path).map_err(|source| CatalogIndexError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let reader = BufReader::new(file);
        let persisted: PersistedIndex =
            bincode::deserialize_from(reader).map_err(|source| CatalogIndexError::Bincode {
                path: path.to_path_buf(),
                source,
            })?;
        let mut gaia_id_to_row = HashMap::with_capacity(persisted.gaia_ids.len());
        for (row_idx, &gid) in persisted.gaia_ids.iter().enumerate() {
            gaia_id_to_row.insert(gid, row_idx);
        }
        Ok(Self {
            gaia_ids: persisted.gaia_ids,
            ra_deg: persisted.ra_deg,
            dec_deg: persisted.dec_deg,
            distance_pc: persisted.distance_pc,
            teff_k: persisted.teff_k,
            luminosity_sun: persisted.luminosity_sun,
            unit_vec: persisted.unit_vec,
            spectral: persisted.spectral,
            has_hz_planet: persisted.has_hz_planet,
            gaia_id_to_row,
        })
    }

    /// Build a [`StarInfo`] view for a given row index.
    fn star_info(&self, row: usize) -> StarInfo {
        StarInfo {
            row,
            gaia_id: self.gaia_ids[row],
            ra_deg: self.ra_deg[row],
            dec_deg: self.dec_deg[row],
            distance_pc: self.distance_pc[row],
            teff_k: self.teff_k[row],
            luminosity_sun: self.luminosity_sun[row],
            spectral: self.spectral[row].clone(),
            has_hz_planet: self.has_hz_planet[row],
        }
    }

    /// Look up a star by Gaia DR3 source ID.
    pub fn lookup_by_gaia_id(&self, gaia_id: u64) -> Option<StarInfo> {
        self.gaia_id_to_row
            .get(&gaia_id)
            .map(|&row| self.star_info(row))
    }

    /// Cone search: return every star within `radius_deg` of the
    /// point `(center_ra_deg, center_dec_deg)`, as measured by
    /// angular separation on the celestial sphere.
    ///
    /// The implementation is a brute-force dot-product scan against
    /// the precomputed unit-vector column. At ~116k rows this takes
    /// a few ms per query; see the module doc for when to promote
    /// this to a k-d tree.
    pub fn cone_search(
        &self,
        center_ra_deg: f64,
        center_dec_deg: f64,
        radius_deg: f64,
    ) -> Vec<StarInfo> {
        let center = radec_to_unit(center_ra_deg, center_dec_deg);
        let cos_threshold = radius_deg.to_radians().cos();
        let mut out = Vec::new();
        for (row, v) in self.unit_vec.iter().enumerate() {
            let dot = v[0] * center[0] + v[1] * center[1] + v[2] * center[2];
            if dot >= cos_threshold {
                out.push(self.star_info(row));
            }
        }
        out
    }

    /// Return every star whose primary spectral class matches `class`.
    /// Subclass is ignored.
    pub fn filter_by_spectral(&self, class: SpectralClass) -> Vec<StarInfo> {
        (0..self.len())
            .filter(|&row| self.spectral[row].class == class)
            .map(|row| self.star_info(row))
            .collect()
    }

    /// Return every star with `min_pc <= distance_pc <= max_pc`.
    pub fn filter_by_distance(&self, min_pc: f64, max_pc: f64) -> Vec<StarInfo> {
        (0..self.len())
            .filter(|&row| {
                let d = self.distance_pc[row];
                d >= min_pc && d <= max_pc
            })
            .map(|row| self.star_info(row))
            .collect()
    }

    /// Return every star flagged as an HZ host during [`Self::build`].
    pub fn filter_hz_hosts(&self) -> Vec<StarInfo> {
        (0..self.len())
            .filter(|&row| self.has_hz_planet[row])
            .map(|row| self.star_info(row))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    fn gaia_fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/gaia_sample.parquet")
    }

    fn exo_fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/exoplanet_sample.csv")
    }

    fn build_fixture_index() -> CatalogIndex {
        let gaia = GaiaReader::open(gaia_fixture_path()).expect("open gaia fixture");
        let exo = ExoplanetCatalog::load(&exo_fixture_path()).expect("load exo fixture");
        CatalogIndex::build(&gaia, &exo).expect("build index")
    }

    #[test]
    fn build_from_fixture() {
        let index = build_fixture_index();
        assert_eq!(index.len(), 50);
        assert!(!index.is_empty());
    }

    #[test]
    fn lookup_by_gaia_id_round_trip() {
        let index = build_fixture_index();
        let gaia = GaiaReader::open(gaia_fixture_path()).unwrap();
        let raw_rows: Vec<_> = gaia.rows().collect::<Result<_, _>>().unwrap();
        // Pick the middle fixture row and assert every column agrees.
        let target = &raw_rows[raw_rows.len() / 2];
        let hit = index
            .lookup_by_gaia_id(target.gaia_id)
            .expect("id present in index");
        assert_eq!(hit.gaia_id, target.gaia_id);
        assert!((hit.ra_deg - target.ra_deg).abs() < 1e-9);
        assert!((hit.dec_deg - target.dec_deg).abs() < 1e-9);
        assert!((hit.distance_pc - target.distance_pc).abs() < 1e-9);
        assert!((hit.teff_k - target.teff_k).abs() < 1e-9);
        assert!((hit.luminosity_sun - target.luminosity_sun).abs() < 1e-9);
    }

    #[test]
    fn cone_search_includes_center() {
        let index = build_fixture_index();
        // Pick a known fixture star and cone-search at its coordinates
        // with a 0.001 deg radius; it must land in the result set.
        let target = index.lookup_by_gaia_id(index.gaia_ids[0]).unwrap();
        let hits = index.cone_search(target.ra_deg, target.dec_deg, 0.001);
        assert!(
            hits.iter().any(|s| s.gaia_id == target.gaia_id),
            "cone at exact coordinates must contain the center star"
        );
    }

    #[test]
    fn cone_search_excludes_far_star() {
        let index = build_fixture_index();
        // Find two stars at least 10 degrees apart on the sphere.
        let a = &index.gaia_ids[0];
        let star_a = index.lookup_by_gaia_id(*a).unwrap();
        let far = (0..index.len())
            .find(|&row| {
                let sep = angular_sep_deg(
                    star_a.ra_deg,
                    star_a.dec_deg,
                    index.ra_deg[row],
                    index.dec_deg[row],
                );
                sep > 10.0
            })
            .expect("fixture must contain at least two stars > 10 deg apart");
        let far_gid = index.gaia_ids[far];
        // Tight cone around star_a: should not include the far star.
        let hits = index.cone_search(star_a.ra_deg, star_a.dec_deg, 1.0);
        assert!(hits.iter().all(|s| s.gaia_id != far_gid));
    }

    fn angular_sep_deg(ra1: f64, dec1: f64, ra2: f64, dec2: f64) -> f64 {
        let a = radec_to_unit(ra1, dec1);
        let b = radec_to_unit(ra2, dec2);
        let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
        dot.acos().to_degrees()
    }

    #[test]
    fn spectral_filter_narrows() {
        let index = build_fixture_index();
        let m = index.filter_by_spectral(SpectralClass::M);
        let g = index.filter_by_spectral(SpectralClass::G);
        // Sanity: partitioning by spectral class produces disjoint
        // subsets and the sum can't exceed the total population.
        assert!(m.len() + g.len() <= index.len());
        // The 50-nearest-stars fixture is M-heavy (many red dwarfs).
        assert!(!m.is_empty(), "nearest-50 fixture must contain M dwarfs");
        for s in &m {
            assert_eq!(s.spectral.class, SpectralClass::M);
        }
        for s in &g {
            assert_eq!(s.spectral.class, SpectralClass::G);
        }
    }

    #[test]
    fn distance_filter() {
        let index = build_fixture_index();
        let close = index.filter_by_distance(0.0, 5.0);
        assert!(!close.is_empty(), "fixture must contain stars within 5 pc");
        for s in &close {
            assert!(s.distance_pc >= 0.0);
            assert!(s.distance_pc <= 5.0);
        }
    }

    #[test]
    fn hz_hosts_intersection() {
        let gaia = GaiaReader::open(gaia_fixture_path()).unwrap();
        let exo = ExoplanetCatalog::load(&exo_fixture_path()).unwrap();
        let index = CatalogIndex::build(&gaia, &exo).unwrap();
        let hosts = index.filter_hz_hosts();
        for host in &hosts {
            let inner = hz_inner_au(host.luminosity_sun);
            let outer = hz_outer_au(host.luminosity_sun);
            let planets = exo.lookup_by_gaia_id(host.gaia_id);
            let any_in_hz = planets.iter().any(|p| {
                p.semi_major_axis_au
                    .is_some_and(|a| a >= inner && a <= outer)
            });
            assert!(
                any_in_hz,
                "host {} flagged as HZ but no planet's semi-major axis is inside [{inner}, {outer}]",
                host.gaia_id
            );
        }
    }

    #[test]
    fn save_load_round_trip() {
        let index = build_fixture_index();
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        index.save(tmp.path()).expect("save");
        let restored = CatalogIndex::load(tmp.path()).expect("load");

        assert_eq!(restored.gaia_ids, index.gaia_ids);
        assert_eq!(restored.ra_deg, index.ra_deg);
        assert_eq!(restored.dec_deg, index.dec_deg);
        assert_eq!(restored.distance_pc, index.distance_pc);
        assert_eq!(restored.teff_k, index.teff_k);
        assert_eq!(restored.luminosity_sun, index.luminosity_sun);
        assert_eq!(restored.unit_vec, index.unit_vec);
        assert_eq!(restored.spectral, index.spectral);
        assert_eq!(restored.has_hz_planet, index.has_hz_planet);
        // The transient map must rebuild correctly.
        for (gid, row) in &index.gaia_id_to_row {
            assert_eq!(restored.gaia_id_to_row.get(gid), Some(row));
        }
    }

    #[test]
    fn cone_search_perf_smoke() {
        // Fixture is only 50 rows, but this exercise validates the
        // hot loop is well-formed. Real perf is asserted at the
        // integration-level in the CAT-07 NOTE in TASKS.md.
        let index = build_fixture_index();
        let start = Instant::now();
        for _ in 0..1000 {
            let _ = index.cone_search(0.0, 0.0, 30.0);
        }
        let elapsed = start.elapsed();
        // 1000 queries over 50 rows should be essentially instantaneous.
        assert!(elapsed.as_millis() < 2000, "fixture cone search is slow");
    }

    #[test]
    fn spectral_from_teff_boundaries() {
        // Spot-check the Harvard boundaries.
        assert_eq!(
            IndexedSpectralType::from_teff(5778.0).class,
            SpectralClass::G
        );
        assert_eq!(
            IndexedSpectralType::from_teff(3000.0).class,
            SpectralClass::M
        );
        assert_eq!(
            IndexedSpectralType::from_teff(4500.0).class,
            SpectralClass::K
        );
        assert_eq!(
            IndexedSpectralType::from_teff(9500.0).class,
            SpectralClass::A
        );
        assert_eq!(
            IndexedSpectralType::from_teff(50_000.0).class,
            SpectralClass::O
        );
        // Below the M floor clamps to M9.
        let below = IndexedSpectralType::from_teff(1500.0);
        assert_eq!(below.class, SpectralClass::M);
        assert_eq!(below.subtype, 9);
    }
}
