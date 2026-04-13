//! World persistence: save/load world files with manifest tracking.
//!
//! Handles serialization of complete world state to disk using both JSON
//! (human-readable) and bincode (compact binary) formats, with a manifest
//! file for versioning and integrity checking.

pub mod bin_io;
pub mod manifest;
pub mod provenance_io;
pub mod world_io;

pub use bin_io::{load_bin, save_bin};
pub use provenance_io::{load_provenance, save_provenance};
pub use world_io::{WorldDirectory, region_file_name};
