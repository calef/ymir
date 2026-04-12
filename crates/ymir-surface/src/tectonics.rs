//! Tectonic plate generation and boundary classification for continental
//! structure and mountain range placement.
//!
//! Plates are seeded on the geodesic grid and expanded via parallel BFS, producing
//! a Voronoi-like partition of the surface. Each plate has a motion vector tangent
//! to the sphere and an oceanic/continental designation. Boundaries between plates
//! are classified by the relative motion of their owners: plates moving toward each
//! other form convergent boundaries (mountain building), plates moving apart form
//! divergent boundaries (rifts), and plates sliding past each other form transform
//! boundaries. A per-tile elevation bias field captures the cumulative effect of
//! plate type and nearby boundaries, feeding into heightmap generation downstream.

use crate::geodesic::GeodesicGrid;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use ymir_core::prng::WorldRng;

/// Type of boundary between two adjacent plates, determined by relative motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundaryType {
    /// Plates are moving toward each other. Mountain building, positive elevation bias.
    Convergent,
    /// Plates are moving apart. Rifts or oceanic ridges, negative elevation bias.
    Divergent,
    /// Plates are sliding past each other. No strong elevation bias.
    Transform,
}

/// A single tectonic plate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plate {
    /// Plate identifier (index into `TectonicData::plates`).
    pub id: usize,
    /// Tile index from which this plate's flood fill originated.
    pub seed_tile: usize,
    /// Number of tiles currently assigned to this plate.
    pub tile_count: usize,
    /// Unit vector tangent to the sphere at `seed_tile`, representing plate motion.
    pub motion_direction: [f64; 3],
    /// Whether this is an oceanic plate (otherwise continental).
    pub is_oceanic: bool,
}

/// A boundary segment between two adjacent tiles belonging to different plates.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlateBoundary {
    /// Lower-indexed tile in the boundary pair.
    pub tile_a: usize,
    /// Higher-indexed tile in the boundary pair.
    pub tile_b: usize,
    /// Plate id owning `tile_a`.
    pub plate_a: usize,
    /// Plate id owning `tile_b`.
    pub plate_b: usize,
    /// Classified boundary type.
    pub boundary_type: BoundaryType,
}

/// Full tectonic simulation result for a geodesic grid.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TectonicData {
    /// All plates in the simulation.
    pub plates: Vec<Plate>,
    /// Plate assignment for each tile, indexed by tile index.
    pub tile_plate_assignment: Vec<usize>,
    /// All plate boundary segments.
    pub boundaries: Vec<PlateBoundary>,
    /// Per-tile elevation bias in [-1, 1], indexed by tile index.
    pub elevation_bias: Vec<f64>,
}

/// Configuration parameters for tectonic generation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TectonicConfig {
    /// Number of plates to seed.
    pub plate_count: usize,
    /// Fraction of plates that should be oceanic (0.0 = all continental, 1.0 = all oceanic).
    pub oceanic_fraction: f64,
}

impl Default for TectonicConfig {
    fn default() -> Self {
        Self {
            plate_count: 12,
            oceanic_fraction: 0.6,
        }
    }
}

/// Threshold on the normalized dot product between relative motion and separation
/// direction used to distinguish convergent/divergent from transform boundaries.
const BOUNDARY_CLASSIFICATION_THRESHOLD: f64 = 0.3;

/// Elevation bias applied to both tiles of a convergent boundary.
const CONVERGENT_BIAS: f64 = 0.3;
/// Elevation bias applied to both tiles of a divergent boundary.
const DIVERGENT_BIAS: f64 = -0.2;
/// Baseline elevation bias for tiles on oceanic plates.
const OCEANIC_BASELINE: f64 = -0.3;
/// Baseline elevation bias for tiles on continental plates.
const CONTINENTAL_BASELINE: f64 = 0.1;

