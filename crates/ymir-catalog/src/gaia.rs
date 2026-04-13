//! Gaia DR3 catalog parsing and star property extraction.
//!
//! Reads the pre-filtered Gaia DR3 working catalog (see
//! [`crate`] docs and `ARCHITECTURE.md` §6.1): stars within 100 parsecs of
//! Sol with parallax error < 20 percent and known effective temperature
//! and luminosity. The on-disk format is Parquet, produced by
//! `scripts/fetch_gaia_catalog.py` from an ADQL query against the ESA
//! Gaia archive TAP service.
//!
//! The full catalog is ~hundreds of MB and lives at
//! `data/catalog/gaia_dr3_100pc.parquet`; it is not committed. A small
//! fixture (`crates/ymir-catalog/fixtures/gaia_sample.parquet`, ~50
//! rows) is committed for unit tests and CI.
//!
//! # Citation
//!
//! Gaia Collaboration, Vallenari A., Brown A.G.A., Prusti T., et al.
//! 2023, *A&A* 674, A1 ("Gaia Data Release 3: Summary of the content
//! and survey properties"). Always cite the DR3 release paper plus the
//! ESA/Gaia mission paper (Gaia Collaboration 2016, *A&A* 595, A1) when
//! publishing results that use this module.
//!
//! # Example
//!
//! ```no_run
//! use ymir_catalog::gaia::GaiaReader;
//!
//! let reader = GaiaReader::open("data/catalog/gaia_dr3_100pc.parquet")?;
//! for row in reader.rows_within_pc(10.0) {
//!     let star = row?;
//!     println!("{}: {:.2} pc, T_eff={:.0} K", star.gaia_id, star.distance_pc, star.teff_k);
//! }
//! # Ok::<(), ymir_catalog::gaia::GaiaError>(())
//! ```

use std::fs::File;
use std::path::{Path, PathBuf};

use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::{Row, RowAccessor};
use parquet::schema::types::SchemaDescPtr;
use serde::{Deserialize, Serialize};

/// Errors returned when opening or iterating a Gaia Parquet file.
#[derive(Debug, thiserror::Error)]
pub enum GaiaError {
    /// Failed to open the file or initialize a reader. Wraps the
    /// underlying `parquet` crate error and includes the path that
    /// produced it for easier debugging.
    #[error("failed to open Gaia Parquet file {path}: {source}")]
    Open {
        /// The path the caller passed in.
        path: PathBuf,
        /// The underlying error from the parquet crate.
        #[source]
        source: parquet::errors::ParquetError,
    },

