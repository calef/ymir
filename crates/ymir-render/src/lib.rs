//! 2D map rendering: globe projections, detail views, and overlay compositing.
//!
//! Produces PNG images of planetary surfaces using software rendering. Supports
//! multiple map projections and compositable data overlays (elevation, climate,
//! biome, political boundaries).

pub mod color_maps;
pub mod detail_renderer;
pub mod globe_renderer;
pub mod overlays;
pub mod projections;

pub use color_maps::elevation_to_rgb;
pub use globe_renderer::{
    GlobeRenderConfig, render_skeleton_mollweide, render_skeleton_mollweide_to_path,
};
pub use projections::{MapProjection, Mollweide};
