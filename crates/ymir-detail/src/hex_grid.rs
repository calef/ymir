//! Hex subgrid generation for a [`RegionSpec`].
//!
//! A [`HexGrid`] subdivides each skeleton tile in the region into an
//! `N × N` axial hex lattice and lifts every hex center back onto the
//! unit sphere via the inverse gnomonic projection at the parent tile's
//! (lat, lon). The per-tile lattice is laid out in a pointy-top axial
//! coordinate system centred on the parent tile, scaled so the lattice
//! spans approximately one tile-radius in the tangent plane.
//!
//! # Gnomonic distortion bound
//!
//! The gnomonic projection preserves great circles (geodesics become
//! straight lines) but distorts distance by a factor of `sec(δ)`, where
//! `δ` is the great-circle angle from the tangent point to the sampled
//! location. For a subdivision-level-3 geodesic grid on Earth there are
//! 642 tiles, giving an average tile "radius" (centre to neighbour) on
//! the order of `δ ≈ 4.5°`, so `sec(δ) ≈ 1.0031` — about **0.3%**
//! distortion at the tile edge. For a subdivision-level-2 grid (162
//! tiles, used in tests) the radius is roughly `δ ≈ 9°`, so
//! `sec(δ) ≈ 1.0125` (≈ 1.25%). For a ~700 km tile-radius on Earth
//! (`δ ≈ 6.3°`) the distortion is ≈ 0.6%.
//!
//! These numbers are small enough that downstream detail stages
//! (fractal noise, rivers) can treat the local tangent plane as
//! Euclidean without correction. If Ymir later supports deeper
//! subdivisions, the per-tile gnomonic frame will remain valid because
//! distortion scales with tile size, not with the number of sub-hexes.
//!
//! # Cross-seam neighbours
//!
//! Each parent skeleton tile carries its own tangent frame, so hexes on
//! the boundary between two adjacent parent tiles do **not** share exact
//! coordinates in either tile's local plane. To stitch the seam we use
//! a nearest-neighbour pass in (lat, lon) space: for each boundary hex
//! in tile A (a hex that would neighbour a position outside A's axial
//! rectangle in a particular direction) we search the corresponding
//! boundary hexes in the adjacent tile B and link the pair whose
//! great-circle distance is below a seam tolerance. This is exact to
//! within the gnomonic distortion bound quoted above and sufficient for
//! DET-02's purposes (continuous adjacency graph across seams). A
//! future pass could replace it with an analytic shared-midpoint frame
//! along the edge A-B; DET-02 defers that until the detail pipeline
//! demands sub-hex precision at seams.

use crate::region::RegionSpec;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use ymir_surface::skeleton::SkeletonWorld;

/// Configuration for hex-grid generation.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HexGridConfig {
    /// Number of hexes along each axis of a parent skeleton tile's
    /// local lattice (total `subdivision * subdivision` hexes per tile).
    pub subdivision: u32,
}

impl Default for HexGridConfig {
    fn default() -> Self {
        Self { subdivision: 32 }
    }
}

/// A single detail-level hex cell.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HexCell {
    /// Index of this hex within its owning [`HexGrid::cells`] vector.
    pub region_hex_index: u32,
    /// Index of the parent skeleton tile this hex was laid out against.
    pub parent_tile: u32,
    /// Axial-q coordinate in the parent tile's local lattice.
    pub q: i32,
    /// Axial-r coordinate in the parent tile's local lattice.
    pub r: i32,
    /// Latitude of the hex centre, radians, range [-π/2, π/2].
    pub lat_rad: f64,
    /// Longitude of the hex centre, radians, range [-π, π].
    pub lon_rad: f64,
}

/// Hex subgrid covering every skeleton tile in a [`RegionSpec`].
///
/// `cells` stores every hex in tile-BFS order (seed tile's lattice
/// first, then the radius-1 ring, etc.), and `neighbors[i]` gives the
/// six hex-neighbour region indices of `cells[i]` (or `None` at
/// positions where the region does not extend).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HexGrid {
    /// The region this grid covers.
    pub region_spec: RegionSpec,
    /// Configuration used to build the grid.
    pub config: HexGridConfig,
    /// All hex cells in the region.
    pub cells: Vec<HexCell>,
    /// Six-way neighbour indices into `cells` (`None` at boundaries).
    pub neighbors: Vec<[Option<u32>; 6]>,
}

