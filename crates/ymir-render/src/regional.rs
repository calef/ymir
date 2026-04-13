//! Regional detail renderer: rasterize a [`RegionalDetail`] into a high-res
//! PNG using a local tangent-plane projection centred on the region's seed
//! tile.
//!
//! The output combines three layers:
//!
//! 1. **Biome fill.** Each pixel is coloured by the nearest hex's biome via
//!    [`crate::biome_palette::biome_color`]. Nearest-hex lookup is a
//!    brute-force scan over all hexes in the region (radius-1 Earth: ~6k
//!    hexes), which is acceptable for Phase 3.
//! 2. **Hillshade.** A per-hex shading factor is derived from the signed
//!    elevation difference between the hex and a directional average of its
//!    neighbours. Faces sloping towards the illuminator brighten; faces
//!    sloping away darken. The strength knob scales the effect.
//! 3. **River overlay.** Hexes with `flow_accumulation` above the configured
//!    threshold and not flagged as lakes are tinted towards a river blue,
//!    with the mix intensity scaling with accumulation. Lakes already show
//!    as `CoastalShallow` / `TitanMethaneSea` via DET-06 so no extra
//!    compositing is needed for them.
//!
//! The projection is a local tangent-plane at the region's seed tile
//! (`world.grid.tiles[region.spec.tile_index]`). Both `Orthographic` and
//! `Stereographic` variants are supported; the default is `Orthographic`
//! because at the radius-1 tile extent the difference is visually minor and
//! the orthographic path is cheaper to compute.

use crate::biome_palette::biome_color;
use image::RgbImage;
use serde::{Deserialize, Serialize};
use ymir_detail::RegionalDetail;
use ymir_surface::skeleton::SkeletonWorld;

/// Local projection used by [`render_regional_detail`].
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum RegionProjection {
    /// Orthographic: project each point onto the tangent plane using the
    /// perpendicular component (i.e. clip the normal component to zero).
    /// Cheap, good for small regions, clips anything past the visible
    /// hemisphere.
    #[default]
    Orthographic,
    /// Stereographic: conformal projection, preserves angles and small
    /// shapes at the expense of area distortion further from the tangent
    /// point. Useful if the region spans closer to a hemisphere.
    Stereographic,
}

/// Configuration for [`render_regional_detail`].
///
/// All fields are public so callers can tweak individual knobs; the defaults
/// target a 1024x1024 render of a radius-1 Earth region with moderate
/// hillshade and the DET-06 default river threshold.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct RegionalRenderConfig {
    /// Output image width in pixels.
    pub width: u32,
    /// Output image height in pixels.
    pub height: u32,
    /// Hillshade strength, expected in `[0.0, 1.0]`. Zero disables
    /// hillshade; one saturates per-channel shading at the hex's own
    /// `abs(delta_m) / hillshade_normalization_m`.
    pub hillshade_strength: f32,
    /// Minimum `flow_accumulation` (in hex units) for a non-lake hex to be
    /// painted as a river overlay. Mirrors
    /// [`ymir_detail::DetailBiomeConfig`]'s default (50.0).
    pub river_flow_threshold: f32,
    /// Projection used to map (lat, lon) to pixel coordinates.
    pub projection: RegionProjection,
}

impl Default for RegionalRenderConfig {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 1024,
            hillshade_strength: 0.4,
            river_flow_threshold: 50.0,
            projection: RegionProjection::Orthographic,
        }
    }
}

/// Normalisation for the per-hex hillshade delta (metres of relief). A hex
/// whose neighbour-averaged elevation delta equals this value maps to a
/// full `hillshade_strength` brightening or darkening. Chosen to match the
/// elevation amplitude of DET-03's default FBM field (~250 m).
const HILLSHADE_NORMALIZATION_M: f64 = 250.0;

/// Fixed off-region background. Deep near-black so the projected region
/// pops visually and no biome colour collides with it.
const OFF_REGION_BG: [u8; 3] = [10, 10, 12];

