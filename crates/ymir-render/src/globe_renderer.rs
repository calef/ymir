//! Globe-scale rendering of the full planetary surface onto 2D projections.
//!
//! Phase 1 implements a simple software rasterizer: for each output pixel,
//! invert the projection back to (lat, lon), find the nearest geodesic tile
//! (by maximum dot product on the unit sphere), and color the pixel by the
//! tile's elevation through [`crate::color_maps::elevation_to_rgb`].

use crate::color_maps::elevation_to_rgb;
use crate::projections::Mollweide;
use image::{ImageError, RgbImage};
use std::path::Path;
use ymir_surface::skeleton::SkeletonWorld;

/// Configuration for a globe render: framebuffer size and clear color.
#[derive(Debug, Clone)]
pub struct GlobeRenderConfig {
    /// Output image width in pixels.
    pub width: u32,
    /// Output image height in pixels.
    pub height: u32,
    /// Background (off-map) color as RGB.
    pub background: [u8; 3],
}

impl Default for GlobeRenderConfig {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 512,
            background: [0, 0, 0],
        }
    }
}

/// Render a [`SkeletonWorld`] to a Mollweide-projected RGB image.
///
/// The algorithm is straightforward: for every pixel, invert the Mollweide
/// projection to (lat, lon), convert that to a unit 3D vector, and pick the
/// tile whose center has the largest dot product with that vector (i.e. the
/// nearest tile by great-circle distance). The tile's elevation is then run
/// through [`elevation_to_rgb`].
///
/// Pixels that fall outside the projected ellipse keep the configured
/// background color.
///
/// Complexity: `O(W * H * N)` where `N` is the tile count. Acceptable for the
/// Phase 1 default of 1024 x 512 against ~10k tiles. A bucketed acceleration
/// is added when this becomes a bottleneck; for now, simplicity wins.
pub fn render_skeleton_mollweide(world: &SkeletonWorld, cfg: &GlobeRenderConfig) -> RgbImage {
    let width = cfg.width.max(1);
    let height = cfg.height.max(1);

    // Pre-extract tile centers as a flat array for tight inner-loop access.
    let centers: Vec<[f64; 3]> = world.grid.tiles.iter().map(|t| t.center).collect();
    let elevations = &world.elevation.elevations_m;
    debug_assert_eq!(centers.len(), elevations.len());

    let bg = image::Rgb(cfg.background);
    let mut img = RgbImage::from_pixel(width, height, bg);

    let inv_w = 1.0 / width as f64;
    let inv_h = 1.0 / height as f64;

    for py in 0..height {
        // Sample at pixel center: (py + 0.5) / H, then map to [-1, 1] with y up.
        let ny = 1.0 - ((py as f64 + 0.5) * inv_h) * 2.0;
        for px in 0..width {
            let nx = ((px as f64 + 0.5) * inv_w) * 2.0 - 1.0;

            let Some((lat, lon)) = Mollweide::inverse(nx, ny) else {
                continue;
            };

            // (lat, lon) radians -> unit 3D vector. Match
            // ymir-surface::geodesic::latlon_to_xyz which uses
            // x = cos(lat)*cos(lon), y = cos(lat)*sin(lon), z = sin(lat).
            let cos_lat = lat.cos();
            let qx = cos_lat * lon.cos();
            let qy = cos_lat * lon.sin();
            let qz = lat.sin();

            // Find the tile center with the maximum dot product (nearest neighbor
            // on the unit sphere).
            let mut best_idx = 0usize;
            let mut best_dot = f64::NEG_INFINITY;
            for (i, c) in centers.iter().enumerate() {
                let d = c[0] * qx + c[1] * qy + c[2] * qz;
                if d > best_dot {
                    best_dot = d;
                    best_idx = i;
                }
            }

            let rgb = elevation_to_rgb(elevations[best_idx]);
            img.put_pixel(px, py, image::Rgb(rgb));
        }
    }

    img
}

