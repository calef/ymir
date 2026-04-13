//! Per-hex biome refinement at detail scale.
//!
//! [`DetailBiomes::build`] classifies every hex in a [`HexGrid`] with the
//! same Whittaker diagram used by `ymir-biome`, using a locally refined
//! temperature and humidity. Lakes (from [`DetailFlow::is_lake`]) and
//! river channels (from [`DetailFlow::flow_accumulation`] above a
//! configurable threshold) are then overlaid onto the classification.
//! Finally a Markov smoothing pass identical to `BIOME-03`'s is run over
//! the hex-adjacency graph to remove speckle.
//!
//! The refinement pipeline is:
//!
//! 1. For each hex, derive a refined temperature by starting from the
//!    parent skeleton tile's temperature and applying a lapse-rate
//!    correction against the elevation delta between the hex and the
//!    parent tile. The default lapse rate is Earth tropospheric
//!    (`0.0065 K/m`); `DetailBiomeConfig::lapse_rate_k_per_m` overrides.
//! 2. Classify the hex with [`ymir_biome::classify`] using the refined
//!    temperature and the hex's humidity from [`DetailMoisture::per_hex`],
//!    restricted to the parent world's palette.
//! 3. Overlay lakes: every hex with `flow.is_lake[h]` is repainted to the
//!    palette's "water body" biome. For Earth-like worlds that is
//!    [`Biome::CoastalShallow`] (inland lake analogue of the shelf
//!    biome). Other palettes fall back to
//!    [`BiomePalette::default_biome`] when that variant is itself a
//!    water body, otherwise the lake overlay is skipped for that
//!    palette (documented in `water_body_biome_for`).
//! 4. Overlay rivers: every hex with `flow.flow_accumulation[h] >=
//!    config.river_flow_threshold` is repainted to the palette's river
//!    biome. For Earth-like worlds that is [`Biome::Wetland`]; other
//!    palettes skip the river overlay (no sensible river biome in the
//!    palette without carrying CoastalShallow/Wetland into them).
//! 5. Boundary pinning: every hex on the region edge (any hex whose
//!    neighbour array contains a `None`) is pinned to its parent
//!    skeleton tile's biome from the supplied [`BiomeMap`]. Pinning
//!    happens BEFORE the Markov smoother runs so the smoother treats
//!    boundary hexes as fixed Dirichlet conditions: the smoother reads
//!    their biomes as context but never flips them. This guarantees
//!    detail biomes at the region edge agree with the parent skeleton
//!    tile.
//! 6. Markov smoothing: reuse [`ymir_biome::smooth_biomes`] against a
//!    hex-adjacency `Vec<Vec<usize>>` built from `HexGrid::neighbors`.
//!    After smoothing, re-pin boundary hexes (smoother may have nudged
//!    them despite the fixed-context contract in edge cases where the
//!    boundary hex's own biome is outside the palette and the threshold
//!    check trivially allows a flip).
//!
//! # Palette membership
//!
//! Every returned biome is a member of the parent world's palette. The
//! Whittaker classifier already enforces this (see `ymir-biome`); the
//! lake and river overlays use only palette-member biomes; and boundary
//! pinning copies biomes from the parent `BiomeMap`, which itself only
//! ever contains palette members after its own smoothing pass.

use crate::flow::DetailFlow;
use crate::hex_grid::HexGrid;
use crate::moisture::DetailMoisture;
use crate::region::RegionSpec;
use crate::{DetailElevation, HexCell};
use serde::{Deserialize, Serialize};
use ymir_biome::markov::{SmoothingConfig, smooth_biomes};
use ymir_biome::palette::{Biome, BiomePalette};
use ymir_biome::weight_schema::default_transitions;
use ymir_biome::whittaker::classify;
use ymir_biome::{BiomeMap, BiomeMapConfig};
use ymir_climate::ClimateMap;
use ymir_surface::skeleton::SkeletonWorld;

