//! Unified `Catalog` query facade over Gaia DR3, the NASA Exoplanet
//! Archive, and the spatial/spectral index.
//!
//! Wraps [`GaiaReader`], [`ExoplanetCatalog`], and [`CatalogIndex`] behind
//! a single type so call sites (CLI, GUI, validation harness) can resolve
//! a star by name or Gaia source ID, list candidates by spectral class /
//! distance / HZ-host flag, and obtain a fully populated
//! [`StarContext`] with provenance wired through as [`ymir_core::Source::Observed`].
//!
//! # Legacy path
//!
//! The Phase 1 / 2 hardcoded [`StarContext::tau_ceti`] and
//! [`crate::sol::sol_context`] fixtures remain importable so the binary's
//! `--builtin` mode (added in CAT-09) can reproduce the original
//! deterministic test worlds. The catalog facade is purely additive.
//!
//! # Name aliases
//!
//! Common-name → Gaia source ID aliases are loaded from a TSV shipped at
//! `crates/ymir-catalog/data/common_names.tsv`. The file is embedded at
//! compile time via `include_str!` so no runtime filesystem lookup is
//! needed and tests don't have to plumb a separate path. "Sol" / "Sun"
//! are handled specially (see the module doc on [`Catalog::resolve`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog_index::{CatalogIndex, CatalogIndexError, IndexedSpectralType, StarInfo};
use crate::exoplanet::{ExoplanetCatalog, ExoplanetError, ExoplanetRecord};
use crate::gaia::{GaiaError, GaiaReader};
use crate::star_context::{SpectralClass, StarContext};

/// Embedded common-name → Gaia source ID alias table. One entry per
/// line, tab-separated. `#`-prefixed and blank lines are ignored.
const COMMON_NAMES_TSV: &str = include_str!("../data/common_names.tsv");

/// Query shape for [`Catalog::list`].
///
/// Every field is optional; an all-`None` query returns every indexed
/// star. Combinators are AND: a row must match every populated
/// predicate to be included in the result.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CatalogQuery {
    /// Filter to a single Harvard spectral class (O/B/A/F/G/K/M).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spectral: Option<SpectralClass>,
    /// Inclusive lower bound on distance in parsecs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_distance_pc: Option<f64>,
    /// Inclusive upper bound on distance in parsecs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_distance_pc: Option<f64>,
    /// If `true`, keep only stars flagged as hosting at least one known
    /// HZ planet (see [`CatalogIndex::filter_hz_hosts`]).
    #[serde(default)]
    pub hz_hosts_only: bool,
}

/// Compact row shape returned by [`Catalog::list`] and [`Catalog::summary`].
///
/// Trades the richer [`StarContext`] (with HZ math and exoplanet
/// records) for a small, cheap-to-copy struct suitable for populating a
/// table UI or a CLI `ymir list-stars` listing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StarSummary {
    /// Gaia DR3 source ID.
    pub gaia_id: u64,
    /// First resolvable common name (per the alias table), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_name: Option<String>,
    /// Right ascension in degrees (ICRS).
    pub ra_deg: f64,
    /// Declination in degrees (ICRS).
    pub dec_deg: f64,
    /// Inverse-parallax distance in parsecs.
    pub distance_pc: f64,
    /// GSP-Phot effective temperature in Kelvin.
    pub teff_k: f64,
    /// FLAME luminosity in solar luminosities.
    pub luminosity_sun: f64,
    /// Harvard class + subtype derived from T_eff.
    pub spectral: IndexedSpectralType,
    /// Whether the index flagged this star as hosting a known HZ planet.
    pub has_hz_planet: bool,
}

