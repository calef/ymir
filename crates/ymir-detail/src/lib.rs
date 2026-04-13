//! Detail-level terrain generation: rivers, local features, and chunk-based refinement.
//!
//! Refines the global skeleton into high-resolution terrain chunks with rivers,
//! erosion features, and biome-appropriate surface detail.

pub mod biomes;
pub mod detail_chunk;
pub mod detail_grid;
pub mod detail_pipeline;
pub mod elevation;
pub mod flow;
pub mod hex_grid;
pub mod moisture;
pub mod region;
pub mod regional;
pub mod rivers;

pub use biomes::{DetailBiomeConfig, DetailBiomes};
pub use elevation::{DetailElevation, DetailElevationConfig};
pub use flow::{DetailFlow, DetailFlowConfig};
pub use hex_grid::{HexCell, HexGrid, HexGridConfig};
pub use moisture::{DetailMoisture, DetailMoistureConfig};
pub use region::{RegionSpec, detail_rng};
pub use regional::{RegionalDetail, RegionalDetailConfig};