/// Default dry-adiabatic lapse rate in K/m (Earth tropospheric value).
///
/// Matches the `NitrogenOxygen`/`ThickN2H2O` branch of
/// `ymir_climate::temperature::derive_lapse_rate`. Used when the caller
/// does not supply a body-specific override.
pub const DEFAULT_LAPSE_RATE_K_PER_M: f64 = 0.0065;

/// Default river detection threshold in hex-units of flow accumulation.
///
/// Tuned against a radius-1 Earth seed=1 region at subdivision 32 where
/// the maximum flow accumulation reached ≈ 589.0 (see DET-05 stats).
/// A threshold of 16 hexes yields a sparse but clearly connected river
/// network on that region (roughly the top 3% of flow values).
pub const DEFAULT_RIVER_FLOW_THRESHOLD: f32 = 16.0;

/// Configuration for [`DetailBiomes::build`].
///
/// `neighbor_agreement_threshold` is forwarded into
/// [`SmoothingConfig::flip_threshold`] for the Markov smoothing pass; it
/// uses the same semantics as `BIOME-03` (the argmax candidate must beat
/// the current biome by this fraction of their combined score to flip).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct DetailBiomeConfig {
    /// Flow accumulation threshold (in hex units) above which a hex is
    /// tagged as a river channel.
    pub river_flow_threshold: f32,
    /// Number of Markov smoothing iterations over the hex graph.
    pub markov_iterations: u32,
    /// Lapse rate (K/m) used to adjust parent-tile temperature for the
    /// hex's elevation. Defaults to Earth tropospheric `0.0065`.
    pub lapse_rate_k_per_m: f64,
    /// Flip-threshold forwarded into [`SmoothingConfig::flip_threshold`].
    pub neighbor_agreement_threshold: f64,
}

impl Default for DetailBiomeConfig {
    fn default() -> Self {
        Self {
            river_flow_threshold: DEFAULT_RIVER_FLOW_THRESHOLD,
            markov_iterations: 2,
            lapse_rate_k_per_m: DEFAULT_LAPSE_RATE_K_PER_M,
            neighbor_agreement_threshold: 0.6,
        }
    }
}

/// Per-hex biome classification for a region.
///
/// `per_hex[i]` matches the ordering of `HexGrid::cells[i]`; every entry
/// is a member of the world's palette.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetailBiomes {
    /// The region this biome map covers.
    pub spec: RegionSpec,
    /// Palette that every biome in `per_hex` is a member of.
    pub palette: BiomePalette,
    /// One biome per hex cell in [`HexGrid::cells`] order.
    pub per_hex: Vec<Biome>,
}

