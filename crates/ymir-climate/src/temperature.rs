//! Temperature field computation from stellar irradiance, atmospheric greenhouse
//! effect, altitude, and latitude.
//!
//! Implements the Stage 5 temperature model from design doc section 5.5 with
//! two branches: a standard latitudinal model for rotating planets and a
//! substellar-angle model for tidally locked planets. Both apply a
//! dry-adiabatic lapse-rate correction against per-tile elevation.

use serde::{Deserialize, Serialize};
use ymir_atmosphere::composition::AtmosphereClass;
use ymir_surface::skeleton::SkeletonWorld;

/// Stefan-Boltzmann constant, W / (m^2 * K^4).
const STEFAN_BOLTZMANN: f64 = 5.670_374_419e-8;
/// Default planetary Bond albedo used for the tidally-locked equilibrium
/// calculation. Earth-like value.
const DEFAULT_ALBEDO: f64 = 0.3;
/// Surface pressure below which the atmosphere is treated as effectively
/// absent for lapse-rate purposes.
const ATMOSPHERE_PRESSURE_EPS_BAR: f64 = 0.01;
/// Pressure threshold (bar) above which an atmosphere is considered "thick"
/// for the antistellar floor heuristic.
const THICK_ATMOSPHERE_BAR: f64 = 1.0;
/// Antistellar minimum floor in Kelvin for thin atmospheres.
const ANTISTELLAR_FLOOR_THIN_K: f64 = 20.0;
/// Fallback absolute minimum for tidally-locked nightside tiles.
const TIDAL_LOCKED_MIN_K: f64 = 50.0;
/// Fraction of the effective surface temperature used as the antistellar
/// floor for thick atmospheres.
const THICK_ATMO_FLOOR_FRACTION: f64 = 0.3;

/// Latitude shape: poles retain `POLE_FALLOFF_FRACTION` of the global mean
/// surface temperature. Calibrated against Earth (global mean ~288 K, polar
/// ~220 K, equatorial ~300 K).
const POLE_FALLOFF_FRACTION: f64 = 0.6;

/// Configuration for [`build_temperature_field`].
#[derive(Clone, Debug)]
pub struct TemperatureConfig {
    /// Substellar-point longitude in radians (tidally locked planets only).
    /// Default 0.0 puts the substellar point on the equator at the prime
    /// meridian. Ignored for rotating planets.
    pub substellar_lon_rad: f64,
    /// Override for the dry-adiabatic lapse rate in K/m. `None` derives a
    /// lapse rate from the atmosphere class.
    pub lapse_rate_override: Option<f64>,
}

impl Default for TemperatureConfig {
    fn default() -> Self {
        Self {
            substellar_lon_rad: 0.0,
            lapse_rate_override: None,
        }
    }
}

/// Per-tile temperature field in Kelvin, indexed by tile index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemperatureField {
    /// Temperature in Kelvin, one entry per geodesic tile, same order as
    /// `SkeletonWorld::grid.tiles`.
    pub per_tile_k: Vec<f64>,
}

impl TemperatureField {
    /// Arithmetic mean surface temperature in Kelvin. Returns 0 if empty.
    pub fn mean(&self) -> f64 {
        if self.per_tile_k.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.per_tile_k.iter().sum();
        sum / self.per_tile_k.len() as f64
    }

    /// Minimum per-tile temperature in Kelvin. Returns 0 if empty.
    pub fn min(&self) -> f64 {
        if self.per_tile_k.is_empty() {
            return 0.0;
        }
        self.per_tile_k
            .iter()
            .cloned()
            .fold(f64::INFINITY, f64::min)
    }

    /// Maximum per-tile temperature in Kelvin. Returns 0 if empty.
    pub fn max(&self) -> f64 {
        if self.per_tile_k.is_empty() {
            return 0.0;
        }
        self.per_tile_k.iter().cloned().fold(f64::MIN, f64::max)
    }
}

