//! Global surface generation: geodesic grids, tectonics, and macro-scale heightmaps.
//!
//! Builds the planetary surface skeleton using geodesic sphere subdivision,
//! tectonic plate simulation, and coherent noise for terrain generation.

pub mod geodesic;
pub mod heightmap;
pub mod hex;
pub mod noise;
pub mod skeleton;
pub mod tectonics;

pub use heightmap::{ElevationMap, HeightmapConfig, generate_heightmap};
pub use noise::SphericalFbm;
pub use skeleton::{PlateType, SkeletonTile, SkeletonWorld};
pub use tectonics::{
    BoundaryType, Plate, PlateBoundary, TectonicConfig, TectonicData, generate_tectonics,
};
