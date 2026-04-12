//! Atmospheric composition derivation from outgassing models, stellar
//! metallicity, and volatile delivery estimates.
//!
//! This is the v1 simplified composition model described in design section
//! 5.3. It chooses one of a handful of atmospheric archetypes based on the
//! planet's bulk properties (mass, radius, insolation) and which gas species
//! the retention model says it can actually hold on to. The output is a
//! mole-fraction breakdown, a surface pressure estimate, and a coarse
//! [`AtmosphereClass`] used downstream by the biome palette.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ymir_system::orbital_body::OrbitalBody;

use crate::retention::Gas;

/// High-level classification of an atmosphere, used by downstream stages
/// (biome palette, visuals) to switch between qualitatively different
/// regimes without having to inspect every mole fraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AtmosphereClass {
    /// Body retains no appreciable atmosphere.
    None,
    /// Thin CO2-dominated atmosphere, Mars-like (< 0.1 bar).
    ThinCO2,
    /// Thick CO2-dominated atmosphere, Venus-like (> 1 bar).
    ThickCO2,
    /// Thick N2/H2O atmosphere, Titan-like.
    ThickN2H2O,
    /// Earth-like N2/O2 mix; requires molecular oxygen.
    NitrogenOxygen,
    /// H2/He-dominated gas giant or mini-Neptune envelope.
    HydrogenHelium,
}

/// Normalize a map of mole fractions so that the values sum to 1.
fn normalize(map: &mut HashMap<Gas, f64>) {
    let sum: f64 = map.values().sum();
    if sum > 0.0 {
        for v in map.values_mut() {
            *v /= sum;
        }
    }
}

/// Metallicity-driven pressure multiplier.
///
/// Higher-metallicity stars produce planets with more volatiles, which show
/// up here as thicker atmospheres. The multiplier is `(1 + 0.5 * [Fe/H])`
/// clamped to `[0.3, 3.0]` so that extreme metallicity values can't produce
/// unphysical pressures.
fn metallicity_multiplier(metallicity: f64) -> f64 {
    (1.0 + 0.5 * metallicity).clamp(0.3, 3.0)
}

/// Derive an atmospheric composition, surface pressure, and high-level
/// [`AtmosphereClass`] from a planet's bulk properties, the set of gases
/// retention says it can hold, and its host star's metallicity.
///
/// The returned composition is a `HashMap<Gas, f64>` of mole fractions that
/// sum to approximately 1.0 (within floating-point tolerance). When the body
/// has no retained atmosphere the map is empty and the pressure is zero.
pub fn derive_composition(
    body: &OrbitalBody,
    retained: &[Gas],
    metallicity: f64,
    enable_biology: bool,
) -> (HashMap<Gas, f64>, f64, AtmosphereClass) {
    let retains = |g: Gas| retained.contains(&g);
    let metal_mult = metallicity_multiplier(metallicity);

    // No atmosphere: empty retention set, or body too small to hold onto
    // anything over geological time.
    if retained.is_empty() || body.radius < 0.3 {
        return (HashMap::new(), 0.0, AtmosphereClass::None);
    }

    // Gas giant: large radius, still retains H2.
    if body.radius >= 4.0 && retains(Gas::H2) {
        let mut comp = HashMap::new();
        comp.insert(Gas::H2, 0.96);
        comp.insert(Gas::He, 0.04);
        let pressure = 1000.0 * metal_mult;
        return (comp, pressure, AtmosphereClass::HydrogenHelium);
    }

    // Super-Earth / sub-Neptune with an H2/He envelope.
    if body.radius >= 2.0 && retains(Gas::H2) {
        let mut comp = HashMap::new();
        comp.insert(Gas::H2, 0.50);
        comp.insert(Gas::He, 0.10);
        comp.insert(Gas::H2O, 0.20);
        comp.insert(Gas::CH4, 0.10);
        comp.insert(Gas::N2, 0.10);
        let pressure = 50.0 * metal_mult;
        return (comp, pressure, AtmosphereClass::HydrogenHelium);
    }

    // Terran regime (roughly 0.3 - 2.0 R_earth here, since we already
    // filtered tiny bodies above).

    // Earth-like: biology flag on, in the habitable zone, and retains both
    // nitrogen and free oxygen.
    if enable_biology && body.is_in_hz && retains(Gas::O2) && retains(Gas::N2) {
        let mut comp = HashMap::new();
        comp.insert(Gas::N2, 0.78);
        comp.insert(Gas::O2, 0.21);
        comp.insert(Gas::Ar, 0.0093);
        comp.insert(Gas::CO2, 0.0004);
        comp.insert(Gas::H2O, 0.003);
        normalize(&mut comp);
        let pressure = 1.0 * metal_mult;
        return (comp, pressure, AtmosphereClass::NitrogenOxygen);
    }

    // CO2-dominated atmospheres (Mars-like or Venus-like).
    if retains(Gas::CO2) {
        if body.mass > 0.5 {
            // Venus-like thick CO2.
            let mut comp = HashMap::new();
            comp.insert(Gas::CO2, 0.95);
            comp.insert(Gas::N2, 0.04);
            comp.insert(Gas::Ar, 0.007);
            comp.insert(Gas::H2O, 0.003);
            normalize(&mut comp);
            // Pressure scales with mass and metallicity; cap at 100 bar.
            let raw = 10.0 * body.mass * metal_mult;
            let pressure = raw.min(100.0);
            return (comp, pressure, AtmosphereClass::ThickCO2);
        } else {
            // Mars-like thin CO2.
            let mut comp = HashMap::new();
            comp.insert(Gas::CO2, 0.95);
            comp.insert(Gas::N2, 0.03);
            comp.insert(Gas::Ar, 0.02);
            normalize(&mut comp);
            let pressure = 0.006 * body.mass * metal_mult;
            return (comp, pressure, AtmosphereClass::ThinCO2);
        }
    }

    // N2-dominated (Titan-like), if CO2 is gone but N2 survived.
    if retains(Gas::N2) {
        let mut comp = HashMap::new();
        comp.insert(Gas::N2, 0.90);
        comp.insert(Gas::Ar, 0.05);
        comp.insert(Gas::H2O, 0.05);
        normalize(&mut comp);
        let pressure = 0.5 * metal_mult;
        return (comp, pressure, AtmosphereClass::ThickN2H2O);
    }

    // Fallback: whatever is retained, equally weighted and normalized.
    let mut comp = HashMap::new();
    for &g in retained {
        comp.insert(g, 1.0);
    }
    normalize(&mut comp);
    let pressure = 0.1 * metal_mult;
    let class = best_class_for(&comp);
    (comp, pressure, class)
}