    /// An I/O error from the underlying file handle.
    #[error("I/O error reading Gaia Parquet file: {0}")]
    Io(#[from] std::io::Error),

    /// A row failed to decode. Usually a schema mismatch (missing
    /// required column, unexpected null, wrong physical type).
    #[error("failed to decode Gaia row at index {index}: {source}")]
    Row {
        /// Zero-based row index within the file.
        index: usize,
        /// The underlying parquet error.
        #[source]
        source: parquet::errors::ParquetError,
    },

    /// A required column is missing from the file schema.
    #[error("required column '{0}' missing from Gaia Parquet schema")]
    MissingColumn(&'static str),
}

/// A single star record from the pre-filtered Gaia DR3 catalog.
///
/// Nullable FLAME fields (`mass_sun`, `radius_sun`, `age_gyr`) and the
/// metallicity field stay `Option`; the astrometric and GSP-Phot T_eff
/// fields are always present because the fetch ADQL filters them to
/// `IS NOT NULL`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StarRow {
    /// Gaia DR3 `source_id`. Fits in 63 bits but is conventionally
    /// printed as an unsigned integer.
    pub gaia_id: u64,
    /// Right ascension in degrees (ICRS).
    pub ra_deg: f64,
    /// Declination in degrees (ICRS).
    pub dec_deg: f64,
    /// Parallax in milliarcseconds.
    pub parallax_mas: f64,
    /// Parallax standard error in milliarcseconds.
    pub parallax_error_mas: f64,
    /// Inverse-parallax distance in parsecs (1000 / parallax_mas).
    /// Accurate only for well-constrained parallaxes (the working
    /// catalog filters to parallax_over_error > 5).
    pub distance_pc: f64,
    /// GSP-Phot effective temperature in Kelvin.
    pub teff_k: f64,
    /// FLAME luminosity in solar luminosities.
    pub luminosity_sun: f64,
    /// GSP-Phot metallicity [M/H] (dex). Optional.
    pub feh: Option<f64>,
    /// FLAME mass in solar masses. Optional.
    pub mass_sun: Option<f64>,
    /// FLAME radius in solar radii. Optional.
    pub radius_sun: Option<f64>,
    /// FLAME age in gigayears. Optional.
    pub age_gyr: Option<f64>,
}

impl StarRow {
    /// Build a `StarRow` from a decoded parquet `Row`. The row-access
    /// API returns `Result<T>` per column; any mismatch propagates up
    /// as a `GaiaError::Row`.
    fn from_row(row: &Row, index: usize) -> Result<Self, GaiaError> {
        let mut idx = 0usize;
        // Closure to advance the column cursor and tag errors with the row index.
        let get_i64 = |row: &Row, i: usize| -> Result<i64, GaiaError> {
            row.get_long(i)
                .map_err(|source| GaiaError::Row { index, source })
        };
        let get_f64 = |row: &Row, i: usize| -> Result<f64, GaiaError> {
            row.get_double(i)
                .map_err(|source| GaiaError::Row { index, source })
        };
        // Optional f64 via either double or a null. The record reader
        // reports nulls as Field::Null, which `get_double` surfaces as
        // an error; treat that as `None`.
        let get_opt_f64 = |row: &Row, i: usize| -> Option<f64> { row.get_double(i).ok() };

        let source_id = get_i64(row, idx)?;
        idx += 1;
        let ra = get_f64(row, idx)?;
        idx += 1;
        let dec = get_f64(row, idx)?;
        idx += 1;
        let parallax = get_f64(row, idx)?;
        idx += 1;
        let parallax_error = get_f64(row, idx)?;
        idx += 1;
        let teff = get_f64(row, idx)?;
        idx += 1;
        let lum = get_f64(row, idx)?;
        idx += 1;
        let feh = get_opt_f64(row, idx);
        idx += 1;
        let mass = get_opt_f64(row, idx);
        idx += 1;
        let radius = get_opt_f64(row, idx);
        idx += 1;
        let age = get_opt_f64(row, idx);

        // Gaia source_id fits in 63 bits so the cast is lossless.
        #[allow(clippy::cast_sign_loss)]
        let gaia_id = source_id as u64;
        let distance_pc = if parallax > 0.0 {
            1000.0 / parallax
        } else {
            f64::INFINITY
        };

        Ok(Self {
            gaia_id,
            ra_deg: ra,
            dec_deg: dec,
            parallax_mas: parallax,
            parallax_error_mas: parallax_error,
            distance_pc,
            teff_k: teff,
            luminosity_sun: lum,
            feh,
            mass_sun: mass,
            radius_sun: radius,
            age_gyr: age,
        })
    }
}

/// Streaming reader over a Gaia DR3 Parquet catalog file.
///
/// A `GaiaReader` owns the opened parquet file handle and exposes the
/// schema descriptor plus row iterators. Rows are decoded one at a
/// time so the full catalog never lives in memory.
pub struct GaiaReader {
    path: PathBuf,
    file_reader: SerializedFileReader<File>,
    schema: SchemaDescPtr,
}

impl GaiaReader {
    /// Open a Gaia Parquet file. Returns an error if the file is
    /// missing, unreadable, or not a valid Parquet file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, GaiaError> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).map_err(GaiaError::Io)?;
        let file_reader = SerializedFileReader::new(file).map_err(|source| GaiaError::Open {
            path: path.clone(),
            source,
        })?;
        let schema = file_reader.metadata().file_metadata().schema_descr_ptr();
        // Validate that the expected columns are present. The fetch
        // script emits a fixed column order; re-validate here so a
        // drifted schema fails fast with a useful message instead of a
        // mysterious decode error later.
        let expected = [
            "source_id",
            "ra",
            "dec",
            "parallax",
            "parallax_error",
            "teff_gspphot",
            "lum_flame",
        ];
        let root = schema.root_schema();
        let field_names: Vec<&str> = root.get_fields().iter().map(|f| f.name()).collect();
        for col in expected {
            if !field_names.contains(&col) {
                return Err(GaiaError::MissingColumn(col));
            }
        }
        Ok(Self {
            path,
            file_reader,
            schema,
        })
    }

