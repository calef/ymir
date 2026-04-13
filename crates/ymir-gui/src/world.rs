//! Loaded-world handle and error reporting for the GUI.
//!
//! Phase-4 GUI operates on pre-generated worlds written by the `ymir` CLI.
//! A [`LoadedWorld`] holds the handle the panels read from; [`WorldLoadError`]
//! is the unified error surface used by File → Open World.
//!
//! The scaffold stores only the directory path; downstream tasks (GUI-02/03/04)
//! extend this struct with parsed catalogs, system data, and surface grids
//! as those crates expose their loaders.

use std::path::{Path, PathBuf};

/// Handle to a world directory that has been opened by the GUI.
///
/// The scaffold records only the directory. Downstream tasks add fields for
/// the parsed catalog, system, surface, climate, biome, and provenance blobs
/// as each crate exposes its reader. Do not eagerly load everything — lazy
/// loading per-panel keeps the UI responsive for 500k-star catalogs.
#[derive(Debug, Clone)]
pub struct LoadedWorld {
    path: PathBuf,
}

impl LoadedWorld {
    /// Constructs a [`LoadedWorld`] from a directory path.
    ///
    /// The scaffold does not validate the directory's contents; GUI-02 will
    /// introduce actual catalog-loading checks. Returns [`WorldLoadError`]
    /// for non-existent paths so the menu wiring is already in the right
    /// shape.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, WorldLoadError> {
        let path = path.into();
        if !path.exists() {
            return Err(WorldLoadError::NotFound(path));
        }
        if !path.is_dir() {
            return Err(WorldLoadError::NotADirectory(path));
        }
        Ok(Self { path })
    }

    /// Returns the on-disk world directory.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Errors surfaced by the File → Open World flow.
#[derive(Debug)]
pub enum WorldLoadError {
    /// Path does not exist on disk.
    NotFound(PathBuf),

    /// Path exists but is not a directory.
    NotADirectory(PathBuf),
}

impl std::fmt::Display for WorldLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(p) => write!(f, "world directory does not exist: {}", p.display()),
            Self::NotADirectory(p) => write!(f, "path is not a directory: {}", p.display()),
        }
    }
}

impl std::error::Error for WorldLoadError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_missing_path_reports_not_found() {
        let err = LoadedWorld::open("/definitely/not/a/path/ymir-test").unwrap_err();
        assert!(matches!(err, WorldLoadError::NotFound(_)));
    }

    #[test]
    fn open_non_directory_reports_not_a_directory() {
        // Any existing file will do; use the current executable if present,
        // otherwise skip gracefully.
        if let Ok(exe) = std::env::current_exe() {
            let err = LoadedWorld::open(&exe).unwrap_err();
            assert!(matches!(err, WorldLoadError::NotADirectory(_)));
        }
    }

    #[test]
    fn open_existing_dir_succeeds() {
        let tmp = std::env::temp_dir();
        let world = LoadedWorld::open(&tmp).unwrap();
        assert_eq!(world.path(), tmp.as_path());
    }
}
