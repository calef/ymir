//! Tectonic-plate Mollweide rendering: colours each tile by its plate ID.
//!
//! Each plate is assigned one of a fixed set of visually distinct colours via a
//! cycling palette. Adjacent plates with different IDs are guaranteed to differ
//! in colour as long as the palette has at least as many entries as there are
//! plates (the palette wraps if not, so some plates will share colours when
//! there are more plates than palette entries).

use crate::projections::Mollweide;
use image::RgbImage;
use ymir_surface::skeleton::SkeletonWorld;

/// Fixed off-map background for plate renders. Matches
/// [`crate::overlays::BIOME_OFF_MAP_BG`].
pub const PLATES_OFF_MAP_BG: [u8; 3] = [10, 10, 12];

/// 16-colour palette designed for tectonic plate visualisation. Colours are
/// perceptually spread to make adjacent plates easy to distinguish.
const PLATE_PALETTE: &[[u8; 3]] = &[
    [230, 80, 60],   // warm red
    [60, 130, 210],  // ocean blue
    [80, 190, 100],  // land green
    [210, 170, 50],  // sandy yellow
    [150, 80, 200],  // violet
    [40, 180, 180],  // teal
    [230, 130, 50],  // orange
    [120, 60, 40],   // dark brown
    [200, 100, 160], // pink
    [90, 140, 60],   // olive
    [50, 80, 180],   // deep blue
    [220, 220, 80],  // chartreuse
    [180, 60, 120],  // magenta
    [60, 170, 130],  // seafoam
    [180, 140, 80],  // khaki
    [100, 100, 200], // periwinkle
];

/// Configuration for a plate Mollweide render.
#[derive(Debug, Clone)]
pub struct PlatesRenderConfig {
    /// Output image width in pixels.
    pub width: u32,
    /// Output image height in pixels.
    pub height: u32,
}

impl Default for PlatesRenderConfig {
    fn default() -> Self {
        Self {
            width: 2048,
            height: 1024,
        }
    }
}

/// Render a tectonic plate map: each tile is coloured by its plate ID using a
/// 16-colour cycling palette. Plates wrap around the palette when there are
/// more plates than palette entries.
///
/// Off-map pixels are set to [`PLATES_OFF_MAP_BG`].
pub fn render_plates_mollweide(world: &SkeletonWorld, cfg: &PlatesRenderConfig) -> RgbImage {
    let width = cfg.width.max(1);
    let height = cfg.height.max(1);

    let centers: Vec<[f64; 3]> = world.grid.tiles.iter().map(|t| t.center).collect();
    let plate_ids = &world.tectonics.tile_plate_assignment;
    debug_assert_eq!(centers.len(), plate_ids.len());

    let mut img = RgbImage::from_pixel(width, height, image::Rgb(PLATES_OFF_MAP_BG));

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

            let plate_id = plate_ids[best_idx];
            let rgb = PLATE_PALETTE[plate_id % PLATE_PALETTE.len()];
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
    use ymir_catalog::star_context::{SpectralClass, SpectralType, StarContext};
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

    fn small_world() -> SkeletonWorld {
        let atmo = earth_atmosphere();
        SkeletonWorld::build(earth_body(), atmo, 3, 2024)
    }

    fn small_cfg() -> PlatesRenderConfig {
        PlatesRenderConfig {
            width: 64,
            height: 32,
        }
    }

    #[test]
    fn plates_output_dimensions_match() {
        let world = small_world();
        let img = render_plates_mollweide(&world, &small_cfg());
        assert_eq!(img.width(), 64);
        assert_eq!(img.height(), 32);
    }

    #[test]
    fn plates_image_is_non_empty() {
        let world = small_world();
        let img = render_plates_mollweide(&world, &small_cfg());
        let has_colored = img.pixels().any(|p| p.0 != PLATES_OFF_MAP_BG);
        assert!(has_colored, "plates render produced only background pixels");
    }

    #[test]
    fn plates_corners_are_background() {
        let world = small_world();
        let cfg = small_cfg();
        let img = render_plates_mollweide(&world, &cfg);
        assert_eq!(img.get_pixel(0, 0).0, PLATES_OFF_MAP_BG);
        assert_eq!(img.get_pixel(cfg.width - 1, 0).0, PLATES_OFF_MAP_BG);
        assert_eq!(img.get_pixel(0, cfg.height - 1).0, PLATES_OFF_MAP_BG);
        assert_eq!(
            img.get_pixel(cfg.width - 1, cfg.height - 1).0,
            PLATES_OFF_MAP_BG
        );
    }

    #[test]
    fn plates_render_is_deterministic() {
        let world = small_world();
        let cfg = small_cfg();
        let a = render_plates_mollweide(&world, &cfg);
        let b = render_plates_mollweide(&world, &cfg);
        assert_eq!(a.as_raw(), b.as_raw());
    }

    #[test]
    fn plates_colors_come_from_palette() {
        let world = small_world();
        let img = render_plates_mollweide(&world, &small_cfg());
        // Every non-background pixel must be a color from PLATE_PALETTE.
        for p in img.pixels() {
            if p.0 == PLATES_OFF_MAP_BG {
                continue;
            }
            let found = PLATE_PALETTE.contains(&p.0);
            assert!(
                found,
                "pixel {:?} is not in PLATE_PALETTE and not background",
                p.0
            );
        }
    }

    #[test]
    fn plates_has_multiple_distinct_colors() {
        // A level-3 world has multiple plates so the image should show at
        // least two distinct palette colours.
        let world = small_world();
        let img = render_plates_mollweide(&world, &small_cfg());
        let distinct: std::collections::HashSet<[u8; 3]> = img
            .pixels()
            .filter(|p| p.0 != PLATES_OFF_MAP_BG)
            .map(|p| p.0)
            .collect();
        assert!(
            distinct.len() >= 2,
            "expected multiple plates; got {} distinct colors",
            distinct.len()
        );
    }

    // Satisfy the unused-import lint for `sun` in tests that don't call it.
    #[allow(dead_code)]
    fn _use_sun() -> StarContext {
        sun()
    }
}
