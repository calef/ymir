//! NASA Exoplanet Archive data ingestion and cross-matching with stellar catalogs.

use serde::{Deserialize, Serialize};

/// Method by which an exoplanet was discovered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DiscoveryMethod {
    RadialVelocity,
    Transit,
    DirectImaging,
    Microlensing,
    Timing,
    Other(String),
}

/// A single exoplanet record, representing a known or candidate planet from the
/// NASA Exoplanet Archive. Fields are `Option` where measurements may not be
/// available for every planet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExoplanetRecord {
    /// Planet designation, e.g. "Tau Ceti e"
    pub name: String,
    /// Host star name, e.g. "Tau Ceti"
    pub host_star: String,
    /// How the planet was detected
    pub discovery_method: DiscoveryMethod,
    /// Year of discovery or publication
    pub discovery_year: u16,
    /// Orbital period in days
    pub orbital_period: Option<f64>,
    /// Semi-major axis in AU
    pub semi_major_axis: Option<f64>,
    /// Orbital eccentricity (0 = circular, <1 = elliptical)
    pub eccentricity: Option<f64>,
    /// Orbital inclination in degrees
    pub inclination: Option<f64>,
    /// Planet mass in Earth masses
    pub mass: Option<f64>,
    /// Planet radius in Earth radii
    pub radius: Option<f64>,
    /// Equilibrium temperature in Kelvin
    pub equilibrium_temp: Option<f64>,
}

impl ExoplanetRecord {
    /// Returns known/candidate planets of Tau Ceti.
    /// Based on Feng+ 2017 radial velocity detections.
    pub fn tau_ceti_system() -> Vec<ExoplanetRecord> {
        vec![
            ExoplanetRecord {
                name: "Tau Ceti g".into(),
                host_star: "Tau Ceti".into(),
                discovery_method: DiscoveryMethod::RadialVelocity,
                discovery_year: 2017,
                orbital_period: Some(20.0),
                semi_major_axis: Some(0.133),
                eccentricity: None,
                inclination: None,
                mass: Some(1.75),
                radius: None,
                equilibrium_temp: None,
            },
            ExoplanetRecord {
                name: "Tau Ceti h".into(),
                host_star: "Tau Ceti".into(),
                discovery_method: DiscoveryMethod::RadialVelocity,
                discovery_year: 2017,
                orbital_period: Some(49.0),
                semi_major_axis: Some(0.243),
                eccentricity: None,
                inclination: None,
                mass: Some(1.83),
                radius: None,
                equilibrium_temp: None,
            },
            ExoplanetRecord {
                name: "Tau Ceti e".into(),
                host_star: "Tau Ceti".into(),
                discovery_method: DiscoveryMethod::RadialVelocity,
                discovery_year: 2017,
                orbital_period: Some(163.0),
                semi_major_axis: Some(0.538),
                eccentricity: None,
                inclination: None,
                mass: Some(3.93),
                radius: None,
                equilibrium_temp: None,
            },
            ExoplanetRecord {
                name: "Tau Ceti f".into(),
                host_star: "Tau Ceti".into(),
                discovery_method: DiscoveryMethod::RadialVelocity,
                discovery_year: 2017,
                orbital_period: Some(636.0),
                semi_major_axis: Some(1.334),
                eccentricity: None,
                inclination: None,
                mass: Some(3.93),
                radius: None,
                equilibrium_temp: None,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tau_ceti_system_has_four_planets() {
        let planets = ExoplanetRecord::tau_ceti_system();
        assert_eq!(planets.len(), 4);
    }

    #[test]
    fn tau_ceti_all_radial_velocity() {
        let planets = ExoplanetRecord::tau_ceti_system();
        for planet in &planets {
            assert_eq!(planet.discovery_method, DiscoveryMethod::RadialVelocity);
        }
    }

    #[test]
    fn tau_ceti_semi_major_axes_increasing() {
        let planets = ExoplanetRecord::tau_ceti_system();
        let axes: Vec<f64> = planets
            .iter()
            .map(|p| {
                p.semi_major_axis
                    .expect("all Tau Ceti planets have semi_major_axis")
            })
            .collect();
        for window in axes.windows(2) {
            assert!(
                window[0] < window[1],
                "semi-major axes should be in increasing order: {} >= {}",
                window[0],
                window[1]
            );
        }
    }

    #[test]
    fn serde_round_trip() {
        let planets = ExoplanetRecord::tau_ceti_system();
        let json = serde_json::to_string(&planets).expect("serialize");
        let deserialized: Vec<ExoplanetRecord> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.len(), planets.len());
        for (orig, deser) in planets.iter().zip(deserialized.iter()) {
            assert_eq!(orig.name, deser.name);
            assert_eq!(orig.discovery_method, deser.discovery_method);
            assert_eq!(orig.semi_major_axis, deser.semi_major_axis);
            assert_eq!(orig.mass, deser.mass);
        }
    }
}
