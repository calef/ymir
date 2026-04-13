//! Prevailing wind pattern computation from planetary rotation, temperature
//! differentials, and surface topology.
//!
//! Implements the Stage 5 wind model from design doc section 5.5.
//!
//! Two regimes are supported:
//!
//! - **Rotating planets:** latitude-banded prevailing winds alternating between
//!   easterlies and westerlies. Cell count scales with rotation rate (faster
//!   rotation → narrower, more numerous cells), following a rough
//!   `sqrt(rotation_ratio) * 6` heuristic documented inline.
//! - **Tidally locked planets:** a single radial outflow cell from the
//!   substellar point toward the antistellar point, projected into local
//!   tangent coordinates (east/north).
//!
//! Airless worlds (surface pressure below a trace threshold) yield zero
//! winds and `cell_count = 0`.

use serde::{Deserialize, Serialize};
use ymir_surface::skeleton::SkeletonWorld;

/// Surface pressure (bar) below which the atmosphere is treated as effectively
/// absent for wind purposes. Matches the convention used in [`temperature`].
const ATMOSPHERE_PRESSURE_EPS_BAR: f64 = 0.01;

/// Baseline default wind speed for Earth-like inputs, m/s. See
/// [`WindConfig::default`].
const DEFAULT_BASE_SPEED_MS: f64 = 5.0;

/// Earth reference rotation period in hours, used to non-dimensionalize the
/// rotation rate in the cell-count formula.
const EARTH_ROTATION_HOURS: f64 = 24.0;

/// Multiplier in the cell-count formula. Chosen so Earth (24 h) yields 6 cells
/// (3 Hadley/Ferrel/Polar per hemisphere × 2 hemispheres).
const CELL_COUNT_MULTIPLIER: f64 = 6.0;

/// Minimum cell count for rotating planets. A body with two hemispheric cells
/// is the floor (one cell per hemisphere, no mid-latitude westerlies).
const MIN_CELL_COUNT: u32 = 2;

/// Upper bound on cell count; prevents runaway for very fast rotators and
/// keeps band widths wider than a few degrees at typical grid resolutions.
const MAX_CELL_COUNT: u32 = 24;

/// Empirical strengthening factor for tidally locked winds. Models of
/// tidally locked exoplanet atmospheres (e.g. substellar convection +
/// day-night overturning) tend to produce surface winds 1.5–3x stronger than
/// equivalent Earth-like latitudinal flows; 2.0 is a convenient middle value.
const TIDAL_LOCKED_SPEED_FACTOR: f64 = 2.0;

/// A local wind vector in a tile's surface tangent frame.
///
/// `u` is the eastward component in m/s (positive eastward) and `v` is the
/// northward component in m/s (positive northward). For tidally locked worlds
/// this is a cartesian projection of the radial flow from the substellar
/// point onto the local tangent plane.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct WindVector {
    /// Eastward wind speed in m/s.
    pub u: f32,
    /// Northward wind speed in m/s.
    pub v: f32,
}

impl WindVector {
    /// Wind speed magnitude in m/s.
    pub fn speed(self) -> f64 {
        ((self.u as f64).powi(2) + (self.v as f64).powi(2)).sqrt()
    }
}

/// Per-tile wind field for a [`SkeletonWorld`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindField {
    /// Wind vector for each geodesic tile in the same order as
    /// `SkeletonWorld::grid.tiles`.
    pub per_tile: Vec<WindVector>,
    /// Number of latitude (or radial) cells. For rotating planets this is
    /// the total number of Hadley/Ferrel/Polar cells per hemisphere times
    /// two (i.e. the full-globe count). For tidally locked planets this is
    /// `1` (substellar → antistellar). For airless worlds this is `0`.
    pub cell_count: u32,
}

