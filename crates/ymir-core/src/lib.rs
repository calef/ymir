//! Core types, traits, and provenance tracking for the Ymir pipeline.
//!
//! This crate defines the foundational types shared across all pipeline stages,
//! including the `Sourced<T>` wrapper for provenance tracking, override file
//! parsing, dependency graph resolution, and deterministic PRNG management.

pub mod dependency_graph;
pub mod override_file;
pub mod prng;
pub mod provenance;
pub mod sourced;
pub mod traits;

pub use dependency_graph::{PipelineDirtyState, Stage};
pub use override_file::{OverrideError, OverrideFile, StageOverrides};
pub use prng::WorldRng;
pub use sourced::{Source, Sourced};
pub use traits::{GeoTile, PipelineStage, SphericalPoint};