/// River tint target colour. Non-lake hexes above the flow threshold are
/// blended towards this colour proportional to `flow_accumulation /
/// (flow_accumulation + threshold)`, i.e. saturating smoothly as
/// accumulation grows.
const RIVER_BLUE: [u8; 3] = [0x20, 0x60, 0xa0];

/// Render a [`RegionalDetail`] to an RGB image.
///
/// The seed tile of `region.spec` must reference a valid tile in `world`;
/// the tangent frame is anchored at that tile's `(lat, lon)`. Each hex is
/// rasterised by nearest-neighbour assignment against the projected hex
/// centres, then the per-hex colour is composited with hillshade and a
/// river tint.
///
/// # Panics
///
/// Panics if `region.spec.tile_index` is out of bounds for `world`.
pub fn render_regional_detail(
    region: &RegionalDetail,
    world: &SkeletonWorld,
    config: &RegionalRenderConfig,
) -> RgbImage {
    let width = config.width.max(1);
    let height = config.height.max(1);

    let seed_idx = region.spec.tile_index as usize;
    assert!(
        seed_idx < world.tile_count(),
        "RegionSpec::tile_index {seed_idx} out of bounds (tile_count = {})",
        world.tile_count()
    );
    let seed_tile = world.tile(seed_idx);
    let frame = TangentFrame::at(seed_tile.lat_rad, seed_tile.lon_rad);

    // Precompute projected hex positions and per-hex colours. All three
    // arrays share `region.hex_grid.cells` ordering.
    let cells = &region.hex_grid.cells;
    let n = cells.len();

    // Project each hex centre. `None` means the hex is on the hemisphere
    // facing away from the tangent point (only possible for `Orthographic`
    // at regions spanning more than a hemisphere; in practice never hit
    // for Phase 3 radius-1 regions). These hexes are skipped during
    // nearest-neighbour search so they never contribute a pixel.
    let mut projected: Vec<Option<(f64, f64)>> = Vec::with_capacity(n);
    for cell in cells {
        projected.push(frame.project(cell.lat_rad, cell.lon_rad, config.projection));
    }

    // Compute projection extent in tangent-plane units so we can fit the
    // visible hexes into the image with a small margin.
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for p in projected.iter().flatten() {
        min_x = min_x.min(p.0);
        max_x = max_x.max(p.0);
        min_y = min_y.min(p.1);
        max_y = max_y.max(p.1);
    }
    if !min_x.is_finite() || !min_y.is_finite() {
        // No projectable hexes (should not happen for valid regions).
        return RgbImage::from_pixel(width, height, image::Rgb(OFF_REGION_BG));
    }

    // Uniform scale so aspect ratio is preserved; 2% margin.
    let span_x = (max_x - min_x).max(1e-9);
    let span_y = (max_y - min_y).max(1e-9);
    let margin = 0.02;
    let scale_x = (width as f64 * (1.0 - 2.0 * margin)) / span_x;
    let scale_y = (height as f64 * (1.0 - 2.0 * margin)) / span_y;
    let scale = scale_x.min(scale_y);
    let cx = 0.5 * (min_x + max_x);
    let cy = 0.5 * (min_y + max_y);

    // Pixel coordinates (f64, image coords) of each projected hex. Hexes
    // that failed to project get a sentinel so the nearest-neighbour scan
    // skips them.
    let centres_px: Vec<Option<(f64, f64)>> = projected
        .iter()
        .map(|p| {
            p.map(|(x, y)| {
                let sx = 0.5 * width as f64 + (x - cx) * scale;
                // Image y axis points down; tangent-plane y axis points up.
                let sy = 0.5 * height as f64 - (y - cy) * scale;
                (sx, sy)
            })
        })
        .collect();

    // Per-hex base biome colour, then tinted by river overlay. Hillshade
    // is applied per pixel (identical for every pixel of a given hex, but
    // computed once in the hex pass below so the inner loop stays tight).
    let per_hex_base: Vec<[u8; 3]> = (0..n)
        .map(|i| {
            let biome = region.biomes.per_hex[i];
            let mut rgb = biome_color(biome);

            // River overlay: only on non-lake hexes whose flow meets the
            // threshold. Lakes already read as water-body biomes through
            // DET-06 so we skip them here. Mix strength is a smooth
            // saturating function of accumulation so big rivers read as
            // deeper blue than just-qualifying streams.
            let flow = region.flow.flow_accumulation[i];
            let is_lake = region.flow.is_lake[i];
            if !is_lake && flow >= config.river_flow_threshold {
                let t = (flow / (flow + config.river_flow_threshold)).clamp(0.0, 1.0);
                rgb = mix_rgb(rgb, RIVER_BLUE, t);
            }
            rgb
        })
        .collect();

    // Per-hex hillshade multiplier. +1 => full brighten, -1 => full
    // darken; scaled by `config.hillshade_strength` before application.
    let per_hex_shade: Vec<f32> = compute_hillshade(region);

    // Rasterise. For each pixel, find the nearest projected hex centre,
    // then apply biome colour + river tint + hillshade. Pixels whose
    // nearest centre is too far away fall back to the off-region bg so
    // we don't smear the nearest hex across the whole canvas margin.
    let mut img = RgbImage::from_pixel(width, height, image::Rgb(OFF_REGION_BG));

    // Maximum distance (in pixels) a pixel may be from a hex centre and
    // still be painted. Scales with the lattice cell size so regions of
    // different extents still look filled. We overestimate to avoid
    // pinholes: `2.0 * (scale * lattice_cell_width)` in pixels, derived
    // from the median hex spacing rather than the tightest spacing so
    // edge hexes aren't left unpainted.
    let paint_radius_px = estimate_paint_radius_px(&centres_px);
    let paint_radius_sq = paint_radius_px * paint_radius_px;

    for py in 0..height {
        let pyf = py as f64 + 0.5;
        for px in 0..width {
            let pxf = px as f64 + 0.5;
            let mut best_idx: Option<usize> = None;
            let mut best_d2 = f64::INFINITY;
            for (i, c) in centres_px.iter().enumerate() {
                if let Some((cx, cy)) = *c {
                    let dx = cx - pxf;
                    let dy = cy - pyf;
                    let d2 = dx * dx + dy * dy;
                    if d2 < best_d2 {
                        best_d2 = d2;
                        best_idx = Some(i);
                    }
                }
            }
            let Some(i) = best_idx else { continue };
            if best_d2 > paint_radius_sq {
                continue;
            }
            let base = per_hex_base[i];
            let shade = per_hex_shade[i] * config.hillshade_strength;
            let rgb = apply_shade(base, shade);
            img.put_pixel(px, py, image::Rgb(rgb));
        }
    }

    img
}

