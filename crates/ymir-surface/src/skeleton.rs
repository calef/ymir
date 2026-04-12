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

        let heightmap_cfg = HeightmapConfig::default();
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

    fn earth() -> OrbitalBody {
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
