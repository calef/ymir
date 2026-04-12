//! Read and write operations for complete world state files.
//!
//! Manages the directory structure for a generated world, including paths to
//! the manifest, binary data files, and on-demand detail chunks.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::manifest::WorldManifest;

/// Manages the directory layout for a single generated world.
///
/// The expected structure mirrors the design doc (section 6.2):
/// ```text
/// <root>/
///   manifest.json
///   overrides.json
///   skeleton.bin
///   climate.bin
///   biomes.bin
///   detail/
/// ```
#[derive(Clone, Debug)]
pub struct WorldDirectory {
    /// Root path of the world directory.
    pub root: PathBuf,
}

impl WorldDirectory {
    /// Create the world directory and its required subdirectories.
    /// Returns an error if the directories cannot be created.
    pub fn create(root: &Path) -> Result<Self, io::Error> {
        fs::create_dir_all(root)?;
        fs::create_dir_all(root.join("detail"))?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Path to `manifest.json`.
    pub fn manifest_path(&self) -> PathBuf {
        self.root.join("manifest.json")
    }

    /// Path to `skeleton.bin`.
    pub fn skeleton_path(&self) -> PathBuf {
        self.root.join("skeleton.bin")
    }

    /// Path to `climate.bin`.
    pub fn climate_path(&self) -> PathBuf {
        self.root.join("climate.bin")
    }

    /// Path to `biomes.bin`.
    pub fn biomes_path(&self) -> PathBuf {
        self.root.join("biomes.bin")
    }

    /// Path to `overrides.json`.
    pub fn overrides_path(&self) -> PathBuf {
        self.root.join("overrides.json")
    }

    /// Path to the `detail/` subdirectory for on-demand region chunks.
    pub fn detail_dir(&self) -> PathBuf {
        self.root.join("detail")
    }

    /// Write a manifest to `manifest.json` inside this world directory.
    pub fn save_manifest(&self, manifest: &WorldManifest) -> Result<(), io::Error> {
        manifest.save(&self.manifest_path())
    }

    /// Load the manifest from `manifest.json` inside this world directory.
    pub fn load_manifest(&self) -> Result<WorldManifest, io::Error> {
        WorldManifest::load(&self.manifest_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::GenerationConfig;

    fn sample_manifest() -> WorldManifest {
        WorldManifest {
            version: "1.0".to_string(),
            pipeline_version: "0.1.0".to_string(),
            star_name: "Proxima Centauri".to_string(),
            star_catalog_id: Some("HIP 70890".to_string()),
            planet_index: 0,
            planet_name: Some("Proxima b".to_string()),
            seed: 99,
            created_at: "2026-04-11T08:30:00Z".to_string(),
            overrides_file: None,
            stages_computed: vec!["stellar".to_string(), "system".to_string()],
            config: GenerationConfig {
                grid_subdivision_level: 4,
                enable_biology: false,
                continental_fraction: Some(0.3),
            },
        }
    }

    #[test]
    fn create_builds_directory_structure() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let world_root = tmp.path().join("test_world");

        let wd = WorldDirectory::create(&world_root).expect("create failed");

        assert!(wd.root.is_dir());
        assert!(wd.detail_dir().is_dir());
    }

    #[test]
    fn path_helpers_return_expected_names() {
        let wd = WorldDirectory {
            root: PathBuf::from("/tmp/fake_world"),
        };

        assert_eq!(
            wd.manifest_path(),
            PathBuf::from("/tmp/fake_world/manifest.json")
        );
        assert_eq!(
            wd.skeleton_path(),
            PathBuf::from("/tmp/fake_world/skeleton.bin")
        );
        assert_eq!(
            wd.climate_path(),
            PathBuf::from("/tmp/fake_world/climate.bin")
        );
        assert_eq!(
            wd.biomes_path(),
            PathBuf::from("/tmp/fake_world/biomes.bin")
        );
        assert_eq!(
            wd.overrides_path(),
            PathBuf::from("/tmp/fake_world/overrides.json")
        );
        assert_eq!(wd.detail_dir(), PathBuf::from("/tmp/fake_world/detail"));
    }

    #[test]
    fn save_and_load_manifest_through_directory() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let wd = WorldDirectory::create(tmp.path().join("world").as_path()).expect("create failed");

        let manifest = sample_manifest();
        wd.save_manifest(&manifest).expect("save failed");
        let loaded = wd.load_manifest().expect("load failed");

        assert_eq!(manifest, loaded);
    }
}
