//! Global heightmap generation combining tectonic structure with coherent noise
//! to produce macro-scale elevation data.
//!
//! Elevations are stored in meters, one value per geodesic tile, in the same
//! index order as the grid's `tiles` vector. The algorithm sums a tectonic
//! bias (plate-type baseline plus boundary contributions) with a gravity-scaled
//! fractal-noise displacement, then clamps against a crust yield-strength
//! model for maximum mountain height.

use crate::geodesic::GeodesicGrid;
use crate::noise::SphericalFbm;
use crate::tectonics::{BoundaryType, TectonicData};
use serde::{Deserialize, Serialize};
use ymir_system::orbital_body::OrbitalBody;

/// Configuration parameters for heightmap generation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeightmapConfig {
    /// Weight applied to the tectonic bias contribution (meters as-is).
    pub tectonic_weight: f64,
    /// Weight applied to the gravity-scaled fractal-noise contribution.
    pub noise_weight: f64,
    /// Number of FBM octaves.
    pub octaves: u32,
    /// Base frequency for the first FBM octave.
    pub base_frequency: f64,
    /// Per-octave frequency multiplier.
    pub lacunarity: f64,
    /// Per-octave amplitude multiplier.
    pub gain: f64,
}

impl Default for HeightmapConfig {
    fn default() -> Self {
        Self {
            tectonic_weight: 1.0,
            noise_weight: 1.0,
            octaves: 6,
            base_frequency: 2.0,
            lacunarity: 2.0,
            gain: 0.5,
        }
    }
}

/// Per-tile elevation in meters, indexed by tile index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElevationMap {
    /// One elevation in meters per tile, same order as `GeodesicGrid::tiles`.
    pub elevations_m: Vec<f64>,
}

/// Earth's surface gravity in m/s^2, used as the reference for gravity-scaled
/// noise amplitude.
const EARTH_GRAVITY: f64 = 9.81;
/// Reference relief (peak-to-trough) on an Earth-gravity world, in meters.
/// Earth's mean relief is roughly 2.5 km from the median crust; this is used
/// as the noise amplitude baseline before gravity scaling.
const EARTH_RELIEF_M: f64 = 2500.0;
/// Minimum noise amplitude scale factor. Very high-gravity planets get
/// clamped here so mountains don't vanish completely.
const NOISE_SCALE_MIN: f64 = 0.3;
/// Maximum noise amplitude scale factor. Very low-gravity planets get
/// clamped here to avoid runaway relief on tiny moons.
const NOISE_SCALE_MAX: f64 = 3.0;
/// Approximate crustal yield strength in Pa, used for the `h_max = S / (rho*g)`
/// mountain-height cap. 2e8 Pa is a common proxy for silicate crust.
const CRUST_YIELD_STRENGTH_PA: f64 = 2.0e8;

/// Baseline elevation for tiles on oceanic plates, in meters. Roughly the
/// mean depth of Earth's ocean basins.
const OCEANIC_BASELINE_M: f64 = -4000.0;
/// Baseline elevation for tiles on continental plates, in meters. Roughly
/// Earth's mean continental elevation.
const CONTINENTAL_BASELINE_M: f64 = 200.0;
/// Additional bias applied to tiles participating in a convergent boundary.
const CONVERGENT_BIAS_M: f64 = 2000.0;
/// Additional bias applied to tiles participating in a divergent boundary.
const DIVERGENT_BIAS_M: f64 = -1500.0;

