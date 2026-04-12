//! Detail-level terrain generation: rivers, local features, and chunk-based refinement.
//!
//! Refines the global skeleton into high-resolution terrain chunks with rivers,
//! erosion features, and biome-appropriate surface detail.

pub mod detail_chunk;
pub mod detail_grid;
pub mod detail_pipeline;
pub mod rivers;
