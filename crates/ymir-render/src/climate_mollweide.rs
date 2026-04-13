//! Climate-layer Mollweide rendering: temperature and moisture colormaps.
//!
//! Each function follows the same nearest-tile projection pipeline used by
//! [`crate::globe_renderer`] and [`crate::biome_mollweide`]: for every output
//! pixel, invert the Mollweide projection to (lat, lon), convert to a unit
//! vector, find the nearest tile by dot product, and map that tile's climate
//! value to an RGB colour.

use crate::color_maps::{moisture_to_rgb, temperature_to_rgb};
use crate::projections::Mollweide;
use image::RgbImage;
use ymir_climate::ClimateMap;
use ymir_surface::skeleton::SkeletonWorld;

/// Fixed off-map background for climate renders. Matches [`crate::overlays::BIOME_OFF_MAP_BG`].
pub const CLIMATE_OFF_MAP_BG: [u8; 3] = [10, 10, 12];

/// Configuration for a climate-layer Mollweide render.
#[derive(Debug, Clone)]
pub struct ClimateRenderConfig {
    /// Output image width in pixels.
    pub width: u32,
    /// Output image height in pixels.
    pub height: u32,
}

impl Default for ClimateRenderConfig {
    fn default() -> Self {
        Self {
            width: 2048,
            height: 1024,
        }
    }
}

/// Render a surface temperature map using the thermal colour ramp from
/// [`crate::color_maps::temperature_to_rgb`].
///
/// # Panics
///
/// Panics if `climate.temperature.per_tile_k.len()` does not equal
/// `world.grid.tiles.len()`.
pub fn render_temperature_mollweide(
    world: &SkeletonWorld,
    climate: &ClimateMap,
    cfg: &ClimateRenderConfig,
) -> RgbImage {
    assert_eq!(
        climate.temperature.per_tile_k.len(),
        world.grid.tiles.len(),
        "temperature field length must match grid tile count"
    );

    render_scalar_mollweide(world, cfg, |idx| {
        temperature_to_rgb(climate.temperature.per_tile_k[idx])
    })
}

/// Render a surface moisture map using the green-blue colour ramp from
/// [`crate::color_maps::moisture_to_rgb`].
///
/// # Panics
///
/// Panics if `climate.moisture.per_tile.len()` does not equal
/// `world.grid.tiles.len()`.
pub fn render_moisture_mollweide(
    world: &SkeletonWorld,
    climate: &ClimateMap,
    cfg: &ClimateRenderConfig,
) -> RgbImage {
    assert_eq!(
        climate.moisture.per_tile.len(),
        world.grid.tiles.len(),
        "moisture field length must match grid tile count"
    );

    render_scalar_mollweide(world, cfg, |idx| {
        moisture_to_rgb(climate.moisture.per_tile[idx])
    })
}