impl DetailBiomes {
    /// Build a refined biome map for every hex in `grid`.
    ///
    /// See the module docs for the full algorithm.
    ///
    /// # Panics
    ///
    /// Panics if the per-hex arrays (`elevation.per_hex_m`,
    /// `moisture.per_hex`, `flow.flow_accumulation`, `flow.is_lake`) do
    /// not all have the same length as `grid.cells`, or if `biomes`
    /// does not cover every parent skeleton tile referenced by the
    /// grid's hex cells.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        world: &SkeletonWorld,
        climate: &ClimateMap,
        biomes: &BiomeMap,
        grid: &HexGrid,
        elevation: &DetailElevation,
        moisture: &DetailMoisture,
        flow: &DetailFlow,
        config: DetailBiomeConfig,
    ) -> DetailBiomes {
        let n = grid.cells.len();
        assert_eq!(
            elevation.per_hex_m.len(),
            n,
            "DetailElevation::per_hex_m length {} != HexGrid::cells len {n}",
            elevation.per_hex_m.len(),
        );
        assert_eq!(
            moisture.per_hex.len(),
            n,
            "DetailMoisture::per_hex length {} != HexGrid::cells len {n}",
            moisture.per_hex.len(),
        );
        assert_eq!(
            flow.flow_accumulation.len(),
            n,
            "DetailFlow::flow_accumulation length {} != HexGrid::cells len {n}",
            flow.flow_accumulation.len(),
        );
        assert_eq!(
            flow.is_lake.len(),
            n,
            "DetailFlow::is_lake length {} != HexGrid::cells len {n}",
            flow.is_lake.len(),
        );
        assert_eq!(
            biomes.per_tile.len(),
            world.grid.tiles.len(),
            "BiomeMap::per_tile length {} != world.grid.tiles len {}",
            biomes.per_tile.len(),
            world.grid.tiles.len(),
        );

        let palette = biomes.palette;
        let parent_temp = &climate.temperature.per_tile_k;

        // --- 1. Whittaker classification with lapse-rate adjusted T. ---
        let mut per_hex: Vec<Biome> = Vec::with_capacity(n);
        for (i, cell) in grid.cells.iter().enumerate() {
            let parent = cell.parent_tile as usize;
            let parent_elev = world.elevation.elevations_m[parent];
            let parent_t = parent_temp[parent];
            let hex_elev = elevation.per_hex_m[i];
            let t_refined =
                (parent_t - config.lapse_rate_k_per_m * (hex_elev - parent_elev)).max(0.0);
            let h = moisture.per_hex[i];
            let b = classify(t_refined, h, palette);
            // `classify` may return a biome outside the palette only for
            // NoSurface (which has a single member); otherwise it is
            // guaranteed in-palette. Defensive fallback to the parent
            // tile's biome if anything slips through.
            let b = if palette.contains(b) {
                b
            } else {
                biomes.per_tile[parent]
            };
            per_hex.push(b);
        }

        // --- 3. Lake overlay. ---
        if let Some(lake_biome) = water_body_biome_for(palette) {
            for (biome, &is_lake) in per_hex.iter_mut().zip(flow.is_lake.iter()) {
                if is_lake {
                    *biome = lake_biome;
                }
            }
        }

        // --- 4. River overlay. ---
        if let Some(river_biome) = river_biome_for(palette) {
            for ((biome, &is_lake), &accum) in per_hex
                .iter_mut()
                .zip(flow.is_lake.iter())
                .zip(flow.flow_accumulation.iter())
            {
                // Rivers do not overwrite lakes; lakes took priority in step 3.
                if is_lake {
                    continue;
                }
                if accum >= config.river_flow_threshold {
                    *biome = river_biome;
                }
            }
        }

        // --- 5. Boundary pinning (pre-smooth). ---
        let boundary = boundary_mask(&grid.neighbors);
        for i in 0..n {
            if boundary[i] {
                let parent = grid.cells[i].parent_tile as usize;
                per_hex[i] = biomes.per_tile[parent];
            }
        }

        // --- 6. Markov smoothing on the hex adjacency graph. ---
        let neighbor_lists: Vec<Vec<usize>> = grid
            .neighbors
            .iter()
            .map(|slots| slots.iter().flatten().map(|u| *u as usize).collect())
            .collect();
        let transitions = default_transitions(palette);
        let smoothing_cfg = SmoothingConfig {
            iterations: config.markov_iterations,
            flip_threshold: config.neighbor_agreement_threshold,
        };
        // The smoother operates synchronously on a double buffer; to
        // hold boundary hexes fixed we snapshot their biomes, run the
        // smoother, and then restore the snapshot. This matches the
        // "Dirichlet boundary condition" strategy described in the
        // module docs.
        let boundary_snapshot: Vec<Option<Biome>> = boundary
            .iter()
            .enumerate()
            .map(|(i, &b)| if b { Some(per_hex[i]) } else { None })
            .collect();
        smooth_biomes(&neighbor_lists, &mut per_hex, &transitions, &smoothing_cfg);
        for (i, snap) in boundary_snapshot.iter().enumerate() {
            if let Some(b) = snap {
                per_hex[i] = *b;
            }
        }

        // --- 7. Re-apply lake and river overlays on interior hexes
        // after smoothing. ---
        // The Markov smoother runs over a homogeneous neighborhood score
        // and can flip an isolated lake or river hex back to its
        // surrounding biome. Physical features (lakes, rivers) are
        // authoritative — repaint them so interior `is_lake` hexes
        // always carry the palette's water-body biome and interior
        // river hexes always carry the palette's river biome. Boundary
        // hexes are left pinned to the parent skeleton tile's biome so
        // the boundary-agreement invariant holds; a lake or river that
        // crosses the region edge is represented as the surrounding
        // parent-tile biome at the seam and reappears as the overlay
        // one cell inside.
        if let Some(lake_biome) = water_body_biome_for(palette) {
            for (i, (biome, &is_lake)) in per_hex.iter_mut().zip(flow.is_lake.iter()).enumerate() {
                if !boundary[i] && is_lake {
                    *biome = lake_biome;
                }
            }
        }
        if let Some(river_biome) = river_biome_for(palette) {
            for (i, ((biome, &is_lake), &accum)) in per_hex
                .iter_mut()
                .zip(flow.is_lake.iter())
                .zip(flow.flow_accumulation.iter())
                .enumerate()
            {
                if boundary[i] || is_lake {
                    continue;
                }
                if accum >= config.river_flow_threshold {
                    *biome = river_biome;
                }
            }
        }

        DetailBiomes {
            spec: grid.region_spec.clone(),
            palette,
            per_hex,
        }
    }
}

