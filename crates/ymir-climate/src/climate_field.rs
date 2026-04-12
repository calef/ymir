//! The `ClimateMap` type bundling the three climate subfields (temperature,
//! moisture, wind) produced by Stage 5 of the pipeline.
//!
//! Provides a one-call [`ClimateMap::build`] entry point that evaluates the
//! fields in the correct dependency order:
//! 1. [`TemperatureField`] (independent of the others),
//! 2. [`WindField`] (independent of the others in v1; the wind-transport
//!    coupling into moisture is a future enhancement),
//! 3. [`MoistureField`] (depends on temperature; wind is passed alongside for
//!    forward compatibility with an upslope/downslope transport term).

use crate::moisture::{MoistureConfig, MoistureField, build_moisture_field};
use crate::temperature::{TemperatureConfig, TemperatureField, build_temperature_field};
use crate::wind::{WindConfig, WindField, build_wind_field};
use serde::{Deserialize, Serialize};
use ymir_surface::SkeletonWorld;

/// Configuration bundle for the three climate subcomponents.
///
/// Each field is the configuration struct used by the corresponding
/// subcomponent's builder. `Default` yields defaults for all three.
#[derive(Clone, Debug, Default)]
pub struct ClimateConfig {
    /// Configuration for the temperature field.
    pub temperature: TemperatureConfig,
    /// Configuration for the moisture field.
    pub moisture: MoistureConfig,
    /// Configuration for the wind field.
    pub wind: WindConfig,
}

/// The global climate state for a skeleton world: temperature, moisture, and
/// wind.
///
/// All three subfields index into tiles in the same order as
/// `SkeletonWorld::grid.tiles`, so `tile_count` is the shared length.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClimateMap {
    /// Per-tile surface temperature field in Kelvin.
    pub temperature: TemperatureField,
    /// Per-tile relative humidity field in `[0, 1]`.
    pub moisture: MoistureField,
    /// Per-tile wind field with eastward/northward components in m/s.
    pub wind: WindField,
}

impl ClimateMap {
    /// Build all three fields in the required order: temperature first
    /// (independent), wind second (independent), moisture last (depends on
    /// temperature; wind-transport coupling is deferred to a future
    /// enhancement).
    pub fn build(world: &SkeletonWorld, cfg: &ClimateConfig) -> Self {
        let temperature = build_temperature_field(world, &cfg.temperature);
        let wind = build_wind_field(world, &cfg.wind);
        let moisture = build_moisture_field(world, &temperature, &cfg.moisture);
        Self {
            temperature,
            moisture,
            wind,
        }
    }

    /// Number of tiles (same across all three fields).
    pub fn tile_count(&self) -> usize {
        self.temperature.per_tile_k.len()
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

    fn earth_like_world(seed: u64) -> SkeletonWorld {
        SkeletonWorld::build(earth_body(), earth_atmosphere(), 3, seed)
    }

    #[test]
    fn build_is_deterministic() {
        let world_a = earth_like_world(42);
        let world_b = earth_like_world(42);
        let cfg = ClimateConfig::default();
        let a = ClimateMap::build(&world_a, &cfg);
        let b = ClimateMap::build(&world_b, &cfg);

        assert_eq!(a.temperature.per_tile_k, b.temperature.per_tile_k);
        assert_eq!(a.moisture.per_tile, b.moisture.per_tile);
        assert_eq!(a.wind.cell_count, b.wind.cell_count);
        assert_eq!(a.wind.per_tile.len(), b.wind.per_tile.len());
        for (i, (wa, wb)) in a
            .wind
            .per_tile
            .iter()
            .zip(b.wind.per_tile.iter())
            .enumerate()
        {
            assert_eq!(wa.u, wb.u, "tile {i} u differs");
            assert_eq!(wa.v, wb.v, "tile {i} v differs");
        }
    }

    #[test]
    fn tile_counts_match_across_fields() {
        let world = earth_like_world(2024);
        let map = ClimateMap::build(&world, &ClimateConfig::default());
        let n = world.grid.tiles.len();
        assert_eq!(map.temperature.per_tile_k.len(), n);
        assert_eq!(map.moisture.per_tile.len(), n);
        assert_eq!(map.wind.per_tile.len(), n);
        assert_eq!(map.tile_count(), n);
    }

    #[test]
    fn serde_round_trip() {
        let world = earth_like_world(2024);
        let map = ClimateMap::build(&world, &ClimateConfig::default());

        let bytes = bincode::serialize(&map).expect("serialize");
        let decoded: ClimateMap = bincode::deserialize(&bytes).expect("deserialize");

        assert_eq!(
            decoded.temperature.per_tile_k.len(),
            map.temperature.per_tile_k.len()
        );
        assert_eq!(decoded.moisture.per_tile.len(), map.moisture.per_tile.len());
        assert_eq!(decoded.wind.per_tile.len(), map.wind.per_tile.len());
        assert_eq!(decoded.wind.cell_count, map.wind.cell_count);

        // Sampling of tile values at a few indices.
        let n = map.tile_count();
        let sample_indices = [0usize, n / 4, n / 2, (3 * n) / 4, n - 1];
        for &i in &sample_indices {
            assert_eq!(
                decoded.temperature.per_tile_k[i], map.temperature.per_tile_k[i],
                "temperature tile {i} differs after round trip"
            );
            assert_eq!(
                decoded.moisture.per_tile[i], map.moisture.per_tile[i],
                "moisture tile {i} differs after round trip"
            );
            assert_eq!(
                decoded.wind.per_tile[i].u, map.wind.per_tile[i].u,
                "wind u tile {i} differs after round trip"
            );
            assert_eq!(
                decoded.wind.per_tile[i].v, map.wind.per_tile[i].v,
                "wind v tile {i} differs after round trip"
            );
        }
    }

    #[test]
    fn earth_like_has_reasonable_globals() {
        let world = earth_like_world(2024);
        let map = ClimateMap::build(&world, &ClimateConfig::default());

        let mean_temp = map.temperature.mean();
        assert!(
            (280.0..=300.0).contains(&mean_temp),
            "Earth-like mean temperature {mean_temp} K outside [280, 300]"
        );

        let mean_humidity = map.moisture.mean();
        assert!(
            (0.2..=0.7).contains(&mean_humidity),
            "Earth-like mean humidity {mean_humidity} outside [0.2, 0.7]"
        );

        let mean_wind = map.wind.mean_speed();
        assert!(
            (0.5..=20.0).contains(&mean_wind),
            "Earth-like mean wind speed {mean_wind} m/s outside [0.5, 20.0]"
        );
    }

    #[test]
    fn airless_world_has_zero_winds() {
        let mut atmo = earth_atmosphere();
        atmo.surface_pressure = 0.0;
        atmo.class = AtmosphereClass::None;
        let world = SkeletonWorld::build(earth_body(), atmo, 3, 7);
        let map = ClimateMap::build(&world, &ClimateConfig::default());
        assert_eq!(map.wind.cell_count, 0);
        for (i, w) in map.wind.per_tile.iter().enumerate() {
            assert_eq!(w.u, 0.0, "tile {i} u nonzero on airless world");
            assert_eq!(w.v, 0.0, "tile {i} v nonzero on airless world");
        }
    }
}