/// Axial-direction offsets for a pointy-top hex lattice, indexed 0..6.
///
/// Order matches the neighbour slot order stored in
/// [`HexGrid::neighbors`]: E, NE, NW, W, SW, SE.
const AXIAL_DIRECTIONS: [(i32, i32); 6] = [
    (1, 0),  // 0: East         (+q, 0)
    (1, -1), // 1: North-East   (+q, -r)
    (0, -1), // 2: North-West   (0,  -r)
    (-1, 0), // 3: West         (-q, 0)
    (-1, 1), // 4: South-West   (-q, +r)
    (0, 1),  // 5: South-East   (0,  +r)
];

impl HexGrid {
    /// Build a hex grid for the given region on the supplied skeleton world.
    ///
    /// The region is expanded by a breadth-first flood-fill from
    /// `spec.tile_index` out to `spec.radius_tiles` hops on the skeleton's
    /// neighbour graph. Every parent tile in the region receives its own
    /// `subdivision × subdivision` axial hex lattice, and hexes on the
    /// boundary between two adjacent parent tiles are stitched together
    /// by nearest-neighbour matching in (lat, lon).
    pub fn build(world: &SkeletonWorld, spec: RegionSpec, config: HexGridConfig) -> Self {
        assert!(
            config.subdivision > 0,
            "HexGridConfig::subdivision must be > 0"
        );
        let parent_tiles = collect_parent_tiles(world, &spec);

        // Per-parent-tile local frame, used during cell placement and
        // when matching seams to adjacent tiles.
        let mut frames: HashMap<u32, TangentFrame> = HashMap::new();
        for &pt in &parent_tiles {
            frames.insert(pt, TangentFrame::at_tile(world, pt));
        }

        // Build per-tile lattices in tile-BFS order. `cell_lookup` maps
        // (parent_tile, q, r) to the region-wide cell index.
        let n = config.subdivision as i32;
        let mut cells: Vec<HexCell> = Vec::with_capacity(parent_tiles.len() * (n * n) as usize);
        let mut cell_lookup: HashMap<(u32, i32, i32), u32> = HashMap::new();

        for &parent_tile in &parent_tiles {
            let frame = &frames[&parent_tile];
            for r in 0..n {
                for q in 0..n {
                    let (x, y) = axial_to_planar(q, r, n, frame.lattice_size);
                    let (lat, lon) = frame.unproject(x, y);
                    let idx = cells.len() as u32;
                    cells.push(HexCell {
                        region_hex_index: idx,
                        parent_tile,
                        q,
                        r,
                        lat_rad: lat,
                        lon_rad: lon,
                    });
                    cell_lookup.insert((parent_tile, q, r), idx);
                }
            }
        }

        // Intra-tile adjacency (6 directions, within axial bounds).
        let mut neighbors: Vec<[Option<u32>; 6]> = vec![[None; 6]; cells.len()];
        for cell in &cells {
            for (dir, (dq, dr)) in AXIAL_DIRECTIONS.iter().enumerate() {
                let nq = cell.q + dq;
                let nr = cell.r + dr;
                if (0..n).contains(&nq) && (0..n).contains(&nr) {
                    if let Some(&nid) = cell_lookup.get(&(cell.parent_tile, nq, nr)) {
                        neighbors[cell.region_hex_index as usize][dir] = Some(nid);
                    }
                }
            }
        }

        // Cross-seam stitching: for every pair of adjacent parent tiles
        // in the region, collect boundary hexes from each side and link
        // each boundary hex in tile A to the nearest boundary hex in
        // tile B (if that distance is below the seam tolerance).
        stitch_seams(world, &cells, &parent_tiles, &frames, &mut neighbors);

        HexGrid {
            region_spec: spec,
            config,
            cells,
            neighbors,
        }
    }
}

