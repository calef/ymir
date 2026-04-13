//! 2D map rendering: globe projections, detail views, and overlay compositing.
//!
//! Produces PNG images of planetary surfaces using software rendering. Supports
//! multiple map projections and compositable data overlays (elevation, climate,
//! biome, political boundaries).

pub mod biome_mollweide;
pub mod biome_palette;
pub mod color_maps;
pub mod detail_renderer;
pub mod globe_renderer;
pub mod overlays;
pub mod projections;
pub mod regional;

pub use biome_mollweide::{BiomeRenderConfig, render_biome_mollweide};
pub use biome_palette::biome_color;
pub use color_maps::elevation_to_rgb;
pub use globe_renderer::{
    GlobeRenderConfig, render_skeleton_mollweide, render_skeleton_mollweide_to_path,
};
pub use overlays::{
    BIOME_OFF_MAP_BG, ConfidenceLevel, ELEVATION_OFF_MAP_BG, apply_confidence_overlay,
    confidence_level_from_report, desaturate_pixel, render_confidence_from_report,
    render_confidence_overlay, saturation_scale,
};
pub use projections::{MapProjection, Mollweide};
pub use regional::{RegionProjection, RegionalRenderConfig, render_regional_detail};
