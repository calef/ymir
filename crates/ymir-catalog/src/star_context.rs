//! The `StarContext` type representing a fully characterized star with its
//! spectral class, luminosity, metallicity, and known planetary companions.
//!
//! Includes habitable zone calculations using the Kopparapu et al. (2013, 2014)
//! parametric model.

use serde::{Deserialize, Serialize};
use std::fmt;

// TODO: Once CORE-01 is fully integrated, change the plain f64 fields
// (effective_temp, luminosity, metallicity, mass, radius, age, distance)
// to Sourced<f64> for provenance tracking. For now we use plain f64 to
// avoid coupling to a potentially in-progress implementation.

/// Spectral classification of a star.
///
/// Stores the Harvard spectral letter class, numeric subtype (0-9), and
/// luminosity class (e.g. "V" for main-sequence).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpectralType {
    pub class: SpectralClass,
    pub subtype: u8,
    pub luminosity_class: String,
}

impl fmt::Display for SpectralType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}{}", self.class, self.subtype, self.luminosity_class)
    }
}

/// Harvard spectral class letter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpectralClass {
    O,
    B,
    A,
    F,
    G,
    K,
    M,
}

impl fmt::Display for SpectralClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let letter = match self {
            SpectralClass::O => "O",
            SpectralClass::B => "B",
            SpectralClass::A => "A",
            SpectralClass::F => "F",
            SpectralClass::G => "G",
            SpectralClass::K => "K",
            SpectralClass::M => "M",
        };
        write!(f, "{letter}")
    }
}

/// Coefficients for one habitable zone boundary in the Kopparapu+ (2013) model.
///
/// The effective stellar flux at a given boundary is:
///   S_eff = s_eff_sun + a*T + b*T^2 + c*T^3 + d*T^4
/// where T = T_eff - 5780.
struct HzCoefficients {
    s_eff_sun: f64,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

/// Moist greenhouse (conservative inner edge).
const MOIST_GREENHOUSE: HzCoefficients = HzCoefficients {
    s_eff_sun: 1.0140,
    a: 8.1774e-5,
    b: 1.7063e-9,
    c: -4.3241e-12,
    d: -6.6462e-16,
};

/// Maximum greenhouse (conservative outer edge).
const MAX_GREENHOUSE: HzCoefficients = HzCoefficients {
    s_eff_sun: 0.3438,
    a: 5.8942e-5,
    b: 1.6558e-9,
    c: -3.0045e-12,
    d: -5.2983e-16,
};

/// Recent Venus (optimistic inner edge).
const RECENT_VENUS: HzCoefficients = HzCoefficients {
    s_eff_sun: 1.7763,
    a: 1.4335e-4,
    b: 3.3954e-9,
    c: -7.6364e-12,
    d: -1.1950e-15,
};

/// Early Mars (optimistic outer edge).
const EARLY_MARS: HzCoefficients = HzCoefficients {
    s_eff_sun: 0.3179,
    a: 5.4513e-5,
    b: 1.5313e-9,
    c: -2.7786e-12,
    d: -4.8997e-16,
};

/// Compute the effective stellar flux for a given HZ boundary.
fn s_eff(coeffs: &HzCoefficients, t_eff: f64) -> f64 {
    let t = t_eff - 5780.0;
    coeffs.s_eff_sun + coeffs.a * t + coeffs.b * t * t + coeffs.c * t.powi(3) + coeffs.d * t.powi(4)
}

/// Convert effective stellar flux to orbital distance in AU.
///
/// d = sqrt(L / S_eff) where L is in solar luminosities.
fn flux_to_au(luminosity: f64, s_eff: f64) -> f64 {
    (luminosity / s_eff).sqrt()
}

/// Fully characterized star with catalog data and derived habitable zone boundaries.
///
/// Created via [`StarContext::from_params`] which computes all derived fields
/// from the input catalog measurements.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StarContext {
    /// Catalog identifier, e.g. "Gaia DR3 4472832130942575872"
    pub catalog_id: String,
    /// Common name if known, e.g. "Tau Ceti"
    pub name: Option<String>,
    /// Spectral classification
    pub spectral_type: SpectralType,