/// Generate an elevation map for `grid` using `tectonics` as the macro
/// structure and gravity-scaled fractal noise for smaller-scale relief.
///
/// Algorithm:
/// 1. Per-tile tectonic bias: plate-type baseline plus per-tile contributions
///    from any convergent/divergent boundaries the tile participates in.
/// 2. Per-tile noise displacement: spherical FBM sampled at the tile center,
///    scaled by a gravity-dependent amplitude that targets Earth-like relief
///    at Earth gravity.
/// 3. Combine with the config weights and clamp against a yield-strength
///    mountain-height cap (asymmetric: trenches are allowed twice as deep as
///    mountains are tall to accommodate subduction bathymetry).
pub fn generate_heightmap(
    grid: &GeodesicGrid,
    tectonics: &TectonicData,
    body: &OrbitalBody,
    seed: u64,
    cfg: &HeightmapConfig,
) -> ElevationMap {
    let n = grid.tiles.len();

    // --- 1. Tectonic bias in meters -----------------------------------------
    let mut tectonic_bias_m = vec![0.0_f64; n];
    for (tile_idx, &plate_id) in tectonics.tile_plate_assignment.iter().enumerate() {
        tectonic_bias_m[tile_idx] = if tectonics.plates[plate_id].is_oceanic {
            OCEANIC_BASELINE_M
        } else {
            CONTINENTAL_BASELINE_M
        };
    }
    for b in &tectonics.boundaries {
        let delta = match b.boundary_type {
            BoundaryType::Convergent => CONVERGENT_BIAS_M,
            BoundaryType::Divergent => DIVERGENT_BIAS_M,
            BoundaryType::Transform => 0.0,
        };
        if delta != 0.0 {
            tectonic_bias_m[b.tile_a] += delta;
            tectonic_bias_m[b.tile_b] += delta;
        }
    }

    // --- 2. Gravity-scaled noise amplitude ----------------------------------
    // Lower gravity → less weight pulling the crust down → taller features.
    // The clamp keeps extreme low-gravity bodies from producing relief on a
    // scale the rest of the pipeline can't reason about.
    let gravity = body.surface_gravity.max(0.01);
    let noise_scale = (EARTH_GRAVITY / gravity).clamp(NOISE_SCALE_MIN, NOISE_SCALE_MAX);
    let noise_amplitude_m = EARTH_RELIEF_M * noise_scale;

    let fbm = SphericalFbm::new(
        seed,
        cfg.octaves,
        cfg.lacunarity,
        cfg.gain,
        cfg.base_frequency,
    );

    // --- 3. Yield-strength mountain cap -------------------------------------
    // body.density is in g/cm^3; convert to kg/m^3.
    let density_kg_m3 = (body.density * 1000.0).max(500.0);
    let h_max = CRUST_YIELD_STRENGTH_PA / (density_kg_m3 * gravity);

    // --- 4. Combine per tile and clamp --------------------------------------
    let mut elevations_m = Vec::with_capacity(n);
    for (i, tile) in grid.tiles.iter().enumerate() {
        let lat = tile.lat.to_radians();
        let lon = tile.lon.to_radians();
        let noise_m = fbm.sample(lat, lon) * noise_amplitude_m;
        let raw = cfg.tectonic_weight * tectonic_bias_m[i] + cfg.noise_weight * noise_m;
        let clamped = raw.clamp(-2.0 * h_max, h_max);
        elevations_m.push(clamped);
    }

    ElevationMap { elevations_m }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tectonics::{TectonicConfig, generate_tectonics};
    use ymir_core::prng::WorldRng;
    use ymir_system::orbital_body::{OrbitalBody, PlanetType};

    fn earth_like() -> OrbitalBody {
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

    fn low_gravity() -> OrbitalBody {
        let mut b = earth_like();
        b.surface_gravity = 3.71; // Mars-like
        b.density = 3.93;
        b.name = Some("MarsLike".into());
        b
    }

    fn high_gravity() -> OrbitalBody {
        let mut b = earth_like();
        b.surface_gravity = 25.0;
        b.density = 7.0;
        b.name = Some("HeavyWorld".into());
        b
    }

    fn make_scene(seed: u64, body: &OrbitalBody) -> (GeodesicGrid, TectonicData) {
        let grid = GeodesicGrid::new(3);
        let mut rng = WorldRng::new(seed);
        let tectonics = generate_tectonics(&grid, &TectonicConfig::default(), &mut rng);
        let _ = body; // kept for parity
        (grid, tectonics)
    }

    #[test]
    fn determinism_same_seed_same_output() {
        let body = earth_like();
        let (grid, tectonics) = make_scene(42, &body);
        let cfg = HeightmapConfig::default();
        let a = generate_heightmap(&grid, &tectonics, &body, 123, &cfg);
        let b = generate_heightmap(&grid, &tectonics, &body, 123, &cfg);
        assert_eq!(a.elevations_m, b.elevations_m);
    }

    #[test]
    fn length_matches_tile_count() {
        let body = earth_like();
        let (grid, tectonics) = make_scene(1, &body);
        let map = generate_heightmap(&grid, &tectonics, &body, 1, &HeightmapConfig::default());
        assert_eq!(map.elevations_m.len(), grid.tiles.len());
    }

    #[test]
    fn no_nan_or_infinite() {
        let body = earth_like();
        let (grid, tectonics) = make_scene(7, &body);
        let map = generate_heightmap(&grid, &tectonics, &body, 7, &HeightmapConfig::default());
        for (i, &h) in map.elevations_m.iter().enumerate() {
            assert!(h.is_finite(), "tile {i} elevation non-finite: {h}");
        }
    }

    #[test]
    fn lower_gravity_produces_higher_max_elevation() {
        // Same grid and tectonic realization, same noise seed; only gravity
        // differs. Lower gravity should permit larger maximum relief.
        let grid = GeodesicGrid::new(3);
        let mut rng = WorldRng::new(42);
        let tectonics = generate_tectonics(&grid, &TectonicConfig::default(), &mut rng);
        let cfg = HeightmapConfig::default();

        let low = low_gravity();
        let high = high_gravity();

        let low_map = generate_heightmap(&grid, &tectonics, &low, 999, &cfg);
        let high_map = generate_heightmap(&grid, &tectonics, &high, 999, &cfg);

        let low_max = low_map
            .elevations_m
            .iter()
            .cloned()
            .fold(f64::MIN, f64::max);
        let high_max = high_map
            .elevations_m
            .iter()
            .cloned()
            .fold(f64::MIN, f64::max);
        assert!(
            low_max > high_max,
            "expected low-gravity max {low_max} > high-gravity max {high_max}"
        );
    }

    #[test]
    fn convergent_tiles_higher_than_divergent() {
        // Run several seeds to avoid a single unlucky realization.
        let grid = GeodesicGrid::new(3);
        let body = earth_like();
        let cfg = HeightmapConfig::default();

        let mut had_both = false;
        for seed in 0..12_u64 {
            let mut rng = WorldRng::new(seed);
            let tectonics = generate_tectonics(&grid, &TectonicConfig::default(), &mut rng);
            let map = generate_heightmap(&grid, &tectonics, &body, seed, &cfg);

            let mut conv: Vec<f64> = Vec::new();
            let mut div: Vec<f64> = Vec::new();
            for b in &tectonics.boundaries {
                match b.boundary_type {
                    BoundaryType::Convergent => {
                        conv.push(map.elevations_m[b.tile_a]);
                        conv.push(map.elevations_m[b.tile_b]);
                    }
                    BoundaryType::Divergent => {
                        div.push(map.elevations_m[b.tile_a]);
                        div.push(map.elevations_m[b.tile_b]);
                    }
                    _ => {}
                }
            }
            if conv.is_empty() || div.is_empty() {
                continue;
            }
            had_both = true;
            let mean_conv: f64 = conv.iter().sum::<f64>() / conv.len() as f64;
            let mean_div: f64 = div.iter().sum::<f64>() / div.len() as f64;
            assert!(
                mean_conv > mean_div,
                "seed {seed}: convergent mean {mean_conv} not greater than divergent mean {mean_div}"
            );
            break;
        }
        assert!(
            had_both,
            "no seed produced both convergent and divergent boundaries"
        );
    }

    #[test]
    fn elevations_within_yield_strength_cap() {
        let body = earth_like();
        let (grid, tectonics) = make_scene(11, &body);
        let map = generate_heightmap(&grid, &tectonics, &body, 11, &HeightmapConfig::default());

        let density_kg_m3 = body.density * 1000.0;
        let h_max = CRUST_YIELD_STRENGTH_PA / (density_kg_m3 * body.surface_gravity);
        let floor = -2.0 * h_max;
        for (i, &h) in map.elevations_m.iter().enumerate() {
            assert!(
                h <= h_max + 1e-6,
                "tile {i} elevation {h} exceeds h_max {h_max}"
            );
            assert!(
                h >= floor - 1e-6,
                "tile {i} elevation {h} below floor {floor}"
            );
        }
    }
}