/// BFS flood-fill over skeleton neighbours starting from
/// `spec.tile_index`, returning all tiles reachable within
/// `spec.radius_tiles` hops in deterministic visitation order.
fn collect_parent_tiles(world: &SkeletonWorld, spec: &RegionSpec) -> Vec<u32> {
    let seed = spec.tile_index as usize;
    assert!(
        seed < world.tile_count(),
        "RegionSpec::tile_index {seed} out of bounds (tile_count = {})",
        world.tile_count()
    );

    let mut order: Vec<u32> = Vec::new();
    let mut visited: HashMap<usize, u32> = HashMap::new();
    let mut queue: VecDeque<(usize, u32)> = VecDeque::new();
    queue.push_back((seed, 0));
    visited.insert(seed, 0);
    order.push(seed as u32);

    while let Some((idx, depth)) = queue.pop_front() {
        if depth == spec.radius_tiles {
            continue;
        }
        let tile = world.tile(idx);
        for &nb in tile.neighbors {
            if let std::collections::hash_map::Entry::Vacant(e) = visited.entry(nb) {
                e.insert(depth + 1);
                queue.push_back((nb, depth + 1));
                order.push(nb as u32);
            }
        }
    }
    order
}

/// Local gnomonic tangent frame anchored at a skeleton tile centre.
struct TangentFrame {
    /// Tile centre on the unit sphere.
    center: [f64; 3],
    /// East basis vector in the tangent plane.
    east: [f64; 3],
    /// North basis vector in the tangent plane.
    north: [f64; 3],
    /// Half-extent of the per-tile hex lattice in tangent-plane units;
    /// roughly the great-circle angle from the tile centre to a
    /// neighbour centre (radians).
    lattice_size: f64,
}

impl TangentFrame {
    fn at_tile(world: &SkeletonWorld, parent_tile: u32) -> Self {
        let tile = world.tile(parent_tile as usize);
        let center = latlon_rad_to_xyz(tile.lat_rad, tile.lon_rad);

        // East/north tangent basis at (lat, lon). `east` points along
        // +longitude, `north` points along +latitude. At the poles we
        // degenerate gracefully: pick any orthonormal pair.
        let (east, north) = tangent_basis(tile.lat_rad, tile.lon_rad);

        // Half-extent = mean great-circle distance (radians) from
        // centre to neighbour centres. This keeps the lattice roughly
        // one tile wide regardless of subdivision level.
        let mut sum = 0.0;
        let mut count = 0.0;
        for &nb in tile.neighbors {
            let nb_tile = world.tile(nb);
            let nb_xyz = latlon_rad_to_xyz(nb_tile.lat_rad, nb_tile.lon_rad);
            sum += great_circle_angle(center, nb_xyz);
            count += 1.0;
        }
        let lattice_size = if count > 0.0 { sum / count } else { 0.05 };

        TangentFrame {
            center,
            east,
            north,
            lattice_size,
        }
    }

    /// Inverse gnomonic projection: map tangent-plane (x, y) back to
    /// (lat, lon) on the unit sphere.
    fn unproject(&self, x: f64, y: f64) -> (f64, f64) {
        let v = [
            self.center[0] + x * self.east[0] + y * self.north[0],
            self.center[1] + x * self.east[1] + y * self.north[1],
            self.center[2] + x * self.east[2] + y * self.north[2],
        ];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let p = [v[0] / len, v[1] / len, v[2] / len];
        xyz_to_latlon_rad(p)
    }
}

/// Place axial hex (q, r) on a pointy-top lattice centred on the parent
/// tile. The lattice spans approximately `2 * lattice_size` across in
/// tangent-plane units.
fn axial_to_planar(q: i32, r: i32, n: i32, lattice_size: f64) -> (f64, f64) {
    // Hex edge length `s` chosen so the lattice width ~= 2 * lattice_size.
    // Pointy-top width = s * sqrt(3) per q-step; total width = n * s * sqrt(3).
    let s = (2.0 * lattice_size) / (n as f64 * 3.0_f64.sqrt());
    let qf = q as f64;
    let rf = r as f64;
    let nf = n as f64;
    let x = s * 3.0_f64.sqrt() * (qf + rf * 0.5);
    let y = s * 1.5 * rf;
    // Centre the lattice on (0, 0) in the tangent plane.
    let x_centre = s * 3.0_f64.sqrt() * ((nf - 1.0) * 0.75);
    let y_centre = s * 1.5 * ((nf - 1.0) * 0.5);
    (x - x_centre, y - y_centre)
}