/// Errors returned when opening or querying a [`Catalog`].
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// Gaia Parquet file failed to open or decode.
    #[error("Gaia catalog error: {0}")]
    Gaia(#[from] GaiaError),

    /// NASA Exoplanet Archive CSV failed to load.
    #[error("Exoplanet catalog error: {0}")]
    Exoplanet(#[from] ExoplanetError),

    /// Index build failed (usually wraps the underlying Gaia error).
    #[error("Catalog index error: {0}")]
    Index(#[from] CatalogIndexError),

    /// A required auxiliary file (e.g., the alias TSV) was missing.
    #[error("missing auxiliary data: {path}")]
    MissingAux {
        /// Path or label that failed to resolve.
        path: PathBuf,
    },
}

/// Unified query facade over the Gaia, Exoplanet, and spatial/spectral
/// index components.
///
/// Open via [`Catalog::open`]. The catalog owns decoded exoplanet data
/// and the in-memory index; the [`GaiaReader`] is held only to echo its
/// source path for diagnostics and can be dropped once the index is
/// built (the reader does not cache rows).
pub struct Catalog {
    exo: ExoplanetCatalog,
    index: CatalogIndex,
    /// Lowercased common name → Gaia source ID.
    name_to_gaia_id: HashMap<String, u64>,
    /// Gaia source ID → first-seen common name for the reverse-lookup
    /// used when populating [`StarSummary::common_name`].
    gaia_id_to_name: HashMap<u64, String>,
    /// Path to the Gaia Parquet file (kept for diagnostics).
    #[allow(dead_code)]
    gaia_path: PathBuf,
    /// Path to the Exoplanet CSV file (kept for diagnostics).
    #[allow(dead_code)]
    exo_path: PathBuf,
}

impl Catalog {
    /// Open a catalog backed by the given Gaia Parquet file and
    /// Exoplanet Archive CSV. Builds the in-memory index eagerly; the
    /// heavy lifting happens here so subsequent queries are cheap.
    pub fn open(gaia_path: &Path, exo_path: &Path) -> Result<Self, CatalogError> {
        let gaia = GaiaReader::open(gaia_path)?;
        let exo = ExoplanetCatalog::load(exo_path)?;
        let index = CatalogIndex::build(&gaia, &exo)?;

        let (name_to_gaia_id, gaia_id_to_name) = parse_alias_table(COMMON_NAMES_TSV);

        Ok(Self {
            exo,
            index,
            name_to_gaia_id,
            gaia_id_to_name,
            gaia_path: gaia_path.to_path_buf(),
            exo_path: exo_path.to_path_buf(),
        })
    }

    /// Number of indexed stars.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Resolve a name or Gaia source ID string to a fully populated
    /// [`StarContext`].
    ///
    /// Resolution order:
    /// 1. Special-case "Sol" / "Sun" → [`StarContext::from_sun`]. The
    ///    Sun has no Gaia DR3 row, so falling through would incorrectly
    ///    return `None`.
    /// 2. Pure-digit query → treat as a Gaia source ID and look up
    ///    directly in the index.
    /// 3. Otherwise, case-insensitive lookup in the alias table, then
    ///    Gaia-index lookup on the mapped source ID.
    ///
    /// Returns `None` if the name is unknown or the mapped Gaia ID is
    /// absent from the loaded catalog (e.g., when running against the
    /// small fixture).
    pub fn resolve(&self, query: &str) -> Option<StarContext> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return None;
        }

        // 1. Sun special-case.
        let lower = trimmed.to_ascii_lowercase();
        if lower == "sol" || lower == "sun" {
            return Some(StarContext::from_sun());
        }

        // 2. Pure-digit → Gaia source ID.
        if trimmed.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(gid) = trimmed.parse::<u64>() {
                return self.resolve_by_gaia_id(gid);
            }
        }

        // 3. Alias table.
        let gid = *self.name_to_gaia_id.get(&lower)?;
        self.resolve_by_gaia_id(gid)
    }

    /// Return the raw exoplanet rows joined to the given Gaia source ID.
    ///
    /// Added for the CAT-09 CLI so `describe-star` can render a per-planet
    /// table with orbital period / semi-major axis / mass / radius without
    /// re-opening the underlying Archive CSV.
    pub fn exoplanets_for(&self, gaia_id: u64) -> &[ExoplanetRecord] {
        self.exo.lookup_by_gaia_id(gaia_id)
    }

    /// Reverse-lookup the first common name associated with a Gaia source
    /// ID, per the embedded alias table. Returns `None` if no alias was
    /// recorded for the ID.
    ///
    /// Added for the CAT-09 CLI so the `list-stars` table can display a
    /// human-friendly name column; the library exposes it publicly because
    /// the GUI star browser (GUI-02) will want the same mapping.
    pub fn common_name_for(&self, gaia_id: u64) -> Option<&str> {
        self.gaia_id_to_name.get(&gaia_id).map(String::as_str)
    }

    /// Return a [`StarSummary`] for the given Gaia source ID, or `None`
    /// if the ID is absent from the index.
    pub fn summary(&self, gaia_id: u64) -> Option<StarSummary> {
        let info = self.index.lookup_by_gaia_id(gaia_id)?;
        Some(self.star_summary(&info))
    }

    /// Apply the given query against the index and return matching
    /// summaries. Order is the index's native row order.
    pub fn list(&self, query: &CatalogQuery) -> Vec<StarSummary> {
        // Start from the narrowest filter available so we scan as
        // little as possible.
        let candidates: Vec<StarInfo> = if let Some(class) = &query.spectral {
            self.index.filter_by_spectral(class.clone())
        } else if query.min_distance_pc.is_some() || query.max_distance_pc.is_some() {
            let min = query.min_distance_pc.unwrap_or(0.0);
            let max = query.max_distance_pc.unwrap_or(f64::INFINITY);
            self.index.filter_by_distance(min, max)
        } else if query.hz_hosts_only {
            self.index.filter_hz_hosts()
        } else {
            // Unconstrained: use the open-bounds distance filter as a
            // cheap full scan (the index has no dedicated `all()`
            // accessor yet; this walks once and clones into StarInfo).
            self.index.filter_by_distance(0.0, f64::INFINITY)
        };

        candidates
            .into_iter()
            .filter(|s| {
                if let Some(min) = query.min_distance_pc
                    && s.distance_pc < min
                {
                    return false;
                }
                if let Some(max) = query.max_distance_pc
                    && s.distance_pc > max
                {
                    return false;
                }
                if query.hz_hosts_only && !s.has_hz_planet {
                    return false;
                }
                true
            })
            .map(|s| self.star_summary(&s))
            .collect()
    }

    /// Build a [`StarSummary`] for a resolved [`StarInfo`], filling in
    /// the common name from the reverse-lookup map if the ID has one.
    fn star_summary(&self, info: &StarInfo) -> StarSummary {
        StarSummary {
            gaia_id: info.gaia_id,
            common_name: self.gaia_id_to_name.get(&info.gaia_id).cloned(),
            ra_deg: info.ra_deg,
            dec_deg: info.dec_deg,
            distance_pc: info.distance_pc,
            teff_k: info.teff_k,
            luminosity_sun: info.luminosity_sun,
            spectral: info.spectral.clone(),
            has_hz_planet: info.has_hz_planet,
        }
    }

    /// Internal helper: Gaia-ID → full `StarContext` with exoplanet
    /// overlay. Returns `None` if the ID isn't in the index.
    fn resolve_by_gaia_id(&self, gaia_id: u64) -> Option<StarContext> {
        let info = self.index.lookup_by_gaia_id(gaia_id)?;
        let planets = self.exo.lookup_by_gaia_id(gaia_id);
        let mut ctx = StarContext::from_catalog(&info, planets);
        // Prefer the alias table's name if the exoplanet join didn't
        // supply one (e.g., a star with no known planets).
        if ctx.name.is_none()
            && let Some(name) = self.gaia_id_to_name.get(&gaia_id)
        {
            ctx.name = Some(name.clone());
        }
        Some(ctx)
    }
}

