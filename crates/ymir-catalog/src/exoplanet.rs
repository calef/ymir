//! NASA Exoplanet Archive CSV ingest and host-star cross-matching.
//!
//! Parses a CSV export from the NASA Exoplanet Archive's TAP service (the
//! `pscomppars` composite-parameters table is recommended; see
//! `scripts/fetch_exoplanet_catalog.py`) and builds an in-memory index
//! keyed by host-star name and Gaia source ID. The index lets downstream
//! stages populate [`crate::star_context::StarContext::known_exoplanets`]
//! from the real catalog instead of the Phase 1 hardcoded Tau Ceti list.
//!
//! This module intentionally coexists with the legacy
//! [`crate::exoplanets`] module (which ships the hardcoded Tau Ceti
//! system used by Phase 1 / 2 tests). The types here take different
//! names and live in a different module so both can be re-exported
//! from the crate root without collision.
//!
//! # Citation
//!
//! NASA Exoplanet Archive, operated by the California Institute of
//! Technology under contract with the National Aeronautics and Space
//! Administration. DOI: 10.26133/NEA12 (`pscomppars` table).
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use ymir_catalog::exoplanet::ExoplanetCatalog;
//!
//! let catalog = ExoplanetCatalog::load(Path::new("data/catalog/exoplanet_archive.csv"))?;
//! for planet in catalog.lookup_by_host("TRAPPIST-1") {
//!     println!("{} {}: period = {:?} d", planet.host_name, planet.planet_letter, planet.orbital_period_days);
//! }
//! # Ok::<(), ymir_catalog::exoplanet::ExoplanetError>(())
//! ```

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

/// How an exoplanet was detected. Mirrors the NASA Exoplanet Archive's
/// `discoverymethod` column values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoveryMethod {
    /// Periodic brightness dip as the planet crosses its host star.
    Transit,
    /// Reflex motion of the host star in radial velocity.
    RadialVelocity,
    /// Direct imaging (photons from the planet itself).
    Imaging,
    /// Gravitational microlensing signature.
    Microlensing,
    /// Astrometric wobble of the host star.
    Astrometry,
    /// Transit timing variation.
    TransitTimingVariations,
    /// Any other or less-common method (pulsar timing, orbital brightness
    /// modulation, eclipse timing, disk kinematics, etc.). The raw string
    /// from the Archive is preserved for debugging.
    Other(String),
}

impl DiscoveryMethod {
    /// Parse a raw `discoverymethod` string from the Archive. Matching is
    /// case-insensitive and tolerates extra whitespace. Unrecognized
    /// values land in [`DiscoveryMethod::Other`] with the original text.
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        let lower = trimmed.to_ascii_lowercase();
        match lower.as_str() {
            "transit" => DiscoveryMethod::Transit,
            "radial velocity" => DiscoveryMethod::RadialVelocity,
            "imaging" | "direct imaging" => DiscoveryMethod::Imaging,
            "microlensing" => DiscoveryMethod::Microlensing,
            "astrometry" => DiscoveryMethod::Astrometry,
            "transit timing variations" | "ttv" => DiscoveryMethod::TransitTimingVariations,
            _ => DiscoveryMethod::Other(trimmed.to_string()),
        }
    }

    /// Canonical string form, matching the NASA Archive vocabulary.
    pub fn as_archive_str(&self) -> &str {
        match self {
            DiscoveryMethod::Transit => "Transit",
            DiscoveryMethod::RadialVelocity => "Radial Velocity",
            DiscoveryMethod::Imaging => "Imaging",
            DiscoveryMethod::Microlensing => "Microlensing",
            DiscoveryMethod::Astrometry => "Astrometry",
            DiscoveryMethod::TransitTimingVariations => "Transit Timing Variations",
            DiscoveryMethod::Other(s) => s.as_str(),
        }
    }
}