/// Shared Mollweide rasteriser. For every output pixel, inverts the projection
/// to (lat, lon), finds the nearest tile, and calls `color_fn` to obtain the
/// pixel color. Off-map pixels are set to [`CLIMATE_OFF_MAP_BG`].
fn render_scalar_mollweide<F>(
    world: &SkeletonWorld,
    cfg: &ClimateRenderConfig,
    color_fn: F,
) -> RgbImage
where
    F: Fn(usize) -> [u8; 3],
{
    let width = cfg.width.max(1);
    let height = cfg.height.max(1);

    let centers: Vec<[f64; 3]> = world.grid.tiles.iter().map(|t| t.center).collect();
    let mut img = RgbImage::from_pixel(width, height, image::Rgb(CLIMATE_OFF_MAP_BG));

    let inv_w = 1.0 / width as f64;
    let inv_h = 1.0 / height as f64;

    for py in 0..height {
        let ny = 1.0 - ((py as f64 + 0.5) * inv_h) * 2.0;
        for px in 0..width {
            let nx = ((px as f64 + 0.5) * inv_w) * 2.0 - 1.0;

            let Some((lat, lon)) = Mollweide::inverse(nx, ny) else {
                continue;
            };

            let cos_lat = lat.cos();
            let qx = cos_lat * lon.cos();
            let qy = cos_lat * lon.sin();
            let qz = lat.sin();

            let mut best_idx = 0usize;
            let mut best_dot = f64::NEG_INFINITY;
            for (i, c) in centers.iter().enumerate() {
                let d = c[0] * qx + c[1] * qy + c[2] * qz;
                if d > best_dot {
                    best_dot = d;
                    best_idx = i;
                }
            }

            let rgb = color_fn(best_idx);
            img.put_pixel(px, py, image::Rgb(rgb));
        }
    }

    img
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use ymir_atmosphere::atmosphere_model::AtmosphereModel;
    use ymir_atmosphere::composition::AtmosphereClass;
    use ymir_atmosphere::retention::Gas;
    use ymir_climate::{ClimateConfig, ClimateMap};
    use ymir_core::Sourced;
    use ymir_system::orbital_body::{OrbitalBody, PlanetType};

    fn d(v: f64) -> Sourced<f64> {
        Sourced::derived(v, "test")
    }

    fn earth_body() -> OrbitalBody {
        OrbitalBody {
            semi_major_axis: d(1.0),
            eccentricity: d(0.0167),
            inclination: d(0.0),
            axial_tilt: d(23.4),
            mass: d(1.0),
            radius: d(1.0),
            density: d(5.51),
            surface_gravity: d(9.81),
            solar_irradiance: d(1361.0),
            equilibrium_temp: d(254.0),
            tidal_locked: Sourced::derived(false, "test"),
            rotation_period: d(24.0),
            is_in_hz: true,
            planet_type: PlanetType::Terran,
            name: Some("Earth".into()),
            is_known_exoplanet: false,
            continental_fraction: None,
        }
    }

    fn earth_atmosphere() -> AtmosphereModel {
        let mut composition = BTreeMap::new();
        composition.insert(Gas::N2, 0.78);
        composition.insert(Gas::O2, 0.21);
        composition.insert(Gas::H2O, 0.01);
        AtmosphereModel {
            surface_pressure: d(1.0),
            composition: Sourced::derived(composition, "test"),
            greenhouse_factor: d(288.0 / 254.0),
            effective_surface_temp: d(288.0),
            scale_height: d(8.0),
            moisture_capacity: d(1.0),
            uv_surface_flux: d(0.05),
            class: AtmosphereClass::NitrogenOxygen,
            retained: vec![Gas::N2, Gas::O2, Gas::H2O],
        }
    }

    fn small_world() -> SkeletonWorld {
        let atmo = earth_atmosphere();
        SkeletonWorld::build(earth_body(), atmo, 3, 2024)
    }

    fn small_climate(world: &SkeletonWorld) -> ClimateMap {
        let cfg = ClimateConfig::default();
        ClimateMap::build(world, &cfg)
    }

    fn small_cfg() -> ClimateRenderConfig {
        ClimateRenderConfig {
            width: 64,
            height: 32,
        }
    }

    // --- temperature ---

    #[test]
    fn temperature_output_dimensions_match() {
        let world = small_world();
        let climate = small_climate(&world);
        let img = render_temperature_mollweide(&world, &climate, &small_cfg());
        assert_eq!(img.width(), 64);
        assert_eq!(img.height(), 32);
    }

    #[test]
    fn temperature_image_is_non_empty() {
        let world = small_world();
        let climate = small_climate(&world);
        let img = render_temperature_mollweide(&world, &climate, &small_cfg());
        // At least one pixel should differ from the off-map background.
        let has_colored = img.pixels().any(|p| p.0 != CLIMATE_OFF_MAP_BG);
        assert!(
            has_colored,
            "temperature render produced only background pixels"
        );
    }

    #[test]
    fn temperature_corners_are_background() {
        let world = small_world();
        let climate = small_climate(&world);
        let cfg = small_cfg();
        let img = render_temperature_mollweide(&world, &climate, &cfg);
        assert_eq!(img.get_pixel(0, 0).0, CLIMATE_OFF_MAP_BG);
        assert_eq!(img.get_pixel(cfg.width - 1, 0).0, CLIMATE_OFF_MAP_BG);
        assert_eq!(img.get_pixel(0, cfg.height - 1).0, CLIMATE_OFF_MAP_BG);
        assert_eq!(
            img.get_pixel(cfg.width - 1, cfg.height - 1).0,
            CLIMATE_OFF_MAP_BG
        );
    }

    #[test]
    fn temperature_render_is_deterministic() {
        let world = small_world();
        let climate = small_climate(&world);
        let cfg = small_cfg();
        let a = render_temperature_mollweide(&world, &climate, &cfg);
        let b = render_temperature_mollweide(&world, &climate, &cfg);
        assert_eq!(a.as_raw(), b.as_raw());
    }

    #[test]
    fn temperature_color_distribution_reasonable() {
        // The rendered image should contain more than one distinct color
        // (i.e. there is actual temperature variation across the surface).
        let world = small_world();
        let climate = small_climate(&world);
        let img = render_temperature_mollweide(&world, &climate, &small_cfg());

        let distinct: std::collections::HashSet<[u8; 3]> = img
            .pixels()
            .filter(|p| p.0 != CLIMATE_OFF_MAP_BG)
            .map(|p| p.0)
            .collect();
        assert!(
            distinct.len() >= 2,
            "expected temperature variation; got {} distinct colors",
            distinct.len()
        );
    }

    // --- moisture ---

    #[test]
    fn moisture_output_dimensions_match() {
        let world = small_world();
        let climate = small_climate(&world);
        let img = render_moisture_mollweide(&world, &climate, &small_cfg());
        assert_eq!(img.width(), 64);
        assert_eq!(img.height(), 32);
    }

    #[test]
    fn moisture_image_is_non_empty() {
        let world = small_world();
        let climate = small_climate(&world);
        let img = render_moisture_mollweide(&world, &climate, &small_cfg());
        let has_colored = img.pixels().any(|p| p.0 != CLIMATE_OFF_MAP_BG);
        assert!(
            has_colored,
            "moisture render produced only background pixels"
        );
    }

    #[test]
    fn moisture_corners_are_background() {
        let world = small_world();
        let climate = small_climate(&world);
        let cfg = small_cfg();
        let img = render_moisture_mollweide(&world, &climate, &cfg);
        assert_eq!(img.get_pixel(0, 0).0, CLIMATE_OFF_MAP_BG);
        assert_eq!(img.get_pixel(cfg.width - 1, 0).0, CLIMATE_OFF_MAP_BG);
        assert_eq!(img.get_pixel(0, cfg.height - 1).0, CLIMATE_OFF_MAP_BG);
        assert_eq!(
            img.get_pixel(cfg.width - 1, cfg.height - 1).0,
            CLIMATE_OFF_MAP_BG
        );
    }

    #[test]
    fn moisture_render_is_deterministic() {
        let world = small_world();
        let climate = small_climate(&world);
        let cfg = small_cfg();
        let a = render_moisture_mollweide(&world, &climate, &cfg);
        let b = render_moisture_mollweide(&world, &climate, &cfg);
        assert_eq!(a.as_raw(), b.as_raw());
    }

    #[test]
    fn moisture_color_distribution_reasonable() {
        let world = small_world();
        let climate = small_climate(&world);
        let img = render_moisture_mollweide(&world, &climate, &small_cfg());

        let distinct: std::collections::HashSet<[u8; 3]> = img
            .pixels()
            .filter(|p| p.0 != CLIMATE_OFF_MAP_BG)
            .map(|p| p.0)
            .collect();
        assert!(
            distinct.len() >= 2,
            "expected moisture variation; got {} distinct colors",
            distinct.len()
        );
    }
}
