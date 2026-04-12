//! Biome-colored Mollweide rendering of a [`SkeletonWorld`] against a
//! [`BiomeMap`].
//!
//! Mirrors [`crate::globe_renderer::render_skeleton_mollweide`] but colors
//! each pixel by the nearest tile's biome (through [`biome_color`]) rather
//! than by elevation. The projection pipeline, nearest-tile search, and
//! off-map handling are intentionally identical.

use crate::biome_palette::biome_color;
use crate::projections::Mollweide;
use image::RgbImage;
use ymir_biome::BiomeMap;
use ymir_surface::skeleton::SkeletonWorld;

/// Configuration for a biome-colored Mollweide render.
///
/// Kept intentionally narrow; the off-map background is a fixed deep gray
/// (distinct from any biome color) so the renderer stays a single-knob entry
/// point for the Phase 1 CLI.
#[derive(Debug, Clone)]
pub struct BiomeRenderConfig {
    /// Output image width in pixels.
    pub width: u32,
    /// Output image height in pixels.
    pub height: u32,
}

impl Default for BiomeRenderConfig {
    fn default() -> Self {
        Self {
            width: 2048,
            height: 1024,
        }
    }
}

/// Fixed off-map background used for pixels that fall outside the Mollweide
/// ellipse. Deep near-black so the projected disk pops visually and no biome
/// color collides with it.
const OFF_MAP_BG: [u8; 3] = [10, 10, 12];