impl WindField {
    /// Arithmetic mean wind speed across all tiles, in m/s. Returns 0 if empty.
    pub fn mean_speed(&self) -> f64 {
        if self.per_tile.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.per_tile.iter().map(|w| w.speed()).sum();
        sum / self.per_tile.len() as f64
    }

    /// Maximum per-tile wind speed, in m/s. Returns 0 if empty.
    pub fn max_speed(&self) -> f64 {
        self.per_tile
            .iter()
            .map(|w| w.speed())
            .fold(0.0_f64, f64::max)
    }
}

/// Configuration for [`build_wind_field`].
#[derive(Clone, Debug)]
pub struct WindConfig {
    /// Substellar-point longitude in radians. Used only for tidally locked
    /// bodies. Default `0.0` places the substellar point at the prime
    /// meridian on the equator.
    pub substellar_lon_rad: f64,
    /// Baseline wind-speed scale in m/s. Earth-like default of ~5 m/s.
    pub base_speed_ms: f64,
}

impl Default for WindConfig {
    fn default() -> Self {
        Self {
            substellar_lon_rad: 0.0,
            base_speed_ms: DEFAULT_BASE_SPEED_MS,
        }
    }
}

/// Compute the number of global circulation cells for a rotating planet.
///
/// Heuristic: `cell_count = ((24 h / rotation_period_hours).sqrt() * 6).round()`,
/// clamped to `[2, 24]` and rounded to the nearest even integer so the
/// pattern is symmetric across the equator.
///
/// Intuition:
/// - Earth (24 h) → `sqrt(1) * 6 = 6` cells (3 per hemisphere).
/// - Fast rotator (6 h) → `sqrt(4) * 6 = 12` cells.
/// - Slow rotator (240 h) → `sqrt(0.1) * 6 ≈ 1.9` → clamped to 2.
fn rotating_cell_count(rotation_period_hours: f64) -> u32 {
    let ratio = if rotation_period_hours > 0.0 {
        EARTH_ROTATION_HOURS / rotation_period_hours
    } else {
        1.0
    };
    let raw = (ratio.sqrt() * CELL_COUNT_MULTIPLIER).round();
    let clamped = raw.clamp(MIN_CELL_COUNT as f64, MAX_CELL_COUNT as f64) as u32;
    // Force even so the band layout is mirror-symmetric across the equator.
    if clamped.is_multiple_of(2) {
        clamped
    } else {
        (clamped + 1).min(MAX_CELL_COUNT)
    }
}

