//! Per-hex elevation with fractal perturbation on top of the skeleton baseline.
//!
//! [`DetailElevation::build`] produces one elevation sample per [`crate::HexCell`]
//! in a [`HexGrid`] by combining two signals:
//!
//! 1. A **seam-continuous baseline**: each hex's baseline elevation is an
//!    inverse-distance-weighted blend of every skeleton tile within a
//!    radius of `2 *` parent-tile spacing. The blend is a pure function
//!    of the hex's (lat, lon), independent of which skeleton tile the
//!    hex was laid out against, so baseline elevation is automatically
//!    continuous across skeleton-tile seams. The parent tile's elevation
//!    dominates near the parent's centre and the weight smoothly
//!    redistributes to neighbouring tile elevations as the hex
//!    approaches a seam.
//! 2. A **sphere-wide FBM perturbation**: a single [`SphericalFbm`]
//!    instance seeded from the region's detail RNG, sampled at the hex's
//!    (lat, lon). Because FBM is evaluated in spherical coordinates, it
//!    is automatically continuous across seams as well. The perturbation
//!    amplitude is scaled by a gravity-dependent cap so low-gravity bodies
//!    do not grow Earth-sized mountains.
//!
//! # Gravity-scaled amplitude cap
//!
//! Mountain height scales with the square-root of gravity under the
//! brittle-failure stress-balance model (max sustainable relief
//! `h ∝ σ_yield / (ρ g)`, but for terrain roughness dominated by isostatic
//! and mechanical limits the empirical fit is closer to `h ∝ 1/√g`). We
//! use
//!
//! ```text
//! max_displacement_m = amplitude_m * sqrt(earth_g / body_gravity)
//! ```
//!
//! so Earth (g ≈ 9.81 m/s²) uses the configured amplitude exactly, Mars
//! (g ≈ 3.71 m/s²) gets ≈ 1.62× the amplitude, and a 2g super-earth gets
//! ≈ 0.71× the amplitude.

use crate::hex_grid::HexGrid;
use crate::region::{RegionSpec, detail_rng};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use ymir_surface::noise::SphericalFbm;
use ymir_surface::skeleton::SkeletonWorld;

/// Earth's surface gravity in m/s², used to normalise the amplitude cap.
const EARTH_G_M_S2: f64 = 9.81;

/// Configuration for the detail-level elevation perturbation.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct DetailElevationConfig {
    /// Number of FBM octaves summed together.
    pub octaves: u32,
    /// Per-octave frequency multiplier.
    pub lacunarity: f64,
    /// Per-octave amplitude multiplier.
    pub gain: f64,
    /// Frequency of the first FBM octave on the unit sphere.
    pub base_frequency: f64,
    /// Reference perturbation amplitude in metres, measured at Earth
    /// gravity. Actual per-hex displacement is clamped by a
    /// gravity-scaled cap; see the module docs.
    pub amplitude_m: f64,
}

impl Default for DetailElevationConfig {
    fn default() -> Self {
        Self {
            octaves: 5,
            lacunarity: 2.0,
            gain: 0.5,
            base_frequency: 300.0,
            amplitude_m: 300.0,
        }
    }
}

/// Per-hex elevation in metres for every cell in a [`HexGrid`].
///
/// `per_hex_m[i]` matches the ordering of `HexGrid::cells[i]`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetailElevation {
    /// The region this elevation was generated for.
    pub spec: RegionSpec,
    /// Elevation (metres) for each hex in the grid, in grid-cell order.
    pub per_hex_m: Vec<f64>,
}