    // Catalog measurements (plain f64 for now)
    // TODO: These should become Sourced<f64> once CORE-01 integration is complete.
    /// Effective temperature in Kelvin
    pub effective_temp: f64,
    /// Luminosity in solar luminosities
    pub luminosity: f64,
    /// Metallicity as [Fe/H]
    pub metallicity: f64,
    /// Mass in solar masses
    pub mass: f64,
    /// Radius in solar radii
    pub radius: f64,
    /// Estimated age in Gyr
    pub age: f64,
    /// Distance from Sol in parsecs
    pub distance: f64,

    // Derived habitable zone boundaries (AU)
    /// Conservative inner HZ edge (moist greenhouse), AU
    pub hz_inner: f64,
    /// Conservative outer HZ edge (maximum greenhouse), AU
    pub hz_outer: f64,
    /// Optimistic inner HZ edge (recent Venus), AU
    pub hz_inner_optimistic: f64,
    /// Optimistic outer HZ edge (early Mars), AU
    pub hz_outer_optimistic: f64,
    /// UV flux at HZ midpoint in relative solar units
    pub uv_flux_hz: f64,

    /// Known exoplanets from the NASA Exoplanet Archive (populated by CAT-02)
    pub known_exoplanets: Vec<String>,
}

impl StarContext {
    /// Construct a `StarContext` from raw catalog measurements.
    ///
    /// Computes all derived fields (habitable zone boundaries, UV flux estimate).
    #[allow(clippy::too_many_arguments)]
    pub fn from_params(
        catalog_id: impl Into<String>,
        name: Option<String>,
        spectral_type: SpectralType,
        effective_temp: f64,
        luminosity: f64,
        metallicity: f64,
        mass: f64,
        radius: f64,
        age: f64,
        distance: f64,
    ) -> Self {
        let hz_inner = flux_to_au(luminosity, s_eff(&MOIST_GREENHOUSE, effective_temp));
        let hz_outer = flux_to_au(luminosity, s_eff(&MAX_GREENHOUSE, effective_temp));
        let hz_inner_optimistic = flux_to_au(luminosity, s_eff(&RECENT_VENUS, effective_temp));
        let hz_outer_optimistic = flux_to_au(luminosity, s_eff(&EARLY_MARS, effective_temp));

        // Rough UV flux estimate at HZ midpoint, scaled relative to Sol.
        // UV output scales roughly as T_eff^4 (Stefan-Boltzmann), and flux falls
        // as 1/d^2. We evaluate at the midpoint of the conservative HZ.
        let hz_midpoint = (hz_inner + hz_outer) / 2.0;
        let uv_flux_hz = if hz_midpoint > 0.0 {
            luminosity / (hz_midpoint * hz_midpoint)
        } else {
            0.0
        };

        StarContext {
            catalog_id: catalog_id.into(),
            name,
            spectral_type,
            effective_temp,
            luminosity,
            metallicity,
            mass,
            radius,
            age,
            distance,
            hz_inner,
            hz_outer,
            hz_inner_optimistic,
            hz_outer_optimistic,
            uv_flux_hz,
            known_exoplanets: Vec::new(),
        }
    }