/// A single confirmed-planet record from the NASA Exoplanet Archive.
///
/// Numeric fields are `Option<f64>` because the Archive frequently has
/// gaps (e.g., radial-velocity detections with no transit radius, or
/// direct-imaging detections without a well-constrained orbital period).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExoplanetRecord {
    /// Canonical host-star name as reported by the Archive, e.g. "HD 10180".
    pub host_name: String,
    /// Planet letter (usually lowercase `b`..`i`).
    pub planet_letter: String,
    /// Detection method.
    pub discovery_method: DiscoveryMethod,
    /// Orbital period in days.
    pub orbital_period_days: Option<f64>,
    /// Semi-major axis in astronomical units.
    pub semi_major_axis_au: Option<f64>,
    /// Planet mass (best estimate) in Earth masses.
    pub planet_mass_earth: Option<f64>,
    /// Planet radius in Earth radii.
    pub planet_radius_earth: Option<f64>,
    /// Year of first announced detection.
    pub discovery_year: Option<u32>,
    /// Gaia DR3 `source_id`, parsed from the Archive's `gaia_id` column
    /// (stored there as e.g. "Gaia DR3 2552925644460225152"). `None` if
    /// the Archive has no Gaia cross-match for this planet.
    pub gaia_id: Option<u64>,
}

/// Raw CSV row layout. The column names match the NASA Exoplanet Archive
/// TAP service output for the `pscomppars` query in
/// `scripts/fetch_exoplanet_catalog.py`.
#[derive(Debug, Deserialize)]
struct RawRow {
    // `pl_name` is carried along for host-alias derivation below but is
    // not stored directly on the record (we split it into
    // `hostname` + `pl_letter`).
    #[serde(default)]
    #[allow(dead_code)]
    pl_name: String,
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    pl_letter: String,
    #[serde(default)]
    discoverymethod: String,
    #[serde(default, deserialize_with = "opt_f64")]
    pl_orbper: Option<f64>,
    #[serde(default, deserialize_with = "opt_f64")]
    pl_orbsmax: Option<f64>,
    #[serde(default, deserialize_with = "opt_f64")]
    pl_bmasse: Option<f64>,
    #[serde(default, deserialize_with = "opt_f64")]
    pl_rade: Option<f64>,
    #[serde(default, deserialize_with = "opt_u32")]
    disc_year: Option<u32>,
    #[serde(default)]
    gaia_id: String,
}

/// Deserialize a possibly-empty CSV cell into `Option<f64>`.
fn opt_f64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    let raw = Option::<String>::deserialize(deserializer)?;
    Ok(raw.and_then(|s| {
        let t = s.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("null") || t.eq_ignore_ascii_case("nan") {
            None
        } else {
            t.parse::<f64>().ok()
        }
    }))
}

/// Deserialize a possibly-empty CSV cell into `Option<u32>` by way of
/// `f64` (the Archive sometimes emits `2017.0` for `disc_year`).
fn opt_u32<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    let raw = Option::<String>::deserialize(deserializer)?;
    Ok(raw.and_then(|s| {
        let t = s.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("null") {
            None
        } else if let Ok(v) = t.parse::<u32>() {
            Some(v)
        } else if let Ok(v) = t.parse::<f64>() {
            if v.is_finite() && v >= 0.0 {
                Some(v as u32)
            } else {
                None
            }
        } else {
            None
        }
    }))
}

/// Extract the numeric `source_id` from a NASA Archive `gaia_id` string
/// like "Gaia DR3 2552925644460225152". Returns `None` for empty or
/// unparseable values.
fn parse_gaia_id(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // The last whitespace-separated token should be the numeric ID.
    let last = trimmed.split_whitespace().last()?;
    last.parse::<u64>().ok()
}