/// Pick a best-effort [`AtmosphereClass`] for an arbitrary composition when
/// none of the named archetypes applied.
fn best_class_for(comp: &HashMap<Gas, f64>) -> AtmosphereClass {
    let frac = |g: Gas| comp.get(&g).copied().unwrap_or(0.0);
    if frac(Gas::H2) + frac(Gas::He) > 0.5 {
        AtmosphereClass::HydrogenHelium
    } else if frac(Gas::CO2) > 0.5 {
        AtmosphereClass::ThinCO2
    } else if frac(Gas::N2) > 0.5 {
        AtmosphereClass::ThickN2H2O
    } else if frac(Gas::O2) > 0.1 && frac(Gas::N2) > 0.1 {
        AtmosphereClass::NitrogenOxygen
    } else if comp.is_empty() {
        AtmosphereClass::None
    } else {
        AtmosphereClass::ThinCO2
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ymir_system::orbital_body::{OrbitalBody, PlanetType};

    fn body_with(
        mass: f64,
        radius: f64,
        equilibrium_temp: f64,
        is_in_hz: bool,
        planet_type: PlanetType,
    ) -> OrbitalBody {
        OrbitalBody {
            semi_major_axis: 1.0,
            eccentricity: 0.0,
            inclination: 0.0,
            axial_tilt: 0.0,
            mass,
            radius,
            density: 5.51,
            surface_gravity: 9.81,
            solar_irradiance: 1361.0,
            equilibrium_temp,
            tidal_locked: false,
            rotation_period: 24.0,
            is_in_hz,
            planet_type,
            name: None,
            is_known_exoplanet: false,
        }
    }

    fn sum_fractions(comp: &HashMap<Gas, f64>) -> f64 {
        comp.values().sum()
    }

    #[test]
    fn earth_like_returns_nitrogen_oxygen() {
        let body = body_with(1.0, 1.0, 254.0, true, PlanetType::Terran);
        let retained = vec![Gas::N2, Gas::O2, Gas::CO2, Gas::H2O, Gas::Ar];
        let (comp, pressure, class) = derive_composition(&body, &retained, 0.0, true);
        assert_eq!(class, AtmosphereClass::NitrogenOxygen);
        let n2 = comp.get(&Gas::N2).copied().unwrap_or(0.0);
        let o2 = comp.get(&Gas::O2).copied().unwrap_or(0.0);
        assert!(n2 > o2, "N2 should dominate, got N2={n2} O2={o2}");
        assert!(n2 > 0.7, "N2 should be near 0.78, got {n2}");
        assert!(
            pressure > 0.8 && pressure < 1.2,
            "pressure near 1 bar, got {pressure}"
        );
        assert!((sum_fractions(&comp) - 1.0).abs() < 0.01);
    }

    #[test]
    fn mars_like_returns_thin_co2() {
        let body = body_with(0.107, 0.532, 210.0, false, PlanetType::Terran);
        let retained = vec![Gas::CO2, Gas::N2, Gas::Ar];
        let (comp, pressure, class) = derive_composition(&body, &retained, 0.0, false);
        assert_eq!(class, AtmosphereClass::ThinCO2);
        let co2 = comp.get(&Gas::CO2).copied().unwrap_or(0.0);
        assert!(co2 > 0.9, "CO2 should dominate, got {co2}");
        assert!(
            pressure < 0.1,
            "thin CO2 pressure should be low, got {pressure}"
        );
        assert!((sum_fractions(&comp) - 1.0).abs() < 0.01);
    }

    #[test]
    fn venus_like_returns_thick_co2() {
        let body = body_with(0.815, 0.95, 327.0, false, PlanetType::Terran);
        let retained = vec![Gas::CO2, Gas::N2, Gas::Ar];
        let (comp, pressure, class) = derive_composition(&body, &retained, 0.0, false);
        assert_eq!(class, AtmosphereClass::ThickCO2);
        let co2 = comp.get(&Gas::CO2).copied().unwrap_or(0.0);
        assert!(co2 > 0.9, "CO2 should dominate, got {co2}");
        assert!(
            pressure > 1.0,
            "Venus-like pressure should be >1 bar, got {pressure}"
        );
        assert!((sum_fractions(&comp) - 1.0).abs() < 0.01);
    }

    #[test]
    fn gas_giant_returns_hydrogen_helium() {
        let body = body_with(318.0, 11.0, 125.0, false, PlanetType::GasGiant);
        let retained: Vec<Gas> = Gas::all().into_iter().collect();
        let (comp, pressure, class) = derive_composition(&body, &retained, 0.0, false);
        assert_eq!(class, AtmosphereClass::HydrogenHelium);
        let h2 = comp.get(&Gas::H2).copied().unwrap_or(0.0);
        assert!(h2 > 0.9, "H2 should dominate, got {h2}");
        assert!(pressure > 100.0);
        assert!((sum_fractions(&comp) - 1.0).abs() < 0.01);
    }

    #[test]
    fn no_atmosphere_when_retained_is_empty() {
        let body = body_with(1.0, 1.0, 254.0, true, PlanetType::Terran);
        let (comp, pressure, class) = derive_composition(&body, &[], 0.0, true);
        assert_eq!(class, AtmosphereClass::None);
        assert!(comp.is_empty());
        assert_eq!(pressure, 0.0);
    }

    #[test]
    fn tiny_body_has_no_atmosphere() {
        let body = body_with(0.01, 0.2, 200.0, false, PlanetType::Terran);
        let retained = vec![Gas::CO2, Gas::N2];
        let (comp, pressure, class) = derive_composition(&body, &retained, 0.0, false);
        assert_eq!(class, AtmosphereClass::None);
        assert!(comp.is_empty());
        assert_eq!(pressure, 0.0);
    }

    #[test]
    fn mole_fractions_sum_to_one_across_archetypes() {
        // Earth-like
        let earth = body_with(1.0, 1.0, 254.0, true, PlanetType::Terran);
        let (c, _, _) = derive_composition(
            &earth,
            &[Gas::N2, Gas::O2, Gas::CO2, Gas::H2O, Gas::Ar],
            0.0,
            true,
        );
        assert!((sum_fractions(&c) - 1.0).abs() < 0.01);

        // Mars-like
        let mars = body_with(0.107, 0.532, 210.0, false, PlanetType::Terran);
        let (c, _, _) = derive_composition(&mars, &[Gas::CO2, Gas::N2, Gas::Ar], 0.0, false);
        assert!((sum_fractions(&c) - 1.0).abs() < 0.01);

        // Venus-like
        let venus = body_with(0.815, 0.95, 327.0, false, PlanetType::Terran);
        let (c, _, _) = derive_composition(&venus, &[Gas::CO2, Gas::N2], 0.0, false);
        assert!((sum_fractions(&c) - 1.0).abs() < 0.01);

        // Gas giant
        let giant = body_with(318.0, 11.0, 125.0, false, PlanetType::GasGiant);
        let retained: Vec<Gas> = Gas::all().into_iter().collect();
        let (c, _, _) = derive_composition(&giant, &retained, 0.0, false);
        assert!((sum_fractions(&c) - 1.0).abs() < 0.01);

        // N2-dominated fallback
        let titan_like = body_with(0.5, 0.8, 180.0, false, PlanetType::Terran);
        let (c, _, _) = derive_composition(&titan_like, &[Gas::N2, Gas::Ar], 0.0, false);
        assert!((sum_fractions(&c) - 1.0).abs() < 0.01);
    }

    #[test]
    fn metallicity_scales_pressure() {
        let body = body_with(1.0, 1.0, 254.0, true, PlanetType::Terran);
        let retained = vec![Gas::N2, Gas::O2, Gas::CO2, Gas::H2O, Gas::Ar];
        let (_, p_low, _) = derive_composition(&body, &retained, -0.5, true);
        let (_, p_mid, _) = derive_composition(&body, &retained, 0.0, true);
        let (_, p_high, _) = derive_composition(&body, &retained, 0.5, true);
        assert!(p_low < p_mid);
        assert!(p_mid < p_high);
    }

    #[test]
    fn biology_off_skips_nitrogen_oxygen() {
        let body = body_with(1.0, 1.0, 254.0, true, PlanetType::Terran);
        let retained = vec![Gas::N2, Gas::O2, Gas::CO2, Gas::H2O, Gas::Ar];
        let (_, _, class) = derive_composition(&body, &retained, 0.0, false);
        // With biology disabled, falls through to CO2-dominated since CO2 retained.
        assert_ne!(class, AtmosphereClass::NitrogenOxygen);
    }
}