/// Render a [`SkeletonWorld`] / [`BiomeMap`] pair to a Mollweide-projected
/// RGB image, coloring each tile by its biome.
///
/// The algorithm matches the elevation Mollweide renderer: for every output
/// pixel invert the projection to (lat, lon), convert to a unit vector, and
/// pick the tile whose center maximizes the dot product. The picked tile's
/// biome drives the pixel color via [`biome_color`].
///
/// # Panics
///
/// Panics if `biomes.per_tile.len()` does not equal
/// `world.grid.tiles.len()`; both are produced together in the normal
/// pipeline, so a mismatch is a programmer error.
pub fn render_biome_mollweide(
    world: &SkeletonWorld,
    biomes: &BiomeMap,
    cfg: &BiomeRenderConfig,
) -> RgbImage {
    let width = cfg.width.max(1);
    let height = cfg.height.max(1);

    assert_eq!(
        biomes.per_tile.len(),
        world.grid.tiles.len(),
        "biome map length must match grid tile count"
    );

    // Flat arrays for the inner loop.
    let centers: Vec<[f64; 3]> = world.grid.tiles.iter().map(|t| t.center).collect();
    let per_tile = &biomes.per_tile;

    let mut img = RgbImage::from_pixel(width, height, image::Rgb(OFF_MAP_BG));

    let inv_w = 1.0 / width as f64;
    let inv_h = 1.0 / height as f64;

    for py in 0..height {
        let ny = 1.0 - ((py as f64 + 0.5) * inv_h) * 2.0;
        for px in 0..width {
            let nx = ((px as f64 + 0.5) * inv_w) * 2.0 - 1.0;

            let Some((lat, lon)) = Mollweide::inverse(nx, ny) else {
                continue;
            };

            // (lat, lon) radians -> unit 3D vector, matching
            // ymir-surface::geodesic::latlon_to_xyz.
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

            let rgb = biome_color(per_tile[best_idx]);
            img.put_pixel(px, py, image::Rgb(rgb));
        }
    }

    img
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use ymir_atmosphere::atmosphere_model::AtmosphereModel;
    use ymir_atmosphere::composition::AtmosphereClass;
    use ymir_atmosphere::retention::Gas;
    use ymir_biome::{Biome, BiomePalette};
    use ymir_system::orbital_body::{OrbitalBody, PlanetType};

    fn earth_body() -> OrbitalBody {
        OrbitalBody {
            semi_major_axis: 1.0,
            eccentricity: 0.0167,
            inclination: 0.0,
            axial_tilt: 23.4,
            mass: 1.0,
            radius: 1.0,
            density: 5.51,
            surface_gravity: 9.81,
            solar_irradiance: 1361.0,
            equilibrium_temp: 254.0,
            tidal_locked: false,
            rotation_period: 24.0,
            is_in_hz: true,
            planet_type: PlanetType::Terran,
            name: Some("Earth".into()),
            is_known_exoplanet: false,
        }
    }

    fn earth_atmosphere() -> AtmosphereModel {
        let mut composition = BTreeMap::new();
        composition.insert(Gas::N2, 0.78);
        composition.insert(Gas::O2, 0.21);
        composition.insert(Gas::H2O, 0.01);
        AtmosphereModel {
            surface_pressure: 1.0,
            composition,
            greenhouse_factor: 288.0 / 254.0,
            effective_surface_temp: 288.0,
            scale_height: 8.0,
            moisture_capacity: 1.0,
            uv_surface_flux: 0.05,
            class: AtmosphereClass::NitrogenOxygen,
            retained: vec![Gas::N2, Gas::O2, Gas::H2O],
        }
    }

    /// Build a small skeleton world with the given atmosphere.
    fn small_world(atmo: AtmosphereModel) -> SkeletonWorld {
        SkeletonWorld::build(earth_body(), atmo, 3, 2024)
    }

    /// Construct a synthetic `BiomeMap` that cycles through the palette's
    /// biomes deterministically, covering every tile.
    fn cycling_biome_map(world: &SkeletonWorld, palette: BiomePalette) -> BiomeMap {
        let variants = palette.biomes();
        let per_tile: Vec<Biome> = (0..world.grid.tiles.len())
            .map(|i| variants[i % variants.len()])
            .collect();
        BiomeMap { per_tile, palette }
    }

    /// Construct a uniform `BiomeMap` where every tile gets `biome`.
    fn uniform_biome_map(world: &SkeletonWorld, palette: BiomePalette, biome: Biome) -> BiomeMap {
        BiomeMap {
            per_tile: vec![biome; world.grid.tiles.len()],
            palette,
        }
    }

    #[test]
    fn default_config_dimensions() {
        let cfg = BiomeRenderConfig::default();
        assert_eq!(cfg.width, 2048);
        assert_eq!(cfg.height, 1024);
    }

    #[test]
    fn output_dimensions_match_config() {
        let world = small_world(earth_atmosphere());
        let biomes = cycling_biome_map(&world, BiomePalette::EarthLike);
        let cfg = BiomeRenderConfig {
            width: 128,
            height: 64,
        };
        let img = render_biome_mollweide(&world, &biomes, &cfg);
        assert_eq!(img.width(), 128);
        assert_eq!(img.height(), 64);
    }

    #[test]
    fn render_is_deterministic() {
        let world = small_world(earth_atmosphere());
        let biomes = cycling_biome_map(&world, BiomePalette::EarthLike);
        let cfg = BiomeRenderConfig {
            width: 96,
            height: 48,
        };
        let a = render_biome_mollweide(&world, &biomes, &cfg);
        let b = render_biome_mollweide(&world, &biomes, &cfg);
        assert_eq!(a.as_raw(), b.as_raw());
    }

    #[test]
    fn earth_like_palette_contains_green_and_blue() {
        // Synthetic Earth-like world: half Ocean, half TemperateForest. We
        // don't rely on the real climate pipeline here; we just want to
        // verify the renderer actually emits palette-recognizable colors.
        let world = small_world(earth_atmosphere());
        let per_tile: Vec<Biome> = (0..world.grid.tiles.len())
            .map(|i| {
                if i % 2 == 0 {
                    Biome::Ocean
                } else {
                    Biome::TemperateForest
                }
            })
            .collect();
        let biomes = BiomeMap {
            per_tile,
            palette: BiomePalette::EarthLike,
        };
        let cfg = BiomeRenderConfig {
            width: 128,
            height: 64,
        };
        let img = render_biome_mollweide(&world, &biomes, &cfg);

        // Count pixels inside the ellipse and classify by dominant channel.
        let mut in_map = 0usize;
        let mut blue_dom = 0usize;
        let mut green_dom = 0usize;
        for (px, py, pix) in img.enumerate_pixels() {
            let nx = ((px as f64 + 0.5) / cfg.width as f64) * 2.0 - 1.0;
            let ny = 1.0 - ((py as f64 + 0.5) / cfg.height as f64) * 2.0;
            if Mollweide::inverse(nx, ny).is_none() {
                continue;
            }
            in_map += 1;
            let [r, g, b] = pix.0;
            if b > r && b > g {
                blue_dom += 1;
            } else if g > r && g > b {
                green_dom += 1;
            }
        }
        assert!(in_map > 0);
        let blue_frac = blue_dom as f64 / in_map as f64;
        let green_frac = green_dom as f64 / in_map as f64;
        assert!(
            blue_frac > 0.2,
            "expected > 20% blue pixels, got {blue_frac:.3}"
        );
        assert!(
            green_frac > 0.2,
            "expected > 20% green pixels, got {green_frac:.3}"
        );
    }

    #[test]
    fn mars_like_palette_is_reddish() {
        // A Mars-atmo world: fill every tile with MartianDustPlain. We
        // don't need the atmosphere's palette to match (we stamp the
        // BiomeMap directly), but we pass a Mars atmo through anyway so
        // the skeleton is built consistently.
        let mut composition = BTreeMap::new();
        composition.insert(Gas::CO2, 0.95);
        composition.insert(Gas::N2, 0.03);
        let mars_atmo = AtmosphereModel {
            surface_pressure: 0.006,
            composition,
            greenhouse_factor: 1.02,
            effective_surface_temp: 210.0,
            scale_height: 11.0,
            moisture_capacity: 0.0,
            uv_surface_flux: 0.8,
            class: AtmosphereClass::ThinCO2,
            retained: vec![Gas::CO2, Gas::N2],
        };
        let world = small_world(mars_atmo);
        let biomes = uniform_biome_map(&world, BiomePalette::MarsLike, Biome::MartianDustPlain);
        let cfg = BiomeRenderConfig {
            width: 96,
            height: 48,
        };
        let img = render_biome_mollweide(&world, &biomes, &cfg);

        let mut in_map = 0usize;
        let mut red_dominant = 0usize;
        for (px, py, pix) in img.enumerate_pixels() {
            let nx = ((px as f64 + 0.5) / cfg.width as f64) * 2.0 - 1.0;
            let ny = 1.0 - ((py as f64 + 0.5) / cfg.height as f64) * 2.0;
            if Mollweide::inverse(nx, ny).is_none() {
                continue;
            }
            in_map += 1;
            let [r, _g, b] = pix.0;
            if r > b {
                red_dominant += 1;
            }
        }
        assert!(in_map > 0);
        let frac = red_dominant as f64 / in_map as f64;
        assert!(frac > 0.9, "expected majority red-dominant, got {frac:.3}");
    }

    #[test]
    fn off_map_corners_are_background() {
        let world = small_world(earth_atmosphere());
        let biomes = uniform_biome_map(&world, BiomePalette::EarthLike, Biome::Ocean);
        let cfg = BiomeRenderConfig {
            width: 64,
            height: 32,
        };
        let img = render_biome_mollweide(&world, &biomes, &cfg);
        assert_eq!(img.get_pixel(0, 0).0, OFF_MAP_BG);
        assert_eq!(img.get_pixel(cfg.width - 1, 0).0, OFF_MAP_BG);
        assert_eq!(img.get_pixel(0, cfg.height - 1).0, OFF_MAP_BG);
        assert_eq!(img.get_pixel(cfg.width - 1, cfg.height - 1).0, OFF_MAP_BG);
    }
}