    /// Factory method returning a `StarContext` for Tau Ceti (HD 10700) using
    /// well-established observational values.
    pub fn tau_ceti() -> Self {
        Self::from_params(
            "HD 10700",
            Some("Tau Ceti".to_string()),
            SpectralType {
                class: SpectralClass::G,
                subtype: 8,
                luminosity_class: "V".to_string(),
            },
            5344.0, // T_eff (K)
            0.488,  // luminosity (L_sun)
            -0.55,  // metallicity [Fe/H]
            0.783,  // mass (M_sun)
            0.793,  // radius (R_sun)
            5.8,    // age (Gyr)
            3.65,   // distance (pc)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a StarContext for the Sun.
    fn sun() -> StarContext {
        StarContext::from_params(
            "Sun",
            Some("Sol".to_string()),
            SpectralType {
                class: SpectralClass::G,
                subtype: 2,
                luminosity_class: "V".to_string(),
            },
            5780.0, // T_eff
            1.0,    // L_sun
            0.0,    // [Fe/H]
            1.0,    // M_sun
            1.0,    // R_sun
            4.6,    // age
            0.0,    // distance
        )
    }

    #[test]
    fn sun_hz_conservative_inner() {
        let sol = sun();
        // For the Sun, moist greenhouse S_eff_sun = 1.014, so d = sqrt(1/1.014) ~ 0.993 AU
        assert!(
            (sol.hz_inner - 0.99).abs() < 0.02,
            "Sun conservative inner HZ should be ~0.99 AU, got {}",
            sol.hz_inner
        );
    }

    #[test]
    fn sun_hz_conservative_outer() {
        let sol = sun();
        // For the Sun, max greenhouse S_eff_sun = 0.3438, so d = sqrt(1/0.3438) ~ 1.705 AU
        assert!(
            (sol.hz_outer - 1.70).abs() < 0.05,
            "Sun conservative outer HZ should be ~1.70 AU, got {}",
            sol.hz_outer
        );
    }

    #[test]
    fn tau_ceti_hz_sanity() {
        let tc = StarContext::tau_ceti();
        assert!(tc.hz_inner > 0.0, "HZ inner must be positive");
        assert!(tc.hz_outer > 0.0, "HZ outer must be positive");
        assert!(
            tc.hz_inner < tc.hz_outer,
            "inner ({}) must be less than outer ({})",
            tc.hz_inner,
            tc.hz_outer
        );
        // Tau Ceti is a G8V star with L=0.488; HZ should be roughly 0.5-1.2 AU range
        assert!(
            tc.hz_inner > 0.4 && tc.hz_inner < 1.0,
            "Tau Ceti inner HZ looks unreasonable: {}",
            tc.hz_inner
        );
        assert!(
            tc.hz_outer > 0.8 && tc.hz_outer < 1.5,
            "Tau Ceti outer HZ looks unreasonable: {}",
            tc.hz_outer
        );
    }

    #[test]
    fn tau_ceti_optimistic_wider_than_conservative() {
        let tc = StarContext::tau_ceti();
        assert!(
            tc.hz_inner_optimistic < tc.hz_inner,
            "optimistic inner should be closer to star than conservative inner"
        );
        assert!(
            tc.hz_outer_optimistic > tc.hz_outer,
            "optimistic outer should be farther than conservative outer"
        );
    }

    #[test]
    fn hz_inner_less_than_outer_across_range() {
        // Property-style test: for T_eff from 2600 to 7200 in steps,
        // the conservative inner HZ must always be less than the outer.
        for t in (2600..=7200).step_by(100) {
            let t_eff = t as f64;
            // Use a fixed luminosity scaled roughly by (T/5780)^4 to keep things physical
            let luminosity = (t_eff / 5780.0).powi(4) * 0.1; // arbitrary but monotonic
            let star = StarContext::from_params(
                format!("test_{t}"),
                None,
                SpectralType {
                    class: SpectralClass::M,
                    subtype: 0,
                    luminosity_class: "V".to_string(),
                },
                t_eff,
                luminosity,
                0.0,
                1.0,
                1.0,
                5.0,
                10.0,
            );
            assert!(
                star.hz_inner < star.hz_outer,
                "At T_eff={t_eff}: inner ({}) >= outer ({})",
                star.hz_inner,
                star.hz_outer
            );
            assert!(
                star.hz_inner_optimistic < star.hz_outer_optimistic,
                "At T_eff={t_eff}: optimistic inner ({}) >= optimistic outer ({})",
                star.hz_inner_optimistic,
                star.hz_outer_optimistic
            );
        }
    }

    #[test]
    fn spectral_type_display() {
        let st = SpectralType {
            class: SpectralClass::G,
            subtype: 2,
            luminosity_class: "V".to_string(),
        };
        assert_eq!(st.to_string(), "G2V");

        let st2 = SpectralType {
            class: SpectralClass::M,
            subtype: 3,
            luminosity_class: "V".to_string(),
        };
        assert_eq!(st2.to_string(), "M3V");

        let st3 = SpectralType {
            class: SpectralClass::K,
            subtype: 5,
            luminosity_class: "III".to_string(),
        };
        assert_eq!(st3.to_string(), "K5III");
    }

    #[test]
    fn sun_at_5780_coefficients_equal_s_eff_sun() {
        // When T_eff = 5780, T_star = 0, so S_eff should exactly equal S_eff_sun.
        let eps = 1e-10;
        assert!((s_eff(&MOIST_GREENHOUSE, 5780.0) - 1.0140).abs() < eps);
        assert!((s_eff(&MAX_GREENHOUSE, 5780.0) - 0.3438).abs() < eps);
        assert!((s_eff(&RECENT_VENUS, 5780.0) - 1.7763).abs() < eps);
        assert!((s_eff(&EARLY_MARS, 5780.0) - 0.3179).abs() < eps);
    }
}
