//! Composite biome classification: Whittaker + Markov smoothing across a
//! whole [`SkeletonWorld`].
//!
//! [`BiomeMap`] is the Stage 6 capstone that ties the `ymir-biome` crate
//! together. Given a skeleton grid and its climate fields, it selects a
//! palette from the world's atmosphere, classifies each tile with the
//! Whittaker lookup, then runs a Markov smoothing pass that tracks the
//! grid's neighbor topology.

use crate::markov::{SmoothingConfig, smooth_biomes};
use crate::palette::{Biome, BiomePalette, palette_for};
use crate::water_overlay::{WaterOverlayConfig, apply_water_overlay};
use crate::weight_schema::default_transitions;
use crate::whittaker::classify;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ymir_climate::ClimateMap;
use ymir_surface::SkeletonWorld;

/// Configuration for [`BiomeMap::build`].
///
/// Carries the smoothing parameters for the Markov pass and the water-biome
/// overlay parameters for the pre-smoothing overlay pass introduced in
/// `BIOME-05`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BiomeMapConfig {
    /// Markov smoothing parameters.
    pub smoothing: SmoothingConfig,
    /// Water-biome overlay parameters (Ocean, CoastalShallow, Wetland,
    /// AlpineMeadow). Runs between Whittaker classification and Markov
    /// smoothing. Defaults to an enabled overlay with Earth-calibrated
    /// thresholds.
    pub water_overlay: WaterOverlayConfig,
}

/// Per-tile biome classification covering every tile in the skeleton grid.
///
/// `per_tile` is indexed the same way as `SkeletonWorld::grid.tiles` and
/// `ClimateMap::*` subfields. `palette` records which palette drove the
/// classification so downstream consumers (rendering, persistence) can
/// colorize or analyze without re-deriving it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BiomeMap {
    /// One biome per tile. `per_tile.len() == world.grid.tiles.len()`.
    pub per_tile: Vec<Biome>,
    /// The palette selected from the world's atmosphere. Every biome in
    /// `per_tile` is a member of this palette after smoothing.
    pub palette: BiomePalette,
}

impl BiomeMap {
    /// Build a biome map from a skeleton world and its climate fields.
    ///
    /// Algorithm:
    /// 1. Pick a palette from `world.atmosphere.class` via [`palette_for`].
    /// 2. Classify each tile with [`classify`] using the tile's temperature
    ///    (Kelvin) and humidity.
    /// 3. Apply the water-biome overlay (Ocean, CoastalShallow, Wetland,
    ///    AlpineMeadow) using skeleton elevation and climate. The overlay
    ///    is a no-op for non-Earth-like palettes.
    /// 4. Build a `Vec<Vec<usize>>` neighbor table from
    ///    `world.grid.tiles[i].neighbors` and run [`smooth_biomes`] with
    ///    `cfg.smoothing` and the palette's default transition table.
    ///
    /// # Panics
    ///
    /// Panics if `climate.temperature.per_tile_k.len()`,
    /// `climate.moisture.per_tile.len()`, and `world.grid.tiles.len()`
    /// disagree. All three are always produced together in normal flow, so
    /// this is treated as a programmer error rather than a runtime fault.
    pub fn build(world: &SkeletonWorld, climate: &ClimateMap, cfg: &BiomeMapConfig) -> Self {
        let n = world.grid.tiles.len();
        assert_eq!(
            climate.temperature.per_tile_k.len(),
            n,
            "temperature field length must match grid tile count"
        );
        assert_eq!(
            climate.moisture.per_tile.len(),
            n,
            "moisture field length must match grid tile count"
        );

        let palette = palette_for(world.atmosphere.class);

        // 1. Raw Whittaker classification per tile.
        let mut per_tile: Vec<Biome> = (0..n)
            .map(|i| {
                let t = climate.temperature.per_tile_k[i];
                let h = climate.moisture.per_tile[i];
                classify(t, h, palette)
            })
            .collect();

        // 2. Water-biome overlay: paint Ocean / CoastalShallow / Wetland /
        //    AlpineMeadow using skeleton elevation and climate. No-op on
        //    non-Earth-like palettes.
        apply_water_overlay(&mut per_tile, palette, world, climate, &cfg.water_overlay);

        // 3. Build neighbor lists for the smoother. GridTile neighbors are
        //    already Vec<usize> with valid indices, so we just clone the
        //    structure into the flat shape the smoother expects. This
        //    adapter is intentionally here (not in markov.rs) so that the
        //    ymir-biome::markov module stays independent of ymir-surface.
        let neighbor_lists: Vec<Vec<usize>> = world
            .grid
            .tiles
            .iter()
            .map(|t| t.neighbors.clone())
            .collect();

        // 4. Markov smoothing. NoSurface palettes short-circuit inside
        //    smooth_biomes (only one candidate), so this is safe for every
        //    palette.
        let transitions = default_transitions(palette);
        smooth_biomes(&neighbor_lists, &mut per_tile, &transitions, &cfg.smoothing);

        Self { per_tile, palette }
    }