impl DetailElevation {
    /// Build detail-level elevation for every hex in `grid`.
    ///
    /// `world_seed` must be the same seed used to generate `world`; it
    /// mixes with `grid.region_spec.tile_index` to seed a region-local
    /// RNG from which the FBM seed is drawn.
    pub fn build(
        world: &SkeletonWorld,
        grid: &HexGrid,
        world_seed: u64,
        config: DetailElevationConfig,
    ) -> Self {
        let mut rng = detail_rng(world_seed, grid.region_spec.tile_index);
        let fbm_seed = rng.next_u64();
        let fbm = SphericalFbm::new(
            fbm_seed,
            config.octaves,
            config.lacunarity,
            config.gain,
            config.base_frequency,
        );

        let body_g = world.body.surface_gravity.inner().max(1.0e-6);
        let max_displacement_m = config.amplitude_m * (EARTH_G_M_S2 / body_g).sqrt();

        let mut per_hex_m = Vec::with_capacity(grid.cells.len());
        for cell in &grid.cells {
            let baseline = blended_baseline(world, cell.parent_tile, cell.lat_rad, cell.lon_rad);
            let noise = fbm.sample(cell.lat_rad, cell.lon_rad);
            let displacement = (noise * max_displacement_m)
                .clamp(-max_displacement_m * 1.2, max_displacement_m * 1.2);
            per_hex_m.push(baseline + displacement);
        }

        DetailElevation {
            spec: grid.region_spec.clone(),
            per_hex_m,
        }
    }
}

/// Compute a seam-continuous baseline elevation at a given (lat, lon)
/// on the sphere.
///
/// The baseline is an inverse-fourth-power distance-weighted mean over
/// every skeleton tile centre:
///
/// ```text
/// baseline(lat, lon) = Σ_i elev_i / (eps + d_i)^4   /   Σ_i 1 / (eps + d_i)^4
/// ```
///
/// where `d_i` is the great-circle angle from the query point to tile
/// `i`'s centre. The fourth-power kernel decays so quickly that tiles
/// more than a few tile-spacings away contribute negligibly, but the
/// weighting function itself is a smooth, purely positional mapping:
/// the value at any point on the sphere does not depend on which
/// parent skeleton tile a hex was laid out against, so the baseline is
/// continuous across every seam by construction.
///
/// `parent_tile` is accepted for API symmetry with the surrounding
/// module and is unused inside the baseline formula itself.
pub(crate) fn blended_baseline(
    world: &SkeletonWorld,
    _parent_tile: u32,
    lat_rad: f64,
    lon_rad: f64,
) -> f64 {
    const EPS: f64 = 1.0e-9;

    let hex_xyz = latlon_rad_to_xyz(lat_rad, lon_rad);

    let mut weighted_sum = 0.0;
    let mut weight_total = 0.0;
    for i in 0..world.tile_count() {
        let t = world.tile(i);
        let t_xyz = latlon_rad_to_xyz(t.lat_rad, t.lon_rad);
        let d = great_circle_angle(hex_xyz, t_xyz) + EPS;
        // Inverse fourth power so far tiles are effectively ignored.
        let w = 1.0 / (d * d * d * d);
        weighted_sum += t.elevation_m * w;
        weight_total += w;
    }

    weighted_sum / weight_total
}

/// Convert (lat_rad, lon_rad) to unit-sphere xyz.
fn latlon_rad_to_xyz(lat: f64, lon: f64) -> [f64; 3] {
    let cl = lat.cos();
    [cl * lon.cos(), cl * lon.sin(), lat.sin()]
}

