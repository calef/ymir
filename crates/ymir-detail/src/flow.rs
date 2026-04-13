//! Flow accumulation and lake formation on a [`HexGrid`].
//!
//! [`DetailFlow::build`] takes a [`HexGrid`] and its [`DetailElevation`] and
//! produces four per-hex arrays:
//!
//! 1. `filled_elevation_m` — original elevation with local minima filled via
//!    the Planchon-Darboux depression-filling algorithm. Hexes on the region
//!    boundary (any cell with a `None` in its neighbour array) act as
//!    drainage outlets: water flowing off the edge of the region is
//!    considered to have left the simulation.
//! 2. `downstream` — for every hex, the region-hex index of the single
//!    steepest-descent neighbour on the filled surface, or `None` if the
//!    hex is a boundary cell that discharges out of the region.
//! 3. `flow_accumulation` — number of hexes (including the hex itself)
//!    whose drainage path passes through each hex. Rivers are implicit:
//!    hexes with accumulation above a caller-chosen threshold are river
//!    cells.
//! 4. `is_lake` — hexes whose `filled_elevation` exceeds the original
//!    elevation by more than the configured epsilon. These are the hexes
//!    that sit below the lip of a drainage basin and were "filled in" by
//!    Planchon-Darboux.
//!
//! # Why D6 / steepest-descent
//!
//! With only six neighbours per cell (or five at pentagon boundaries) D6
//! steepest-descent is a reasonable analogue of D8 flow on a square
//! raster. The gnomonic tangent-plane spacing between hex centres is
//! roughly uniform across a region, so we route on filled elevation
//! directly without weighting by horizontal distance. Multiple-flow-
//! direction (MFD) is a future refinement; DET-05 only needs a
//! deterministic baseline.
//!
//! # Planchon-Darboux variant
//!
//! We use the standard iterative Planchon-Darboux relaxation:
//!
//! ```text
//! filled[h] = max(elev[h], min_neighbour(filled) + epsilon)
//! ```
//!
//! with `filled` initialised to `+∞` except at boundary hexes, where it
//! is pinned to the hex's own elevation. We iterate to convergence
//! (no cell changes by more than `epsilon / 2` between sweeps). The
//! worst-case sweep count is `O(diameter)` of the boundary-reachable
//! subgraph; for a radius-1 subdivision-32 Earth region this converges
//! in well under 64 sweeps in practice.

use crate::elevation::DetailElevation;
use crate::hex_grid::HexGrid;
use crate::region::RegionSpec;
use serde::{Deserialize, Serialize};

/// Configuration for [`DetailFlow::build`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct DetailFlowConfig {
    /// Small positive slope (metres) added during Planchon-Darboux depression
    /// filling. Hexes whose filled elevation exceeds their original elevation
    /// by more than this value are classified as lakes.
    pub lake_fill_epsilon_m: f64,
}

impl Default for DetailFlowConfig {
    fn default() -> Self {
        Self {
            lake_fill_epsilon_m: 1.0e-3,
        }
    }
}

/// Per-hex flow routing state.
///
/// All four arrays are indexed by [`HexGrid::cells`] order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetailFlow {
    /// The region this flow field was generated for.
    pub spec: RegionSpec,
    /// Number of hexes whose drainage path passes through each hex
    /// (including the hex itself). Units: dimensionless "hexes' worth" of
    /// water; multiply by per-hex area to get a physical discharge.
    pub flow_accumulation: Vec<f32>,
    /// True if the hex's filled elevation exceeds its original elevation
    /// by more than the configured lake-fill epsilon (i.e. the hex sits
    /// below the lip of a depression that Planchon-Darboux flooded).
    pub is_lake: Vec<bool>,
    /// Elevation (metres) after Planchon-Darboux depression filling.
    pub filled_elevation_m: Vec<f64>,
    /// Region-hex index of the single steepest-descent neighbour on the
    /// filled surface, or `None` for boundary cells that discharge out of
    /// the region.
    pub downstream: Vec<Option<u32>>,
}

