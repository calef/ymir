//! Planetary system generation: orbital placement, bulk properties, and tidal modeling.
//!
//! Given a `StarContext`, this crate derives plausible planetary orbits, masses,
//! radii, and tidal states, producing `OrbitalBody` records for each planet.

pub mod bulk_properties;
pub mod orbital_body;
pub mod placement;
pub mod tidal;

pub use bulk_properties::{
    classify_planet, density_gcc, derive_body, equilibrium_temperature, mass_from_radius,
    solar_irradiance, surface_gravity_ms2,
};
pub use orbital_body::{OrbitalBody, PlanetType};
pub use placement::{PlacedPlanet, PlacementConfig, place_planets};
pub use tidal::{is_tidally_locked, orbital_period_years};