/// Estimate a per-pixel paint radius from the median nearest-neighbour
/// distance between projected hex centres. We want a radius large enough
/// to cover the gaps between adjacent hexes (so the region reads as a
/// solid filled shape) but small enough that the region footprint has a
/// crisp edge against the off-region background.
fn estimate_paint_radius_px(centres: &[Option<(f64, f64)>]) -> f64 {
    // Use a small random sample of hexes to estimate spacing in O(N * k)
    // rather than O(N^2). Sample by stride so the choice is deterministic.
    let valid: Vec<(f64, f64)> = centres.iter().filter_map(|c| *c).collect();
    if valid.len() < 2 {
        return 4.0;
    }
    let stride = (valid.len() / 32).max(1);
    let mut nn: Vec<f64> = Vec::new();
    for (i, &(x, y)) in valid.iter().enumerate().step_by(stride) {
        let mut best = f64::INFINITY;
        for (j, &(x2, y2)) in valid.iter().enumerate() {
            if i == j {
                continue;
            }
            let dx = x - x2;
            let dy = y - y2;
            let d2 = dx * dx + dy * dy;
            if d2 < best {
                best = d2;
            }
        }
        nn.push(best.sqrt());
    }
    nn.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = nn[nn.len() / 2];
    // Paint within ~1.25x median spacing; a pure hexagonal packing would
    // need sqrt(3)/3 * spacing, but the projection + lattice-edge mismatch
    // means a slight overshoot fills all gaps.
    (median * 1.25).max(2.0)
}

