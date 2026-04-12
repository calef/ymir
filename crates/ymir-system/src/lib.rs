//! Planetary system generation: orbital placement, bulk properties, and tidal modeling.
//!
//! Given a `StarContext`, this crate derives plausible planetary orbits, masses,
//! radii, and tidal states, producing `OrbitalBody` records for each planet.

pub mod bulk_properties;
pub mod orbital_body;
pub mod placement;
pub mod tidal;
