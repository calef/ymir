//! Geodesic sphere construction and subdivision for the planetary surface grid.
//!
//! Builds an icosahedral geodesic grid by recursive subdivision of triangular faces,
//! then constructs the dual mesh where each original vertex becomes a polygonal tile.
//! The 12 original icosahedron vertices produce pentagonal tiles (5 neighbors);
//! all subdivision vertices produce hexagonal tiles (6 neighbors).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use ymir_core::traits::{GeoTile, SphericalPoint};

/// Convert unit-sphere xyz coordinates to (latitude_degrees, longitude_degrees).
pub fn xyz_to_latlon(xyz: [f64; 3]) -> (f64, f64) {
    let [x, y, z] = xyz;
    let lat = z.asin().to_degrees();
    let lon = y.atan2(x).to_degrees();
    (lat, lon)
}

/// Convert (latitude_degrees, longitude_degrees) to unit-sphere xyz coordinates.
pub fn latlon_to_xyz(lat_deg: f64, lon_deg: f64) -> [f64; 3] {
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();
    [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]
}

/// Normalize a 3D vector to unit length.
fn normalize(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

/// Midpoint of two 3D points, projected onto the unit sphere.
fn midpoint_on_sphere(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    normalize([
        (a[0] + b[0]) * 0.5,
        (a[1] + b[1]) * 0.5,
        (a[2] + b[2]) * 0.5,
    ])
}

/// A single tile on the geodesic grid (dual mesh vertex).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GridTile {
    /// Index of this tile in the grid's tile array.
    pub index: usize,
    /// XYZ position on the unit sphere.
    pub center: [f64; 3],
    /// Latitude in degrees.
    pub lat: f64,
    /// Longitude in degrees.
    pub lon: f64,
    /// Indices of adjacent tiles.
    pub neighbors: Vec<usize>,
    /// Normalized elevation in [0, 1]. Default 0.5.
    pub elevation: f64,
    /// Relative area of this tile (approximate).
    pub area: f64,
}

impl GeoTile for GridTile {
    fn lat(&self) -> f64 {
        self.lat
    }
    fn lon(&self) -> f64 {
        self.lon
    }
    fn elevation(&self) -> f64 {
        self.elevation
    }
    fn area(&self) -> f64 {
        self.area
    }
}

impl SphericalPoint for GridTile {
    fn lat_rad(&self) -> f64 {
        self.lat.to_radians()
    }
    fn lon_rad(&self) -> f64 {
        self.lon.to_radians()
    }
}

/// An icosahedral geodesic grid.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeodesicGrid {
    /// Number of recursive subdivisions applied to the base icosahedron.
    pub subdivision_level: u32,
    /// Vertex positions on the unit sphere.
    pub vertices: Vec<[f64; 3]>,
    /// Tiles (dual mesh polygons, one per vertex).
    pub tiles: Vec<GridTile>,
}

impl GeodesicGrid {
    /// Build a geodesic grid at the given subdivision level.
    ///
    /// Level 0 produces the base icosahedron dual (12 tiles).
    /// Total tiles = 10 * 4^N + 2.
    pub fn new(subdivision_level: u32) -> Self {
        let (vertices, faces) = build_icosahedron();
        let (vertices, faces) = subdivide(vertices, faces, subdivision_level);
        let tiles = build_dual_tiles(&vertices, &faces);

        GeodesicGrid {
            subdivision_level,
            vertices,
            tiles,
        }
    }

    /// Expected number of tiles for a given subdivision level.
    pub fn expected_tile_count(level: u32) -> usize {
        10 * 4_usize.pow(level) + 2
    }
}

/// Build the 12 vertices and 20 faces of a regular icosahedron on the unit sphere.
fn build_icosahedron() -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    // Golden ratio
    let phi = (1.0 + 5.0_f64.sqrt()) / 2.0;

    // 12 vertices of a regular icosahedron (will be normalized to unit sphere)
    let raw_verts = [
        [-1.0, phi, 0.0],
        [1.0, phi, 0.0],
        [-1.0, -phi, 0.0],
        [1.0, -phi, 0.0],
        [0.0, -1.0, phi],
        [0.0, 1.0, phi],
        [0.0, -1.0, -phi],
        [0.0, 1.0, -phi],
        [phi, 0.0, -1.0],
        [phi, 0.0, 1.0],
        [-phi, 0.0, -1.0],
        [-phi, 0.0, 1.0],
    ];

    let vertices: Vec<[f64; 3]> = raw_verts.iter().map(|v| normalize(*v)).collect();

    // 20 triangular faces (vertex indices, wound consistently)
    let faces: Vec<[usize; 3]> = vec![
        // 5 faces around vertex 0
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        // 5 adjacent faces
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        // 5 faces around vertex 3
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        // 5 adjacent faces
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];

    (vertices, faces)
}