/// Compute a per-hex hillshade multiplier in `[-1, 1]` from the elevation
/// field. Uses the simple "current - mean(neighbours)" relief proxy,
/// normalised by [`HILLSHADE_NORMALIZATION_M`]. Ridges brighten, valleys
/// darken. Hexes with no neighbours (shouldn't exist) get zero.
fn compute_hillshade(region: &RegionalDetail) -> Vec<f32> {
    let elev = &region.elevation.per_hex_m;
    let neighbors = &region.hex_grid.neighbors;
    let n = elev.len();
    let mut out = Vec::with_capacity(n);
    for (i, slots) in neighbors.iter().enumerate() {
        let mut sum = 0.0f64;
        let mut count = 0usize;
        for slot in slots.iter().flatten() {
            sum += elev[*slot as usize];
            count += 1;
        }
        if count == 0 {
            out.push(0.0);
            continue;
        }
        let mean = sum / count as f64;
        let delta = elev[i] - mean;
        let shade = (delta / HILLSHADE_NORMALIZATION_M).clamp(-1.0, 1.0) as f32;
        out.push(shade);
    }
    out
}

/// Apply a scalar shade factor in `[-1, 1]` to an RGB triplet. Positive
/// values brighten each channel towards 255; negative values darken
/// towards 0. The effect is linear in each channel.
fn apply_shade(rgb: [u8; 3], shade: f32) -> [u8; 3] {
    let s = shade.clamp(-1.0, 1.0);
    let adjust = |c: u8| -> u8 {
        let cf = f32::from(c);
        let target = if s >= 0.0 { 255.0 } else { 0.0 };
        let v = cf + (target - cf) * s.abs();
        v.clamp(0.0, 255.0) as u8
    };
    [adjust(rgb[0]), adjust(rgb[1]), adjust(rgb[2])]
}

/// Linearly blend two RGB triplets. `t = 0` returns `a`, `t = 1` returns
/// `b`.
fn mix_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 {
        let xf = f32::from(x);
        let yf = f32::from(y);
        (xf + (yf - xf) * t).clamp(0.0, 255.0) as u8
    };
    [mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2])]
}

/// Local tangent frame at a (lat, lon) on the unit sphere. Mirrors the
/// gnomonic frame used in `ymir-detail::hex_grid`, specialised for the
/// two projections this renderer supports.
struct TangentFrame {
    /// Tangent-point unit vector.
    center: [f64; 3],
    /// East tangent basis (unit vector).
    east: [f64; 3],
    /// North tangent basis (unit vector).
    north: [f64; 3],
}

impl TangentFrame {
    fn at(lat_rad: f64, lon_rad: f64) -> Self {
        let cos_lat = lat_rad.cos();
        let center = [
            cos_lat * lon_rad.cos(),
            cos_lat * lon_rad.sin(),
            lat_rad.sin(),
        ];
        let sin_lat = lat_rad.sin();
        let cos_lon = lon_rad.cos();
        let sin_lon = lon_rad.sin();
        let east = [-sin_lon, cos_lon, 0.0];
        let north = [-sin_lat * cos_lon, -sin_lat * sin_lon, cos_lat];
        TangentFrame {
            center,
            east,
            north,
        }
    }

