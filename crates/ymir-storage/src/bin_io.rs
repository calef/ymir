//! Generic bincode save/load helpers for pipeline artifacts.
//!
//! `ymir-storage` cannot depend on sibling crates like `ymir-climate` or
//! `ymir-biome` (see the crate dependency graph in the workspace docs), so
//! the concrete `climate.bin` / `biomes.bin` writers live in the binary
//! crate. These generic helpers centralize the buffered-I/O + bincode error
//! translation so callers don't re-implement them.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::File;
use std::io;
use std::io::{BufReader, BufWriter};
use std::path::Path;

/// Serialize any serde-compatible value to a bincode file using a buffered
/// writer.
///
/// Errors are mapped onto [`io::Error`] so callers only need to handle a
/// single error type. Bincode-level failures surface as
/// [`io::ErrorKind::InvalidData`].
pub fn save_bin<T: Serialize, P: AsRef<Path>>(path: P, value: &T) -> Result<(), io::Error> {
    let file = File::create(path.as_ref())?;
    let writer = BufWriter::new(file);
    bincode::serialize_into(writer, value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Load any serde-compatible value from a bincode file using a buffered
/// reader.
///
/// Errors are mapped onto [`io::Error`]. Missing files propagate as
/// [`io::ErrorKind::NotFound`]; malformed bincode contents surface as
/// [`io::ErrorKind::InvalidData`].
pub fn load_bin<T: DeserializeOwned, P: AsRef<Path>>(path: P) -> Result<T, io::Error> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);
    bincode::deserialize_from(reader).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::path::PathBuf;

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        name: String,
        values: Vec<f64>,
        flag: bool,
    }

    fn sample() -> Sample {
        Sample {
            name: "ymir".to_string(),
            values: vec![1.0, 2.5, -3.25, 0.0],
            flag: true,
        }
    }

    #[test]
    fn round_trips_simple_struct() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sample.bin");

        let original = sample();
        save_bin(&path, &original).expect("save");
        let loaded: Sample = load_bin(&path).expect("load");
        assert_eq!(original, loaded);
    }

    #[test]
    fn round_trips_plain_vec() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("vec.bin");

        let original: Vec<u32> = vec![7, 11, 13, 17, 19];
        save_bin(&path, &original).expect("save");
        let loaded: Vec<u32> = load_bin(&path).expect("load");
        assert_eq!(original, loaded);
    }

    #[test]
    fn load_missing_file_errors() {
        let path = PathBuf::from("/tmp/ymir_does_not_exist_bin_io.bin");
        let result: Result<Sample, _> = load_bin(&path);
        let err = result.expect_err("should fail");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn load_truncated_fixed_size_file_errors() {
        // A 4-byte u32 bincode value requires exactly 4 bytes; writing 2 bytes
        // forces a short read and maps to InvalidData. We avoid testing with
        // arbitrary malformed input for dynamically-sized types because
        // bincode trusts length prefixes and will try to allocate before
        // failing, which would abort the test process rather than returning
        // an error.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("short.bin");
        std::fs::write(&path, b"\x00\x00").expect("write");

        let result: Result<u32, _> = load_bin(&path);
        let err = result.expect_err("should fail");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
