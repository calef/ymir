//! Capstone composite that orchestrates the full DET-02..06 pipeline for a
//! single [`RegionSpec`].
//!
//! [`RegionalDetail::build`] runs the five detail stages in dependency
//! order on a region:
//!
//! 1. [`HexGrid::build`] lays out the sub-tile hex lattice across every
//!    skeleton tile in the region.
//! 2. [`DetailElevation::build`] samples a seam-continuous baseline and
//!    gravity-scaled FBM perturbation per hex, keyed off the world seed.
//! 3. [`DetailMoisture::build`] applies a first-pass orographic
//!    lift/shadow correction to the parent-tile humidity.
//! 4. [`DetailFlow::build`] runs Planchon-Darboux depression filling,
//!    steepest-descent routing, and flow accumulation.
//! 5. [`DetailBiomes::build`] reclassifies each hex against the
//!    Whittaker diagram at the refined temperature/humidity, overlays
//!    lakes and rivers, pins boundary hexes to the parent skeleton
//!    tile's biome, and runs a Markov smoothing pass.
//!
//! The composite is fully serializable so downstream stages (STOR-03
//! persistence, REND-03 rendering) can round-trip it through bincode.

use crate::biomes::{DetailBiomeConfig, DetailBiomes};
use crate::elevation::{DetailElevation, DetailElevationConfig};
use crate::flow::{DetailFlow, DetailFlowConfig};
use crate::hex_grid::{HexGrid, HexGridConfig};
use crate::moisture::{DetailMoisture, DetailMoistureConfig};
use crate::region::RegionSpec;
use serde::{Deserialize, Serialize};
use ymir_biome::BiomeMap;
use ymir_climate::ClimateMap;
use ymir_surface::skeleton::SkeletonWorld;

/// Configuration bundle for [`RegionalDetail::build`].
///
/// The `seed` is mixed with `RegionSpec::tile_index` by [`crate::region::detail_rng`]
/// to derive the region's deterministic PRNG stream; use the same value
/// that built the supplied [`SkeletonWorld`] so the detail pass stays
/// reproducible across runs.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct RegionalDetailConfig {
    /// World seed. Must match the seed used to build the parent skeleton.
    pub seed: u64,
    /// Hex lattice configuration forwarded to [`HexGrid::build`].
    pub hex_grid: HexGridConfig,
    /// Detail-elevation configuration forwarded to [`DetailElevation::build`].
    pub elevation: DetailElevationConfig,
    /// Orographic-moisture configuration forwarded to [`DetailMoisture::build`].
    pub moisture: DetailMoistureConfig,
    /// Flow-routing configuration forwarded to [`DetailFlow::build`].
    pub flow: DetailFlowConfig,
    /// Biome-refinement configuration forwarded to [`DetailBiomes::build`].
    pub biomes: DetailBiomeConfig,
}

/// Full regional-detail artifact for one [`RegionSpec`].
///
/// Holds every per-hex output produced by DET-02..06 in a single
/// serializable container. All inner arrays share the ordering of
/// `hex_grid.cells`; see each inner stage's docs for units and
/// semantics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegionalDetail {
    /// Region this composite covers.
    pub spec: RegionSpec,
    /// Hex lattice produced by DET-02.
    pub hex_grid: HexGrid,
    /// Per-hex elevation from DET-03.
    pub elevation: DetailElevation,
    /// Per-hex refined humidity from DET-04.
    pub moisture: DetailMoisture,
    /// Per-hex flow routing from DET-05.
    pub flow: DetailFlow,
    /// Per-hex biome classification from DET-06.
    pub biomes: DetailBiomes,
}