impl DetailFlow {
    /// Build flow accumulation, lakes, and downstream routing for every
    /// hex in `grid`.
    ///
    /// `elevation.per_hex_m` must align with `grid.cells`.
    pub fn build(grid: &HexGrid, elevation: &DetailElevation, config: DetailFlowConfig) -> Self {
        assert_eq!(
            elevation.per_hex_m.len(),
            grid.cells.len(),
            "DetailElevation length {} does not match HexGrid cell count {}",
            elevation.per_hex_m.len(),
            grid.cells.len(),
        );
        assert_eq!(
            grid.neighbors.len(),
            grid.cells.len(),
            "HexGrid::neighbors length {} does not match cell count {}",
            grid.neighbors.len(),
            grid.cells.len(),
        );

        let n = grid.cells.len();
        let boundary = boundary_mask(&grid.neighbors);
        let filled = planchon_darboux_fill(
            &elevation.per_hex_m,
            &grid.neighbors,
            &boundary,
            config.lake_fill_epsilon_m,
        );
        let downstream = compute_downstream(&filled, &grid.neighbors, &boundary);
        let flow_accumulation = accumulate_flow(&filled, &downstream);

        let mut is_lake = vec![false; n];
        for i in 0..n {
            if filled[i] - elevation.per_hex_m[i] > config.lake_fill_epsilon_m {
                is_lake[i] = true;
            }
        }

        DetailFlow {
            spec: grid.region_spec.clone(),
            flow_accumulation,
            is_lake,
            filled_elevation_m: filled,
            downstream,
        }
    }
}

/// Mark every hex that has at least one `None` neighbour slot as a
/// region-boundary cell. Boundary cells act as drainage outlets for the
/// Planchon-Darboux fill and are assigned `downstream = None`.
fn boundary_mask(neighbors: &[[Option<u32>; 6]]) -> Vec<bool> {
    neighbors
        .iter()
        .map(|slots| slots.iter().any(|s| s.is_none()))
        .collect()
}

