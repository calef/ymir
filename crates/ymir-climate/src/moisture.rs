//! Global moisture field computation: latitude-banded (or radial-from-substellar
//! for tidally locked bodies) atmospheric humidity modulated by a
//! Clausius-Clapeyron-style capacity factor, with a local orographic bias from
//! the surface gradient.
//!
//! Per design doc section 5.5, moisture is a two-component model: a
//! circulation-driven base field plus wind-driven orographic transport. The
//! wind-driven term belongs to a later stage (CLIM-03/CLIM-04); this module
//! provides only a local elevation-gradient proxy so that windward-ish slopes
//! gain moisture and leeward-ish slopes lose it without depending on the wind
//! field. The resulting per-tile values are relative humidity fractions in
//! `[0, 1]`.
//!
//! On bodies that cannot sustain water vapor (no H2O in the retained gas set
//! or surface pressure below 0.01 bar), the field is all zeros.

use serde::{Deserialize, Serialize};
use ymir_atmosphere::retention::Gas;
use ymir_surface::skeleton::SkeletonWorld;

use crate::temperature::TemperatureField;

/// Surface-pressure floor below which the atmosphere cannot sustain a
/// water-vapor reservoir, in bar.
const DRY_PRESSURE_EPS_BAR: f64 = 0.01;

/// Reference surface temperature for the Clausius-Clapeyron scaling, K.
/// Roughly Earth's global mean.
const CC_REF_TEMP_K: f64 = 288.0;
/// Reference surface pressure for the Clausius-Clapeyron scaling, bar.
const CC_REF_PRESSURE_BAR: f64 = 1.0;
/// Lower bound for the humidity ceiling derived from Clausius-Clapeyron. Even
/// a very cold, very thin atmosphere admits a tiny amount of vapor.
const CEILING_MIN: f64 = 0.05;
/// Upper bound for the humidity ceiling. Prevents super-Earth thick
/// atmospheres from saturating uniformly.
const CEILING_MAX: f64 = 1.0;

// Latitude-band coefficients. Peaks and troughs roughly match the Earth
// Hadley/Ferrel/Polar cell structure: ITCZ near the equator, horse latitudes
// near 30 degrees, moderate mid-latitude humidity near 50 degrees, dry poles.
const ITCZ_PEAK: f64 = 0.8;
const HORSE_TROUGH: f64 = 0.2;
const MIDLAT_PEAK: f64 = 0.5;
const POLE_FLOOR: f64 = 0.1;
const LAT_HORSE_DEG: f64 = 30.0;
const LAT_MIDLAT_DEG: f64 = 50.0;
const LAT_POLE_DEG: f64 = 75.0;

// Tidally locked substellar/antistellar coefficients.
const TIDAL_DAYSIDE_PEAK: f64 = 0.8;
const TIDAL_TERMINATOR_FLOOR: f64 = 0.1;
const TIDAL_TERMINATOR_BAND_RAD: f64 = 0.35; // ~20 degrees either side

/// Per-tile global moisture field indexed by tile.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MoistureField {
    /// Relative humidity fraction per tile in `[0.0, 1.0]`, same order as
    /// `SkeletonWorld::grid.tiles`.
    pub per_tile: Vec<f64>,
}

impl MoistureField {
    /// Arithmetic mean relative humidity. Returns 0 if empty.
    pub fn mean(&self) -> f64 {
        if self.per_tile.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.per_tile.iter().sum();
        sum / self.per_tile.len() as f64
    }

    /// Maximum per-tile humidity. Returns 0 if empty.
    pub fn max(&self) -> f64 {
        if self.per_tile.is_empty() {
            return 0.0;
        }
        self.per_tile.iter().cloned().fold(f64::MIN, f64::max)
    }

    /// Minimum per-tile humidity. Returns 0 if empty.
    pub fn min(&self) -> f64 {
        if self.per_tile.is_empty() {
            return 0.0;
        }
        self.per_tile.iter().cloned().fold(f64::INFINITY, f64::min)
    }

