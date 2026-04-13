//! The `SkeletonWorld` type representing the complete global surface structure
//! before climate and biome assignment.
//!
//! `SkeletonWorld` bundles the geodesic grid, tectonic simulation result,
//! per-tile elevation map, the underlying orbital body, and its derived
//! atmosphere. It's the Phase 1 payload that downstream climate/biome stages
//! consume.

use crate::geodesic::GeodesicGrid;
use crate::heightmap::{ElevationMap, HeightmapConfig, generate_heightmap};
use crate::tectonics::{TectonicConfig, TectonicData, generate_tectonics};
use serde::{Deserialize, Serialize};
use ymir_atmosphere::atmosphere_model::AtmosphereModel;
use ymir_core::prng::WorldRng;
use ymir_system::orbital_body::OrbitalBody;

/// High-level plate classification exposed via the per-tile view.
///
/// Mirrors the `is_oceanic` flag on `Plate` but presents it as a typed enum
/// so downstream consumers can match exhaustively without boolean guessing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlateType {
    /// Continental crust (lower density, higher baseline elevation).
    Continental,
    /// Oceanic crust (higher density, lower baseline elevation).
    Oceanic,
}

impl PlateType {
    /// Construct a `PlateType` from the `is_oceanic` flag on a `Plate`.
    pub fn from_is_oceanic(is_oceanic: bool) -> Self {
        if is_oceanic {
            PlateType::Oceanic
        } else {
            PlateType::Continental
        }
    }
}

/// Zero-copy view of a single skeleton tile's derived state.
///
/// Combines the geodesic grid's geometry, the heightmap's elevation, and
/// the tectonics stage's plate assignment into one per-tile accessor.
/// Obtained via [`SkeletonWorld::tile`] or iterated via
/// [`SkeletonWorld::tiles`].
///
/// This is a transient borrowed projection over the owning `SkeletonWorld`
/// and intentionally does not implement `Serialize`/`Deserialize`; persist
/// the owning `SkeletonWorld` instead.
#[derive(Debug)]
pub struct SkeletonTile<'a> {
    /// Index of this tile in the grid's tile array.
    pub index: usize,
    /// Latitude of the tile center in radians, range [-π/2, π/2].
    pub lat_rad: f64,
    /// Longitude of the tile center in radians, range [-π, π].
    pub lon_rad: f64,
    /// Elevation in meters at this tile's center.
    pub elevation_m: f64,
    /// Identifier of the tectonic plate this tile belongs to.
    pub plate_id: usize,
    /// Classification of the owning plate (oceanic vs. continental).
    pub plate_type: PlateType,
    /// Neighbor tile indices. Pentagonal tiles have length 5, hexagonal
    /// tiles have length 6. All entries are valid tile indices (there
    /// are no sentinel values).
    pub neighbors: &'a [usize],
}

/// Complete Phase 1 global surface: grid + tectonics + elevation, plus the
/// body and atmosphere context that downstream stages depend on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkeletonWorld {
    /// Geodesic grid on which all per-tile data is indexed.
    pub grid: GeodesicGrid,
    /// Tectonic plates, boundaries, and elevation bias field.
    pub tectonics: TectonicData,
    /// Per-tile elevation in meters.
    pub elevation: ElevationMap,
    /// The orbital body this surface belongs to.
    pub body: OrbitalBody,
    /// Derived atmosphere for the body.
    pub atmosphere: AtmosphereModel,
    /// World seed used for deterministic regeneration.
    pub seed: u64,
}

impl SkeletonWorld {
    /// Build a `SkeletonWorld` from a body, atmosphere, grid subdivision level,
    /// and world seed.
    ///
    /// Derives independent child seeds for tectonics and heightmap from the
    /// world seed via [`WorldRng::child`] so overrides in one stage don't
    /// perturb the other.
    pub fn build(
        body: OrbitalBody,
        atmosphere: AtmosphereModel,
        subdivision_level: u32,
        seed: u64,
    ) -> Self {
        let grid = GeodesicGrid::new(subdivision_level);

        let mut root = WorldRng::new(seed);
        let mut tectonics_rng = root.child("tectonics");
        let heightmap_rng = root.child("heightmap");

        let tectonic_cfg = TectonicConfig::default();
        let tectonics = generate_tectonics(&grid, &tectonic_cfg, &mut tectonics_rng);

        // WorldRng doesn't expose its seed; derive a stable u64 from the
        // heightmap child by drawing one value. Since `heightmap_rng` is
        // freshly forked from a deterministic root, this is reproducible.
        let mut heightmap_rng = heightmap_rng;
        let heightmap_seed = heightmap_rng.next_u64();

        let mut heightmap_cfg = HeightmapConfig::default();
        // If the body carries an observational continental-fraction override,
        // hand it to the heightmap generator so it can calibrate the
        // sea-level threshold. Otherwise fall back to physics-derived
        // elevations (threshold falls out of the tectonic + noise model).
        if let Some(cf) = body.continental_fraction.as_ref() {
            heightmap_cfg.target_continental_fraction = Some(*cf.inner());
        }
        let elevation =
            generate_heightmap(&grid, &tectonics, &body, heightmap_seed, &heightmap_cfg);

        SkeletonWorld {
            grid,
            tectonics,
            elevation,
            body,
            atmosphere,
            seed,
        }
    }

    /// Total number of tiles on the underlying geodesic grid.
    pub fn tile_count(&self) -> usize {
        self.grid.tiles.len()
    }