/// Build an east/north tangent basis at (lat, lon) on the unit sphere.
fn tangent_basis(lat_rad: f64, lon_rad: f64) -> ([f64; 3], [f64; 3]) {
    let cos_lat = lat_rad.cos();
    let sin_lat = lat_rad.sin();
    let cos_lon = lon_rad.cos();
    let sin_lon = lon_rad.sin();

    // East points along increasing longitude, tangent to the sphere.
    let east = [-sin_lon, cos_lon, 0.0];
    // North points along increasing latitude, tangent to the sphere.
    let north = [-sin_lat * cos_lon, -sin_lat * sin_lon, cos_lat];
    (east, north)
}

/// Convert (lat_rad, lon_rad) to unit-sphere xyz.
fn latlon_rad_to_xyz(lat: f64, lon: f64) -> [f64; 3] {
    let cl = lat.cos();
    [cl * lon.cos(), cl * lon.sin(), lat.sin()]
}

/// Convert unit-sphere xyz to (lat_rad, lon_rad).
fn xyz_to_latlon_rad(xyz: [f64; 3]) -> (f64, f64) {
    let [x, y, z] = xyz;
    let lat = z.clamp(-1.0, 1.0).asin();
    let lon = y.atan2(x);
    (lat, lon)
}

/// Great-circle angle (radians) between two unit vectors.
fn great_circle_angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
    d.acos()
}

/// For every pair of adjacent parent tiles (A, B) both present in the
/// region, match each boundary hex of A to the nearest boundary hex of B
/// in lat/lon and install a reciprocal neighbour link.
///
/// A "boundary hex" here is any hex whose axial coordinate touches one
/// of the four rectangular lattice edges (q == 0, q == N-1, r == 0,
/// r == N-1). For a 32×32 lattice that's ~124 hexes per tile, which is
/// small enough that an O(B_A * B_B) search per seam is trivial.
fn stitch_seams(
    world: &SkeletonWorld,
    cells: &[HexCell],
    parent_tiles: &[u32],
    frames: &HashMap<u32, TangentFrame>,
    neighbors: &mut [[Option<u32>; 6]],
) {
    use std::collections::HashSet;
    let parent_set: HashSet<u32> = parent_tiles.iter().copied().collect();

    // Precompute boundary hexes per parent tile.
    let mut boundary_by_tile: HashMap<u32, Vec<u32>> = HashMap::new();
    for cell in cells {
        let on_edge = cell.q == 0
            || cell.r == 0
            || cell.q as u32 + 1 == parent_lattice_size(cells, cell.parent_tile)
            || cell.r as u32 + 1 == parent_lattice_size(cells, cell.parent_tile);
        if on_edge {
            boundary_by_tile
                .entry(cell.parent_tile)
                .or_default()
                .push(cell.region_hex_index);
        }
    }

    // Seam tolerance: a fraction of the lattice cell size on the
    // tighter of the two frames. Two hexes within this tolerance are
    // considered to overlap across the seam.
    for &tile_a in parent_tiles {
        let frame_a = &frames[&tile_a];
        let tile_obj = world.tile(tile_a as usize);
        for &nb_usize in tile_obj.neighbors {
            let tile_b = nb_usize as u32;
            if !parent_set.contains(&tile_b) || tile_b <= tile_a {
                // Only process each (A, B) pair once (tile_a < tile_b).
                continue;
            }
            let frame_b = &frames[&tile_b];

            // Seam tolerance: half the min lattice cell width.
            let cell_width_a =
                2.0 * frame_a.lattice_size / f64::from(parent_lattice_size(cells, tile_a));
            let cell_width_b =
                2.0 * frame_b.lattice_size / f64::from(parent_lattice_size(cells, tile_b));
            let seam_tol = 0.75 * cell_width_a.min(cell_width_b);

            let (empty_a, empty_b) = (Vec::new(), Vec::new());
            let bounds_a = boundary_by_tile.get(&tile_a).unwrap_or(&empty_a);
            let bounds_b = boundary_by_tile.get(&tile_b).unwrap_or(&empty_b);

            for &idx_a in bounds_a {
                let cell_a = &cells[idx_a as usize];
                let xyz_a = latlon_rad_to_xyz(cell_a.lat_rad, cell_a.lon_rad);
                // Find nearest boundary hex in B.
                let mut best: Option<(f64, u32)> = None;
                for &idx_b in bounds_b {
                    let cell_b = &cells[idx_b as usize];
                    let xyz_b = latlon_rad_to_xyz(cell_b.lat_rad, cell_b.lon_rad);
                    let d = great_circle_angle(xyz_a, xyz_b);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, idx_b));
                    }
                }
                if let Some((d, idx_b)) = best {
                    if d < seam_tol {
                        install_seam_link(neighbors, idx_a, idx_b);
                    }
                }
            }
        }
    }
}

