//! The `AtmosphereModel` type representing a complete atmospheric
//! characterization including composition, pressure profile, and greenhouse
//! parameters.
//!
//! `AtmosphereModel::derive` is the stage-3 entry point: given an
//! [`OrbitalBody`] and its [`StarContext`], it runs the retention model,
//! picks a composition archetype, and computes greenhouse warming, scale
//! height, moisture capacity, and UV transmission.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ymir_catalog::star_context::StarContext;
use ymir_system::orbital_body::OrbitalBody;

use crate::composition::{AtmosphereClass, derive_composition};
use crate::greenhouse::{greenhouse_factor, surface_temperature};
use crate::retention::{Gas, compute_retention_for_body};

/// Earth's surface gravity in m/s^2, used to normalize other bodies.
const EARTH_SURFACE_GRAVITY: f64 = 9.81;
/// Earth-calibrated reference equilibrium temperature for the scale-height
/// expression.
const SCALE_HEIGHT_REF_TEMP: f64 = 254.0;
/// Reference mean molar mass (N2) for the scale-height expression, g/mol.
const SCALE_HEIGHT_REF_MOLAR_MASS: f64 = 28.0;
/// Earth scale-height reference value in km.
const SCALE_HEIGHT_REF_KM: f64 = 8.0;
/// Reference surface temperature for moisture-capacity scaling, K.
const MOISTURE_REF_TEMP: f64 = 288.0;
/// Moisture capacity ceiling.
const MOISTURE_CAP: f64 = 100.0;

/// A complete atmospheric characterization, produced by
/// [`AtmosphereModel::derive`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AtmosphereModel {
    /// Surface pressure, bar.
    pub surface_pressure: f64,
    /// Composition as mole fractions, summing to ~1.0.
    pub composition: HashMap<Gas, f64>,
    /// Multiplicative greenhouse temperature factor.
    pub greenhouse_factor: f64,
    /// Effective surface temperature after greenhouse, K.
    pub effective_surface_temp: f64,
    /// Pressure scale height, km.
    pub scale_height: f64,
    /// Relative moisture capacity (Earth = 1.0).
    pub moisture_capacity: f64,
    /// Fraction of stellar UV reaching the surface, 0-1.
    pub uv_surface_flux: f64,
    /// Coarse classification used by downstream stages.
    pub class: AtmosphereClass,
    /// Gas species retained over geological time.
    pub retained: Vec<Gas>,
}

impl AtmosphereModel {
    /// Build an [`AtmosphereModel`] for a body around a given star.
    ///
    /// `enable_biology` controls whether the composition model is allowed to
    /// produce a free-oxygen (Earth-like) atmosphere. Without biology the
    /// nitrogen/oxygen archetype is disabled and bodies fall through to
    /// abiotic regimes.
    pub fn derive(body: &OrbitalBody, star: &StarContext, enable_biology: bool) -> AtmosphereModel {
        let retention = compute_retention_for_body(body);
        let retained = retention.retained.clone();

        let (composition, surface_pressure, class) =
            derive_composition(body, &retained, star.metallicity, enable_biology);

        let gh = greenhouse_factor(&composition, surface_pressure);
        let effective_surface_temp = surface_temperature(body.equilibrium_temp, gh);

        let scale_height =
            compute_scale_height(&composition, effective_surface_temp, body.surface_gravity);

        let moisture_capacity = compute_moisture_capacity(surface_pressure, effective_surface_temp);

        let uv_surface_flux = compute_uv_surface_flux(class, surface_pressure);

        AtmosphereModel {
            surface_pressure,
            composition,
            greenhouse_factor: gh,
            effective_surface_temp,
            scale_height,
            moisture_capacity,
            uv_surface_flux,
            class,
            retained,
        }
    }
}

/// Mean molar mass of a composition, in g/mol. Returns zero for an empty
/// composition.
fn mean_molar_mass(composition: &HashMap<Gas, f64>) -> f64 {
    composition
        .iter()
        .map(|(g, f)| g.molar_mass() * f)
        .sum::<f64>()
}

/// Scale height in km, approximated as
/// `H = 8 km * (T / 254) * (28 / M_avg) * (9.81 / g)`.
///
/// Returns zero for an empty composition or zero gravity.
fn compute_scale_height(
    composition: &HashMap<Gas, f64>,
    effective_temp: f64,
    surface_gravity: f64,
) -> f64 {
    if composition.is_empty() || surface_gravity <= 0.0 {
        return 0.0;
    }
    let m_avg = mean_molar_mass(composition);
    if m_avg <= 0.0 {
        return 0.0;
    }
    let g_rel = surface_gravity / EARTH_SURFACE_GRAVITY;
    SCALE_HEIGHT_REF_KM * (effective_temp / SCALE_HEIGHT_REF_TEMP)
        / (m_avg / SCALE_HEIGHT_REF_MOLAR_MASS)
        / g_rel
}