/// Build a per-tile wind field for the given [`SkeletonWorld`].
///
/// Algorithm (design doc section 5.5):
///
/// 1. If the atmosphere is below the trace pressure threshold, return a zero
///    field with `cell_count = 0`.
/// 2. For tidally locked bodies, set `cell_count = 1` and compute a radial
///    outflow from the substellar point. The per-tile flow direction in the
///    `(u, v)` tangent basis uses the simplification
///    `u ∝ sin(Δlon) * cos(lat)`, `v ∝ sin(lat)`, which is the lowest-order
///    tangent projection of the surface gradient of the substellar angle.
///    Speed follows `base_speed * 2 * sin(substellar_angle)` so it is zero at
///    the substellar and antistellar points and maximal at the terminator.
///    The extra factor of 2 reflects the empirically stronger circulation
///    observed in tidally locked atmosphere models.
/// 3. For rotating bodies, derive a cell count from rotation rate, split the
///    globe into `cell_count` equal-latitude bands (half per hemisphere), and
///    alternate easterly/westerly u-components starting with easterly at the
///    equator (trade winds), then westerly, then polar easterly, and so on.
///    Speed magnitude varies sinusoidally across each band, peaking in the
///    middle and going to zero at band boundaries.
///    Meridional (north-south) transport is set to zero in v1; Phase 3 will
///    add it.
pub fn build_wind_field(world: &SkeletonWorld, cfg: &WindConfig) -> WindField {
    let n = world.grid.tiles.len();
    let mut per_tile = Vec::with_capacity(n);

    // Airless / trace-atmosphere worlds: zero winds, zero cells.
    if *world.atmosphere.surface_pressure.inner() < ATMOSPHERE_PRESSURE_EPS_BAR {
        per_tile.resize(n, WindVector::default());
        return WindField {
            per_tile,
            cell_count: 0,
        };
    }

    let body = &world.body;
    let base = cfg.base_speed_ms;

    if *body.tidal_locked.inner() {
        // Single radial cell from substellar to antistellar.
        let sub_lon = cfg.substellar_lon_rad;
        for tile in &world.grid.tiles {
            let lat = tile.lat.to_radians();
            let lon = tile.lon.to_radians();

            // Angular distance from the substellar point (substellar_lat = 0).
            let cos_angle = lat.cos() * (lon - sub_lon).cos();
            let sin_angle = (1.0 - cos_angle.powi(2)).max(0.0).sqrt();

            // Simplified tangent direction pointing away from the substellar
            // point: u ∝ sin(Δlon) * cos(lat), v ∝ sin(lat).
            let d_lon = lon - sub_lon;
            let dir_u = d_lon.sin() * lat.cos();
            let dir_v = lat.sin();
            let dir_mag = (dir_u * dir_u + dir_v * dir_v).sqrt();

            let speed = base * TIDAL_LOCKED_SPEED_FACTOR * sin_angle;

            let (u, v) = if dir_mag > 1e-9 {
                (speed * dir_u / dir_mag, speed * dir_v / dir_mag)
            } else {
                // At the substellar (or antistellar) singularity, speed is
                // already ~0; emit a zero vector rather than NaN.
                (0.0, 0.0)
            };

            per_tile.push(WindVector {
                u: u as f32,
                v: v as f32,
            });
        }

        return WindField {
            per_tile,
            cell_count: 1,
        };
    }

    // Rotating planet: latitude-banded prevailing winds.
    let cell_count = rotating_cell_count(*body.rotation_period.inner());
    let cells_per_hemisphere = (cell_count / 2).max(1);
    // Band width in radians of latitude.
    let band_width = std::f64::consts::FRAC_PI_2 / cells_per_hemisphere as f64;

    for tile in &world.grid.tiles {
        let lat = tile.lat.to_radians();
        let abs_lat = lat.abs();

        // Band index in [0, cells_per_hemisphere). 0 is equatorial, the last
        // index is polar. Clamp to handle tiles exactly at a pole.
        let band_idx = ((abs_lat / band_width).floor() as u32).min(cells_per_hemisphere - 1);

        // Direction: alternate easterly / westerly starting with easterly at
        // the equator. Even bands (0, 2, ...) are easterly (u<0); odd bands
        // are westerly (u>0). For Earth's 3 cells per hemisphere this gives
        // trade → westerly → polar easterly.
        let easterly = band_idx.is_multiple_of(2);

        // Band-local position in radians, running 0 at the inner edge to
        // π at the outer edge. Speed = base * (0.5 + 0.5 * sin(band_position))
        // peaks at the band midpoint and tapers to half of base at edges.
        let band_inner_lat = band_idx as f64 * band_width;
        let band_position = (abs_lat - band_inner_lat) / band_width * std::f64::consts::PI;
        let speed_mag = base * (0.5 + 0.5 * band_position.sin());

        let sign = if easterly { -1.0 } else { 1.0 };
        let u = sign * speed_mag;

        // Meridional transport deferred to Phase 3.
        let v = 0.0;

        per_tile.push(WindVector {
            u: u as f32,
            v: v as f32,
        });
    }

    WindField {
        per_tile,
        cell_count,
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
    use ymir_core::Sourced;
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

    fn tidal_body() -> OrbitalBody {
        let mut b = earth_body();
        b.tidal_locked = Sourced::derived(true, "test");
        b.rotation_period = d(24.0 * 300.0);
        b.solar_irradiance = d(900.0);
        b.equilibrium_temp = d(230.0);
        b.name = Some("TidalWorld".into());
        b
    }

    fn build_world(body: OrbitalBody, atmo: AtmosphereModel, seed: u64) -> SkeletonWorld {
        SkeletonWorld::build(body, atmo, 3, seed)
    }

    #[test]
    fn cell_count_earth_like_is_six() {
        assert_eq!(rotating_cell_count(24.0), 6);
    }

    #[test]
    fn cell_count_fast_rotator_greater_than_earth() {
        let fast = rotating_cell_count(6.0);
        assert!(fast > 6, "fast rotator cell count {fast} not > 6");
    }

    #[test]
    fn cell_count_slow_rotator_near_two() {
        let slow = rotating_cell_count(240.0);
        assert!(slow <= 4, "slow rotator cell count {slow} not near 2");
        assert!(slow >= 2, "slow rotator cell count {slow} below floor");
    }

    #[test]
    fn cell_count_is_clamped_and_even() {
        assert_eq!(rotating_cell_count(0.001), MAX_CELL_COUNT);
        assert_eq!(rotating_cell_count(10_000.0), MIN_CELL_COUNT);
        for &h in &[1.0_f64, 3.0, 6.0, 12.0, 24.0, 48.0, 100.0, 240.0] {
            let c = rotating_cell_count(h);
            assert!(c.is_multiple_of(2), "cell count {c} not even for {h}h");
        }
    }

    #[test]
    fn earth_equatorial_tile_is_easterly() {
        let world = build_world(earth_body(), earth_atmosphere(), 2024);
        let cfg = WindConfig::default();
        let field = build_wind_field(&world, &cfg);
        assert_eq!(field.cell_count, 6);

        // Tile closest to the equator.
        let (eq_idx, _) = world
            .grid
            .tiles
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.lat.abs().partial_cmp(&b.lat.abs()).unwrap())
            .unwrap();
        let w = field.per_tile[eq_idx];
        assert!(
            w.u < 0.0,
            "equatorial tile expected easterly (u<0), got u={}",
            w.u
        );
    }

    #[test]
    fn earth_midlatitude_tile_is_westerly() {
        let world = build_world(earth_body(), earth_atmosphere(), 2024);
        let field = build_wind_field(&world, &WindConfig::default());

        // Tile closest to 45 degrees latitude (either hemisphere).
        let (mid_idx, _) = world
            .grid
            .tiles
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = (a.lat.abs() - 45.0).abs();
                let db = (b.lat.abs() - 45.0).abs();
                da.partial_cmp(&db).unwrap()
            })
            .unwrap();
        let w = field.per_tile[mid_idx];
        assert!(
            w.u > 0.0,
            "mid-latitude tile expected westerly (u>0), got u={}",
            w.u
        );
    }

    #[test]
    fn tidally_locked_single_cell_and_outflow_direction() {
        let world = build_world(tidal_body(), earth_atmosphere(), 2024);
        let cfg = WindConfig::default();
        let field = build_wind_field(&world, &cfg);
        assert_eq!(field.cell_count, 1);

        // Tile nearest the substellar point (0, 0).
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

        // Tile nearest the terminator (~90 deg from substellar).
        let term_idx = world
            .grid
            .tiles
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = (a.lat.abs().powi(2) + (a.lon.abs() - 90.0).powi(2)).sqrt();
                let db = (b.lat.abs().powi(2) + (b.lon.abs() - 90.0).powi(2)).sqrt();
                da.partial_cmp(&db).unwrap()
            })
            .map(|(i, _)| i)
            .unwrap();

        let sub_speed = field.per_tile[sub_idx].speed();
        let term_speed = field.per_tile[term_idx].speed();
        assert!(
            sub_speed < 0.5,
            "substellar speed should be ~0, got {sub_speed}"
        );
        assert!(
            term_speed > sub_speed,
            "terminator speed ({term_speed}) should exceed substellar ({sub_speed})"
        );

        // Outflow check: find a tile at positive longitude (east of
        // substellar) and confirm u >= 0 (flow away from substellar).
        let east_idx = world
            .grid
            .tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| t.lon > 30.0 && t.lon < 150.0 && t.lat.abs() < 10.0)
            .min_by(|(_, a), (_, b)| {
                let da = (a.lon - 90.0).abs();
                let db = (b.lon - 90.0).abs();
                da.partial_cmp(&db).unwrap()
            })
            .map(|(i, _)| i);
        if let Some(i) = east_idx {
            let w = field.per_tile[i];
            assert!(
                w.u >= 0.0,
                "east-of-substellar tile should flow east (u>=0), got u={}",
                w.u
            );
        }
    }

    #[test]
    fn airless_world_has_zero_winds_and_zero_cells() {
        let mut atmo = earth_atmosphere();
        atmo.surface_pressure = d(0.0);
        atmo.class = AtmosphereClass::None;
        let world = build_world(earth_body(), atmo, 13);
        let field = build_wind_field(&world, &WindConfig::default());
        assert_eq!(field.cell_count, 0);
        for (i, w) in field.per_tile.iter().enumerate() {
            assert_eq!(w.u, 0.0, "tile {i} u nonzero on airless world");
            assert_eq!(w.v, 0.0, "tile {i} v nonzero on airless world");
        }
    }

    #[test]
    fn deterministic_same_input_same_output() {
        let a = build_world(earth_body(), earth_atmosphere(), 7);
        let b = build_world(earth_body(), earth_atmosphere(), 7);
        let fa = build_wind_field(&a, &WindConfig::default());
        let fb = build_wind_field(&b, &WindConfig::default());
        assert_eq!(fa.cell_count, fb.cell_count);
        assert_eq!(fa.per_tile.len(), fb.per_tile.len());
        for (i, (wa, wb)) in fa.per_tile.iter().zip(fb.per_tile.iter()).enumerate() {
            assert_eq!(wa.u, wb.u, "tile {i} u differs");
            assert_eq!(wa.v, wb.v, "tile {i} v differs");
        }
    }

    #[test]
    fn all_components_are_finite() {
        let world = build_world(earth_body(), earth_atmosphere(), 99);
        let field = build_wind_field(&world, &WindConfig::default());
        for (i, w) in field.per_tile.iter().enumerate() {
            assert!(w.u.is_finite(), "tile {i} u non-finite");
            assert!(w.v.is_finite(), "tile {i} v non-finite");
        }

        // Also check the tidally locked branch.
        let tl_world = build_world(tidal_body(), earth_atmosphere(), 99);
        let tl_field = build_wind_field(&tl_world, &WindConfig::default());
        for (i, w) in tl_field.per_tile.iter().enumerate() {
            assert!(w.u.is_finite(), "tidal tile {i} u non-finite");
            assert!(w.v.is_finite(), "tidal tile {i} v non-finite");
        }
    }

    #[test]
    fn earth_mean_speed_in_plausible_range() {
        let world = build_world(earth_body(), earth_atmosphere(), 2024);
        let field = build_wind_field(&world, &WindConfig::default());
        let mean = field.mean_speed();
        assert!(
            (0.1..=20.0).contains(&mean),
            "Earth mean wind speed {mean} m/s out of plausible range"
        );
        // The max should not exceed a couple of times the base speed given
        // the sinusoidal band profile.
        let max = field.max_speed();
        assert!(max <= DEFAULT_BASE_SPEED_MS * 2.0 + 0.5, "max={max}");
    }

    #[test]
    fn mean_and_max_accessors_empty() {
        let empty = WindField {
            per_tile: vec![],
            cell_count: 0,
        };
        assert_eq!(empty.mean_speed(), 0.0);
        assert_eq!(empty.max_speed(), 0.0);
    }
}