/// Generate tectonic plates, boundaries, and elevation bias on a geodesic grid.
///
/// The algorithm:
/// 1. Select `plate_count` distinct random tiles as plate seeds.
/// 2. Flood-fill (multi-source BFS) from all seeds in lockstep, assigning each
///    tile to the plate whose seed reaches it first.
/// 3. For each plate, sample a random unit tangent vector at its seed position.
/// 4. Assign each plate as oceanic or continental based on `oceanic_fraction`.
/// 5. Enumerate every adjacent tile pair crossing a plate boundary and classify it.
/// 6. Accumulate elevation bias from plate type baseline plus boundary contributions.
pub fn generate_tectonics(
    grid: &GeodesicGrid,
    config: &TectonicConfig,
    rng: &mut WorldRng,
) -> TectonicData {
    let n_tiles = grid.tiles.len();
    let plate_count = config.plate_count.min(n_tiles).max(1);

    let seeds = pick_unique_seeds(n_tiles, plate_count, rng);
    let tile_plate_assignment = flood_fill_plates(grid, &seeds);

    let plates: Vec<Plate> = seeds
        .iter()
        .enumerate()
        .map(|(id, &seed_tile)| {
            let seed_center = grid.tiles[seed_tile].center;
            let motion_direction = random_tangent(seed_center, rng);
            let is_oceanic = rng.next_f64() < config.oceanic_fraction;
            let tile_count = tile_plate_assignment.iter().filter(|&&p| p == id).count();
            Plate {
                id,
                seed_tile,
                tile_count,
                motion_direction,
                is_oceanic,
            }
        })
        .collect();

    let boundaries = find_boundaries(grid, &tile_plate_assignment, &plates);
    let elevation_bias =
        compute_elevation_bias(n_tiles, &tile_plate_assignment, &plates, &boundaries);

    TectonicData {
        plates,
        tile_plate_assignment,
        boundaries,
        elevation_bias,
    }
}

/// Pick `count` distinct tile indices uniformly at random from `[0, n_tiles)`.
fn pick_unique_seeds(n_tiles: usize, count: usize, rng: &mut WorldRng) -> Vec<usize> {
    let mut seeds: Vec<usize> = Vec::with_capacity(count);
    // Partial Fisher-Yates shuffle: generate a permutation prefix of length `count`.
    let mut pool: Vec<usize> = (0..n_tiles).collect();
    for i in 0..count {
        let j = i + (rng.next_f64() * (n_tiles - i) as f64).floor() as usize;
        let j = j.min(n_tiles - 1);
        pool.swap(i, j);
        seeds.push(pool[i]);
    }
    seeds
}

/// Multi-source BFS: every seed expands in lockstep, each unclaimed tile joins the
/// plate whose frontier reaches it first. Ties are broken by lower plate id (the
/// queue ordering), which is deterministic given the seed order.
fn flood_fill_plates(grid: &GeodesicGrid, seeds: &[usize]) -> Vec<usize> {
    let n = grid.tiles.len();
    let mut assignment = vec![usize::MAX; n];
    let mut queue: VecDeque<usize> = VecDeque::with_capacity(n);

    for (plate_id, &seed) in seeds.iter().enumerate() {
        // If two plates share a seed (shouldn't happen via pick_unique_seeds), the
        // earlier plate wins.
        if assignment[seed] == usize::MAX {
            assignment[seed] = plate_id;
            queue.push_back(seed);
        }
    }

    while let Some(tile_idx) = queue.pop_front() {
        let plate = assignment[tile_idx];
        for &nb in &grid.tiles[tile_idx].neighbors {
            if assignment[nb] == usize::MAX {
                assignment[nb] = plate;
                queue.push_back(nb);
            }
        }
    }

    // Sanity: any unreached tile (disconnected grid, which shouldn't happen) goes
    // to plate 0 so downstream code never sees usize::MAX.
    for a in assignment.iter_mut() {
        if *a == usize::MAX {
            *a = 0;
        }
    }

    assignment
}

/// Sample a uniformly random unit vector in the tangent plane at `center` on the
/// unit sphere.
fn random_tangent(center: [f64; 3], rng: &mut WorldRng) -> [f64; 3] {
    // Build a tangent basis at `center`.
    let n = center;
    let ref_axis = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalize(cross(n, ref_axis));
    let v = cross(n, u);

    let theta = rng.next_f64() * 2.0 * std::f64::consts::PI;
    let (s, c) = theta.sin_cos();
    let result = [
        c * u[0] + s * v[0],
        c * u[1] + s * v[1],
        c * u[2] + s * v[2],
    ];
    normalize(result)
}

