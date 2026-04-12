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
use crate::weight_schema::default_transitions;
use crate::whittaker::classify;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ymir_climate::ClimateMap;
use ymir_surface::SkeletonWorld;

/// Configuration for [`BiomeMap::build`].
///
/// Carries the smoothing parameters for the Markov pass that runs after
/// raw Whittaker classification.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BiomeMapConfig {
    /// Markov smoothing parameters.
    pub smoothing: SmoothingConfig,
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
    /// 3. Build a `Vec<Vec<usize>>` neighbor table from
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

        // 2. Build neighbor lists for the smoother. GridTile neighbors are
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

        // 3. Markov smoothing. NoSurface palettes short-circuit inside
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
    use ymir_surface::SkeletonWorld;
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
}