/// Relative moisture capacity, Earth = 1.0. Grows roughly linearly with
/// pressure and exponentially with surface temperature above Earth's mean.
fn compute_moisture_capacity(pressure_bar: f64, effective_temp: f64) -> f64 {
    if pressure_bar <= 0.0 {
        return 0.0;
    }
    let raw = pressure_bar * ((effective_temp - MOISTURE_REF_TEMP) / 30.0).exp();
    raw.min(MOISTURE_CAP)
}

/// Fraction of incident stellar UV reaching the surface.
fn compute_uv_surface_flux(class: AtmosphereClass, pressure_bar: f64) -> f64 {
    match class {
        AtmosphereClass::None => 1.0,
        AtmosphereClass::ThickCO2 | AtmosphereClass::NitrogenOxygen => 0.05,
        AtmosphereClass::HydrogenHelium => 0.01,
        AtmosphereClass::ThickN2H2O => {
            if pressure_bar >= 0.5 {
                0.2
            } else {
                0.6
            }
        }
        AtmosphereClass::ThinCO2 => {
            if pressure_bar >= 0.1 {
                0.4
            } else {
                0.8
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
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
            name: Some("Earth".to_string()),
            is_known_exoplanet: false,
        }
    }

    fn mars() -> OrbitalBody {
        OrbitalBody {
            semi_major_axis: 1.524,
            eccentricity: 0.0934,
            inclination: 1.85,
            axial_tilt: 25.2,
            mass: 0.107,
            radius: 0.532,
            density: 3.93,
            surface_gravity: 3.71,
            solar_irradiance: 586.0,
            equilibrium_temp: 210.0,
            tidal_locked: false,
            rotation_period: 24.6,
            is_in_hz: false,
            planet_type: PlanetType::Terran,
            name: Some("Mars".to_string()),
            is_known_exoplanet: false,
        }
    }

    #[test]
    fn earth_derive_within_10k_of_288() {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        assert_eq!(atmo.class, AtmosphereClass::NitrogenOxygen);
        assert!(
            (atmo.effective_surface_temp - 288.0).abs() < 10.0,
            "Earth effective surface temp should be within 10K of 288K, got {}",
            atmo.effective_surface_temp
        );
        // Scale height should be plausible (~8 km).
        assert!(
            atmo.scale_height > 6.0 && atmo.scale_height < 12.0,
            "Earth scale height should be near 8 km, got {}",
            atmo.scale_height
        );
        // Moisture capacity should be near 1 (Earth reference).
        assert!(
            (atmo.moisture_capacity - 1.0).abs() < 0.3,
            "Earth moisture capacity should be near 1.0, got {}",
            atmo.moisture_capacity
        );
    }

    #[test]
    fn mars_derive_below_270k() {
        let atmo = AtmosphereModel::derive(&mars(), &sun(), false);
        assert!(
            atmo.effective_surface_temp < 270.0,
            "Mars effective surface temp should be below 270 K, got {}",
            atmo.effective_surface_temp
        );
        assert_eq!(atmo.class, AtmosphereClass::ThinCO2);
        // Mars surface pressure ~0.006 bar, extremely thin.
        assert!(atmo.surface_pressure < 0.1);
    }

    #[test]
    fn serde_roundtrip_preserves_all_fields() {
        let atmo = AtmosphereModel::derive(&earth(), &sun(), true);
        let json = serde_json::to_string(&atmo).expect("serialize");
        let back: AtmosphereModel = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(atmo.surface_pressure, back.surface_pressure);
        assert_eq!(atmo.greenhouse_factor, back.greenhouse_factor);
        assert_eq!(atmo.effective_surface_temp, back.effective_surface_temp);
        assert_eq!(atmo.scale_height, back.scale_height);
        assert_eq!(atmo.moisture_capacity, back.moisture_capacity);
        assert_eq!(atmo.uv_surface_flux, back.uv_surface_flux);
        assert_eq!(atmo.class, back.class);
        assert_eq!(atmo.retained, back.retained);
        assert_eq!(atmo.composition.len(), back.composition.len());
        for (gas, frac) in &atmo.composition {
            let round = back.composition.get(gas).copied().unwrap_or(f64::NAN);
            assert!((round - frac).abs() < 1e-12);
        }
    }

    #[test]
    fn no_atmosphere_body_has_zero_scale_height() {
        // Tiny moon-like body that retains nothing.
        let body = OrbitalBody {
            semi_major_axis: 1.0,
            eccentricity: 0.0,
            inclination: 0.0,
            axial_tilt: 0.0,
            mass: 0.01,
            radius: 0.2,
            density: 3.0,
            surface_gravity: 1.6,
            solar_irradiance: 1361.0,
            equilibrium_temp: 254.0,
            tidal_locked: false,
            rotation_period: 24.0,
            is_in_hz: true,
            planet_type: PlanetType::Terran,
            name: None,
            is_known_exoplanet: false,
        };
        let atmo = AtmosphereModel::derive(&body, &sun(), true);
        assert_eq!(atmo.class, AtmosphereClass::None);
        assert_eq!(atmo.surface_pressure, 0.0);
        assert_eq!(atmo.scale_height, 0.0);
        assert_eq!(atmo.moisture_capacity, 0.0);
        assert_eq!(atmo.uv_surface_flux, 1.0);
    }
}