/// Great-circle angle (radians) between two unit vectors.
fn great_circle_angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
    d.acos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex_grid::{HexGrid, HexGridConfig};
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

    fn tiny_world(seed: u64) -> SkeletonWorld {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        SkeletonWorld::build(earth(), atmo, 2, seed)
    }

    #[test]
    fn deterministic_byte_identical() {
        let world = tiny_world(1);
        let spec = RegionSpec::new(3, 1);
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        let cfg = DetailElevationConfig::default();

        let a = DetailElevation::build(&world, &grid, 1, cfg);
        let b = DetailElevation::build(&world, &grid, 1, cfg);

        let sa = serde_json::to_vec(&a).expect("serialize a");
        let sb = serde_json::to_vec(&b).expect("serialize b");
        assert_eq!(sa, sb, "DetailElevation::build is non-deterministic");
    }

    #[test]
    fn per_hex_length_matches_cells() {
        let world = tiny_world(7);
        let spec = RegionSpec::new(0, 1);
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        let elev = DetailElevation::build(&world, &grid, 7, DetailElevationConfig::default());
        assert_eq!(elev.per_hex_m.len(), grid.cells.len());
        for v in &elev.per_hex_m {
            assert!(v.is_finite(), "non-finite elevation: {v}");
        }
    }

    #[test]
    fn cross_tile_seam_continuity() {
        // Neighbouring hexes that straddle a skeleton-tile seam must
        // have elevations close to each other: their baselines are
        // continuous across the seam by construction, and FBM is
        // sampled on the sphere (so it is continuous too). The
        // remaining delta is the FBM gradient times the seam gap,
        // which is small for typical subdivision levels.
        let world = tiny_world(42);
        let spec = RegionSpec::new(0, 1);
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        let cfg = DetailElevationConfig::default();
        let elev = DetailElevation::build(&world, &grid, 42, cfg);

        let mut pairs_checked = 0u32;
        let mut max_delta = 0.0f64;
        for (i, slots) in grid.neighbors.iter().enumerate() {
            let cell_i = &grid.cells[i];
            for slot in slots.iter().flatten() {
                let j = *slot as usize;
                let cell_j = &grid.cells[j];
                if cell_i.parent_tile == cell_j.parent_tile {
                    continue;
                }
                let delta = (elev.per_hex_m[i] - elev.per_hex_m[j]).abs();
                max_delta = max_delta.max(delta);
                pairs_checked += 1;
            }
        }
        assert!(
            pairs_checked > 0,
            "test needs at least one cross-seam neighbour pair"
        );

        // Baseline is position-only (see `blended_baseline` docs) so the
        // only cross-seam delta comes from two FBM samples at nearly
        // (but not exactly) the same lat/lon. At the default
        // `base_frequency = 300`, a seam gap of ~0.75 cell-widths
        // (~0.007 rad at subdivision level 2) can produce FBM deltas of
        // up to ~1-2 in normalized units — we allow 2x the gravity-
        // scaled amplitude cap as the envelope.
        // Baseline is position-only so the only cross-seam delta is the
        // FBM value difference between two nearly-coincident lat/lon
        // samples (seam pairs are within ~0.75 cell widths of each
        // other). Observed on subdivision-2 earth: ≤ 260 m. Threshold is
        // set at 2 × gravity-scaled cap (600 m at Earth defaults) to
        // leave comfortable headroom for FBM phase variation.
        let cap = cfg.amplitude_m * (EARTH_G_M_S2 / *world.body.surface_gravity.inner()).sqrt();
        let threshold = 2.0 * cap;
        assert!(
            max_delta < threshold,
            "max cross-seam elevation delta = {max_delta} m, expected < {threshold} m"
        );
    }

    #[test]
    fn perturbation_bounded_by_gravity_scaled_cap() {
        let world = tiny_world(99);
        let spec = RegionSpec::new(5, 1);
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        let cfg = DetailElevationConfig::default();
        let elev = DetailElevation::build(&world, &grid, 99, cfg);

        let cap = cfg.amplitude_m * (EARTH_G_M_S2 / *world.body.surface_gravity.inner()).sqrt();

        let mut max_abs = 0.0f64;
        for (i, cell) in grid.cells.iter().enumerate() {
            let baseline = blended_baseline(&world, cell.parent_tile, cell.lat_rad, cell.lon_rad);
            let delta = (elev.per_hex_m[i] - baseline).abs();
            max_abs = max_abs.max(delta);
        }

        // 1.2x margin accounts for the OpenSimplex rescaling in
        // `SphericalFbm::sample` which can briefly exceed 1.0 in
        // magnitude. Our implementation also clamps explicitly at
        // 1.2x cap, so this asserts the clamp holds.
        assert!(
            max_abs <= cap * 1.2 + 1.0e-9,
            "max |perturbation| = {max_abs} m, expected <= {} m",
            cap * 1.2
        );
    }

    #[test]
    fn baseline_at_tile_center_matches_tile_elevation() {
        // The weighted blend collapses to the parent tile's elevation
        // when sampled at the tile centre (parent weight dominates as
        // its distance -> 0).
        let world = tiny_world(5);
        for i in 0..world.tile_count().min(10) {
            let tile = world.tile(i);
            let baseline = blended_baseline(&world, i as u32, tile.lat_rad, tile.lon_rad);
            // EPS in blended_baseline means we won't get exact equality
            // but will be extremely close.
            assert!(
                (baseline - tile.elevation_m).abs() < 1.0e-3,
                "baseline at centre {} != tile elevation {} (delta {})",
                baseline,
                tile.elevation_m,
                baseline - tile.elevation_m
            );
        }
    }
}