    /// Project (lat, lon) onto the tangent plane using the configured
    /// projection. Returns `None` for points on the far side of the
    /// sphere (only possible for `Orthographic`).
    fn project(
        &self,
        lat_rad: f64,
        lon_rad: f64,
        projection: RegionProjection,
    ) -> Option<(f64, f64)> {
        let cos_lat = lat_rad.cos();
        let p = [
            cos_lat * lon_rad.cos(),
            cos_lat * lon_rad.sin(),
            lat_rad.sin(),
        ];
        // Dot with the tangent point: equals cos(great-circle angle).
        let cos_c = self.center[0] * p[0] + self.center[1] * p[1] + self.center[2] * p[2];
        let x = self.east[0] * p[0] + self.east[1] * p[1] + self.east[2] * p[2];
        let y = self.north[0] * p[0] + self.north[1] * p[1] + self.north[2] * p[2];
        match projection {
            RegionProjection::Orthographic => {
                if cos_c <= 0.0 {
                    // Far hemisphere; clip.
                    None
                } else {
                    Some((x, y))
                }
            }
            RegionProjection::Stereographic => {
                // Stereographic from the antipode of `center`: k = 2 / (1 + cos_c).
                // Singular only at the exact antipode (cos_c == -1).
                let denom = 1.0 + cos_c;
                if denom.abs() < 1e-9 {
                    None
                } else {
                    let k = 2.0 / denom;
                    Some((k * x, k * y))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use ymir_atmosphere::atmosphere_model::AtmosphereModel;
    use ymir_atmosphere::composition::AtmosphereClass;
    use ymir_atmosphere::retention::Gas;
    use ymir_biome::{BiomeMap, BiomeMapConfig};
    use ymir_climate::{ClimateConfig, ClimateMap};
    use ymir_core::Sourced;
    use ymir_detail::{RegionSpec, RegionalDetail, RegionalDetailConfig};
    use ymir_surface::skeleton::SkeletonWorld;
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

    /// Build an Earth-seeded (world, climate, biome, region) bundle for
    /// rendering tests. Uses subdivision level 2 (162 tiles) and radius-1
    /// so the test is fast but still exercises seam stitching.
    fn build_region(seed: u64, tile: u32, radius: u32) -> (SkeletonWorld, RegionalDetail) {
        let world = SkeletonWorld::build(earth_body(), earth_atmosphere(), 2, seed);
        let climate = ClimateMap::build(&world, &ClimateConfig::default());
        let biomes = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        let spec = RegionSpec::new(tile, radius);
        let cfg = RegionalDetailConfig {
            seed,
            ..RegionalDetailConfig::default()
        };
        let region = RegionalDetail::build(&world, &climate, &biomes, spec, cfg);
        (world, region)
    }

    #[test]
    fn default_config_dimensions() {
        let cfg = RegionalRenderConfig::default();
        assert_eq!(cfg.width, 1024);
        assert_eq!(cfg.height, 1024);
        assert_eq!(cfg.projection, RegionProjection::Orthographic);
    }

    #[test]
    fn output_dimensions_match_config() {
        let (world, region) = build_region(1, 0, 1);
        let cfg_a = RegionalRenderConfig {
            width: 256,
            height: 256,
            ..RegionalRenderConfig::default()
        };
        let img_a = render_regional_detail(&region, &world, &cfg_a);
        assert_eq!(img_a.width(), 256);
        assert_eq!(img_a.height(), 256);

        let cfg_b = RegionalRenderConfig {
            width: 512,
            height: 256,
            ..RegionalRenderConfig::default()
        };
        let img_b = render_regional_detail(&region, &world, &cfg_b);
        assert_eq!(img_b.width(), 512);
        assert_eq!(img_b.height(), 256);
    }

    #[test]
    fn palette_visible_in_output() {
        // Every biome present on at least 5 hexes in the region must
        // produce at least one pixel within 8 of its palette colour in
        // every channel. Hillshade can shift channels by up to 0.4 *
        // 255 ~= 102, so to make this a reasonable invariant we turn
        // hillshade off and push the river threshold high so only raw
        // biome fill survives.
        let (world, region) = build_region(1, 0, 1);
        let cfg = RegionalRenderConfig {
            width: 256,
            height: 256,
            hillshade_strength: 0.0,
            river_flow_threshold: 1.0e9,
            projection: RegionProjection::Orthographic,
        };
        let img = render_regional_detail(&region, &world, &cfg);

        // Count biome occurrences.
        let mut counts: std::collections::HashMap<_, usize> = std::collections::HashMap::new();
        for b in &region.biomes.per_hex {
            *counts.entry(*b).or_default() += 1;
        }
        let target_biomes: Vec<_> = counts
            .iter()
            .filter_map(|(b, c)| if *c >= 5 { Some(*b) } else { None })
            .collect();
        assert!(
            !target_biomes.is_empty(),
            "expected at least one biome with >= 5 hexes in region"
        );

        for biome in target_biomes {
            let want = biome_color(biome);
            let mut found = false;
            for pix in img.pixels() {
                let [r, g, b] = pix.0;
                if r.abs_diff(want[0]) <= 8 && g.abs_diff(want[1]) <= 8 && b.abs_diff(want[2]) <= 8
                {
                    found = true;
                    break;
                }
            }
            assert!(
                found,
                "biome {biome:?} (target {want:?}) not visible in output"
            );
        }
    }

    #[test]
    fn river_hexes_distinguishable() {
        // Rendering the same region with two different river thresholds
        // must yield different buffers: at a very high threshold no hex
        // crosses it, so no river tint is applied.
        let (world, region) = build_region(3, 0, 1);
        let cfg_low = RegionalRenderConfig {
            width: 128,
            height: 128,
            hillshade_strength: 0.0,
            river_flow_threshold: 50.0,
            projection: RegionProjection::Orthographic,
        };
        let cfg_high = RegionalRenderConfig {
            river_flow_threshold: 1.0e9,
            ..cfg_low
        };

        // Make sure at least one hex would cross the low threshold,
        // otherwise the test is a tautology.
        let any_river = region
            .flow
            .flow_accumulation
            .iter()
            .zip(region.flow.is_lake.iter())
            .any(|(f, l)| !l && *f >= cfg_low.river_flow_threshold);
        assert!(
            any_river,
            "test region contains no river-threshold-qualifying hexes"
        );

        let img_low = render_regional_detail(&region, &world, &cfg_low);
        let img_high = render_regional_detail(&region, &world, &cfg_high);
        assert_ne!(
            img_low.as_raw(),
            img_high.as_raw(),
            "river threshold did not affect rendered output"
        );
    }

    #[test]
    fn determinism_byte_identical() {
        let (world, region) = build_region(7, 0, 1);
        let cfg = RegionalRenderConfig {
            width: 128,
            height: 128,
            ..RegionalRenderConfig::default()
        };
        let a = render_regional_detail(&region, &world, &cfg);
        let b = render_regional_detail(&region, &world, &cfg);
        assert_eq!(a.as_raw(), b.as_raw());
    }

    #[test]
    #[ignore = "perf bench, run explicitly with --ignored"]
    fn bench_1024_radius1_render() {
        use std::time::Instant;
        let (world, region) = build_region(42, 0, 1);
        let cfg = RegionalRenderConfig::default();
        // Warm up once (allocations, caches).
        let _ = render_regional_detail(&region, &world, &cfg);
        let t0 = Instant::now();
        let img = render_regional_detail(&region, &world, &cfg);
        let dt = t0.elapsed();
        println!(
            "rendered {}x{} radius-1 region ({} hexes) in {:?}",
            img.width(),
            img.height(),
            region.hex_grid.cells.len(),
            dt
        );
    }

    #[test]
    fn config_serde_round_trip() {
        let cfg = RegionalRenderConfig {
            width: 800,
            height: 600,
            hillshade_strength: 0.33,
            river_flow_threshold: 75.5,
            projection: RegionProjection::Stereographic,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: RegionalRenderConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, cfg);
    }
}