    /// Fraction of tiles with humidity `>= threshold`. Returns 0 if empty.
    pub fn coverage_above(&self, threshold: f64) -> f64 {
        if self.per_tile.is_empty() {
            return 0.0;
        }
        let count = self.per_tile.iter().filter(|&&v| v >= threshold).count();
        count as f64 / self.per_tile.len() as f64
    }
}

/// Configuration for [`build_moisture_field`].
#[derive(Clone, Debug)]
pub struct MoistureConfig {
    /// Substellar-point longitude for tidally locked planets, radians. Ignored
    /// for rotating bodies.
    pub substellar_lon_rad: f64,
    /// Magnitude of the orographic bias. Windward-ish slopes gain moisture and
    /// leeward-ish slopes lose it by at most roughly this fraction per km of
    /// local slope. A full wind-driven upslope/downslope model is a future
    /// enhancement (CLIM-04) once the wind field is available.
    pub orographic_strength: f64,
}

impl Default for MoistureConfig {
    fn default() -> Self {
        Self {
            substellar_lon_rad: 0.0,
            orographic_strength: 0.05,
        }
    }
}

/// Build a global moisture field.
///
/// Algorithm (design doc section 5.5):
/// 1. **Dryness check.** If H2O is absent from the retained gas set or surface
///    pressure is below `DRY_PRESSURE_EPS_BAR`, return an all-zero field. This
///    collapses Mars-like-without-H2O, airless, and H2/He regimes to zero
///    humidity.
/// 2. **Circulation base.** For tidally locked bodies, the base field is
///    `cos(substellar_angle) * TIDAL_DAYSIDE_PEAK` on the dayside, a small
///    terminator floor within `TIDAL_TERMINATOR_BAND_RAD` of the terminator,
///    and zero on the antistellar hemisphere. For rotating bodies the base is
///    a piecewise-cosine blend of four anchor points: equator ITCZ peak,
///    horse-latitude trough near 30 degrees, mid-latitude peak near 50
///    degrees, and polar floor past 75 degrees.
/// 3. **Clausius-Clapeyron ceiling.** `cc_scale = (T/288)^2 * sqrt(P/1)` is
///    computed with surface temperature and pressure, then clamped to
///    `[CEILING_MIN, CEILING_MAX]`. Interpreting the field as relative
///    humidity, this acts as a cap on the maximum reachable humidity: a cold
///    or thin atmosphere cannot get very wet even at the ITCZ.
/// 4. **Orographic modulation.** Without wind, a local slope proxy biases the
///    field: a tile higher than its neighbor mean gains
///    `cfg.orographic_strength` per km of positive slope, and loses it per km
///    of negative slope. This captures a qualitative "windward moistens,
///    leeward dries" trend in a wind-agnostic way. Fully wind-aware transport
///    is deferred to a later climate stage once CLIM-03 lands.
/// 5. **Clamp** final values to `[0, 1]`.
pub fn build_moisture_field(
    world: &SkeletonWorld,
    temperature: &TemperatureField,
    cfg: &MoistureConfig,
) -> MoistureField {
    let n = world.grid.tiles.len();

    let atmo = &world.atmosphere;
    let has_h2o = atmo.composition.contains_key(&Gas::H2O);
    let is_dry = !has_h2o || atmo.surface_pressure < DRY_PRESSURE_EPS_BAR;
    if is_dry {
        return MoistureField {
            per_tile: vec![0.0; n],
        };
    }

    // Clausius-Clapeyron ceiling, evaluated at the global reference (not
    // per-tile) so it sets a planet-scale cap rather than reacting to each
    // tile's local temperature. This keeps the field interpretable as
    // "fraction of the local maximum capacity" for a given planet.
    let t_ref = atmo.effective_surface_temp;
    let p_ref = atmo.surface_pressure;
    let cc_scale = (t_ref / CC_REF_TEMP_K).powi(2) * (p_ref / CC_REF_PRESSURE_BAR).max(0.0).sqrt();
    let ceiling = cc_scale.clamp(CEILING_MIN, CEILING_MAX);

    let body = &world.body;
    let mut per_tile = Vec::with_capacity(n);

    for (i, tile) in world.grid.tiles.iter().enumerate() {
        let base = if body.tidal_locked {
            tidal_base_humidity(tile.lat, tile.lon, cfg.substellar_lon_rad)
        } else {
            latitude_base_humidity(tile.lat)
        };

        let mut humidity = base * ceiling;

        // Orographic bias: positive when this tile is higher than its
        // neighbors' mean (likely windward shoulder / upland), negative when
        // lower (rain-shadow). Convert elevation difference from meters to
        // kilometers before applying the strength coefficient.
        let elevation_m = world.elevation.elevations_m[i];
        let neighbors = &tile.neighbors;
        if !neighbors.is_empty() {
            let nb_mean: f64 = neighbors
                .iter()
                .map(|&j| world.elevation.elevations_m[j])
                .sum::<f64>()
                / neighbors.len() as f64;
            let slope_km = (elevation_m - nb_mean) / 1000.0;
            humidity += cfg.orographic_strength * slope_km;
        }

        // Silence unused warnings from the temperature parameter; we keep it
        // in the API for future per-tile Clausius-Clapeyron work.
        let _ = temperature.per_tile_k.get(i);

        per_tile.push(humidity.clamp(0.0, 1.0));
    }

    MoistureField { per_tile }
}