/// Enumerate plate boundaries by walking every adjacent tile pair with `a < b`.
fn find_boundaries(
    grid: &GeodesicGrid,
    tile_plate_assignment: &[usize],
    plates: &[Plate],
) -> Vec<PlateBoundary> {
    let mut boundaries = Vec::new();
    for (a, tile) in grid.tiles.iter().enumerate() {
        for &b in &tile.neighbors {
            if a >= b {
                continue;
            }
            let pa = tile_plate_assignment[a];
            let pb = tile_plate_assignment[b];
            if pa == pb {
                continue;
            }
            let boundary_type = classify_boundary(
                grid.tiles[a].center,
                grid.tiles[b].center,
                plates[pa].motion_direction,
                plates[pb].motion_direction,
            );
            boundaries.push(PlateBoundary {
                tile_a: a,
                tile_b: b,
                plate_a: pa,
                plate_b: pb,
                boundary_type,
            });
        }
    }
    boundaries
}

/// Classify a boundary by projecting the relative motion (plate_a minus plate_b)
/// onto the separation direction from `a` toward `b`. If plate_a is moving toward
/// `b` faster than plate_b is, the boundary is convergent; the reverse indicates
/// divergence; near-zero projection indicates transform motion.
fn classify_boundary(
    center_a: [f64; 3],
    center_b: [f64; 3],
    motion_a: [f64; 3],
    motion_b: [f64; 3],
) -> BoundaryType {
    let sep = normalize([
        center_b[0] - center_a[0],
        center_b[1] - center_a[1],
        center_b[2] - center_a[2],
    ]);
    let rel = [
        motion_a[0] - motion_b[0],
        motion_a[1] - motion_b[1],
        motion_a[2] - motion_b[2],
    ];
    // Positive projection means plate_a is approaching plate_b's side.
    let projection = dot(rel, sep);

    if projection > BOUNDARY_CLASSIFICATION_THRESHOLD {
        BoundaryType::Convergent
    } else if projection < -BOUNDARY_CLASSIFICATION_THRESHOLD {
        BoundaryType::Divergent
    } else {
        BoundaryType::Transform
    }
}