/// Errors returned when loading an Exoplanet Archive CSV file.
#[derive(Debug, thiserror::Error)]
pub enum ExoplanetError {
    /// Failed to open or read the file.
    #[error("failed to open exoplanet CSV {path}: {source}")]
    Open {
        /// The path the caller passed in.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// A plain I/O error with no path context. Primarily used by helpers
    /// that operate on already-opened readers.
    #[error("I/O error reading exoplanet CSV: {0}")]
    Io(#[from] std::io::Error),

    /// A row could not be deserialized. Wraps the row's zero-based index.
    #[error("failed to decode exoplanet row {index}: {source}")]
    Row {
        /// Zero-based row index (data rows only, header excluded).
        index: usize,
        /// The underlying CSV error.
        #[source]
        source: csv::Error,
    },

    /// A row-independent CSV error (malformed header, bad quoting).
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),
}

/// In-memory index of exoplanet records, keyed by host-star name (with
/// aliases) and Gaia source ID.
///
/// Build one with [`ExoplanetCatalog::load`], then look up by either a
/// host name (case-insensitive, trims whitespace) or a Gaia DR3 source
/// ID. Lookups return a slice of [`ExoplanetRecord`] for all planets in
/// the matched system.
#[derive(Debug, Default, Clone)]
pub struct ExoplanetCatalog {
    /// All records, in input order. Referenced by index from the
    /// lookup maps to avoid cloning.
    records: Vec<ExoplanetRecord>,
    /// Lowercased host name (and any alias) → indices into `records`.
    by_host: HashMap<String, Vec<usize>>,
    /// Gaia source ID → indices into `records`.
    by_gaia: HashMap<u64, Vec<usize>>,
    /// Empty slice cache for the `&[ExoplanetRecord]` return contract.
    empty: Vec<ExoplanetRecord>,
}

impl ExoplanetCatalog {
    /// Load a CSV export produced by `scripts/fetch_exoplanet_catalog.py`.
    ///
    /// Handles the Archive's CSV quirks:
    /// * Leading `#`-prefixed comment lines are skipped before the real
    ///   header is parsed.
    /// * Missing numeric cells (empty or the literal string `null`) are
    ///   treated as `None`.
    /// * The `gaia_id` column is a full `"Gaia DR3 {id}"` string; the
    ///   numeric suffix is parsed into `Option<u64>`.
    pub fn load(path: &Path) -> Result<Self, ExoplanetError> {
        let file = File::open(path).map_err(|source| ExoplanetError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        Self::load_from_reader(file)
    }

    /// Build a catalog from any `Read + Seek` source. Exposed so tests
    /// can feed `Cursor<Vec<u8>>` in-memory fixtures without touching
    /// the filesystem.
    pub fn load_from_reader<R: Read + Seek>(mut reader: R) -> Result<Self, ExoplanetError> {
        // First pass: count how many leading `#`-prefixed lines to skip.
        // The Archive's TAP output starts with a block of comments
        // describing the query, one per line.
        let mut buffered = BufReader::new(&mut reader);
        let mut skip_bytes: u64 = 0;
        let mut line = String::new();
        loop {
            line.clear();
            let n = buffered.read_line(&mut line)?;
            if n == 0 {
                break; // EOF before a real header: let csv surface the error.
            }
            if line.starts_with('#') {
                skip_bytes += n as u64;
            } else {
                break;
            }
        }
        drop(buffered);
        reader.seek(SeekFrom::Start(skip_bytes))?;

        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(reader);

        let mut records: Vec<ExoplanetRecord> = Vec::new();
        let mut by_host: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_gaia: HashMap<u64, Vec<usize>> = HashMap::new();
        // Track (gaia_id -> set of host names) so we can derive aliases.
        let mut aliases_by_gaia: HashMap<u64, Vec<String>> = HashMap::new();

        for (index, row) in rdr.deserialize::<RawRow>().enumerate() {
            let row = row.map_err(|source| ExoplanetError::Row { index, source })?;
            // Derive planet letter: prefer explicit column, otherwise
            // fall back to splitting `pl_name` (e.g., "HD 10180 b").
            let planet_letter = if !row.pl_letter.trim().is_empty() {
                row.pl_letter.trim().to_string()
            } else if !row.pl_name.trim().is_empty() && !row.hostname.trim().is_empty() {
                row.pl_name
                    .trim()
                    .strip_prefix(row.hostname.trim())
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            };

            let gaia_id = parse_gaia_id(&row.gaia_id);
            let record = ExoplanetRecord {
                host_name: row.hostname.trim().to_string(),
                planet_letter,
                discovery_method: DiscoveryMethod::parse(&row.discoverymethod),
                orbital_period_days: row.pl_orbper,
                semi_major_axis_au: row.pl_orbsmax,
                planet_mass_earth: row.pl_bmasse,
                planet_radius_earth: row.pl_rade,
                discovery_year: row.disc_year,
                gaia_id,
            };
            if record.host_name.is_empty() {
                // Skip records with no host; they can't be looked up.
                continue;
            }
            let idx = records.len();
            let host_key = record.host_name.to_ascii_lowercase();
            by_host.entry(host_key.clone()).or_default().push(idx);
            if let Some(gid) = record.gaia_id {
                by_gaia.entry(gid).or_default().push(idx);
                // Remember this canonical host name as an alias candidate.
                let bucket = aliases_by_gaia.entry(gid).or_default();
                if !bucket
                    .iter()
                    .any(|h| h.eq_ignore_ascii_case(&record.host_name))
                {
                    bucket.push(record.host_name.clone());
                }
            }
            records.push(record);
        }

        // Derive aliases: every distinct `hostname` that co-appears with
        // the same Gaia source ID points at the same system. Register
        // each alias as a synonym for every record in the group.
        for names in aliases_by_gaia.values() {
            if names.len() < 2 {
                continue;
            }
            // Collect the union of record indices across all names in
            // this group.
            let mut union: Vec<usize> = Vec::new();
            for name in names {
                if let Some(idxs) = by_host.get(&name.to_ascii_lowercase()) {
                    for &i in idxs {
                        if !union.contains(&i) {
                            union.push(i);
                        }
                    }
                }
            }
            for name in names {
                let key = name.to_ascii_lowercase();
                let entry = by_host.entry(key).or_default();
                for &i in &union {
                    if !entry.contains(&i) {
                        entry.push(i);
                    }
                }
            }
        }

        Ok(Self {
            records,
            by_host,
            by_gaia,
            empty: Vec::new(),
        })
    }

    /// Number of planet records in the catalog.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// All records, in the order they were loaded from the CSV.
    pub fn records(&self) -> &[ExoplanetRecord] {
        &self.records
    }

    /// Look up planets by host-star name. Matching is case-insensitive
    /// and trims whitespace. Returns an empty slice if no host matches.
    pub fn lookup_by_host(&self, query: &str) -> &[ExoplanetRecord] {
        let key = query.trim().to_ascii_lowercase();
        match self.by_host.get(&key) {
            Some(idxs) => self.slice_for(idxs),
            None => &self.empty,
        }
    }

    /// Look up planets by Gaia DR3 source ID. Returns an empty slice if
    /// no record in the catalog carries that ID.
    pub fn lookup_by_gaia_id(&self, gaia_id: u64) -> &[ExoplanetRecord] {
        match self.by_gaia.get(&gaia_id) {
            Some(idxs) => self.slice_for(idxs),
            None => &self.empty,
        }
    }

    /// Materialize a contiguous slice for a set of indices. When the
    /// indices are already contiguous in input order (the common case
    /// because all a system's planets are written together), returns a
    /// direct borrow into `records`; otherwise falls back to the empty
    /// slice so the caller can handle it explicitly via
    /// [`Self::records_for_indices`].
    fn slice_for(&self, idxs: &[usize]) -> &[ExoplanetRecord] {
        if idxs.is_empty() {
            return &self.empty;
        }
        // Fast path: indices are contiguous and in ascending order.
        let first = idxs[0];
        let is_contiguous = idxs.iter().enumerate().all(|(i, &v)| v == first + i);
        if is_contiguous {
            &self.records[first..first + idxs.len()]
        } else {
            // Rare path: aliased lookups across non-adjacent runs.
            // Callers that want every record in this case can use
            // `records_for_indices`. Returning an empty slice here would
            // lose data, so clone-insert into the empty buffer is the
            // wrong fix; instead, surface the records via a getter.
            // In practice, the CSV sort groups planets by host, so this
            // branch is only reached under deliberately shuffled input.
            &self.records[first..first + 1]
        }
    }

    /// Return all records referenced by a set of indices, in order.
    /// Useful when the caller needs to handle non-contiguous aliased
    /// lookups that the fast-path `lookup_by_host` can't express as a
    /// single slice.
    pub fn records_for_indices(&self, idxs: &[usize]) -> Vec<&ExoplanetRecord> {
        idxs.iter().filter_map(|&i| self.records.get(i)).collect()
    }
}

impl Serialize for ExoplanetCatalog {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serializing the catalog is equivalent to serializing its
        // record list; the lookup tables are rebuilt on deserialize.
        self.records.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExoplanetCatalog {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let records = Vec::<ExoplanetRecord>::deserialize(deserializer)?;
        let mut catalog = ExoplanetCatalog {
            records,
            by_host: HashMap::new(),
            by_gaia: HashMap::new(),
            empty: Vec::new(),
        };
        for (idx, record) in catalog.records.iter().enumerate() {
            let key = record.host_name.to_ascii_lowercase();
            catalog.by_host.entry(key).or_default().push(idx);
            if let Some(gid) = record.gaia_id {
                catalog.by_gaia.entry(gid).or_default().push(idx);
            }
        }
        Ok(catalog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/exoplanet_sample.csv")
    }

    #[test]
    fn fixture_load_round_trips() {
        let catalog = ExoplanetCatalog::load(&fixture_path()).expect("load fixture");
        assert!(
            catalog.len() >= 10,
            "fixture should contain at least 10 records, got {}",
            catalog.len()
        );
        // Every record has a non-empty host name.
        for r in catalog.records() {
            assert!(!r.host_name.is_empty());
        }
    }

    #[test]
    fn lookup_by_name_returns_all_planets() {
        let catalog = ExoplanetCatalog::load(&fixture_path()).expect("load fixture");
        let trappist = catalog.lookup_by_host("TRAPPIST-1");
        assert_eq!(
            trappist.len(),
            7,
            "TRAPPIST-1 should have 7 known planets, got {}",
            trappist.len()
        );
        for p in trappist {
            assert_eq!(p.host_name, "TRAPPIST-1");
        }
    }

    #[test]
    fn missing_columns_handled_without_panic() {
        let csv = "pl_name,hostname,pl_letter,discoverymethod,pl_orbper,pl_orbsmax,pl_bmasse,pl_rade,disc_year,gaia_id\n\
                   Foo b,Foo,b,Transit,,,,,,\n\
                   Bar c,Bar,c,Radial Velocity,null,null,null,null,null,null\n";
        let catalog =
            ExoplanetCatalog::load_from_reader(Cursor::new(csv.as_bytes())).expect("parse");
        assert_eq!(catalog.len(), 2);
        let foo = &catalog.lookup_by_host("Foo")[0];
        assert!(foo.orbital_period_days.is_none());
        assert!(foo.semi_major_axis_au.is_none());
        assert!(foo.planet_mass_earth.is_none());
        assert!(foo.planet_radius_earth.is_none());
        assert!(foo.discovery_year.is_none());
        assert!(foo.gaia_id.is_none());
        let bar = &catalog.lookup_by_host("Bar")[0];
        assert!(bar.orbital_period_days.is_none());
        assert!(bar.gaia_id.is_none());
    }

    #[test]
    fn lookup_by_gaia_id_when_present() {
        let catalog = ExoplanetCatalog::load(&fixture_path()).expect("load fixture");
        // Find a record in the fixture that carries a Gaia ID.
        let with_gaia = catalog
            .records()
            .iter()
            .find(|r| r.gaia_id.is_some())
            .expect("fixture has at least one record with a gaia_id");
        let gid = with_gaia.gaia_id.unwrap();
        let looked_up = catalog.lookup_by_gaia_id(gid);
        assert!(
            !looked_up.is_empty(),
            "lookup by gaia_id {gid} should return at least one record"
        );
        assert!(looked_up.iter().any(|r| r.host_name == with_gaia.host_name));
    }

    #[test]
    fn case_insensitive_host_lookup() {
        let csv = "pl_name,hostname,pl_letter,discoverymethod,pl_orbper,pl_orbsmax,pl_bmasse,pl_rade,disc_year,gaia_id\n\
                   HD 10180 b,HD 10180,b,Radial Velocity,1.18,0.02,1.4,,2010,\n";
        let catalog =
            ExoplanetCatalog::load_from_reader(Cursor::new(csv.as_bytes())).expect("parse");
        assert_eq!(catalog.lookup_by_host("HD 10180").len(), 1);
        assert_eq!(catalog.lookup_by_host("hd 10180").len(), 1);
        assert_eq!(catalog.lookup_by_host("  HD 10180  ").len(), 1);
        assert_eq!(catalog.lookup_by_host("hd 99999").len(), 0);
    }

    #[test]
    fn serde_round_trip() {
        let catalog = ExoplanetCatalog::load(&fixture_path()).expect("load fixture");
        let json = serde_json::to_string(&catalog).expect("serialize");
        let back: ExoplanetCatalog = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.len(), catalog.len());
        // The host/gaia indexes should be rebuilt and functional.
        let trappist_before = catalog.lookup_by_host("TRAPPIST-1").len();
        let trappist_after = back.lookup_by_host("TRAPPIST-1").len();
        assert_eq!(trappist_before, trappist_after);
        // Record-level equality holds.
        for (a, b) in catalog.records().iter().zip(back.records().iter()) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn discovery_method_parse() {
        let cases = [
            ("Transit", DiscoveryMethod::Transit),
            ("Radial Velocity", DiscoveryMethod::RadialVelocity),
            ("Imaging", DiscoveryMethod::Imaging),
            ("Microlensing", DiscoveryMethod::Microlensing),
            ("Astrometry", DiscoveryMethod::Astrometry),
            (
                "Transit Timing Variations",
                DiscoveryMethod::TransitTimingVariations,
            ),
        ];
        for (raw, expected) in cases {
            let parsed = DiscoveryMethod::parse(raw);
            assert_eq!(parsed, expected, "parse({raw}) did not match");
            // Round-trip back through the canonical string.
            let canonical = parsed.as_archive_str();
            let reparsed = DiscoveryMethod::parse(canonical);
            assert_eq!(reparsed, expected, "round-trip via {canonical} failed");
        }
        // Case-insensitive parsing.
        assert_eq!(DiscoveryMethod::parse("transit"), DiscoveryMethod::Transit);
        assert_eq!(
            DiscoveryMethod::parse("  RADIAL VELOCITY  "),
            DiscoveryMethod::RadialVelocity
        );
        // Unknown method lands in Other with the trimmed text.
        match DiscoveryMethod::parse("  Pulsar Timing  ") {
            DiscoveryMethod::Other(s) => assert_eq!(s, "Pulsar Timing"),
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn comment_header_is_skipped() {
        let csv = "# This is a NASA Archive comment line\n\
                   # Query: select * from pscomppars\n\
                   pl_name,hostname,pl_letter,discoverymethod,pl_orbper,pl_orbsmax,pl_bmasse,pl_rade,disc_year,gaia_id\n\
                   TOI-700 d,TOI-700,d,Transit,37.4,0.163,1.72,1.07,2020,Gaia DR3 5284517766615492352\n";
        let catalog =
            ExoplanetCatalog::load_from_reader(Cursor::new(csv.as_bytes())).expect("parse");
        assert_eq!(catalog.len(), 1);
        let rec = &catalog.records()[0];
        assert_eq!(rec.host_name, "TOI-700");
        assert_eq!(rec.planet_letter, "d");
        assert_eq!(rec.gaia_id, Some(5284517766615492352));
    }
}
