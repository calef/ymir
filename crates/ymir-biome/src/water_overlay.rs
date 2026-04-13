//! Water-biome overlay pass that paints Ocean, CoastalShallow, Wetland, and
//! AlpineMeadow biomes from skeleton elevation and climate.
//!
//! The raw Whittaker classifier in [`crate::whittaker`] only looks at
//! temperature and relative humidity. Several biomes require elevation and
//! edge-aware context that Whittaker cannot supply: sub-sea-level ocean vs
//! shallow shelf, low-lying waterlogged wetlands, and high-altitude alpine
//! meadows. This module runs after Whittaker classification and before the
//! Markov smoothing pass to overlay those biomes where the underlying
//! elevation + climate signal supports them.
//!
//! Sea level is always 0 m in the skeleton's elevation map; the heightmap
//! stage (`SURF-03` / `CAT-04`) calibrates the shift so that the observed
//! continental-fraction target is hit.
//!
//! Only the Earth-like palette uses the water and alpine biomes; other
//! palettes are left untouched by this pass. Palette membership is already
//! enforced by `BIOME-01`, so non-Earth-like worlds never see a painted
//! Ocean/CoastalShallow/Wetland/AlpineMeadow biome.

use crate::palette::{Biome, BiomePalette};
use serde::{Deserialize, Serialize};
use ymir_climate::ClimateMap;
use ymir_surface::SkeletonWorld;

/// Elevation (m) at or below which a sub-sea-level tile is classified as
/// deep Ocean rather than CoastalShallow. Tuned to Earth's continental-shelf
/// break (~200 m). Elevations in `[-200, 0)` become CoastalShallow; below
/// -200 m the tile is Ocean.
pub const DEEP_OCEAN_DEPTH_M: f64 = 200.0;

/// Upper elevation bound (m) for Wetland classification. Wetlands sit just
/// above sea level; anything taller is drained too efficiently to stay
/// waterlogged.
pub const WETLAND_MAX_ELEVATION_M: f64 = 100.0;

/// Humidity above which a low-elevation tile is classified as Wetland.
pub const WETLAND_HUMIDITY_THRESHOLD: f64 = 0.7;

/// Elevation (m) above which an Earth-like tile with moderate moisture is
/// classified as AlpineMeadow. Roughly matches Earth's mid-latitude tree
/// line on high mountains.
pub const ALPINE_MIN_ELEVATION_M: f64 = 2500.0;

/// Upper temperature bound (K) for the AlpineMeadow overlay. Above this, the
/// tile is too warm even at altitude for the alpine signature.
pub const ALPINE_MAX_TEMP_K: f64 = 283.0;

/// Lower temperature bound (K) for the AlpineMeadow overlay. Below this the
/// ground is frozen year-round and the Whittaker pick (IceSheet / Tundra)
/// is more appropriate.
pub const ALPINE_MIN_TEMP_K: f64 = 260.0;

/// Minimum humidity for AlpineMeadow. Too dry and the tile stays cold desert
/// or bedrock.
pub const ALPINE_MIN_HUMIDITY: f64 = 0.3;

/// Maximum humidity for AlpineMeadow. Wetter than this and the tile is
/// better classified as a forest or wetland variant.
pub const ALPINE_MAX_HUMIDITY: f64 = 0.8;

/// Configuration for the water-biome overlay pass.
///
/// All thresholds default to the Earth-calibrated values at the top of this
/// module. Disable the pass entirely by setting `enabled` to `false`; that
/// path is used by tests that want to observe raw Whittaker output.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WaterOverlayConfig {
    /// Whether to run the overlay at all. Defaults to `true`.
    pub enabled: bool,
    /// Depth (m) above which a sub-sea-level tile is CoastalShallow instead
    /// of Ocean. Tiles with `elevation <= -deep_ocean_depth_m` become Ocean.
    pub deep_ocean_depth_m: f64,
    /// Upper elevation bound for Wetland classification.
    pub wetland_max_elevation_m: f64,
    /// Humidity threshold for Wetland classification.
    pub wetland_humidity_threshold: f64,
    /// Elevation threshold for AlpineMeadow classification.
    pub alpine_min_elevation_m: f64,
    /// Upper temperature bound (K) for AlpineMeadow classification.
    pub alpine_max_temp_k: f64,
    /// Lower temperature bound (K) for AlpineMeadow classification.
    pub alpine_min_temp_k: f64,
    /// Minimum humidity for AlpineMeadow classification.
    pub alpine_min_humidity: f64,
    /// Maximum humidity for AlpineMeadow classification.
    pub alpine_max_humidity: f64,
}

impl Default for WaterOverlayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            deep_ocean_depth_m: DEEP_OCEAN_DEPTH_M,
            wetland_max_elevation_m: WETLAND_MAX_ELEVATION_M,
            wetland_humidity_threshold: WETLAND_HUMIDITY_THRESHOLD,
            alpine_min_elevation_m: ALPINE_MIN_ELEVATION_M,
            alpine_max_temp_k: ALPINE_MAX_TEMP_K,
            alpine_min_temp_k: ALPINE_MIN_TEMP_K,
            alpine_min_humidity: ALPINE_MIN_HUMIDITY,
            alpine_max_humidity: ALPINE_MAX_HUMIDITY,
        }
    }
}