/// Accumulate per-tile elevation bias from plate type baseline plus boundary
/// contributions, then clamp into [-1, 1].
fn compute_elevation_bias(
    n_tiles: usize,
    tile_plate_assignment: &[usize],
    plates: &[Plate],
    boundaries: &[PlateBoundary],
) -> Vec<f64> {
    let mut bias = vec![0.0_f64; n_tiles];

    for (tile_idx, &plate_id) in tile_plate_assignment.iter().enumerate() {
        bias[tile_idx] += if plates[plate_id].is_oceanic {
            OCEANIC_BASELINE
        } else {
            CONTINENTAL_BASELINE
        };
    }

    for b in boundaries {
        let delta = match b.boundary_type {
            BoundaryType::Convergent => CONVERGENT_BIAS,
            BoundaryType::Divergent => DIVERGENT_BIAS,
            BoundaryType::Transform => 0.0,
        };
        if delta != 0.0 {
            bias[b.tile_a] += delta;
            bias[b.tile_b] += delta;
        }
    }

    for v in bias.iter_mut() {
        *v = v.clamp(-1.0, 1.0);
    }
    bias
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len == 0.0 {
        [0.0, 0.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tectonics(level: u32, seed: u64, plate_count: usize) -> (GeodesicGrid, TectonicData) {
        let grid = GeodesicGrid::new(level);
        let config = TectonicConfig {
            plate_count,
            oceanic_fraction: 0.6,
        };
        let mut rng = WorldRng::new(seed);
        let data = generate_tectonics(&grid, &config, &mut rng);
        (grid, data)
    }

    #[test]
    fn every_tile_assigned_to_valid_plate() {
        let (grid, data) = make_tectonics(3, 42, 12);
        assert_eq!(data.tile_plate_assignment.len(), grid.tiles.len());
        for (i, &p) in data.tile_plate_assignment.iter().enumerate() {
            assert!(
                p < data.plates.len(),
                "tile {i} has invalid plate id {p} (plate count {})",
                data.plates.len()
            );
        }
    }

    #[test]
    fn plate_count_matches_config() {
        let (_grid, data) = make_tectonics(3, 42, 8);
        assert_eq!(data.plates.len(), 8);
    }

    #[test]
    fn plate_tile_counts_sum_to_total() {
        let (grid, data) = make_tectonics(3, 7, 10);
        let sum: usize = data.plates.iter().map(|p| p.tile_count).sum();
        assert_eq!(sum, grid.tiles.len());
    }

    #[test]
    fn boundaries_have_distinct_plates() {
        let (_grid, data) = make_tectonics(3, 99, 10);
        for b in &data.boundaries {
            assert_ne!(b.plate_a, b.plate_b);
        }
    }

    #[test]
    fn boundaries_are_symmetric_and_unique() {
        let (_grid, data) = make_tectonics(3, 99, 10);
        // Each boundary entry should have tile_a < tile_b (canonical order).
        for b in &data.boundaries {
            assert!(b.tile_a < b.tile_b);
        }
        // No duplicate (tile_a, tile_b) pairs.
        let mut seen = std::collections::HashSet::new();
        for b in &data.boundaries {
            assert!(
                seen.insert((b.tile_a, b.tile_b)),
                "duplicate boundary for tiles ({}, {})",
                b.tile_a,
                b.tile_b
            );
        }
    }

    #[test]
    fn elevation_bias_in_range() {
        let (grid, data) = make_tectonics(3, 11, 12);
        assert_eq!(data.elevation_bias.len(), grid.tiles.len());
        for (i, &b) in data.elevation_bias.iter().enumerate() {
            assert!(
                (-1.0..=1.0).contains(&b),
                "tile {i} elevation bias {b} out of range"
            );
        }
    }

    #[test]
    fn convergent_boundaries_raise_average_bias() {
        // If no convergent boundaries exist in one seed, try a few.
        let mut found_convergent = false;
        for seed in 0..10 {
            let (_grid, data) = make_tectonics(3, seed, 12);
            let convergent_tiles: std::collections::HashSet<usize> = data
                .boundaries
                .iter()
                .filter(|b| b.boundary_type == BoundaryType::Convergent)
                .flat_map(|b| [b.tile_a, b.tile_b])
                .collect();
            let non_convergent_tiles: Vec<usize> = (0..data.elevation_bias.len())
                .filter(|i| !convergent_tiles.contains(i))
                .collect();
            if convergent_tiles.is_empty() || non_convergent_tiles.is_empty() {
                continue;
            }
            found_convergent = true;
            let avg_conv: f64 = convergent_tiles
                .iter()
                .map(|&i| data.elevation_bias[i])
                .sum::<f64>()
                / convergent_tiles.len() as f64;
            let avg_other: f64 = non_convergent_tiles
                .iter()
                .map(|&i| data.elevation_bias[i])
                .sum::<f64>()
                / non_convergent_tiles.len() as f64;
            assert!(
                avg_conv > avg_other,
                "convergent avg {avg_conv} should exceed non-convergent avg {avg_other} (seed {seed})"
            );
            break;
        }
        assert!(
            found_convergent,
            "expected at least one seed to produce convergent boundaries"
        );
    }

    #[test]
    fn determinism_same_seed_same_output() {
        let grid = GeodesicGrid::new(3);
        let config = TectonicConfig {
            plate_count: 10,
            oceanic_fraction: 0.6,
        };
        let mut rng1 = WorldRng::new(2024);
        let mut rng2 = WorldRng::new(2024);
        let a = generate_tectonics(&grid, &config, &mut rng1);
        let b = generate_tectonics(&grid, &config, &mut rng2);
        assert_eq!(a.tile_plate_assignment, b.tile_plate_assignment);
        assert_eq!(a.plates.len(), b.plates.len());
        for (pa, pb) in a.plates.iter().zip(b.plates.iter()) {
            assert_eq!(pa.seed_tile, pb.seed_tile);
            assert_eq!(pa.is_oceanic, pb.is_oceanic);
            assert_eq!(pa.motion_direction, pb.motion_direction);
        }
        assert_eq!(a.boundaries.len(), b.boundaries.len());
        assert_eq!(a.elevation_bias, b.elevation_bias);
    }

    #[test]
    fn motion_directions_are_unit_tangent() {
        let (grid, data) = make_tectonics(2, 5, 8);
        for plate in &data.plates {
            let m = plate.motion_direction;
            let len = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-10, "motion not unit length: {len}");
            let c = grid.tiles[plate.seed_tile].center;
            let radial = c[0] * m[0] + c[1] * m[1] + c[2] * m[2];
            assert!(radial.abs() < 1e-10, "motion not tangent: radial {radial}");
        }
    }

    #[test]
    fn seeds_are_unique() {
        let (_grid, data) = make_tectonics(3, 77, 15);
        let mut seeds: Vec<usize> = data.plates.iter().map(|p| p.seed_tile).collect();
        seeds.sort();
        seeds.dedup();
        assert_eq!(seeds.len(), data.plates.len());
    }
}