    /// Returns a zero-copy view combining grid geometry, elevation, and
    /// tectonic plate assignment for the tile at `index`.
    ///
    /// # Panics
    /// Panics if `index >= self.tile_count()`.
    pub fn tile(&self, index: usize) -> SkeletonTile<'_> {
        let tile = &self.grid.tiles[index];
        let plate_id = self.tectonics.tile_plate_assignment[index];
        let plate_type = PlateType::from_is_oceanic(self.tectonics.plates[plate_id].is_oceanic);
        SkeletonTile {
            index,
            lat_rad: tile.lat.to_radians(),
            lon_rad: tile.lon.to_radians(),
            elevation_m: self.elevation.elevations_m[index],
            plate_id,
            plate_type,
            neighbors: &tile.neighbors,
        }
    }

    /// Iterator over [`SkeletonTile`] views for every tile on the grid,
    /// in tile-index order.
    pub fn tiles(&self) -> impl Iterator<Item = SkeletonTile<'_>> {
        (0..self.tile_count()).map(move |i| self.tile(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ymir_atmosphere::atmosphere_model::AtmosphereModel;
    use ymir_catalog::star_context::{SpectralClass, SpectralType, StarContext};
    use ymir_core::Sourced;
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

    fn d(v: f64) -> Sourced<f64> {
        Sourced::derived(v, "test")
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
            tidal_locked: Sourced::derived(false, "test"),
            rotation_period: d(24.0),
            is_in_hz: true,
            planet_type: PlanetType::Terran,
            name: Some("Earth".into()),
            is_known_exoplanet: false,
            continental_fraction: None,
        }
    }

    #[test]
    fn build_is_deterministic() {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        let a = SkeletonWorld::build(earth(), atmo.clone(), 3, 2024);
        let b = SkeletonWorld::build(earth(), atmo, 3, 2024);
        assert_eq!(a.grid.tiles.len(), b.grid.tiles.len());
        assert_eq!(
            a.tectonics.tile_plate_assignment,
            b.tectonics.tile_plate_assignment
        );
        assert_eq!(a.elevation.elevations_m, b.elevation.elevations_m);
        assert_eq!(a.seed, b.seed);
    }

    #[test]
    fn tile_iterator_count_matches_grid() {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        let world = SkeletonWorld::build(earth(), atmo, 2, 13);
        assert_eq!(world.tile_count(), world.grid.tiles.len());
        assert_eq!(world.tiles().count(), world.grid.tiles.len());
    }

    #[test]
    fn tile_accessor_returns_consistent_values() {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        let world = SkeletonWorld::build(earth(), atmo, 2, 13);

        for view in world.tiles() {
            // Elevation matches the underlying map exactly.
            assert_eq!(
                view.elevation_m, world.elevation.elevations_m[view.index],
                "elevation mismatch at tile {}",
                view.index
            );

            // Plate assignment matches the underlying tectonic data.
            let expected_plate = world.tectonics.tile_plate_assignment[view.index];
            assert_eq!(view.plate_id, expected_plate);

            // Plate type agrees with the plate's is_oceanic flag.
            let expected_type =
                PlateType::from_is_oceanic(world.tectonics.plates[expected_plate].is_oceanic);
            assert_eq!(view.plate_type, expected_type);

            // Lat/lon in radians match the grid's degrees converted.
            let grid_tile = &world.grid.tiles[view.index];
            assert!((view.lat_rad - grid_tile.lat.to_radians()).abs() < 1e-12);
            assert!((view.lon_rad - grid_tile.lon.to_radians()).abs() < 1e-12);
        }
    }

    #[test]
    fn tile_neighbors_match_grid() {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        let world = SkeletonWorld::build(earth(), atmo, 2, 13);

        for view in world.tiles() {
            let grid_tile = &world.grid.tiles[view.index];
            assert_eq!(view.neighbors, grid_tile.neighbors.as_slice());
            assert!(
                view.neighbors.len() == 5 || view.neighbors.len() == 6,
                "tile {} has {} neighbors (expected 5 or 6)",
                view.index,
                view.neighbors.len()
            );
        }
    }

    #[test]
    #[should_panic]
    fn tile_out_of_bounds_panics() {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        let world = SkeletonWorld::build(earth(), atmo, 1, 1);
        let _ = world.tile(world.tile_count());
    }

    #[test]
    fn serde_roundtrip() {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        let world = SkeletonWorld::build(earth(), atmo, 2, 7);
        // Use bincode-like binary-exact round trip is not needed; JSON
        // preserves f64 to ~17 significant digits which is lossless for
        // these values in practice, but we compare with a small tolerance
        // to avoid spurious failures from the text round trip.
        let json = serde_json::to_string(&world).expect("serialize");
        let back: SkeletonWorld = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.seed, world.seed);
        assert_eq!(back.grid.tiles.len(), world.grid.tiles.len());
        assert_eq!(
            back.tectonics.tile_plate_assignment,
            world.tectonics.tile_plate_assignment
        );
        assert_eq!(
            back.elevation.elevations_m.len(),
            world.elevation.elevations_m.len()
        );
        for (i, (&a, &b)) in back
            .elevation
            .elevations_m
            .iter()
            .zip(world.elevation.elevations_m.iter())
            .enumerate()
        {
            assert!(
                (a - b).abs() < 1e-6,
                "elevation tile {i} differs after round trip: {a} vs {b}"
            );
        }
    }
}