/// Return the palette's "water body" biome used for lake overlay, or
/// `None` if the palette has no suitable inland-water biome.
///
/// - `EarthLike`: [`Biome::CoastalShallow`] (inland analogue of shallow
///   open water). Deep `Biome::Ocean` is reserved for actually
///   sub-sea-level tiles handled by `BIOME-05`; inland lakes sit above
///   sea level and are more faithfully rendered as shallows.
/// - `TitanLike`: [`Biome::TitanMethaneSea`] — liquid-methane lakes are
///   a real Titan signature.
/// - Other palettes: `None`. Venus/Mars/Airless have no liquid-water
///   biome, and `NoSurface` is a single-variant palette with nothing to
///   swap to.
fn water_body_biome_for(palette: BiomePalette) -> Option<Biome> {
    match palette {
        BiomePalette::EarthLike => Some(Biome::CoastalShallow),
        BiomePalette::TitanLike => Some(Biome::TitanMethaneSea),
        _ => None,
    }
}

/// Return the palette's river biome, or `None` if the palette has no
/// suitable river channel biome.
///
/// - `EarthLike`: [`Biome::Wetland`]. Rivers at hex scale are treated
///   as wetland corridors rather than their own enum variant.
/// - All other palettes: `None`. Venus/Mars/Titan/Airless do not have
///   sustained fluvial biomes in the Phase-2 palette; if a future
///   palette revision adds a river variant this function picks it up.
fn river_biome_for(palette: BiomePalette) -> Option<Biome> {
    match palette {
        BiomePalette::EarthLike => Some(Biome::Wetland),
        _ => None,
    }
}

/// Every hex that has at least one `None` neighbour slot is a region
/// boundary cell. Matches the convention used by [`DetailFlow`].
fn boundary_mask(neighbors: &[[Option<u32>; 6]]) -> Vec<bool> {
    neighbors
        .iter()
        .map(|slots| slots.iter().any(|s| s.is_none()))
        .collect()
}