/// Parse the embedded TSV alias table into forward (name → id) and
/// reverse (id → first name) maps. The reverse map records the first
/// occurrence, which mirrors how a user would expect "Proxima Centauri"
/// (the canonical form) to win over "Proxima Cen" (the alias).
fn parse_alias_table(tsv: &str) -> (HashMap<String, u64>, HashMap<u64, String>) {
    let mut forward: HashMap<String, u64> = HashMap::new();
    let mut reverse: HashMap<u64, String> = HashMap::new();
    for line in tsv.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.splitn(2, '\t');
        let name = match parts.next() {
            Some(n) => n.trim(),
            None => continue,
        };
        let id_str = match parts.next() {
            Some(i) => i.trim(),
            None => continue,
        };
        let Ok(id) = id_str.parse::<u64>() else {
            continue;
        };
        forward.insert(name.to_ascii_lowercase(), id);
        reverse.entry(id).or_insert_with(|| name.to_string());
    }
    (forward, reverse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use ymir_core::Source;

    fn gaia_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/gaia_sample.parquet")
    }

    fn exo_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/exoplanet_sample.csv")
    }

    fn open_fixture_catalog() -> Catalog {
        Catalog::open(&gaia_fixture(), &exo_fixture()).expect("open fixture catalog")
    }

    /// The committed Gaia fixture is the nearest-50 slice, which does
    /// not contain Tau Ceti. Epsilon Eridani IS present at source_id
    /// 5164707970261890560 (T_eff ~5002 K, L ~0.34 L_sun, d ~3.22 pc).
    /// We exercise the "catalog-observed values surface as
    /// Sourced::Observed" invariant against Epsilon Eridani instead; the
    /// Tau Ceti alias stays in the TSV for the full 116k-row catalog.
    #[test]
    fn resolve_epsilon_eridani_returns_observed_context() {
        let catalog = open_fixture_catalog();
        let ctx = catalog
            .resolve("Epsilon Eridani")
            .expect("epsilon eridani must resolve against the fixture");
        assert!(matches!(
            ctx.effective_temp.source(),
            Source::Observed { .. }
        ));
        assert!(matches!(ctx.luminosity.source(), Source::Observed { .. }));
        assert!(matches!(ctx.distance.source(), Source::Observed { .. }));
        // Epsilon Eridani is a K2V at ~5084 K per Campbell+ 2006; the
        // Gaia DR3 GSP-Phot T_eff shipped in the fixture is ~5002 K.
        // Allow ±200 K to absorb the GSP-Phot systematic.
        let teff = *ctx.effective_temp.inner();
        assert!(
            (teff - 5084.0).abs() < 200.0,
            "T_eff {teff} not near Epsilon Eridani's published value"
        );
    }

    /// Explicit Tau-Ceti-shape test: the resolver finds the alias in
    /// the TSV but the Gaia ID isn't in the fixture, so the answer is
    /// `None`. Documents the fixture's limitation and guards the
    /// resolver's graceful miss-path.
    #[test]
    fn resolve_tau_ceti_missing_from_fixture_returns_none() {
        let catalog = open_fixture_catalog();
        assert!(
            catalog.resolve("Tau Ceti").is_none(),
            "Tau Ceti is not in the fixture; resolve should miss cleanly"
        );
    }

    #[test]
    fn resolve_sol_synthesizes_context() {
        let catalog = open_fixture_catalog();
        let ctx = catalog.resolve("Sol").expect("sol synthesized");
        assert_eq!(ctx.name.as_deref(), Some("Sol"));
        assert!((*ctx.effective_temp.inner() - 5778.0).abs() < 1e-6);
        assert!((*ctx.luminosity.inner() - 1.0).abs() < 1e-6);
        assert!((*ctx.distance.inner() - 0.0).abs() < 1e-6);
        assert!(matches!(
            ctx.effective_temp.source(),
            Source::Observed { .. }
        ));
        // "Sun" alias works the same way.
        let ctx2 = catalog.resolve("Sun").expect("sun synthesized");
        assert!((*ctx2.effective_temp.inner() - 5778.0).abs() < 1e-6);
    }

    #[test]
    fn resolve_unknown_returns_none() {
        let catalog = open_fixture_catalog();
        assert!(catalog.resolve("HD 99999999999").is_none());
        assert!(catalog.resolve("").is_none());
        assert!(catalog.resolve("   ").is_none());
    }

    #[test]
    fn resolve_by_gaia_id_string() {
        let catalog = open_fixture_catalog();
        // Proxima Centauri in the fixture.
        let ctx = catalog
            .resolve("5853498713190525696")
            .expect("gaia_id digit lookup");
        assert!(ctx.catalog_id.contains("5853498713190525696"));
        assert!(matches!(
            ctx.effective_temp.source(),
            Source::Observed { .. }
        ));
    }

    #[test]
    fn list_with_spectral_filter() {
        let catalog = open_fixture_catalog();
        let q = CatalogQuery {
            spectral: Some(SpectralClass::M),
            ..Default::default()
        };
        let results = catalog.list(&q);
        assert!(!results.is_empty(), "fixture is M-heavy");
        for s in &results {
            assert_eq!(s.spectral.class, SpectralClass::M);
        }
    }

    #[test]
    fn list_with_distance_filter() {
        let catalog = open_fixture_catalog();
        let q = CatalogQuery {
            max_distance_pc: Some(5.0),
            ..Default::default()
        };
        let results = catalog.list(&q);
        assert!(!results.is_empty(), "fixture has stars within 5 pc");
        for s in &results {
            assert!(s.distance_pc <= 5.0);
        }
        // Narrower bound shrinks the result.
        let q2 = CatalogQuery {
            min_distance_pc: Some(0.0),
            max_distance_pc: Some(2.0),
            ..Default::default()
        };
        let tighter = catalog.list(&q2);
        assert!(tighter.len() <= results.len());
        for s in &tighter {
            assert!(s.distance_pc <= 2.0);
        }
    }

    #[test]
    fn list_with_hz_hosts_only() {
        let catalog = open_fixture_catalog();
        let q = CatalogQuery {
            hz_hosts_only: true,
            ..Default::default()
        };
        let results = catalog.list(&q);
        // The fixture may or may not contain HZ hosts; the invariant is
        // the flag, not a non-empty result.
        for s in &results {
            assert!(s.has_hz_planet, "hz_hosts_only must enforce the flag");
        }
    }

    #[test]
    fn list_combined_filters_intersect() {
        let catalog = open_fixture_catalog();
        let q = CatalogQuery {
            spectral: Some(SpectralClass::M),
            max_distance_pc: Some(5.0),
            ..Default::default()
        };
        let results = catalog.list(&q);
        for s in &results {
            assert_eq!(s.spectral.class, SpectralClass::M);
            assert!(s.distance_pc <= 5.0);
        }
    }

    #[test]
    fn summary_lookup() {
        let catalog = open_fixture_catalog();
        let s = catalog
            .summary(5853498713190525696)
            .expect("Proxima in fixture");
        assert_eq!(s.gaia_id, 5853498713190525696);
        assert_eq!(s.common_name.as_deref(), Some("Proxima Centauri"));
    }

    #[test]
    fn serde_round_trip_catalog_query() {
        let q = CatalogQuery {
            spectral: Some(SpectralClass::G),
            min_distance_pc: Some(1.0),
            max_distance_pc: Some(10.0),
            hz_hosts_only: true,
        };
        let json = serde_json::to_string(&q).expect("ser");
        let back: CatalogQuery = serde_json::from_str(&json).expect("de");
        assert_eq!(back.spectral, q.spectral);
        assert_eq!(back.min_distance_pc, q.min_distance_pc);
        assert_eq!(back.max_distance_pc, q.max_distance_pc);
        assert_eq!(back.hz_hosts_only, q.hz_hosts_only);
    }

    #[test]
    fn serde_round_trip_star_summary() {
        let catalog = open_fixture_catalog();
        let s = catalog.summary(5853498713190525696).expect("fixture");
        let json = serde_json::to_string(&s).expect("ser");
        let back: StarSummary = serde_json::from_str(&json).expect("de");
        assert_eq!(back, s);
    }

    #[test]
    fn common_name_for_reverse_lookup() {
        let catalog = open_fixture_catalog();
        // Proxima Centauri is in both the alias table and the fixture.
        assert_eq!(
            catalog.common_name_for(5853498713190525696),
            Some("Proxima Centauri")
        );
        // An unknown Gaia ID returns None.
        assert_eq!(catalog.common_name_for(0), None);
    }

    #[test]
    fn alias_table_parses_committed_tsv() {
        let (forward, reverse) = parse_alias_table(COMMON_NAMES_TSV);
        assert!(forward.contains_key("proxima centauri"));
        assert!(forward.contains_key("tau ceti"));
        // Reverse map records a canonical display form.
        assert!(reverse.contains_key(&5853498713190525696));
    }
}