/// Planchon-Darboux iterative depression filling.
///
/// Returns a new elevation array where every hex has at least one
/// neighbour whose filled elevation is strictly lower (by at least
/// `epsilon`), except for boundary hexes which are pinned to their
/// original elevation.
fn planchon_darboux_fill(
    orig: &[f64],
    neighbors: &[[Option<u32>; 6]],
    boundary: &[bool],
    epsilon: f64,
) -> Vec<f64> {
    let n = orig.len();
    let mut filled = vec![f64::INFINITY; n];
    for i in 0..n {
        if boundary[i] {
            filled[i] = orig[i];
        }
    }

    // Iterative relaxation. Each sweep visits all non-boundary cells and
    // pulls their filled value down toward the minimum of
    // `orig[h]` and `min_neighbour_filled + epsilon`. Converges when no
    // cell changes by more than half of epsilon in a full sweep.
    let convergence_delta = epsilon * 0.5;
    loop {
        let mut changed = false;
        for i in 0..n {
            if boundary[i] {
                continue;
            }
            let mut min_nb = f64::INFINITY;
            for slot in neighbors[i].iter().flatten() {
                let v = filled[*slot as usize];
                if v < min_nb {
                    min_nb = v;
                }
            }
            if !min_nb.is_finite() {
                continue;
            }
            let candidate = (min_nb + epsilon).max(orig[i]);
            if candidate + convergence_delta < filled[i] {
                filled[i] = candidate;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Any cell still at +∞ had no path to the boundary through the
    // neighbour graph (disconnected component). Clamp it to its original
    // elevation so downstream code never sees an infinity.
    for i in 0..n {
        if !filled[i].is_finite() {
            filled[i] = orig[i];
        }
    }

    filled
}

/// For each non-boundary hex, pick the neighbour with the lowest filled
/// elevation. Boundary hexes drain out of the region (`None`). Ties are
/// broken by lowest region-hex index for determinism.
fn compute_downstream(
    filled: &[f64],
    neighbors: &[[Option<u32>; 6]],
    boundary: &[bool],
) -> Vec<Option<u32>> {
    let n = filled.len();
    let mut downstream = vec![None; n];
    for i in 0..n {
        if boundary[i] {
            continue;
        }
        let here = filled[i];
        let mut best: Option<(f64, u32)> = None;
        for slot in neighbors[i].iter().flatten() {
            let j = *slot;
            let nv = filled[j as usize];
            if nv < here {
                let take = match best {
                    None => true,
                    Some((bv, bi)) => nv < bv || (nv == bv && j < bi),
                };
                if take {
                    best = Some((nv, j));
                }
            }
        }
        downstream[i] = best.map(|(_, j)| j);
    }
    downstream
}

/// Flow accumulation: process hexes in descending order of filled
/// elevation and propagate contribution downstream.
fn accumulate_flow(filled: &[f64], downstream: &[Option<u32>]) -> Vec<f32> {
    let n = filled.len();
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by(|a, b| {
        let fa = filled[*a as usize];
        let fb = filled[*b as usize];
        // Descending by elevation; tie-break by ascending index so that
        // the traversal is deterministic.
        fb.partial_cmp(&fa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(b))
    });

    let mut accum = vec![1.0f32; n];
    for &i in &order {
        if let Some(d) = downstream[i as usize] {
            accum[d as usize] += accum[i as usize];
        }
    }
    accum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevation::DetailElevationConfig;
    use crate::hex_grid::HexGridConfig;
    use ymir_atmosphere::atmosphere_model::AtmosphereModel;
    use ymir_catalog::star_context::{SpectralClass, SpectralType, StarContext};
    use ymir_surface::skeleton::SkeletonWorld;
    use ymir_system::orbital_body::{OrbitalBody, PlanetType};

    fn sun() -> StarContext {
        StarContext::from_params(
            "Sun",
            Some("Sol".to_string()),
            SpectralType {
                class: SpectralClass::G,
                subtype: 2,
                luminosity_class: "V".to_string(),
            },
            5780.0,
            1.0,
            0.0,
            1.0,
            1.0,
            4.6,
            0.0,
        )
    }

    fn d(v: f64) -> ymir_core::Sourced<f64> {
        ymir_core::Sourced::derived(v, "test")
    }

    fn earth() -> OrbitalBody {
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
            tidal_locked: ymir_core::Sourced::derived(false, "test"),
            rotation_period: d(24.0),
            is_in_hz: true,
            planet_type: PlanetType::Terran,
            name: Some("Earth".into()),
            is_known_exoplanet: false,
            continental_fraction: None,
        }
    }

    fn tiny_world(seed: u64) -> SkeletonWorld {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        SkeletonWorld::build(earth(), atmo, 2, seed)
    }

    fn build_flow(seed: u64, spec: RegionSpec) -> (HexGrid, DetailElevation, DetailFlow) {
        let world = tiny_world(seed);
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        let elev = DetailElevation::build(&world, &grid, seed, DetailElevationConfig::default());
        let flow = DetailFlow::build(&grid, &elev, DetailFlowConfig::default());
        (grid, elev, flow)
    }

    #[test]
    fn no_uphill_flow() {
        let (_grid, _elev, flow) = build_flow(1, RegionSpec::new(0, 1));
        let eps = DetailFlowConfig::default().lake_fill_epsilon_m;
        for (i, d) in flow.downstream.iter().enumerate() {
            if let Some(dj) = d {
                let here = flow.filled_elevation_m[i];
                let there = flow.filled_elevation_m[*dj as usize];
                assert!(
                    there <= here + eps,
                    "uphill flow: {i} -> {dj}, {here} -> {there}"
                );
            }
        }
    }

    #[test]
    fn every_basin_terminates_within_bounded_steps() {
        let (grid, _elev, flow) = build_flow(2, RegionSpec::new(7, 1));
        let n = grid.cells.len();
        for start in 0..n {
            let mut cur = start as u32;
            let mut steps = 0usize;
            loop {
                match flow.downstream[cur as usize] {
                    None => break, // boundary outlet
                    Some(next) => {
                        cur = next;
                        steps += 1;
                        assert!(
                            steps <= n,
                            "basin from {start} failed to terminate in {n} steps"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn water_conservation() {
        // The sum of flow leaving the region through outlet cells must
        // equal the total number of hexes in the region (every hex
        // contributes exactly 1 unit to its outlet basin).
        let (grid, _elev, flow) = build_flow(3, RegionSpec::new(0, 1));
        let n = grid.cells.len();

        // Sum accumulation at every outlet (downstream == None). Each
        // hex routes its unit contribution through exactly one outlet,
        // so the grand total matches the hex count exactly.
        let mut total_outlet_flow = 0.0f32;
        let mut outlets = 0u32;
        for (i, d) in flow.downstream.iter().enumerate() {
            if d.is_none() {
                total_outlet_flow += flow.flow_accumulation[i];
                outlets += 1;
            }
        }
        assert!(outlets > 0, "expected at least one outlet");
        // f32 accumulation is exact for integer sums up to 2^24; n fits.
        assert!(
            (total_outlet_flow - n as f32).abs() < 1.0,
            "water conservation failed: outlet flow = {total_outlet_flow}, hexes = {n}"
        );

        // Every non-outlet hex must have flow_accumulation >= 1.0.
        for v in &flow.flow_accumulation {
            assert!(*v >= 1.0, "flow accumulation {v} below unit contribution");
        }
    }

    #[test]
    fn lakes_only_where_filled_above_original() {
        let (_grid, elev, flow) = build_flow(4, RegionSpec::new(0, 1));
        let eps = DetailFlowConfig::default().lake_fill_epsilon_m;
        for (i, &is_lake) in flow.is_lake.iter().enumerate() {
            let delta = flow.filled_elevation_m[i] - elev.per_hex_m[i];
            if is_lake {
                assert!(delta > eps, "lake hex {i} but delta {delta} <= eps {eps}");
            } else {
                assert!(
                    delta <= eps + 1e-12,
                    "non-lake hex {i} but delta {delta} > eps {eps}"
                );
            }
        }
    }

    #[test]
    fn determinism_byte_identical() {
        let world = tiny_world(11);
        let spec = RegionSpec::new(5, 1);
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        let elev = DetailElevation::build(&world, &grid, 11, DetailElevationConfig::default());
        let a = DetailFlow::build(&grid, &elev, DetailFlowConfig::default());
        let b = DetailFlow::build(&grid, &elev, DetailFlowConfig::default());
        let sa = serde_json::to_vec(&a).expect("serialize a");
        let sb = serde_json::to_vec(&b).expect("serialize b");
        assert_eq!(sa, sb, "DetailFlow::build is non-deterministic");
    }

    #[test]
    fn serde_round_trip() {
        let (_grid, _elev, flow) = build_flow(13, RegionSpec::new(9, 1));
        let json = serde_json::to_string(&flow).expect("serialize");
        let back: DetailFlow = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.spec, flow.spec);
        assert_eq!(back.flow_accumulation.len(), flow.flow_accumulation.len());
        assert_eq!(back.is_lake, flow.is_lake);
        assert_eq!(back.downstream, flow.downstream);
        for (a, b) in back
            .filled_elevation_m
            .iter()
            .zip(flow.filled_elevation_m.iter())
        {
            assert!((a - b).abs() < 1e-12);
        }
        for (a, b) in back
            .flow_accumulation
            .iter()
            .zip(flow.flow_accumulation.iter())
        {
            assert!((a - b).abs() < 1e-6);
        }
    }

    /// Instrumented variant of [`DetailFlow::build`] that returns the
    /// Planchon-Darboux iteration count alongside the flow state. Used
    /// only in tests to report algorithm behaviour.
    fn build_with_iter_count(
        grid: &HexGrid,
        elevation: &DetailElevation,
        config: DetailFlowConfig,
    ) -> (DetailFlow, usize) {
        let n = grid.cells.len();
        let boundary = boundary_mask(&grid.neighbors);
        let orig = &elevation.per_hex_m;
        let epsilon = config.lake_fill_epsilon_m;

        let mut filled = vec![f64::INFINITY; n];
        for i in 0..n {
            if boundary[i] {
                filled[i] = orig[i];
            }
        }
        let convergence_delta = epsilon * 0.5;
        let mut iters = 0usize;
        loop {
            iters += 1;
            let mut changed = false;
            for i in 0..n {
                if boundary[i] {
                    continue;
                }
                let mut min_nb = f64::INFINITY;
                for slot in grid.neighbors[i].iter().flatten() {
                    let v = filled[*slot as usize];
                    if v < min_nb {
                        min_nb = v;
                    }
                }
                if !min_nb.is_finite() {
                    continue;
                }
                let candidate = (min_nb + epsilon).max(orig[i]);
                if candidate + convergence_delta < filled[i] {
                    filled[i] = candidate;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for i in 0..n {
            if !filled[i].is_finite() {
                filled[i] = orig[i];
            }
        }
        let downstream = compute_downstream(&filled, &grid.neighbors, &boundary);
        let flow_accumulation = accumulate_flow(&filled, &downstream);
        let mut is_lake = vec![false; n];
        for i in 0..n {
            if filled[i] - orig[i] > epsilon {
                is_lake[i] = true;
            }
        }
        (
            DetailFlow {
                spec: grid.region_spec.clone(),
                flow_accumulation,
                is_lake,
                filled_elevation_m: filled,
                downstream,
            },
            iters,
        )
    }

    #[test]
    fn report_earth_seed1_radius1_stats() {
        // Produces the numbers reported in TASKS.md DET-05 NOTE. Run with
        // `cargo test -p ymir-detail --release -- --nocapture
        // report_earth_seed1_radius1_stats` to see the figures.
        let world = tiny_world(1);
        let spec = RegionSpec::new(0, 1);
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        let elev = DetailElevation::build(&world, &grid, 1, DetailElevationConfig::default());
        let (flow, iters) = build_with_iter_count(&grid, &elev, DetailFlowConfig::default());

        let lake_count = flow.is_lake.iter().filter(|b| **b).count();
        let outlets = flow.downstream.iter().filter(|d| d.is_none()).count();
        let (max_acc, max_idx) =
            flow.flow_accumulation
                .iter()
                .enumerate()
                .fold(
                    (0.0f32, 0usize),
                    |(mv, mi), (i, v)| {
                        if *v > mv { (*v, i) } else { (mv, mi) }
                    },
                );
        let max_cell = &grid.cells[max_idx];
        let max_cell_is_boundary = grid.neighbors[max_idx].iter().any(|s| s.is_none());

        eprintln!(
            "DET-05 stats: cells={} pd_iters={} lakes={} outlets={} max_flow={:.0} at hex {} (parent_tile={} q={} r={} lat={:.4} lon={:.4} boundary={})",
            grid.cells.len(),
            iters,
            lake_count,
            outlets,
            max_acc,
            max_idx,
            max_cell.parent_tile,
            max_cell.q,
            max_cell.r,
            max_cell.lat_rad,
            max_cell.lon_rad,
            max_cell_is_boundary,
        );
    }

    #[test]
    fn config_round_trip() {
        let cfg = DetailFlowConfig {
            lake_fill_epsilon_m: 2.5e-3,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: DetailFlowConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, cfg);
    }
}
