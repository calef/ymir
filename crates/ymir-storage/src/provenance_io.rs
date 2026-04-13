//! Read and write helpers for `provenance.json`.

use std::fs;
use std::io;
use std::path::Path;

use ymir_core::ProvenanceReport;

/// Write a [`ProvenanceReport`] to a JSON file at `path`.
///
/// The output is pretty-printed so it is human-readable in a text editor or
/// `jq` pipeline.
///
/// # Errors
///
/// Returns [`io::Error`] if the file cannot be created or written, or if
/// serialization fails (the latter is wrapped in `io::ErrorKind::Other`).
pub fn save_provenance(path: &Path, report: &ProvenanceReport) -> Result<(), io::Error> {
    let json = serde_json::to_string_pretty(report)
        .map_err(|e| io::Error::other(format!("failed to serialize provenance: {e}")))?;
    fs::write(path, json)
}

/// Load a [`ProvenanceReport`] from a JSON file at `path`.
///
/// # Errors
///
/// Returns [`io::Error`] if the file cannot be read or if the JSON cannot be
/// deserialized into a [`ProvenanceReport`].
pub fn load_provenance(path: &Path) -> Result<ProvenanceReport, io::Error> {
    let contents = fs::read_to_string(path)?;
    serde_json::from_str(&contents).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse provenance.json: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ymir_core::Source;

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("provenance.json");

        let mut report = ProvenanceReport::new();
        let source = Source::Derived {
            from_stage: "skeleton".into(),
        };
        report.add_aggregate_stage("skeleton", &source);

        save_provenance(&path, &report).expect("save");
        let loaded = load_provenance(&path).expect("load");

        assert_eq!(loaded.stages.get("skeleton").unwrap().counts.derived, 1);
    }
}