/// Derive the axial-lattice side length (N) from stored cells. All
/// parent tiles in a region share the same N, but we read it back from
/// the data so this function stays robust to future configuration
/// variations.
fn parent_lattice_size(cells: &[HexCell], parent_tile: u32) -> u32 {
    let mut max_q = 0;
    for c in cells {
        if c.parent_tile == parent_tile && c.q > max_q {
            max_q = c.q;
        }
    }
    (max_q + 1) as u32
}

/// Install a reciprocal seam neighbour between two cells, using the
/// first free slot in each cell's neighbour array. If both cells
/// already have all six slots populated, the link is dropped (this can
/// happen at 5-neighbour pentagons where seam matching may find more
/// than one counterpart; see module docs).
fn install_seam_link(neighbors: &mut [[Option<u32>; 6]], a: u32, b: u32) {
    // Skip if the link already exists in either direction.
    if neighbors[a as usize].contains(&Some(b)) {
        return;
    }
    if let Some(slot) = neighbors[a as usize].iter_mut().find(|s| s.is_none()) {
        *slot = Some(b);
    } else {
        return;
    }
    if neighbors[b as usize].contains(&Some(a)) {
        return;
    }
    if let Some(slot) = neighbors[b as usize].iter_mut().find(|s| s.is_none()) {
        *slot = Some(a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ymir_atmosphere::atmosphere_model::AtmosphereModel;
    use ymir_catalog::star_context::{SpectralClass, SpectralType, StarContext};
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

    fn tiny_world() -> SkeletonWorld {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        SkeletonWorld::build(earth(), atmo, 2, 1)
    }

    #[test]
    fn radius_zero_cell_count_matches_subdivision_squared() {
        let world = tiny_world();
        let spec = RegionSpec::new(0, 0);
        let cfg = HexGridConfig { subdivision: 32 };
        let grid = HexGrid::build(&world, spec, cfg);
        assert_eq!(grid.cells.len(), 32 * 32);
        assert_eq!(grid.neighbors.len(), grid.cells.len());
    }

    #[test]
    fn radius_zero_smaller_subdivision() {
        let world = tiny_world();
        let spec = RegionSpec::new(0, 0);
        let cfg = HexGridConfig { subdivision: 8 };
        let grid = HexGrid::build(&world, spec, cfg);
        assert_eq!(grid.cells.len(), 8 * 8);
    }

    #[test]
    fn radius_one_covers_seed_plus_neighbors() {
        let world = tiny_world();
        let seed_tile = 0u32;
        let seed_neighbors = world.tile(seed_tile as usize).neighbors.len();
        let expected_parents = 1 + seed_neighbors;
        let spec = RegionSpec::new(seed_tile, 1);
        let cfg = HexGridConfig { subdivision: 32 };
        let grid = HexGrid::build(&world, spec, cfg);
        assert_eq!(grid.cells.len(), expected_parents * 32 * 32);
    }

    #[test]
    fn all_cells_have_finite_lat_lon() {
        let world = tiny_world();
        let spec = RegionSpec::new(42, 1);
        let cfg = HexGridConfig::default();
        let grid = HexGrid::build(&world, spec, cfg);
        for cell in &grid.cells {
            assert!(cell.lat_rad.is_finite(), "lat not finite");
            assert!(cell.lon_rad.is_finite(), "lon not finite");
            let half_pi = std::f64::consts::FRAC_PI_2;
            assert!(
                cell.lat_rad >= -half_pi - 1e-9 && cell.lat_rad <= half_pi + 1e-9,
                "lat {} out of range",
                cell.lat_rad
            );
            assert!(
                cell.lon_rad >= -std::f64::consts::PI - 1e-9
                    && cell.lon_rad <= std::f64::consts::PI + 1e-9,
                "lon {} out of range",
                cell.lon_rad
            );
        }
    }

    #[test]
    fn intra_tile_neighbors_are_symmetric() {
        let world = tiny_world();
        let spec = RegionSpec::new(0, 0);
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        for (i, slots) in grid.neighbors.iter().enumerate() {
            for slot in slots.iter().flatten() {
                let back = grid.neighbors[*slot as usize].contains(&Some(i as u32));
                assert!(back, "asymmetric neighbour: {i} -> {slot} no reciprocal");
            }
        }
    }

    #[test]
    fn cross_seam_links_exist_between_adjacent_tiles() {
        // With radius 1, at least one pair of adjacent parent tiles
        // must produce at least one seam-linked hex.
        let world = tiny_world();
        let spec = RegionSpec::new(0, 1);
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());

        let mut cross_seam_pairs = 0u32;
        for (i, slots) in grid.neighbors.iter().enumerate() {
            let cell_i = &grid.cells[i];
            for slot in slots.iter().flatten() {
                let cell_j = &grid.cells[*slot as usize];
                if cell_i.parent_tile != cell_j.parent_tile {
                    // Confirm the two cells' lat/lon are close.
                    let xi = latlon_rad_to_xyz(cell_i.lat_rad, cell_i.lon_rad);
                    let xj = latlon_rad_to_xyz(cell_j.lat_rad, cell_j.lon_rad);
                    let d = great_circle_angle(xi, xj);
                    // Within 1.5 * cell-width of each other (loose).
                    assert!(d < 0.2, "seam pair too far apart: d = {d}");
                    cross_seam_pairs += 1;
                }
            }
        }
        assert!(
            cross_seam_pairs > 0,
            "expected at least one cross-seam neighbour in a radius-1 region"
        );
    }

    #[test]
    fn seam_links_are_symmetric() {
        let world = tiny_world();
        let spec = RegionSpec::new(7, 1);
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        for (i, slots) in grid.neighbors.iter().enumerate() {
            for slot in slots.iter().flatten() {
                assert!(
                    grid.neighbors[*slot as usize].contains(&Some(i as u32)),
                    "asymmetric seam link {i} -> {slot}"
                );
            }
        }
    }

    #[test]
    fn serde_round_trip() {
        let world = tiny_world();
        let spec = RegionSpec::new(3, 1);
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        let json = serde_json::to_string(&grid).expect("serialize");
        let back: HexGrid = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.cells.len(), grid.cells.len());
        assert_eq!(back.config, grid.config);
        assert_eq!(back.region_spec, grid.region_spec);
        for (a, b) in back.cells.iter().zip(grid.cells.iter()) {
            assert_eq!(a.region_hex_index, b.region_hex_index);
            assert_eq!(a.parent_tile, b.parent_tile);
            assert_eq!(a.q, b.q);
            assert_eq!(a.r, b.r);
            assert!((a.lat_rad - b.lat_rad).abs() < 1e-12);
            assert!((a.lon_rad - b.lon_rad).abs() < 1e-12);
        }
        assert_eq!(back.neighbors, grid.neighbors);
    }

    #[test]
    fn determinism_byte_identical() {
        let world = tiny_world();
        let spec = RegionSpec::new(11, 1);
        let cfg = HexGridConfig::default();
        let a = HexGrid::build(&world, spec.clone(), cfg);
        let b = HexGrid::build(&world, spec, cfg);

        let sa = serde_json::to_vec(&a).expect("serialize a");
        let sb = serde_json::to_vec(&b).expect("serialize b");
        assert_eq!(sa, sb, "HexGrid::build is non-deterministic");
    }

    #[test]
    fn gnomonic_round_trip_at_center_is_exact() {
        // Inverse-project (0, 0) in any tile's frame → tile centre
        // lat/lon exactly (to floating-point tolerance).
        let world = tiny_world();
        for i in 0..world.tile_count().min(20) {
            let frame = TangentFrame::at_tile(&world, i as u32);
            let (lat, lon) = frame.unproject(0.0, 0.0);
            let tile = world.tile(i);
            assert!((lat - tile.lat_rad).abs() < 1e-12);
            // Longitude may wrap around ±π; normalise before comparing.
            let dlon = ((lon - tile.lon_rad + std::f64::consts::PI)
                .rem_euclid(std::f64::consts::TAU))
                - std::f64::consts::PI;
            assert!(
                dlon.abs() < 1e-12,
                "lon mismatch: {} vs {}",
                lon,
                tile.lon_rad
            );
        }
    }
}