/// Render the world and write the result to `path` as a PNG (or any format
/// inferred from the path extension by the `image` crate).
pub fn render_skeleton_mollweide_to_path<P: AsRef<Path>>(
    world: &SkeletonWorld,
    cfg: &GlobeRenderConfig,
    path: P,
) -> Result<(), ImageError> {
    let img = render_skeleton_mollweide(world, cfg);
    img.save(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ymir_atmosphere::atmosphere_model::AtmosphereModel;
    use ymir_catalog::star_context::{SpectralClass, SpectralType, StarContext};
    use ymir_system::orbital_body::{OrbitalBody, PlanetType};

    fn sun() -> StarContext {
        StarContext::from_params(
            "Sun",
            Some("Sol".to_string()),
            SpectralType {
                class: SpectralClass::G,
                subtype: 2,
                luminosity_class: "V".to_string(),
            },
            5780.0,
            1.0,
            0.0,
            1.0,
            1.0,
            4.6,
            0.0,
        )
    }

    fn d(v: f64) -> ymir_core::Sourced<f64> {
        ymir_core::Sourced::derived(v, "test")
    }

    fn earth() -> OrbitalBody {
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
            tidal_locked: ymir_core::Sourced::derived(false, "test"),
            rotation_period: d(24.0),
            is_in_hz: true,
            planet_type: PlanetType::Terran,
            name: Some("Earth".into()),
            is_known_exoplanet: false,
            continental_fraction: None,
        }
    }

    fn small_world() -> SkeletonWorld {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        SkeletonWorld::build(earth(), atmo, 3, 2024)
    }

    #[test]
    fn default_config_dimensions() {
        let cfg = GlobeRenderConfig::default();
        assert_eq!(cfg.width, 1024);
        assert_eq!(cfg.height, 512);
        assert_eq!(cfg.background, [0, 0, 0]);
    }

    #[test]
    fn output_dimensions_match_config() {
        let world = small_world();
        let cfg = GlobeRenderConfig {
            width: 128,
            height: 64,
            background: [0, 0, 0],
        };
        let img = render_skeleton_mollweide(&world, &cfg);
        assert_eq!(img.width(), 128);
        assert_eq!(img.height(), 64);
    }

    #[test]
    fn center_pixel_is_colored() {
        let world = small_world();
        let cfg = GlobeRenderConfig {
            width: 128,
            height: 64,
            background: [0, 0, 0],
        };
        let img = render_skeleton_mollweide(&world, &cfg);
        let center = img.get_pixel(cfg.width / 2, cfg.height / 2);
        assert_ne!(center.0, [0, 0, 0], "center pixel left as background");
    }

    #[test]
    fn render_is_deterministic() {
        let world = small_world();
        let cfg = GlobeRenderConfig {
            width: 96,
            height: 48,
            background: [0, 0, 0],
        };
        let a = render_skeleton_mollweide(&world, &cfg);
        let b = render_skeleton_mollweide(&world, &cfg);
        assert_eq!(a.as_raw(), b.as_raw());
    }

    #[test]
    fn most_pixels_inside_ellipse_are_colored() {
        let world = small_world();
        let cfg = GlobeRenderConfig {
            width: 64,
            height: 32,
            background: [0, 0, 0],
        };
        let img = render_skeleton_mollweide(&world, &cfg);

        let mut colored = 0usize;
        let mut total = 0usize;
        for (px, py, pix) in img.enumerate_pixels() {
            // Sample at pixel center; check whether this pixel falls inside
            // the projected ellipse so we only count "should-be-colored" pixels.
            let nx = ((px as f64 + 0.5) / cfg.width as f64) * 2.0 - 1.0;
            let ny = 1.0 - ((py as f64 + 0.5) / cfg.height as f64) * 2.0;
            if Mollweide::inverse(nx, ny).is_some() {
                total += 1;
                if pix.0 != [0, 0, 0] {
                    colored += 1;
                }
            }
        }
        assert!(total > 0);
        let frac = colored as f64 / total as f64;
        assert!(frac > 0.95, "only {frac:.3} of in-ellipse pixels colored");
    }

    #[test]
    fn corner_pixels_off_map_remain_background() {
        let world = small_world();
        let cfg = GlobeRenderConfig {
            width: 64,
            height: 32,
            background: [255, 0, 255], // garish marker for off-map
        };
        let img = render_skeleton_mollweide(&world, &cfg);
        // The four corners are well outside the Mollweide ellipse.
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 255]);
        assert_eq!(img.get_pixel(cfg.width - 1, 0).0, [255, 0, 255]);
        assert_eq!(img.get_pixel(0, cfg.height - 1).0, [255, 0, 255]);
        assert_eq!(
            img.get_pixel(cfg.width - 1, cfg.height - 1).0,
            [255, 0, 255]
        );
    }

    #[test]
    #[ignore = "perf bench, run explicitly with --ignored"]
    fn bench_default_render_level5() {
        use std::time::Instant;
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        let t0 = Instant::now();
        let world = SkeletonWorld::build(earth(), atmo, 5, 42);
        eprintln!(
            "skeleton level 5 ({} tiles): {:.2?}",
            world.grid.tiles.len(),
            t0.elapsed()
        );
        let cfg = GlobeRenderConfig::default();
        let t1 = Instant::now();
        let img = render_skeleton_mollweide(&world, &cfg);
        eprintln!(
            "render {}x{}: {:.2?}",
            img.width(),
            img.height(),
            t1.elapsed()
        );
    }

    #[test]
    fn writes_png_to_path() {
        let world = small_world();
        let cfg = GlobeRenderConfig {
            width: 64,
            height: 32,
            background: [0, 0, 0],
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("globe.png");
        render_skeleton_mollweide_to_path(&world, &cfg, &path).expect("write");
        let metadata = std::fs::metadata(&path).expect("stat");
        assert!(metadata.len() > 0);
    }
}