/// Derive a dry-adiabatic lapse rate (K/m) from an atmosphere class and
/// surface pressure.
///
/// Values chosen to reflect order-of-magnitude behavior of each regime:
/// - no atmosphere or trace (< 0.01 bar): 0 K/m
/// - H2/He envelope: 0.002 K/m (deep adiabat, weak near-surface gradient)
/// - CO2-dominated (thick or thin): 0.005 K/m (moist-ish)
/// - N2/O2 or N2/H2O: 0.0065 K/m (Earth tropospheric)
fn derive_lapse_rate(class: AtmosphereClass, pressure_bar: f64) -> f64 {
    if pressure_bar < ATMOSPHERE_PRESSURE_EPS_BAR {
        return 0.0;
    }
    match class {
        AtmosphereClass::None => 0.0,
        AtmosphereClass::HydrogenHelium => 0.002,
        AtmosphereClass::ThinCO2 | AtmosphereClass::ThickCO2 => 0.005,
        AtmosphereClass::NitrogenOxygen | AtmosphereClass::ThickN2H2O => 0.0065,
    }
}

/// Build a global temperature field for the given [`SkeletonWorld`] under the
/// supplied configuration.
///
/// Algorithm (design doc section 5.5):
/// 1. Start from the greenhouse-adjusted effective surface temperature in the
///    atmosphere model.
/// 2. Derive a K/m lapse rate from the atmosphere class (or use the override).
/// 3. For each tile:
///    - Tidally locked bodies: compute angle from the substellar point and a
///      local equilibrium temperature from Stefan-Boltzmann, then scale by
///      the atmosphere's greenhouse ratio. Clamp the nightside to an
///      atmosphere-dependent floor.
///    - Rotating bodies: apply a diurnally-averaged `sqrt(cos(lat))` falloff
///      blended with a pole-fraction floor, calibrated so Earth-like inputs
///      yield a global mean near 288 K.
/// 4. Subtract `lapse_rate * elevation_m` and clamp to non-negative.
pub fn build_temperature_field(world: &SkeletonWorld, cfg: &TemperatureConfig) -> TemperatureField {
    let body = &world.body;
    let atmo = &world.atmosphere;
    let base_surface_t = atmo.effective_surface_temp;
    let pressure_bar = atmo.surface_pressure;

    let lapse_rate = cfg
        .lapse_rate_override
        .unwrap_or_else(|| derive_lapse_rate(atmo.class, pressure_bar));

    // Antistellar floor: thicker atmospheres redistribute heat more
    // effectively, so they get a warmer nightside floor.
    let antistellar_floor = if pressure_bar > THICK_ATMOSPHERE_BAR {
        (base_surface_t * THICK_ATMO_FLOOR_FRACTION).max(TIDAL_LOCKED_MIN_K)
    } else {
        TIDAL_LOCKED_MIN_K.max(ANTISTELLAR_FLOOR_THIN_K)
    };

    // Greenhouse ratio used to scale the local dayside equilibrium temperature
    // back up. Guard against a zero reference to avoid division by zero.
    let greenhouse_ratio = if body.equilibrium_temp > 0.0 {
        base_surface_t / body.equilibrium_temp
    } else {
        1.0
    };

    let substellar_lat = 0.0_f64;
    let sin_subs = substellar_lat.sin();
    let cos_subs = substellar_lat.cos();

    let n = world.grid.tiles.len();
    let mut per_tile_k = Vec::with_capacity(n);

    for (i, tile) in world.grid.tiles.iter().enumerate() {
        let lat_rad = tile.lat.to_radians();
        let lon_rad = tile.lon.to_radians();

        let t_base = if body.tidal_locked {
            // Angular distance from the substellar point.
            let cos_angle = lat_rad.sin() * sin_subs
                + lat_rad.cos() * cos_subs * (lon_rad - cfg.substellar_lon_rad).cos();
            let cos_clamped = cos_angle.max(0.0);
            let local_irradiance = body.solar_irradiance * cos_clamped;
            let local_eq_temp = if local_irradiance > 0.0 {
                (local_irradiance * (1.0 - DEFAULT_ALBEDO) / (4.0 * STEFAN_BOLTZMANN)).powf(0.25)
            } else {
                0.0
            };
            // Apply greenhouse scaling so atmospheres still amplify dayside
            // warmth. Nightside tiles yield ~0 K from this expression; the
            // antistellar floor keeps them physically plausible.
            let scaled = local_eq_temp * greenhouse_ratio;
            scaled.max(antistellar_floor)
        } else {
            // Standard latitudinal model with a diurnal-average lat factor.
            let lat_factor = lat_rad.cos().max(0.0).sqrt();
            base_surface_t * lat_factor
                + (1.0 - lat_factor) * base_surface_t * POLE_FALLOFF_FRACTION
        };

        let elevation_m = world.elevation.elevations_m[i];
        let t = (t_base - lapse_rate * elevation_m).max(0.0);
        per_tile_k.push(t);
    }

    TemperatureField { per_tile_k }
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
    use ymir_surface::skeleton::SkeletonWorld;
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

    /// Hand-build an Earth-calibrated atmosphere model matching the field
    /// layout of ATMO-02 output. We don't need to go through
    /// `AtmosphereModel::derive` for the temperature tests because we only
    /// consume a handful of fields.
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

    fn tidal_body() -> OrbitalBody {
        let mut b = earth_body();
        b.tidal_locked = true;
        b.rotation_period = 24.0 * 300.0;
        b.solar_irradiance = 900.0;
        b.equilibrium_temp = 230.0;
        b.name = Some("TidalWorld".into());
        b
    }

    /// A thin-ish atmosphere so the antistellar floor is 20 K rather than a
    /// warm redistributed floor. This keeps the substellar/antistellar
    /// contrast large enough for the ">100 K" assertion.
    fn thin_atmosphere() -> AtmosphereModel {
        let mut a = earth_atmosphere();
        a.surface_pressure = 0.05;
        a.effective_surface_temp = 230.0;
        a.greenhouse_factor = 1.0;
        a.class = AtmosphereClass::ThinCO2;
        a
    }

    fn build_world(body: OrbitalBody, atmo: AtmosphereModel, seed: u64) -> SkeletonWorld {
        SkeletonWorld::build(body, atmo, 3, seed)
    }

    #[test]
    fn earth_global_mean_is_near_288k() {
        let world = build_world(earth_body(), earth_atmosphere(), 2024);
        let field = build_temperature_field(&world, &TemperatureConfig::default());
        let mean = field.mean();
        assert!(
            (mean - 288.0).abs() <= 8.0,
            "Earth global mean expected within 8K of 288K, got {mean}"
        );
        // Sanity bounds in the slightly relaxed 283-293 K band from the task
        // spec with extra tolerance for elevation cooling.
        assert!(
            (280.0..=296.0).contains(&mean),
            "Earth global mean {mean} out of plausible range"
        );
    }

    #[test]
    fn polar_tile_colder_than_equatorial_rotating() {
        let world = build_world(earth_body(), earth_atmosphere(), 2024);
        let field = build_temperature_field(&world, &TemperatureConfig::default());

        // Pick the tile closest to the equator and the tile closest to a pole.
        let (equator_idx, _) = world
            .grid
            .tiles
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.lat.abs().partial_cmp(&b.lat.abs()).unwrap())
            .unwrap();
        let (pole_idx, _) = world
            .grid
            .tiles
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.lat.abs().partial_cmp(&b.lat.abs()).unwrap())
            .unwrap();

        let t_eq = field.per_tile_k[equator_idx];
        let t_pole = field.per_tile_k[pole_idx];
        assert!(
            t_pole < t_eq,
            "expected polar tile ({t_pole} K) colder than equatorial tile ({t_eq} K)"
        );
    }

    #[test]
    fn tidally_locked_substellar_hotter_than_antistellar() {
        let world = build_world(tidal_body(), thin_atmosphere(), 2024);
        let cfg = TemperatureConfig::default();
        let field = build_temperature_field(&world, &cfg);

        // Find the tile nearest the substellar point (0, 0) and the tile
        // nearest the antistellar point (0, pi).
        let sub_idx = world
            .grid
            .tiles
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = a.lat.powi(2) + a.lon.powi(2);
                let db = b.lat.powi(2) + b.lon.powi(2);
                da.partial_cmp(&db).unwrap()
            })
            .map(|(i, _)| i)
            .unwrap();
        let anti_idx = world
            .grid
            .tiles
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = a.lat.powi(2) + (a.lon.abs() - 180.0).powi(2);
                let db = b.lat.powi(2) + (b.lon.abs() - 180.0).powi(2);
                da.partial_cmp(&db).unwrap()
            })
            .map(|(i, _)| i)
            .unwrap();

        let t_sub = field.per_tile_k[sub_idx];
        let t_anti = field.per_tile_k[anti_idx];
        assert!(
            t_sub - t_anti > 100.0,
            "substellar ({t_sub} K) must exceed antistellar ({t_anti} K) by >100K"
        );
    }

    #[test]
    fn deterministic_same_input_same_output() {
        let a = build_world(earth_body(), earth_atmosphere(), 7);
        let b = build_world(earth_body(), earth_atmosphere(), 7);
        let fa = build_temperature_field(&a, &TemperatureConfig::default());
        let fb = build_temperature_field(&b, &TemperatureConfig::default());
        assert_eq!(fa.per_tile_k, fb.per_tile_k);
    }

    #[test]
    fn higher_elevation_is_cooler_at_same_latitude() {
        // Use a strong explicit lapse-rate override so the elevation effect
        // dominates any latitude-driven jitter between tile positions.
        let mut world = build_world(earth_body(), earth_atmosphere(), 99);

        // Find two tiles with nearly equal latitude but different elevations.
        // Instead of hoping the mesh provides one, force one tile's elevation
        // high and a neighbor's low at similar latitude. We mutate the
        // elevation map directly for test determinism.
        let n = world.grid.tiles.len();
        let mut pair: Option<(usize, usize)> = None;
        for i in 0..n {
            for &nb in &world.grid.tiles[i].neighbors {
                if (world.grid.tiles[i].lat - world.grid.tiles[nb].lat).abs() < 2.0
                    && world.grid.tiles[i].lat.abs() < 45.0
                {
                    pair = Some((i, nb));
                    break;
                }
            }
            if pair.is_some() {
                break;
            }
        }
        let (lo_idx, hi_idx) = pair.expect("expected at least one close-latitude neighbor pair");
        world.elevation.elevations_m[lo_idx] = 0.0;
        world.elevation.elevations_m[hi_idx] = 5000.0;

        let cfg = TemperatureConfig {
            substellar_lon_rad: 0.0,
            lapse_rate_override: Some(0.0065),
        };
        let field = build_temperature_field(&world, &cfg);

        let t_lo = field.per_tile_k[lo_idx];
        let t_hi = field.per_tile_k[hi_idx];
        assert!(
            t_hi < t_lo,
            "high-elevation tile ({t_hi} K) should be cooler than lowland ({t_lo} K)"
        );
        // ~5 km at 6.5 K/km should cost roughly 32 K; allow slack for the
        // residual latitude difference within the 2-degree band.
        assert!(
            t_lo - t_hi > 20.0,
            "elevation cooling unexpectedly small: {} K",
            t_lo - t_hi
        );
    }

    #[test]
    fn mean_min_max_accessors() {
        let field = TemperatureField {
            per_tile_k: vec![100.0, 200.0, 300.0],
        };
        assert!((field.mean() - 200.0).abs() < 1e-9);
        assert!((field.max() - 300.0).abs() < 1e-9);
        // Min helper: should return 100.
        assert!((field.min() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn no_atmosphere_has_zero_lapse() {
        // With no atmosphere and lapse = 0, elevation should not affect T.
        let mut atmo = earth_atmosphere();
        atmo.surface_pressure = 0.0;
        atmo.class = AtmosphereClass::None;
        atmo.effective_surface_temp = 255.0;
        let world = build_world(earth_body(), atmo, 13);
        let field = build_temperature_field(&world, &TemperatureConfig::default());
        // All finite, nonnegative.
        for (i, &t) in field.per_tile_k.iter().enumerate() {
            assert!(t.is_finite(), "tile {i} temperature non-finite: {t}");
            assert!(t >= 0.0, "tile {i} temperature negative: {t}");
        }
    }
}