/// Compute the latitude-banded base humidity for rotating planets using a
/// piecewise cosine blend of anchor points: ITCZ peak (equator), horse-latitude
/// trough (~30 deg), mid-latitude peak (~50 deg), and pole floor (>75 deg).
fn latitude_base_humidity(lat_deg: f64) -> f64 {
    let abs_lat = lat_deg.abs();

    if abs_lat <= LAT_HORSE_DEG {
        // Equator -> horse latitudes: ITCZ peak down to horse trough.
        let t = abs_lat / LAT_HORSE_DEG;
        cosine_blend(ITCZ_PEAK, HORSE_TROUGH, t)
    } else if abs_lat <= LAT_MIDLAT_DEG {
        // Horse latitudes -> mid-latitude peak.
        let t = (abs_lat - LAT_HORSE_DEG) / (LAT_MIDLAT_DEG - LAT_HORSE_DEG);
        cosine_blend(HORSE_TROUGH, MIDLAT_PEAK, t)
    } else if abs_lat <= LAT_POLE_DEG {
        // Mid-latitudes -> polar floor.
        let t = (abs_lat - LAT_MIDLAT_DEG) / (LAT_POLE_DEG - LAT_MIDLAT_DEG);
        cosine_blend(MIDLAT_PEAK, POLE_FLOOR, t)
    } else {
        POLE_FLOOR
    }
}

/// Cosine ease between `a` and `b` where `t` in `[0, 1]`.
fn cosine_blend(a: f64, b: f64, t: f64) -> f64 {
    let tc = t.clamp(0.0, 1.0);
    let w = 0.5 * (1.0 - (tc * std::f64::consts::PI).cos());
    a * (1.0 - w) + b * w
}

