//! The `OrbitalBody` type representing a single planet with its full set of
//! derived or observed orbital and physical properties.

use serde::{Deserialize, Serialize};

/// Coarse classification of a planet by its bulk properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanetType {
    /// Rocky, below ~1.5 Earth radii.
    Terran,
    /// Rocky but larger, 1.5 to 2 Earth radii.
    SuperEarth,
    /// 2 to 4 Earth radii with a H/He envelope.
    SubNeptune,
    /// 4 to 10 Earth radii (Neptune-like ice giants).
    Neptune,
    /// Above 10 Earth radii (Jupiter-like gas giants).
    GasGiant,
}

/// A single planet with its full set of orbital and physical properties.
///
/// All derived values are computed once at construction (see
/// [`crate::bulk_properties::derive_body`]) and stored in-place so downstream
/// pipeline stages can consume them without recomputation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrbitalBody {
    // Orbital parameters
    /// Semi-major axis in AU.
    pub semi_major_axis: f64,
    /// Orbital eccentricity (0 = circular).
    pub eccentricity: f64,
    /// Orbital inclination in degrees.
    pub inclination: f64,
    /// Axial tilt (obliquity) in degrees.
    pub axial_tilt: f64,

    // Bulk properties
    /// Mass in Earth masses.
    pub mass: f64,
    /// Radius in Earth radii.
    pub radius: f64,
    /// Bulk density in g/cm^3.
    pub density: f64,
    /// Surface gravity in m/s^2.
    pub surface_gravity: f64,

    // Derived from star + orbit
    /// Solar irradiance at the planet's semi-major axis in W/m^2.
    pub solar_irradiance: f64,
    /// Equilibrium blackbody temperature in K (before greenhouse).
    pub equilibrium_temp: f64,
    /// True if the planet is tidally locked.
    pub tidal_locked: bool,
    /// Rotation period in hours (matches orbital period if tidally locked).
    pub rotation_period: f64,

    // Classification
    /// True if the semi-major axis is within the star's conservative HZ.
    pub is_in_hz: bool,
    /// Coarse planetary type based on radius.
    pub planet_type: PlanetType,

    // Identification
    /// Common name if known (e.g., from an exoplanet catalog).
    pub name: Option<String>,
    /// True if this body was anchored to a real exoplanet record.
    pub is_known_exoplanet: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbital_body_fields_populate_and_clone() {
        let body = OrbitalBody {
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
            name: Some("Earth".to_string()),
            is_known_exoplanet: false,
        };
        let cloned = body.clone();
        assert_eq!(cloned.semi_major_axis, body.semi_major_axis);
        assert_eq!(cloned.planet_type, PlanetType::Terran);
        assert_eq!(cloned.name.as_deref(), Some("Earth"));
        assert!(!cloned.tidal_locked);
        assert!(cloned.is_in_hz);
    }

    #[test]
    fn planet_type_variants_are_distinct() {
        assert_ne!(PlanetType::Terran, PlanetType::SuperEarth);
        assert_ne!(PlanetType::SubNeptune, PlanetType::Neptune);
        assert_ne!(PlanetType::Neptune, PlanetType::GasGiant);
    }
}
