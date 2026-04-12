//! World file manifest: version tracking, checksums, and metadata for saved worlds.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

/// Pipeline configuration parameters used during world generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenerationConfig {
    /// Geodesic grid subdivision level (higher = more detail).
    pub grid_subdivision_level: u32,
    /// Whether biological processes (O2 production, vegetation) are modeled.
    pub enable_biology: bool,
    /// Override for continental land fraction. None means the value is derived
    /// from physics priors during the skeleton stage.
    pub continental_fraction: Option<f64>,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            grid_subdivision_level: 5,
            enable_biology: true,
            continental_fraction: None,
        }
    }
}

/// JSON-serializable metadata for a generated world, stored as `manifest.json`
/// inside the world directory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldManifest {
    /// Manifest format version (e.g., "1.0").
    pub version: String,
    /// Version of the ymir pipeline that produced this world.
    pub pipeline_version: String,
    /// Name of the host star (e.g., "Tau Ceti").
    pub star_name: String,
    /// Catalog identifier for the star, if available.
    pub star_catalog_id: Option<String>,
    /// Zero-based index of the planet within the system.
    pub planet_index: usize,
    /// Human-readable planet name, if assigned.
    pub planet_name: Option<String>,
    /// PRNG seed used for generation.
    pub seed: u64,
    /// ISO 8601 timestamp of when the world was generated.
    pub created_at: String,
    /// Path to the overrides JSON file, if one was used.
    pub overrides_file: Option<String>,
    /// Names of pipeline stages that were computed. Valid values:
    /// `"stellar" | "system" | "atmosphere" | "skeleton" | "climate" | "biomes"`.
    /// Stored as free-form strings rather than an enum so the manifest format
    /// stays forward-compatible as later phases add stages.
    pub stages_computed: Vec<String>,
    /// Generation configuration parameters.
    pub config: GenerationConfig,
}

impl WorldManifest {
    /// Write this manifest as pretty-printed JSON to `path`.
    pub fn save(&self, path: &Path) -> Result<(), io::Error> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, json)
    }

    /// Read a manifest from a JSON file at `path`.
    pub fn load(path: &Path) -> Result<Self, io::Error> {
        let data = fs::read_to_string(path)?;
        serde_json::from_str(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_manifest() -> WorldManifest {
        WorldManifest {
            version: "1.0".to_string(),
            pipeline_version: "0.1.0".to_string(),
            star_name: "Tau Ceti".to_string(),
            star_catalog_id: Some("HIP 8102".to_string()),
            planet_index: 1,
            planet_name: Some("Tau Ceti e".to_string()),
            seed: 42,
            created_at: "2026-04-11T12:00:00Z".to_string(),
            overrides_file: None,
            stages_computed: vec![
                "stellar".to_string(),
                "system".to_string(),
                "atmosphere".to_string(),
            ],
            config: GenerationConfig::default(),
        }
    }

    #[test]
    fn generation_config_defaults() {
        let cfg = GenerationConfig::default();
        assert_eq!(cfg.grid_subdivision_level, 5);
        assert!(cfg.enable_biology);
        assert!(cfg.continental_fraction.is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("manifest.json");

        let original = sample_manifest();
        original.save(&path).expect("save failed");
        let loaded = WorldManifest::load(&path).expect("load failed");

        assert_eq!(original, loaded);
    }

    #[test]
    fn json_roundtrip_exact() {
        let manifest = sample_manifest();
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let deserialized: WorldManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, deserialized);
    }

    #[test]
    fn load_nonexistent_file_errors() {
        let result = WorldManifest::load(&PathBuf::from("/tmp/does_not_exist_ymir.json"));
        assert!(result.is_err());
    }
}