/// Recursively subdivide each triangle into 4 sub-triangles, projecting new vertices
/// onto the unit sphere.
fn subdivide(
    mut vertices: Vec<[f64; 3]>,
    mut faces: Vec<[usize; 3]>,
    levels: u32,
) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    for _ in 0..levels {
        let mut edge_midpoints: HashMap<(usize, usize), usize> = HashMap::new();
        let mut new_faces = Vec::with_capacity(faces.len() * 4);

        for &[a, b, c] in &faces {
            let ab = get_or_create_midpoint(&mut vertices, &mut edge_midpoints, a, b);
            let bc = get_or_create_midpoint(&mut vertices, &mut edge_midpoints, b, c);
            let ca = get_or_create_midpoint(&mut vertices, &mut edge_midpoints, c, a);

            new_faces.push([a, ab, ca]);
            new_faces.push([b, bc, ab]);
            new_faces.push([c, ca, bc]);
            new_faces.push([ab, bc, ca]);
        }

        faces = new_faces;
    }

    (vertices, faces)
}

/// Look up or create the midpoint vertex for an edge, returning its index.
fn get_or_create_midpoint(
    vertices: &mut Vec<[f64; 3]>,
    cache: &mut HashMap<(usize, usize), usize>,
    a: usize,
    b: usize,
) -> usize {
    let key = if a < b { (a, b) } else { (b, a) };
    if let Some(&idx) = cache.get(&key) {
        return idx;
    }
    let mid = midpoint_on_sphere(vertices[a], vertices[b]);
    let idx = vertices.len();
    vertices.push(mid);
    cache.insert(key, idx);
    idx
}

/// Build the dual mesh: one tile per vertex, with neighbors derived from shared edges.
///
/// In the triangulation, two vertices are neighbors if they share an edge (appear
/// together in at least one face). We also compute approximate tile areas using the
/// solid angle subtended by each dual polygon.
fn build_dual_tiles(vertices: &[[f64; 3]], faces: &[[usize; 3]]) -> Vec<GridTile> {
    let n = vertices.len();

    // Build adjacency from faces. Use BTreeSet for deterministic neighbor ordering.
    let mut adjacency: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    // Also collect faces per vertex (needed for area computation and neighbor ordering).
    let mut vertex_faces: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (fi, &[a, b, c]) in faces.iter().enumerate() {
        adjacency[a].insert(b);
        adjacency[a].insert(c);
        adjacency[b].insert(a);
        adjacency[b].insert(c);
        adjacency[c].insert(a);
        adjacency[c].insert(b);

        vertex_faces[a].push(fi);
        vertex_faces[b].push(fi);
        vertex_faces[c].push(fi);
    }

    // Compute face centroids (on the sphere) for area calculation.
    let face_centroids: Vec<[f64; 3]> = faces
        .iter()
        .map(|&[a, b, c]| {
            normalize([
                (vertices[a][0] + vertices[b][0] + vertices[c][0]) / 3.0,
                (vertices[a][1] + vertices[b][1] + vertices[c][1]) / 3.0,
                (vertices[a][2] + vertices[b][2] + vertices[c][2]) / 3.0,
            ])
        })
        .collect();

    // Mean area for normalization (total sphere solid angle = 4*pi).
    let mean_area = 4.0 * std::f64::consts::PI / n as f64;

    let tiles: Vec<GridTile> = (0..n)
        .map(|i| {
            let center = vertices[i];
            let (lat, lon) = xyz_to_latlon(center);

            // Order neighbors by angle around the vertex normal for consistent winding.
            let neighbors = order_neighbors_around(center, &adjacency[i], vertices);

            // Approximate tile area: sum solid angles of triangles formed by center
            // and consecutive pairs of surrounding face centroids.
            let area = compute_tile_area(center, &vertex_faces[i], &face_centroids);

            // Normalize area relative to mean.
            let relative_area = area / mean_area;

            GridTile {
                index: i,
                center,
                lat,
                lon,
                neighbors,
                elevation: 0.5,
                area: relative_area,
            }
        })
        .collect();

    tiles
}