/// Base humidity for tidally locked planets. Dayside humidity follows
/// `cos(substellar_angle) * TIDAL_DAYSIDE_PEAK`; a narrow terminator band
/// holds a small floor; the antistellar hemisphere is dry.
fn tidal_base_humidity(lat_deg: f64, lon_deg: f64, substellar_lon_rad: f64) -> f64 {
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();
    // Substellar point assumed on the equator (design doc convention).
    let cos_angle = lat.cos() * (lon - substellar_lon_rad).cos();
    if cos_angle > 0.0 {
        cos_angle * TIDAL_DAYSIDE_PEAK
    } else {
        // Terminator band: within TIDAL_TERMINATOR_BAND_RAD of the
        // terminator, blend smoothly from the dayside curve down to zero via
        // the small terminator floor.
        let angle = cos_angle.acos(); // angle from substellar, in [pi/2, pi]
        let past_terminator = angle - std::f64::consts::FRAC_PI_2;
        if past_terminator < TIDAL_TERMINATOR_BAND_RAD {
            let t = past_terminator / TIDAL_TERMINATOR_BAND_RAD;
            TIDAL_TERMINATOR_FLOOR * (1.0 - t)
        } else {
            0.0
        }
    }
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

    use crate::temperature::{TemperatureConfig, build_temperature_field};

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

    fn mars_atmosphere_no_h2o() -> AtmosphereModel {
        let mut composition = BTreeMap::new();
        composition.insert(Gas::CO2, 0.95);
        composition.insert(Gas::N2, 0.03);
        composition.insert(Gas::Ar, 0.02);
        AtmosphereModel {
            surface_pressure: 0.006,
            composition,
            greenhouse_factor: 1.03,
            effective_surface_temp: 210.0,
            scale_height: 11.0,
            moisture_capacity: 0.0,
            uv_surface_flux: 0.8,
            class: AtmosphereClass::ThinCO2,
            retained: vec![Gas::CO2, Gas::N2, Gas::Ar],
        }
    }

    fn mars_atmosphere_with_trace_h2o() -> AtmosphereModel {
        let mut a = mars_atmosphere_no_h2o();
        // Barely above the dry-pressure floor, with trace H2O in the mix.
        a.surface_pressure = 0.012;
        a.composition.insert(Gas::H2O, 1.0e-5);
        a.retained.push(Gas::H2O);
        a
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

    fn tidal_atmosphere() -> AtmosphereModel {
        // Thin-ish but still holds water vapor.
        let mut a = earth_atmosphere();
        a.surface_pressure = 0.5;
        a.effective_surface_temp = 260.0;
        a
    }

    fn build_world(body: OrbitalBody, atmo: AtmosphereModel, seed: u64) -> SkeletonWorld {
        SkeletonWorld::build(body, atmo, 3, seed)
    }

    fn pick_nearest_latitude(world: &SkeletonWorld, target_lat: f64) -> usize {
        world
            .grid
            .tiles
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (a.lat - target_lat)
                    .abs()
                    .partial_cmp(&(b.lat - target_lat).abs())
                    .unwrap()
            })
            .map(|(i, _)| i)
            .unwrap()
    }

    #[test]
    fn dry_world_is_all_zero() {
        let world = build_world(earth_body(), mars_atmosphere_no_h2o(), 2024);
        let temp = build_temperature_field(&world, &TemperatureConfig::default());
        let field = build_moisture_field(&world, &temp, &MoistureConfig::default());
        assert_eq!(field.per_tile.len(), world.grid.tiles.len());
        for (i, &h) in field.per_tile.iter().enumerate() {
            assert_eq!(h, 0.0, "tile {i} expected zero humidity, got {h}");
        }
        assert_eq!(field.mean(), 0.0);
        assert_eq!(field.max(), 0.0);
        assert_eq!(field.coverage_above(0.01), 0.0);
    }

    #[test]
    fn earth_like_global_mean_in_range() {
        let world = build_world(earth_body(), earth_atmosphere(), 2024);
        let temp = build_temperature_field(&world, &TemperatureConfig::default());
        let field = build_moisture_field(&world, &temp, &MoistureConfig::default());
        let mean = field.mean();
        assert!(
            (0.2..=0.7).contains(&mean),
            "Earth mean humidity {mean} outside [0.2, 0.7]"
        );
        // All values finite and in [0, 1].
        for (i, &h) in field.per_tile.iter().enumerate() {
            assert!(h.is_finite(), "tile {i} humidity non-finite: {h}");
            assert!(
                (0.0..=1.0).contains(&h),
                "tile {i} humidity out of [0,1]: {h}"
            );
        }
    }

    #[test]
    fn earth_poles_drier_than_midlatitudes() {
        let world = build_world(earth_body(), earth_atmosphere(), 2024);
        let temp = build_temperature_field(&world, &TemperatureConfig::default());
        let field = build_moisture_field(&world, &temp, &MoistureConfig::default());

        let pole_idx = pick_nearest_latitude(&world, 85.0);
        let mid_idx = pick_nearest_latitude(&world, 50.0);

        let h_pole = field.per_tile[pole_idx];
        let h_mid = field.per_tile[mid_idx];
        assert!(
            h_pole < h_mid,
            "pole ({h_pole}) should be drier than mid-latitude ({h_mid})"
        );
    }

    #[test]
    fn earth_itcz_wetter_than_horse_latitudes() {
        // Disable orographic bias so latitude bands dominate — mesh-driven
        // elevation noise at a specific tile should not flip the comparison.
        let world = build_world(earth_body(), earth_atmosphere(), 2024);
        let temp = build_temperature_field(&world, &TemperatureConfig::default());
        let cfg = MoistureConfig {
            orographic_strength: 0.0,
            ..MoistureConfig::default()
        };
        let field = build_moisture_field(&world, &temp, &cfg);

        let itcz_idx = pick_nearest_latitude(&world, 0.0);
        let horse_idx = pick_nearest_latitude(&world, 30.0);

        let h_itcz = field.per_tile[itcz_idx];
        let h_horse = field.per_tile[horse_idx];
        assert!(
            h_itcz > h_horse,
            "ITCZ ({h_itcz}) should be wetter than horse-latitude ({h_horse})"
        );
    }

    #[test]
    fn mars_like_ceiling_is_very_low() {
        // Ceiling test: disable orographic bias so we isolate the
        // Clausius-Clapeyron capacity floor; the orographic term can add
        // meaningful humidity on multi-km highlands and would mask the
        // thin-atmosphere ceiling.
        let world = build_world(earth_body(), mars_atmosphere_with_trace_h2o(), 17);
        let temp = build_temperature_field(&world, &TemperatureConfig::default());
        let cfg = MoistureConfig {
            orographic_strength: 0.0,
            ..MoistureConfig::default()
        };
        let field = build_moisture_field(&world, &temp, &cfg);
        let mx = field.max();
        assert!(
            mx < 0.1,
            "Mars-like max humidity should be very low, got {mx}"
        );
        for &h in &field.per_tile {
            assert!((0.0..=1.0).contains(&h), "Mars humidity out of [0,1]: {h}");
        }
    }

    #[test]
    fn tidally_locked_substellar_wetter_than_antistellar() {
        let world = build_world(tidal_body(), tidal_atmosphere(), 2024);
        let cfg = MoistureConfig {
            orographic_strength: 0.0,
            ..MoistureConfig::default()
        };
        let temp = build_temperature_field(
            &world,
            &TemperatureConfig {
                substellar_lon_rad: 0.0,
                lapse_rate_override: None,
            },
        );
        let field = build_moisture_field(&world, &temp, &cfg);

        // Nearest tile to substellar (0, 0).
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
        // Nearest tile to antistellar (0, +/-180).
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

        let h_sub = field.per_tile[sub_idx];
        let h_anti = field.per_tile[anti_idx];
        assert!(
            h_sub > h_anti,
            "substellar ({h_sub}) should be wetter than antistellar ({h_anti})"
        );
    }

    #[test]
    fn deterministic_same_input_same_output() {
        let a = build_world(earth_body(), earth_atmosphere(), 7);
        let b = build_world(earth_body(), earth_atmosphere(), 7);
        let ta = build_temperature_field(&a, &TemperatureConfig::default());
        let tb = build_temperature_field(&b, &TemperatureConfig::default());
        let fa = build_moisture_field(&a, &ta, &MoistureConfig::default());
        let fb = build_moisture_field(&b, &tb, &MoistureConfig::default());
        assert_eq!(fa.per_tile, fb.per_tile);
    }

    #[test]
    fn all_values_finite_and_bounded() {
        let world = build_world(earth_body(), earth_atmosphere(), 99);
        let temp = build_temperature_field(&world, &TemperatureConfig::default());
        let field = build_moisture_field(&world, &temp, &MoistureConfig::default());
        for (i, &h) in field.per_tile.iter().enumerate() {
            assert!(h.is_finite(), "tile {i} non-finite humidity: {h}");
            assert!(
                (0.0..=1.0).contains(&h),
                "tile {i} humidity out of [0,1]: {h}"
            );
        }
    }

    #[test]
    fn mean_min_max_coverage_accessors() {
        let field = MoistureField {
            per_tile: vec![0.0, 0.25, 0.5, 0.75, 1.0],
        };
        assert!((field.mean() - 0.5).abs() < 1e-9);
        assert!((field.min() - 0.0).abs() < 1e-9);
        assert!((field.max() - 1.0).abs() < 1e-9);
        assert!((field.coverage_above(0.5) - 0.6).abs() < 1e-9);
        assert!((field.coverage_above(1.01) - 0.0).abs() < 1e-9);
    }
}
