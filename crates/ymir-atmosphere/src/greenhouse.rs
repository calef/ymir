//! Greenhouse effect calculation from atmospheric composition and pressure,
//! producing effective surface temperature adjustments.
//!
//! Implements the parameterized model referenced in design section 5.3:
//! `T_surface = T_eq * (1 + tau)^0.25`. Optical depth `tau` is derived from
//! CO2 and H2O mole fractions plus a base scattering contribution, each
//! scaled by `sqrt(pressure)`.
//!
//! The coefficients are tuned so that Earth-like inputs (254 K equilibrium
//! temperature, 1 bar, Earth composition) recover roughly 288 K at the
//! surface, and Venus-like inputs (90 bar, CO2-dominated) blow up into the
//! 600 K+ range.

use std::collections::BTreeMap;

use crate::retention::Gas;

/// Coefficient for CO2 + H2O absorption in the optical depth model.
const TAU_ABSORPTION: f64 = 10.0;
/// Coefficient for base Rayleigh/scattering contribution.
const TAU_SCATTERING: f64 = 0.5;

/// Multiplicative greenhouse temperature factor for a given atmospheric
/// composition and surface pressure.
///
/// Returns a factor `f` such that `T_surface = T_eq * f`. For a zero-pressure
/// atmosphere this is exactly 1.0 (no greenhouse). For Earth-calibrated
/// inputs it lands near 1.134.
pub fn greenhouse_factor(composition: &BTreeMap<Gas, f64>, pressure_bar: f64) -> f64 {
    if pressure_bar <= 0.0 {
        return 1.0;
    }
    let co2 = composition.get(&Gas::CO2).copied().unwrap_or(0.0);
    let h2o = composition.get(&Gas::H2O).copied().unwrap_or(0.0);
    let sqrt_p = pressure_bar.sqrt();
    let tau = TAU_ABSORPTION * (co2 + 2.0 * h2o) * sqrt_p + TAU_SCATTERING * sqrt_p;
    (1.0 + tau).powf(0.25)
}

/// Effective surface temperature from equilibrium temperature and greenhouse
/// factor.
pub fn surface_temperature(equilibrium_temp_k: f64, greenhouse: f64) -> f64 {
    equilibrium_temp_k * greenhouse
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn earth_composition() -> BTreeMap<Gas, f64> {
        let mut c = BTreeMap::new();
        c.insert(Gas::N2, 0.78);
        c.insert(Gas::O2, 0.21);
        c.insert(Gas::Ar, 0.0093);
        c.insert(Gas::CO2, 0.0004);
        c.insert(Gas::H2O, 0.01);
        c
    }

    fn venus_composition() -> BTreeMap<Gas, f64> {
        let mut c = BTreeMap::new();
        c.insert(Gas::CO2, 0.95);
        c.insert(Gas::N2, 0.04);
        c.insert(Gas::Ar, 0.007);
        c.insert(Gas::H2O, 0.003);
        c
    }

    #[test]
    fn earth_calibration_produces_288k_within_5k() {
        let comp = earth_composition();
        let factor = greenhouse_factor(&comp, 1.0);
        let surface = surface_temperature(254.0, factor);
        assert!(
            (surface - 288.0).abs() < 5.0,
            "Earth calibration should land within 5K of 288K, got {surface} (factor {factor})"
        );
    }

    #[test]
    fn venus_90_bar_produces_above_600k() {
        let comp = venus_composition();
        let factor = greenhouse_factor(&comp, 90.0);
        // Equilibrium temp for Venus is ~230 K (high albedo); use 230 here.
        let surface = surface_temperature(230.0, factor);
        assert!(
            surface > 600.0,
            "Venus-like greenhouse should exceed 600 K, got {surface} (factor {factor})"
        );
    }

    #[test]
    fn zero_pressure_gives_unit_factor() {
        let comp = earth_composition();
        let factor = greenhouse_factor(&comp, 0.0);
        assert_eq!(factor, 1.0);
    }

    #[test]
    fn negative_pressure_treated_as_zero() {
        let comp = earth_composition();
        let factor = greenhouse_factor(&comp, -1.0);
        assert_eq!(factor, 1.0);
    }

    #[test]
    fn greenhouse_factor_is_monotonic_in_pressure_for_co2() {
        let mut c = BTreeMap::new();
        c.insert(Gas::CO2, 0.95);
        let f_low = greenhouse_factor(&c, 0.1);
        let f_mid = greenhouse_factor(&c, 1.0);
        let f_high = greenhouse_factor(&c, 10.0);
        assert!(f_low < f_mid);
        assert!(f_mid < f_high);
    }

    #[test]
    fn surface_temperature_simple_product() {
        assert!((surface_temperature(100.0, 1.5) - 150.0).abs() < 1e-9);
    }

    #[test]
    fn empty_composition_still_has_base_scattering_greenhouse() {
        let empty = BTreeMap::new();
        let f = greenhouse_factor(&empty, 1.0);
        // Only the base scattering term contributes; factor should be > 1.
        assert!(f > 1.0);
    }
}