    /// Path the reader was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Parquet schema descriptor for the opened file.
    pub fn schema(&self) -> &SchemaDescPtr {
        &self.schema
    }

    /// Iterate all rows in file order. Each yielded item is a
    /// `Result<StarRow>`; callers can `.filter_map(Result::ok)` to
    /// drop malformed rows or propagate the error with `?`.
    pub fn rows(&self) -> impl Iterator<Item = Result<StarRow, GaiaError>> + '_ {
        // `SerializedFileReader::get_row_iter(None)` returns a
        // `RowIter` that yields `Result<Row, ParquetError>` across all
        // row groups. Map each into our domain type.
        let iter = self
            .file_reader
            .get_row_iter(None)
            .expect("schema validated at open time");
        iter.enumerate().map(|(idx, row_result)| match row_result {
            Ok(row) => StarRow::from_row(&row, idx),
            Err(source) => Err(GaiaError::Row { index: idx, source }),
        })
    }

    /// Iterate only rows whose inverse-parallax distance is at most
    /// `max_pc` parsecs. Thin wrapper over `rows()` that applies an
    /// iterator filter; errors are propagated unchanged.
    pub fn rows_within_pc(
        &self,
        max_pc: f64,
    ) -> impl Iterator<Item = Result<StarRow, GaiaError>> + '_ {
        self.rows().filter(move |r| match r {
            Ok(row) => row.distance_pc <= max_pc,
            Err(_) => true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use std::io::Write;

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/gaia_sample.parquet")
    }

    #[test]
    fn gaia_sample_fixture_round_trips() {
        let reader = GaiaReader::open(fixture_path()).expect("open fixture");
        let rows: Vec<StarRow> = reader
            .rows()
            .collect::<Result<_, _>>()
            .expect("decode rows");
        assert_eq!(rows.len(), 50, "fixture is the nearest-50 slice");

        // Proxima Centauri: Gaia DR3 source_id 5853498713160606720.
        // Parallax ~768 mas, so distance ~1.30 pc. RA ~217.39 deg,
        // Dec ~-62.68 deg. We use the fixture row with the largest
        // parallax and spot-check against these numbers; that sidesteps
        // upstream revisions that could shift individual values slightly.
        let nearest = rows
            .iter()
            .max_by(|a, b| a.parallax_mas.partial_cmp(&b.parallax_mas).unwrap())
            .unwrap();
        assert_relative_eq!(nearest.parallax_mas, 768.0, epsilon = 10.0);
        assert_relative_eq!(
            nearest.distance_pc,
            1000.0 / nearest.parallax_mas,
            epsilon = 1e-9
        );
        assert!(nearest.teff_k > 2500.0 && nearest.teff_k < 3500.0);
    }

    #[test]
    fn distance_filter_narrows_correctly() {
        let reader = GaiaReader::open(fixture_path()).expect("open fixture");
        let all: Vec<StarRow> = reader.rows().collect::<Result<_, _>>().unwrap();
        // Manual expected count: stars within 5 pc.
        let expected: usize = all.iter().filter(|s| s.distance_pc <= 5.0).count();

        let filtered: Vec<StarRow> = reader
            .rows_within_pc(5.0)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(filtered.len(), expected);
        assert!(
            !filtered.is_empty(),
            "the nearest-50 fixture must contain at least one star within 5 pc"
        );
        for star in &filtered {
            assert!(star.distance_pc <= 5.0);
        }
    }

    #[test]
    fn malformed_parquet_fails_cleanly() {
        // Non-existent path → Io error.
        let missing = GaiaReader::open("/nonexistent/path/for/gaia/test.parquet");
        assert!(matches!(missing, Err(GaiaError::Io(_))));

        // Garbage bytes that aren't Parquet → Open error with path context.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        {
            let mut f = tmp.reopen().unwrap();
            f.write_all(b"not a parquet file, just some bytes").unwrap();
        }
        let bogus = GaiaReader::open(tmp.path());
        match bogus {
            Err(GaiaError::Open { path, source: _ }) => {
                assert_eq!(path, tmp.path());
            }
            Err(other) => panic!("expected GaiaError::Open, got {other:?}"),
            Ok(_) => panic!("expected GaiaError::Open, got Ok(_)"),
        }
    }
}