impl RegionalDetail {
    /// Build the full detail artifact for `spec` by running DET-02..06
    /// against the supplied parent skeleton, climate, and biome maps.
    ///
    /// Stages run sequentially: [`HexGrid`] →
    /// [`DetailElevation`] → [`DetailMoisture`] → [`DetailFlow`] →
    /// [`DetailBiomes`]. Each stage consumes only the outputs of earlier
    /// stages (plus the shared skeleton/climate/biome context), matching
    /// the dependency graph in `ARCHITECTURE.md` §6.
    pub fn build(
        world: &SkeletonWorld,
        climate: &ClimateMap,
        biomes: &BiomeMap,
        spec: RegionSpec,
        config: RegionalDetailConfig,
    ) -> RegionalDetail {
        let hex_grid = HexGrid::build(world, spec.clone(), config.hex_grid);
        let elevation = DetailElevation::build(world, &hex_grid, config.seed, config.elevation);
        let moisture =
            DetailMoisture::build(world, climate, &hex_grid, &elevation, config.moisture);
        let flow = DetailFlow::build(&hex_grid, &elevation, config.flow);
        let biomes = DetailBiomes::build(
            world,
            climate,
            biomes,
            &hex_grid,
            &elevation,
            &moisture,
            &flow,
            config.biomes,
        );
        RegionalDetail {
            spec,
            hex_grid,
            elevation,
            moisture,
            flow,
            biomes,
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
    use ymir_biome::BiomeMapConfig;
    use ymir_climate::ClimateConfig;
    use ymir_core::Sourced;
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

    /// Build a skeleton + climate + biome bundle for Earth at `seed`.
    fn build_parent_context(seed: u64) -> (SkeletonWorld, ClimateMap, BiomeMap) {
        let world = SkeletonWorld::build(earth_body(), earth_atmosphere(), 2, seed);
        let climate = ClimateMap::build(&world, &ClimateConfig::default());
        let biomes = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        (world, climate, biomes)
    }

    fn default_cfg(seed: u64) -> RegionalDetailConfig {
        RegionalDetailConfig {
            seed,
            ..RegionalDetailConfig::default()
        }
    }

    #[test]
    fn builds_without_panicking() {
        let seed = 1;
        let (world, climate, biomes) = build_parent_context(seed);
        let spec = RegionSpec::new(0, 1);
        let rd = RegionalDetail::build(&world, &climate, &biomes, spec.clone(), default_cfg(seed));

        let n = rd.hex_grid.cells.len();
        assert!(n > 0, "empty region hex grid");
        assert_eq!(rd.hex_grid.neighbors.len(), n);
        assert_eq!(rd.elevation.per_hex_m.len(), n);
        assert_eq!(rd.moisture.per_hex.len(), n);
        assert_eq!(rd.flow.flow_accumulation.len(), n);
        assert_eq!(rd.flow.is_lake.len(), n);
        assert_eq!(rd.flow.filled_elevation_m.len(), n);
        assert_eq!(rd.flow.downstream.len(), n);
        assert_eq!(rd.biomes.per_hex.len(), n);
        assert_eq!(rd.spec, spec);
    }

    #[test]
    fn deterministic_byte_identical() {
        let seed = 42;
        let (world, climate, biomes) = build_parent_context(seed);
        let spec = RegionSpec::new(3, 1);
        let a = RegionalDetail::build(&world, &climate, &biomes, spec.clone(), default_cfg(seed));
        let b = RegionalDetail::build(&world, &climate, &biomes, spec, default_cfg(seed));

        let sa = bincode::serialize(&a).expect("serialize a");
        let sb = bincode::serialize(&b).expect("serialize b");
        assert_eq!(
            sa, sb,
            "RegionalDetail::build is non-deterministic across two back-to-back runs"
        );
    }

    #[test]
    fn serde_round_trip() {
        let seed = 7;
        let (world, climate, biomes) = build_parent_context(seed);
        let spec = RegionSpec::new(5, 1);
        let rd = RegionalDetail::build(&world, &climate, &biomes, spec, default_cfg(seed));

        let bytes = bincode::serialize(&rd).expect("serialize");
        let back: RegionalDetail = bincode::deserialize(&bytes).expect("deserialize");

        assert_eq!(back.spec, rd.spec);
        assert_eq!(back.hex_grid.config, rd.hex_grid.config);
        assert_eq!(back.hex_grid.region_spec, rd.hex_grid.region_spec);
        assert_eq!(back.hex_grid.cells.len(), rd.hex_grid.cells.len());
        assert_eq!(back.hex_grid.neighbors, rd.hex_grid.neighbors);
        for (a, b) in back.hex_grid.cells.iter().zip(rd.hex_grid.cells.iter()) {
            assert_eq!(a.region_hex_index, b.region_hex_index);
            assert_eq!(a.parent_tile, b.parent_tile);
            assert_eq!(a.q, b.q);
            assert_eq!(a.r, b.r);
            assert!((a.lat_rad - b.lat_rad).abs() < 1.0e-12);
            assert!((a.lon_rad - b.lon_rad).abs() < 1.0e-12);
        }
        for (a, b) in back
            .elevation
            .per_hex_m
            .iter()
            .zip(rd.elevation.per_hex_m.iter())
        {
            assert!((a - b).abs() < 1.0e-12);
        }
        for (a, b) in back.moisture.per_hex.iter().zip(rd.moisture.per_hex.iter()) {
            assert!((a - b).abs() < 1.0e-12);
        }
        for (a, b) in back
            .flow
            .filled_elevation_m
            .iter()
            .zip(rd.flow.filled_elevation_m.iter())
        {
            assert!((a - b).abs() < 1.0e-12);
        }
        assert_eq!(back.flow.is_lake, rd.flow.is_lake);
        assert_eq!(back.flow.downstream, rd.flow.downstream);
        for (a, b) in back
            .flow
            .flow_accumulation
            .iter()
            .zip(rd.flow.flow_accumulation.iter())
        {
            assert!((a - b).abs() < 1.0e-6);
        }
        assert_eq!(back.biomes.palette, rd.biomes.palette);
        assert_eq!(back.biomes.per_hex, rd.biomes.per_hex);
    }

    #[test]
    fn boundary_consistency_with_parent_skeleton() {
        let seed = 1;
        let (world, climate, biomes) = build_parent_context(seed);
        let spec = RegionSpec::new(0, 1);
        let rd = RegionalDetail::build(&world, &climate, &biomes, spec, default_cfg(seed));

        let mut checked = 0usize;
        for (i, slots) in rd.hex_grid.neighbors.iter().enumerate() {
            let is_boundary = slots.iter().any(|s| s.is_none());
            if !is_boundary {
                continue;
            }
            let parent = rd.hex_grid.cells[i].parent_tile as usize;
            let parent_biome = biomes.per_tile[parent];
            assert_eq!(
                rd.biomes.per_hex[i], parent_biome,
                "boundary hex {i} parent {parent}: got {:?}, expected {:?}",
                rd.biomes.per_hex[i], parent_biome
            );
            checked += 1;
        }
        assert!(
            checked > 0,
            "expected at least one boundary hex in a radius-1 region"
        );
    }

    #[test]
    fn same_region_regeneration_is_byte_identical() {
        // Two fully independent parent-context builds (skeleton, climate,
        // biomes) from the same world seed must produce byte-identical
        // RegionalDetail output. This exercises every stage's determinism,
        // not just the inner detail RNG.
        let seed = 11;
        let spec = RegionSpec::new(4, 1);
        let cfg = default_cfg(seed);

        let (world_a, climate_a, biomes_a) = build_parent_context(seed);
        let a = RegionalDetail::build(&world_a, &climate_a, &biomes_a, spec.clone(), cfg);

        let (world_b, climate_b, biomes_b) = build_parent_context(seed);
        let b = RegionalDetail::build(&world_b, &climate_b, &biomes_b, spec, cfg);

        let sa = bincode::serialize(&a).expect("serialize a");
        let sb = bincode::serialize(&b).expect("serialize b");
        assert_eq!(
            sa, sb,
            "RegionalDetail regeneration from an independently-built parent context drifted"
        );
    }

    #[test]
    fn config_serde_round_trip() {
        let cfg = RegionalDetailConfig {
            seed: 12345,
            hex_grid: HexGridConfig { subdivision: 16 },
            elevation: DetailElevationConfig {
                octaves: 4,
                lacunarity: 2.1,
                gain: 0.55,
                base_frequency: 280.0,
                amplitude_m: 275.0,
            },
            moisture: DetailMoistureConfig {
                lift_coefficient: 0.45,
                shadow_coefficient: 0.28,
                saturation_ceiling: 0.95,
            },
            flow: DetailFlowConfig {
                lake_fill_epsilon_m: 2.0e-3,
            },
            biomes: DetailBiomeConfig {
                river_flow_threshold: 22.0,
                markov_iterations: 3,
                lapse_rate_k_per_m: 6.0e-3,
                neighbor_agreement_threshold: 0.58,
            },
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: RegionalDetailConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, cfg);
    }
}