// Kept around to silence the "unused import" lint on `HexCell` and
// `BiomeMapConfig`: both are referenced in the doc tests of the public
// API surface but the non-test build only needs the types.
#[allow(dead_code)]
fn _type_anchor(_c: &HexCell, _cfg: &BiomeMapConfig) {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevation::DetailElevationConfig;
    use crate::flow::DetailFlowConfig;
    use crate::hex_grid::HexGridConfig;
    use crate::moisture::DetailMoistureConfig;
    use std::collections::BTreeMap;
    use ymir_atmosphere::atmosphere_model::AtmosphereModel;
    use ymir_atmosphere::composition::AtmosphereClass;
    use ymir_atmosphere::retention::Gas;
    use ymir_climate::{ClimateConfig, ClimateMap};
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

    /// Build the full DET-02..06 pipeline for an Earth seed at radius 1.
    fn build_full_pipeline(
        seed: u64,
        spec: RegionSpec,
    ) -> (
        SkeletonWorld,
        ClimateMap,
        BiomeMap,
        HexGrid,
        DetailElevation,
        DetailMoisture,
        DetailFlow,
    ) {
        let world = SkeletonWorld::build(earth_body(), earth_atmosphere(), 2, seed);
        let climate = ClimateMap::build(&world, &ClimateConfig::default());
        let biomes = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        let elev = DetailElevation::build(&world, &grid, seed, DetailElevationConfig::default());
        let moist = DetailMoisture::build(
            &world,
            &climate,
            &grid,
            &elev,
            DetailMoistureConfig::default(),
        );
        let flow = DetailFlow::build(&grid, &elev, DetailFlowConfig::default());
        (world, climate, biomes, grid, elev, moist, flow)
    }

    #[test]
    fn palette_membership_holds() {
        let (world, climate, biomes, grid, elev, moist, flow) =
            build_full_pipeline(1, RegionSpec::new(0, 1));
        let db = DetailBiomes::build(
            &world,
            &climate,
            &biomes,
            &grid,
            &elev,
            &moist,
            &flow,
            DetailBiomeConfig::default(),
        );
        assert_eq!(db.per_hex.len(), grid.cells.len());
        for (i, b) in db.per_hex.iter().enumerate() {
            assert!(
                db.palette.contains(*b),
                "hex {i} biome {:?} not in palette {:?}",
                b,
                db.palette
            );
        }
    }

    #[test]
    fn lake_hexes_are_water_body() {
        let (world, climate, biomes, grid, elev, moist, flow) =
            build_full_pipeline(1, RegionSpec::new(0, 1));
        let db = DetailBiomes::build(
            &world,
            &climate,
            &biomes,
            &grid,
            &elev,
            &moist,
            &flow,
            DetailBiomeConfig::default(),
        );
        let water = water_body_biome_for(db.palette)
            .expect("EarthLike palette must have a water-body biome");

        let boundary = boundary_mask(&grid.neighbors);
        // Lake hexes that are not pinned to the parent-tile biome at the
        // region boundary should all be the water-body biome. Boundary
        // hexes are pinned to the skeleton biome regardless of being
        // flagged as lakes by the local flow pass (the skeleton already
        // decided what that tile is), so we exclude them here.
        let mut tested = 0usize;
        for (i, &is_lake) in flow.is_lake.iter().enumerate() {
            if is_lake && !boundary[i] {
                assert_eq!(
                    db.per_hex[i], water,
                    "lake hex {i} got {:?}, expected {:?}",
                    db.per_hex[i], water
                );
                tested += 1;
            }
        }
        assert!(
            tested > 0,
            "expected at least one non-boundary lake hex in radius-1 Earth seed 1"
        );
    }

    #[test]
    fn river_hexes_follow_descending_gradient() {
        let (world, climate, biomes, grid, elev, moist, flow) =
            build_full_pipeline(1, RegionSpec::new(0, 1));
        let cfg = DetailBiomeConfig::default();
        let _db = DetailBiomes::build(&world, &climate, &biomes, &grid, &elev, &moist, &flow, cfg);

        // A hex is tagged as river iff (a) it's not a lake and (b) its
        // flow accumulation meets the threshold. Regardless of whether
        // the overlay painted Wetland on it (boundary pinning can
        // restore the parent-tile biome), sanity-check that the flow
        // routing beneath it runs downhill.
        let mut total = 0usize;
        let mut descending = 0usize;
        for i in 0..grid.cells.len() {
            if flow.is_lake[i] {
                continue;
            }
            if flow.flow_accumulation[i] < cfg.river_flow_threshold {
                continue;
            }
            total += 1;
            if let Some(j) = flow.downstream[i] {
                let here = flow.filled_elevation_m[i];
                let there = flow.filled_elevation_m[j as usize];
                if there <= here + 1.0e-9 {
                    descending += 1;
                }
            } else {
                // No downstream: this is an outlet at the region edge,
                // which by definition discharges out of the region.
                descending += 1;
            }
        }
        assert!(
            total > 0,
            "expected at least one river hex at threshold {}",
            cfg.river_flow_threshold
        );
        let ratio = descending as f64 / total as f64;
        assert!(
            ratio >= 0.9,
            "only {descending}/{total} river hexes ({ratio:.3}) flow to a non-ascending downstream",
        );
    }

    #[test]
    fn boundary_agrees_with_parent_tile() {
        let (world, climate, biomes, grid, elev, moist, flow) =
            build_full_pipeline(1, RegionSpec::new(0, 1));
        let db = DetailBiomes::build(
            &world,
            &climate,
            &biomes,
            &grid,
            &elev,
            &moist,
            &flow,
            DetailBiomeConfig::default(),
        );
        let boundary = boundary_mask(&grid.neighbors);
        let mut checked = 0usize;
        for (i, &b) in boundary.iter().enumerate() {
            if b {
                let parent = grid.cells[i].parent_tile as usize;
                let parent_biome = biomes.per_tile[parent];
                assert_eq!(
                    db.per_hex[i], parent_biome,
                    "boundary hex {i} got {:?}, parent tile biome {:?}",
                    db.per_hex[i], parent_biome
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "expected at least one boundary hex in a radius-1 region"
        );
    }

    #[test]
    fn deterministic_byte_identical() {
        let spec = RegionSpec::new(5, 1);
        let (world_a, climate_a, biomes_a, grid_a, elev_a, moist_a, flow_a) =
            build_full_pipeline(11, spec.clone());
        let (world_b, climate_b, biomes_b, grid_b, elev_b, moist_b, flow_b) =
            build_full_pipeline(11, spec);
        let cfg = DetailBiomeConfig::default();
        let a = DetailBiomes::build(
            &world_a, &climate_a, &biomes_a, &grid_a, &elev_a, &moist_a, &flow_a, cfg,
        );
        let b = DetailBiomes::build(
            &world_b, &climate_b, &biomes_b, &grid_b, &elev_b, &moist_b, &flow_b, cfg,
        );
        let sa = serde_json::to_vec(&a).expect("serialize a");
        let sb = serde_json::to_vec(&b).expect("serialize b");
        assert_eq!(sa, sb, "DetailBiomes::build is non-deterministic");
    }

    #[test]
    fn serde_round_trip() {
        let (world, climate, biomes, grid, elev, moist, flow) =
            build_full_pipeline(7, RegionSpec::new(9, 1));
        let db = DetailBiomes::build(
            &world,
            &climate,
            &biomes,
            &grid,
            &elev,
            &moist,
            &flow,
            DetailBiomeConfig::default(),
        );
        let json = serde_json::to_string(&db).expect("serialize");
        let back: DetailBiomes = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.spec, db.spec);
        assert_eq!(back.palette, db.palette);
        assert_eq!(back.per_hex, db.per_hex);
    }

    #[test]
    fn config_serde_round_trip() {
        let cfg = DetailBiomeConfig {
            river_flow_threshold: 12.5,
            markov_iterations: 3,
            lapse_rate_k_per_m: 0.005,
            neighbor_agreement_threshold: 0.55,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: DetailBiomeConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, cfg);
    }
}