/// Order neighbor indices by angle around the vertex normal, producing a consistent
/// winding order for the dual polygon.
fn order_neighbors_around(
    center: [f64; 3],
    neighbor_set: &BTreeSet<usize>,
    vertices: &[[f64; 3]],
) -> Vec<usize> {
    if neighbor_set.is_empty() {
        return Vec::new();
    }

    let neighbors: Vec<usize> = neighbor_set.iter().copied().collect();

    // Build a local tangent frame at `center`.
    // Pick an arbitrary tangent vector by crossing center with a non-parallel axis.
    let n = center;
    let ref_axis = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalize(cross(n, ref_axis));
    let v = cross(n, u);

    // Compute angle for each neighbor projected onto the tangent plane.
    let mut angles: Vec<(usize, f64)> = neighbors
        .iter()
        .map(|&ni| {
            let d = [
                vertices[ni][0] - center[0],
                vertices[ni][1] - center[1],
                vertices[ni][2] - center[2],
            ];
            let x = dot(d, u);
            let y = dot(d, v);
            (ni, y.atan2(x))
        })
        .collect();

    angles.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    angles.iter().map(|&(idx, _)| idx).collect()
}

/// Approximate the solid angle subtended by the dual polygon around a vertex.
/// Uses the face centroids of the surrounding triangles.
fn compute_tile_area(center: [f64; 3], face_indices: &[usize], face_centroids: &[[f64; 3]]) -> f64 {
    if face_indices.is_empty() {
        return 0.0;
    }

    // Order face centroids by angle around the vertex.
    let n = center;
    let ref_axis = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalize(cross(n, ref_axis));
    let v = cross(n, u);

    let mut centroids_with_angle: Vec<([f64; 3], f64)> = face_indices
        .iter()
        .map(|&fi| {
            let c = face_centroids[fi];
            let d = [c[0] - center[0], c[1] - center[1], c[2] - center[2]];
            let x = dot(d, u);
            let y = dot(d, v);
            (c, y.atan2(x))
        })
        .collect();

    centroids_with_angle.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    // Sum spherical triangle areas: (center, centroid_i, centroid_{i+1}).
    let ordered: Vec<[f64; 3]> = centroids_with_angle.iter().map(|&(c, _)| c).collect();
    let k = ordered.len();
    let mut total = 0.0;
    for i in 0..k {
        let j = (i + 1) % k;
        total += spherical_triangle_area(center, ordered[i], ordered[j]);
    }
    total
}