/// Apply the water-biome overlay in place.
///
/// The overlay rewrites per-tile entries in `biomes` whenever the
/// elevation + climate signal indicates Ocean, CoastalShallow, Wetland, or
/// AlpineMeadow. Non-Earth-like palettes are left untouched because those
/// biomes are not in their palette membership tables (see `BIOME-01`).
///
/// # Panics
///
/// Panics if the lengths of `biomes`, `world.grid.tiles`,
/// `climate.temperature.per_tile_k`, and `climate.moisture.per_tile` disagree.
pub fn apply_water_overlay(
    biomes: &mut [Biome],
    palette: BiomePalette,
    world: &SkeletonWorld,
    climate: &ClimateMap,
    cfg: &WaterOverlayConfig,
) {
    if !cfg.enabled {
        return;
    }
    // Only the Earth-like palette contains the water / alpine biomes.
    if palette != BiomePalette::EarthLike {
        return;
    }

    let n = biomes.len();
    assert_eq!(
        n,
        world.grid.tiles.len(),
        "biome vector length must match grid tile count"
    );
    assert_eq!(
        n,
        climate.temperature.per_tile_k.len(),
        "temperature field length must match biome vector length"
    );
    assert_eq!(
        n,
        climate.moisture.per_tile.len(),
        "moisture field length must match biome vector length"
    );

    for (i, biome) in biomes.iter_mut().enumerate() {
        let elevation = world.elevation.elevations_m[i];
        let temp_k = climate.temperature.per_tile_k[i];
        let humidity = climate.moisture.per_tile[i];

        if elevation < 0.0 {
            // Sub-sea-level: Ocean (deep) or CoastalShallow (continental shelf).
            *biome = if elevation <= -cfg.deep_ocean_depth_m {
                Biome::Ocean
            } else {
                Biome::CoastalShallow
            };
            continue;
        }

        // Above sea level. Wetlands take priority over alpine since they are
        // a low-elevation signature and alpine requires high elevation.
        if elevation <= cfg.wetland_max_elevation_m && humidity > cfg.wetland_humidity_threshold {
            *biome = Biome::Wetland;
            continue;
        }

        if elevation >= cfg.alpine_min_elevation_m
            && temp_k >= cfg.alpine_min_temp_k
            && temp_k <= cfg.alpine_max_temp_k
            && humidity >= cfg.alpine_min_humidity
            && humidity <= cfg.alpine_max_humidity
        {
            *biome = Biome::AlpineMeadow;
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
    use ymir_climate::{ClimateConfig, ClimateMap};
    use ymir_core::sourced::Sourced;
    use ymir_surface::SkeletonWorld;
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
            continental_fraction: Some(Sourced::observed(0.29, "ETOPO1", "global topography")),
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

    fn earth_like_bundle(seed: u64) -> (SkeletonWorld, ClimateMap) {
        let world = SkeletonWorld::build(earth_body(), earth_atmosphere(), 3, seed);
        let climate = ClimateMap::build(&world, &ClimateConfig::default());
        (world, climate)
    }

    #[test]
    fn disabled_is_noop() {
        let (world, climate) = earth_like_bundle(2024);
        let mut biomes = vec![Biome::Grassland; world.grid.tiles.len()];
        let before = biomes.clone();
        let cfg = WaterOverlayConfig {
            enabled: false,
            ..Default::default()
        };
        apply_water_overlay(&mut biomes, BiomePalette::EarthLike, &world, &climate, &cfg);
        assert_eq!(biomes, before);
    }

    #[test]
    fn non_earth_palette_is_noop() {
        let (world, climate) = earth_like_bundle(2024);
        // Pretend the palette is Mars-like; the overlay should leave the
        // biome vector alone even though the world/climate has sub-sea-level
        // tiles.
        let mut biomes = vec![Biome::MartianDustPlain; world.grid.tiles.len()];
        let before = biomes.clone();
        apply_water_overlay(
            &mut biomes,
            BiomePalette::MarsLike,
            &world,
            &climate,
            &WaterOverlayConfig::default(),
        );
        assert_eq!(biomes, before);
    }

    #[test]
    fn sub_sea_level_tiles_become_water() {
        let (world, climate) = earth_like_bundle(1);
        let n = world.grid.tiles.len();
        let mut biomes = vec![Biome::Grassland; n];
        apply_water_overlay(
            &mut biomes,
            BiomePalette::EarthLike,
            &world,
            &climate,
            &WaterOverlayConfig::default(),
        );

        for (i, &b) in biomes.iter().enumerate().take(n) {
            let elev = world.elevation.elevations_m[i];
            if elev < 0.0 {
                let expected = if elev <= -DEEP_OCEAN_DEPTH_M {
                    Biome::Ocean
                } else {
                    Biome::CoastalShallow
                };
                assert_eq!(
                    b, expected,
                    "tile {i} elevation={elev} got {b:?} expected {expected:?}",
                );
            } else {
                // Land tiles may have been repainted Wetland or AlpineMeadow
                // depending on climate. Anything else should still be the
                // starting Grassland.
                let ok = matches!(b, Biome::Grassland | Biome::Wetland | Biome::AlpineMeadow);
                assert!(ok, "tile {i} elev={elev} painted unexpectedly: {b:?}");
            }
        }
    }

    #[test]
    fn earth_yields_majority_water_biomes() {
        let (world, climate) = earth_like_bundle(1);
        let n = world.grid.tiles.len();
        // Start with a plausible all-land biome; the overlay should still
        // paint water biomes everywhere elevation < 0.
        let mut biomes = vec![Biome::Grassland; n];
        apply_water_overlay(
            &mut biomes,
            BiomePalette::EarthLike,
            &world,
            &climate,
            &WaterOverlayConfig::default(),
        );
        let water = biomes
            .iter()
            .filter(|b| matches!(b, Biome::Ocean | Biome::CoastalShallow))
            .count();
        let frac = water as f64 / n as f64;
        assert!(
            frac >= 0.60,
            "expected >=60% Ocean+CoastalShallow on Earth @ seed 1, got {frac:.3} ({water}/{n})"
        );
    }
}