    /// Total number of tiles classified.
    pub fn len(&self) -> usize {
        self.per_tile.len()
    }

    /// True if no tiles are classified.
    pub fn is_empty(&self) -> bool {
        self.per_tile.is_empty()
    }

    /// Count of each biome, sorted deterministically by the debug-formatted
    /// variant name. Useful for stable histogram output in CLI tools and
    /// tests.
    pub fn histogram(&self) -> Vec<(Biome, usize)> {
        let mut counts: HashMap<Biome, usize> = HashMap::new();
        for &b in &self.per_tile {
            *counts.entry(b).or_insert(0) += 1;
        }
        let mut out: Vec<(Biome, usize)> = counts.into_iter().collect();
        out.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));
        out
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
    use ymir_core::Sourced;
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

    fn earth_like_world(seed: u64) -> SkeletonWorld {
        SkeletonWorld::build(earth_body(), earth_atmosphere(), 3, seed)
    }

    fn earth_like_bundle(seed: u64) -> (SkeletonWorld, ClimateMap) {
        let world = earth_like_world(seed);
        let climate = ClimateMap::build(&world, &ClimateConfig::default());
        (world, climate)
    }

    #[test]
    fn build_assigns_biome_to_every_tile() {
        let (world, climate) = earth_like_bundle(2024);
        let map = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        assert_eq!(map.len(), world.grid.tiles.len());
        assert!(!map.is_empty());
    }

    #[test]
    fn build_stays_in_palette() {
        let (world, climate) = earth_like_bundle(2024);
        let map = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        assert_eq!(map.palette, BiomePalette::EarthLike);
        for (i, &b) in map.per_tile.iter().enumerate() {
            assert!(
                map.palette.contains(b),
                "tile {i} has out-of-palette biome {b:?}"
            );
        }
    }

    #[test]
    fn build_is_deterministic() {
        let (world_a, climate_a) = earth_like_bundle(42);
        let (world_b, climate_b) = earth_like_bundle(42);
        let cfg = BiomeMapConfig::default();
        let a = BiomeMap::build(&world_a, &climate_a, &cfg);
        let b = BiomeMap::build(&world_b, &climate_b, &cfg);
        assert_eq!(a.per_tile, b.per_tile);
        assert_eq!(a.palette, b.palette);
    }

    #[test]
    fn serde_roundtrip_bincode() {
        let (world, climate) = earth_like_bundle(7);
        let map = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        let bytes = bincode::serialize(&map).expect("serialize");
        let decoded: BiomeMap = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(decoded.palette, map.palette);
        assert_eq!(decoded.per_tile, map.per_tile);
    }

    #[test]
    fn histogram_sums_to_tile_count() {
        let (world, climate) = earth_like_bundle(2024);
        let map = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        let total: usize = map.histogram().iter().map(|(_, n)| *n).sum();
        assert_eq!(total, map.len());
        assert_eq!(total, world.grid.tiles.len());
    }

    #[test]
    fn histogram_is_deterministically_sorted() {
        let (world, climate) = earth_like_bundle(2024);
        let map = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        let h = map.histogram();
        for w in h.windows(2) {
            let a = format!("{:?}", w[0].0);
            let b = format!("{:?}", w[1].0);
            assert!(a < b, "histogram not sorted: {a} !< {b}");
        }
    }

    #[test]
    fn earth_like_world_produces_plausible_mix() {
        let (world, climate) = earth_like_bundle(2024);
        let map = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        let present: std::collections::HashSet<Biome> = map.per_tile.iter().copied().collect();

        // Earth's Whittaker lookup doesn't emit Ocean (reserved for BIOME-04
        // overlays per the whittaker doc comment), so we don't assert on
        // that. What we DO expect on an Earth-calibrated climate field:
        // at least one forest variant, and at least one
        // grassland/desert/savanna variant.
        let has_forest = present.contains(&Biome::TropicalRainforest)
            || present.contains(&Biome::TemperateForest)
            || present.contains(&Biome::BorealForest);
        assert!(
            has_forest,
            "expected at least one forest variant in Earth-like mix, got {present:?}"
        );

        let has_open = present.contains(&Biome::Grassland)
            || present.contains(&Biome::Savanna)
            || present.contains(&Biome::HotDesert)
            || present.contains(&Biome::ColdDesert);
        assert!(
            has_open,
            "expected grassland/savanna/desert variant in Earth-like mix, got {present:?}"
        );
    }

    // ---- BIOME-05 integration tests ---------------------------------------

    fn earth_body_calibrated() -> OrbitalBody {
        let mut b = earth_body();
        b.continental_fraction = Some(ymir_core::Sourced::observed(
            0.29,
            "ETOPO1",
            "global topography",
        ));
        b
    }

    fn mars_body() -> OrbitalBody {
        OrbitalBody {
            semi_major_axis: d(1.524),
            eccentricity: d(0.0934),
            inclination: d(1.85),
            axial_tilt: d(25.19),
            mass: d(0.107),
            radius: d(0.532),
            density: d(3.93),
            surface_gravity: d(3.71),
            solar_irradiance: d(586.2),
            equilibrium_temp: d(210.0),
            tidal_locked: Sourced::derived(false, "test"),
            rotation_period: d(24.62),
            is_in_hz: false,
            planet_type: PlanetType::Terran,
            name: Some("Mars".into()),
            is_known_exoplanet: false,
            continental_fraction: Some(Sourced::observed(1.0, "MOLA", "global topography")),
        }
    }

    fn mars_atmosphere() -> AtmosphereModel {
        let mut composition = BTreeMap::new();
        composition.insert(Gas::CO2, 0.95);
        composition.insert(Gas::N2, 0.03);
        composition.insert(Gas::Ar, 0.02);
        AtmosphereModel {
            surface_pressure: d(0.006),
            composition: Sourced::derived(composition, "test"),
            greenhouse_factor: d(1.02),
            effective_surface_temp: d(210.0),
            scale_height: d(11.1),
            moisture_capacity: d(0.0),
            uv_surface_flux: d(0.6),
            class: AtmosphereClass::ThinCO2,
            retained: vec![Gas::CO2, Gas::N2, Gas::Ar],
        }
    }

    #[test]
    fn earth_calibrated_produces_majority_water_biomes() {
        // With continental_fraction = 0.29, the skeleton heightmap
        // calibrates sea level to give ~71% sub-sea-level tiles. The
        // BIOME-05 overlay turns those into Ocean + CoastalShallow, so the
        // biome map should report ≥60% water tiles (headroom for the
        // Markov smoother nibbling a handful of coastal tiles).
        let world = SkeletonWorld::build(earth_body_calibrated(), earth_atmosphere(), 3, 1);
        let climate = ClimateMap::build(&world, &ClimateConfig::default());
        let map = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        let water = map
            .per_tile
            .iter()
            .filter(|b| matches!(b, Biome::Ocean | Biome::CoastalShallow))
            .count();
        let frac = water as f64 / map.len() as f64;
        assert!(
            frac >= 0.60,
            "Earth @ seed 1 should have ≥60% Ocean+CoastalShallow biomes, got {frac:.3} \
             ({water}/{}); histogram: {:?}",
            map.len(),
            map.histogram(),
        );
    }

    #[test]
    fn mars_produces_no_water_biomes() {
        let world = SkeletonWorld::build(mars_body(), mars_atmosphere(), 3, 1);
        let climate = ClimateMap::build(&world, &ClimateConfig::default());
        let map = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        for (i, b) in map.per_tile.iter().enumerate() {
            assert!(
                !matches!(
                    b,
                    Biome::Ocean | Biome::CoastalShallow | Biome::Wetland | Biome::AlpineMeadow
                ),
                "Mars tile {i} got water/alpine biome {b:?}"
            );
        }
    }

    #[test]
    fn water_overlay_can_be_disabled_via_config() {
        // With the overlay disabled, no Ocean / CoastalShallow should
        // appear even on a heavily sub-sea-level Earth.
        let world = SkeletonWorld::build(earth_body_calibrated(), earth_atmosphere(), 3, 1);
        let climate = ClimateMap::build(&world, &ClimateConfig::default());
        let cfg = BiomeMapConfig {
            water_overlay: WaterOverlayConfig {
                enabled: false,
                ..WaterOverlayConfig::default()
            },
            ..BiomeMapConfig::default()
        };
        let map = BiomeMap::build(&world, &climate, &cfg);
        let water = map
            .per_tile
            .iter()
            .filter(|b| {
                matches!(
                    b,
                    Biome::Ocean | Biome::CoastalShallow | Biome::Wetland | Biome::AlpineMeadow
                )
            })
            .count();
        assert_eq!(water, 0, "disabled overlay still produced water biomes");
    }
}
