//! The `OrbitalBody` type representing a single planet with its full set of
//! derived or observed orbital and physical properties.

use serde::{Deserialize, Serialize};
use ymir_core::Sourced;

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
/// pipeline stages can consume them without recomputation. Every overridable
/// scalar is wrapped in [`Sourced<T>`] so CORE-07 provenance reports and
/// REND-04 confidence overlays can classify per-field origins.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrbitalBody {
    // Orbital parameters
    /// Semi-major axis in AU.
    pub semi_major_axis: Sourced<f64>,
    /// Orbital eccentricity (0 = circular).
    pub eccentricity: Sourced<f64>,
    /// Orbital inclination in degrees.
    pub inclination: Sourced<f64>,
    /// Axial tilt (obliquity) in degrees.
    pub axial_tilt: Sourced<f64>,

    // Bulk properties
    /// Mass in Earth masses.
    pub mass: Sourced<f64>,
    /// Radius in Earth radii.
    pub radius: Sourced<f64>,
    /// Bulk density in g/cm^3.
    pub density: Sourced<f64>,
    /// Surface gravity in m/s^2.
    pub surface_gravity: Sourced<f64>,

    // Derived from star + orbit
    /// Solar irradiance at the planet's semi-major axis in W/m^2.
    pub solar_irradiance: Sourced<f64>,
    /// Equilibrium blackbody temperature in K (before greenhouse).
    pub equilibrium_temp: Sourced<f64>,
    /// True if the planet is tidally locked.
    pub tidal_locked: Sourced<bool>,
    /// Rotation period in hours (matches orbital period if tidally locked).
    pub rotation_period: Sourced<f64>,

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

    /// Observational override for the fraction of the surface above sea level.
    ///
    /// When set, the skeleton stage calibrates its sea-level threshold so this
    /// fraction of tiles end up at or above elevation 0 m. When `None`, the
    /// raw physics-based elevation field is left alone and the sea-level
    /// threshold falls out of the tectonics + noise model. Meant for bodies
    /// with a known hypsometric curve (Earth ~0.29, Mars ~1.0).
    #[serde(default)]
    pub continental_fraction: Option<Sourced<f64>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(v: f64) -> Sourced<f64> {
        Sourced::derived(v, "test")
    }

    #[test]
    fn orbital_body_fields_populate_and_clone() {
        let body = OrbitalBody {
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
            name: Some("Earth".to_string()),
            is_known_exoplanet: false,
            continental_fraction: None,
        };
        let cloned = body.clone();
        assert_eq!(
            *cloned.semi_major_axis.inner(),
            *body.semi_major_axis.inner()
        );
        assert_eq!(cloned.planet_type, PlanetType::Terran);
        assert_eq!(cloned.name.as_deref(), Some("Earth"));
        assert!(!*cloned.tidal_locked.inner());
        assert!(cloned.is_in_hz);
    }

    #[test]
    fn planet_type_variants_are_distinct() {
        assert_ne!(PlanetType::Terran, PlanetType::SuperEarth);
        assert_ne!(PlanetType::SubNeptune, PlanetType::Neptune);
        assert_ne!(PlanetType::Neptune, PlanetType::GasGiant);
    }
}