/// Area of a spherical triangle on the unit sphere using the spherical excess formula.
fn spherical_triangle_area(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    // Compute the three dihedral angles via the scalar triple product approach.
    let ab = cross(a, b);
    let bc = cross(b, c);
    let ca = cross(c, a);

    let nab = normalize(ab);
    let nbc = normalize(bc);
    let nca = normalize(ca);

    // Interior angles
    let angle_a = (-dot(nca, nab)).clamp(-1.0, 1.0).acos();
    let angle_b = (-dot(nab, nbc)).clamp(-1.0, 1.0).acos();
    let angle_c = (-dot(nbc, nca)).clamp(-1.0, 1.0).acos();

    // Spherical excess
    let excess = angle_a + angle_b + angle_c - std::f64::consts::PI;
    excess.max(0.0)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_count_level_0() {
        let grid = GeodesicGrid::new(0);
        assert_eq!(grid.tiles.len(), 12);
        assert_eq!(grid.tiles.len(), GeodesicGrid::expected_tile_count(0));
    }

    #[test]
    fn tile_count_level_1() {
        let grid = GeodesicGrid::new(1);
        assert_eq!(grid.tiles.len(), 42);
        assert_eq!(grid.tiles.len(), GeodesicGrid::expected_tile_count(1));
    }

    #[test]
    fn tile_count_level_2() {
        let grid = GeodesicGrid::new(2);
        assert_eq!(grid.tiles.len(), 162);
        assert_eq!(grid.tiles.len(), GeodesicGrid::expected_tile_count(2));
    }

    #[test]
    fn tile_count_level_3() {
        let grid = GeodesicGrid::new(3);
        assert_eq!(grid.tiles.len(), 642);
        assert_eq!(grid.tiles.len(), GeodesicGrid::expected_tile_count(3));
    }

    #[test]
    fn all_tiles_have_5_or_6_neighbors() {
        for level in 0..=3 {
            let grid = GeodesicGrid::new(level);
            for tile in &grid.tiles {
                assert!(
                    tile.neighbors.len() == 5 || tile.neighbors.len() == 6,
                    "Level {}: tile {} has {} neighbors",
                    level,
                    tile.index,
                    tile.neighbors.len()
                );
            }
        }
    }

    #[test]
    fn exactly_12_pentagonal_tiles() {
        for level in 0..=3 {
            let grid = GeodesicGrid::new(level);
            let pent_count = grid.tiles.iter().filter(|t| t.neighbors.len() == 5).count();
            assert_eq!(
                pent_count, 12,
                "Level {level}: expected 12 pentagonal tiles, got {pent_count}"
            );
        }
    }

    #[test]
    fn neighbor_symmetry() {
        for level in 0..=3 {
            let grid = GeodesicGrid::new(level);
            for tile in &grid.tiles {
                for &nb in &tile.neighbors {
                    assert!(
                        grid.tiles[nb].neighbors.contains(&tile.index),
                        "Level {}: tile {} lists {} as neighbor, but not vice versa",
                        level,
                        tile.index,
                        nb
                    );
                }
            }
        }
    }

    #[test]
    fn all_centers_on_unit_sphere() {
        let grid = GeodesicGrid::new(2);
        for tile in &grid.tiles {
            let r =
                (tile.center[0].powi(2) + tile.center[1].powi(2) + tile.center[2].powi(2)).sqrt();
            assert!(
                (r - 1.0).abs() < 1e-10,
                "Tile {} center distance from origin: {}",
                tile.index,
                r
            );
        }
    }

    #[test]
    fn xyz_latlon_roundtrip() {
        let test_cases = [
            (0.0, 0.0),
            (45.0, 90.0),
            (-45.0, -90.0),
            (90.0, 0.0),
            (-90.0, 0.0),
            (23.5, -170.0),
            (-60.0, 135.5),
        ];
        for (lat, lon) in test_cases {
            let xyz = latlon_to_xyz(lat, lon);
            let (lat2, lon2) = xyz_to_latlon(xyz);
            assert!(
                (lat - lat2).abs() < 1e-10,
                "Latitude roundtrip failed: {lat} -> {lat2}"
            );
            // Handle wraparound at poles where longitude is degenerate
            if lat.abs() < 89.99 {
                assert!(
                    (lon - lon2).abs() < 1e-10,
                    "Longitude roundtrip failed: {lon} -> {lon2}"
                );
            }
        }
    }

    #[test]
    fn lat_lon_ranges() {
        let grid = GeodesicGrid::new(2);
        for tile in &grid.tiles {
            assert!(
                (-90.0..=90.0).contains(&tile.lat),
                "Tile {} lat {} out of range",
                tile.index,
                tile.lat
            );
            assert!(
                (-180.0..=180.0).contains(&tile.lon),
                "Tile {} lon {} out of range",
                tile.index,
                tile.lon
            );
        }
    }

    #[test]
    fn geo_tile_trait_impl() {
        let grid = GeodesicGrid::new(0);
        let tile = &grid.tiles[0];
        // GeoTile methods should return the same values as direct field access
        assert_eq!(GeoTile::lat(tile), tile.lat);
        assert_eq!(GeoTile::lon(tile), tile.lon);
        assert_eq!(GeoTile::elevation(tile), tile.elevation);
        assert_eq!(GeoTile::area(tile), tile.area);
    }

    #[test]
    fn spherical_point_trait_impl() {
        let grid = GeodesicGrid::new(0);
        let tile = &grid.tiles[0];
        let eps = 1e-10;
        assert!((tile.lat_deg() - tile.lat).abs() < eps);
        assert!((tile.lon_deg() - tile.lon).abs() < eps);
        assert!((tile.lat_rad() - tile.lat.to_radians()).abs() < eps);
        assert!((tile.lon_rad() - tile.lon.to_radians()).abs() < eps);
    }

    #[test]
    fn level_5_performance() {
        use std::time::Instant;
        let start = Instant::now();
        let grid = GeodesicGrid::new(5);
        let elapsed = start.elapsed();
        assert_eq!(grid.tiles.len(), 10242);
        assert!(
            elapsed.as_secs_f64() < 1.0,
            "Level 5 took {:.2}s, should be under 1s",
            elapsed.as_secs_f64()
        );
    }

    #[test]
    fn default_elevation_is_half() {
        let grid = GeodesicGrid::new(1);
        for tile in &grid.tiles {
            assert!(
                (tile.elevation - 0.5).abs() < 1e-10,
                "Tile {} elevation should default to 0.5",
                tile.index
            );
        }
    }

    #[test]
    fn areas_are_positive() {
        let grid = GeodesicGrid::new(2);
        for tile in &grid.tiles {
            assert!(
                tile.area > 0.0,
                "Tile {} has non-positive area {}",
                tile.index,
                tile.area
            );
        }
    }

    #[test]
    fn areas_sum_approximately_to_n() {
        // Since areas are relative to mean, they should sum to approximately N (tile count).
        let grid = GeodesicGrid::new(2);
        let sum: f64 = grid.tiles.iter().map(|t| t.area).sum();
        let n = grid.tiles.len() as f64;
        assert!(
            (sum - n).abs() / n < 0.01,
            "Area sum {sum} should be close to tile count {n}"
        );
    }
}
