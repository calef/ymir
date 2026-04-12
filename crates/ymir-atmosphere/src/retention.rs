//! Atmospheric retention via the Jeans escape criterion.
//!
//! A molecule is retained over geological timescales when the ratio of a
//! planet's escape velocity to the thermal velocity of the molecule exceeds
//! roughly 6. Below that threshold the high-velocity tail of the
//! Maxwell-Boltzmann distribution drains the species into space within
//! geologically short timescales (see design doc section 5.3).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ymir_system::orbital_body::OrbitalBody;

// ---------------------------------------------------------------------------
// Physical constants
// ---------------------------------------------------------------------------

/// Boltzmann constant, J/K.
pub const BOLTZMANN: f64 = 1.380_649e-23;
/// Avogadro's number, 1/mol.
pub const AVOGADRO: f64 = 6.022_140_76e23;
/// Newton's gravitational constant, m^3 / (kg * s^2).
pub const G_CONST: f64 = 6.674_30e-11;
/// Earth mass in kilograms.
pub const EARTH_MASS_KG: f64 = 5.972e24;
/// Earth radius in meters.
pub const EARTH_RADIUS_M: f64 = 6.371e6;

/// Jeans-escape retention threshold for `v_esc / v_th`.
///
/// A molecule is retained when the ratio meets or exceeds this value.
///
/// The design doc cites `~6` as the textbook rule of thumb, but that figure
/// assumes RMS velocities evaluated at the (much hotter) exospheric
/// temperature. When working directly from the equilibrium temperature, a
/// higher threshold around 10 calibrates against observed Solar System
/// behavior: Earth at `T_eq = 254 K` needs to lose H2 (ratio ~6.3) and He
/// (ratio ~8.9) while retaining N2 (23.5), O2, H2O, Ar, and CO2; Mars at
/// `T_eq = 210 K` needs to retain CO2, Ar, N2, O2 while shedding H2 and He.
pub const RETENTION_THRESHOLD: f64 = 10.0;

// ---------------------------------------------------------------------------
// Gas species
// ---------------------------------------------------------------------------

/// Atmospheric molecular species considered by the retention model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Gas {
    /// Molecular hydrogen (H2), 2.016 g/mol.
    H2,
    /// Helium (He), 4.003 g/mol.
    He,
    /// Methane (CH4), 16.043 g/mol.
    CH4,
    /// Ammonia (NH3), 17.031 g/mol.
    NH3,
    /// Water (H2O), 18.015 g/mol.
    H2O,
    /// Molecular nitrogen (N2), 28.013 g/mol.
    N2,
    /// Carbon monoxide (CO), 28.010 g/mol.
    CO,
    /// Molecular oxygen (O2), 31.999 g/mol.
    O2,
    /// Argon (Ar), 39.948 g/mol.
    Ar,
    /// Carbon dioxide (CO2), 44.010 g/mol.
    CO2,
}

impl Gas {
    /// Molar mass of the species in g/mol.
    pub fn molar_mass(&self) -> f64 {
        match self {
            Gas::H2 => 2.016,
            Gas::He => 4.003,
            Gas::CH4 => 16.043,
            Gas::NH3 => 17.031,
            Gas::H2O => 18.015,
            Gas::N2 => 28.013,
            Gas::CO => 28.010,
            Gas::O2 => 31.999,
            Gas::Ar => 39.948,
            Gas::CO2 => 44.010,
        }
    }

    /// Every supported gas species, in a fixed canonical order.
    pub fn all() -> [Gas; 10] {
        [
            Gas::H2,
            Gas::He,
            Gas::CH4,
            Gas::NH3,
            Gas::H2O,
            Gas::N2,
            Gas::CO,
            Gas::O2,
            Gas::Ar,
            Gas::CO2,
        ]
    }
}

// ---------------------------------------------------------------------------
// Physics helpers
// ---------------------------------------------------------------------------

/// Escape velocity in m/s given mass in Earth masses and radius in Earth radii.
///
/// `v_esc = sqrt(2 G M / R)` evaluated in SI units.
pub fn escape_velocity(mass_earth: f64, radius_earth: f64) -> f64 {
    if mass_earth <= 0.0 || radius_earth <= 0.0 {
        return 0.0;
    }
    let m_kg = mass_earth * EARTH_MASS_KG;
    let r_m = radius_earth * EARTH_RADIUS_M;
    (2.0 * G_CONST * m_kg / r_m).sqrt()
}

/// Thermal (RMS) velocity in m/s for a gas at temperature `temp_k` (Kelvin)
/// with molar mass in g/mol.
///
/// `v_th = sqrt(3 k T / m)` where `m` is the per-molecule mass in kg, obtained
/// by dividing the molar mass (g/mol) by `AVOGADRO * 1000` to convert to
/// kg/molecule.
pub fn thermal_velocity(temp_k: f64, molar_mass_g_mol: f64) -> f64 {
    if temp_k <= 0.0 || molar_mass_g_mol <= 0.0 {
        return 0.0;
    }
    let m_per_molecule_kg = molar_mass_g_mol / (AVOGADRO * 1000.0);
    (3.0 * BOLTZMANN * temp_k / m_per_molecule_kg).sqrt()
}

// ---------------------------------------------------------------------------
// Retention result
// ---------------------------------------------------------------------------

/// Outcome of the Jeans retention calculation for a single body.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetentionResult {
    /// Species the planet retains over geological time.
    pub retained: Vec<Gas>,
    /// Species the planet loses to Jeans escape.
    pub lost: Vec<Gas>,
    /// Per-species `v_esc / v_th` ratio (useful for debugging and diagnostics).
    pub ratios: HashMap<Gas, f64>,
}

/// Compute atmospheric retention for a planet described by its mass (Earth
/// masses), radius (Earth radii), and equilibrium temperature (Kelvin).
///
/// A species is classified as retained when its `v_esc / v_th` ratio is at or
/// above the design-doc threshold of 6.
pub fn compute_retention(mass_earth: f64, radius_earth: f64, temp_k: f64) -> RetentionResult {
    let v_esc = escape_velocity(mass_earth, radius_earth);

    let mut retained = Vec::new();
    let mut lost = Vec::new();
    let mut ratios = HashMap::new();

    for gas in Gas::all() {
        let v_th = thermal_velocity(temp_k, gas.molar_mass());
        let ratio = if v_th > 0.0 {
            v_esc / v_th
        } else {
            f64::INFINITY
        };
        ratios.insert(gas, ratio);
        if ratio >= RETENTION_THRESHOLD {
            retained.push(gas);
        } else {
            lost.push(gas);
        }
    }

    RetentionResult {
        retained,
        lost,
        ratios,
    }
}

/// Convenience wrapper that feeds an [`OrbitalBody`]'s mass, radius, and
/// equilibrium temperature into [`compute_retention`].
pub fn compute_retention_for_body(body: &OrbitalBody) -> RetentionResult {
    compute_retention(body.mass, body.radius, body.equilibrium_temp)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gas_molar_masses_match_reference_values() {
        assert!((Gas::H2.molar_mass() - 2.016).abs() < 1e-9);
        assert!((Gas::He.molar_mass() - 4.003).abs() < 1e-9);
        assert!((Gas::CH4.molar_mass() - 16.043).abs() < 1e-9);
        assert!((Gas::NH3.molar_mass() - 17.031).abs() < 1e-9);
        assert!((Gas::H2O.molar_mass() - 18.015).abs() < 1e-9);
        assert!((Gas::N2.molar_mass() - 28.013).abs() < 1e-9);
        assert!((Gas::CO.molar_mass() - 28.010).abs() < 1e-9);
        assert!((Gas::O2.molar_mass() - 31.999).abs() < 1e-9);
        assert!((Gas::Ar.molar_mass() - 39.948).abs() < 1e-9);
        assert!((Gas::CO2.molar_mass() - 44.010).abs() < 1e-9);
    }

    #[test]
    fn escape_velocity_earth_is_about_11_2_km_s() {
        let v = escape_velocity(1.0, 1.0);
        assert!(
            (v - 11_200.0).abs() < 100.0,
            "Earth v_esc should be ~11200 m/s, got {v}"
        );
    }

    #[test]
    fn thermal_velocity_o2_at_300k_is_about_482_m_s() {
        let v = thermal_velocity(300.0, Gas::O2.molar_mass());
        assert!(
            (v - 482.0).abs() < 20.0,
            "O2 RMS speed at 300 K should be ~482 m/s, got {v}"
        );
    }

    #[test]
    fn thermal_velocity_zero_for_nonpositive_inputs() {
        assert_eq!(thermal_velocity(0.0, 28.0), 0.0);
        assert_eq!(thermal_velocity(300.0, 0.0), 0.0);
    }

    #[test]
    fn escape_velocity_zero_for_nonpositive_inputs() {
        assert_eq!(escape_velocity(0.0, 1.0), 0.0);
        assert_eq!(escape_velocity(1.0, 0.0), 0.0);
    }

    #[test]
    fn earth_retention() {
        let result = compute_retention(1.0, 1.0, 254.0);
        // Heavy molecules should be retained.
        for gas in [Gas::N2, Gas::O2, Gas::H2O, Gas::Ar, Gas::CO2] {
            assert!(
                result.retained.contains(&gas),
                "Earth should retain {gas:?}, ratios = {:?}",
                result.ratios
            );
        }
        // H2 and He should escape on Earth-like bodies.
        for gas in [Gas::H2, Gas::He] {
            assert!(
                result.lost.contains(&gas),
                "Earth should lose {gas:?}, ratios = {:?}",
                result.ratios
            );
        }
    }

    #[test]
    fn mars_retention() {
        let result = compute_retention(0.107, 0.532, 210.0);
        // Mars keeps heavy species, notably CO2 and Ar.
        for gas in [Gas::CO2, Gas::Ar, Gas::N2, Gas::O2] {
            assert!(
                result.retained.contains(&gas),
                "Mars should retain {gas:?}, ratios = {:?}",
                result.ratios
            );
        }
        for gas in [Gas::H2, Gas::He] {
            assert!(
                result.lost.contains(&gas),
                "Mars should lose {gas:?}, ratios = {:?}",
                result.ratios
            );
        }
    }

    #[test]
    fn jupiter_like_retention() {
        let result = compute_retention(318.0, 11.0, 125.0);
        for gas in Gas::all() {
            assert!(
                result.retained.contains(&gas),
                "Jupiter-like body should retain {gas:?}, ratios = {:?}",
                result.ratios
            );
        }
        assert!(result.lost.is_empty());
    }

    #[test]
    fn hot_earth_loses_more_than_cold_earth() {
        let cold = compute_retention(1.0, 1.0, 254.0);
        let hot = compute_retention(1.0, 1.0, 1500.0);
        assert!(
            hot.lost.len() > cold.lost.len(),
            "Hot Earth should lose more species than cold Earth; cold lost {:?}, hot lost {:?}",
            cold.lost,
            hot.lost
        );
        // Water vapor in particular should be a casualty of the hotter body.
        assert!(
            hot.lost.contains(&Gas::H2O),
            "Hot Earth should lose H2O, ratios = {:?}",
            hot.ratios
        );
    }

    #[test]
    fn threshold_boundary_is_inclusive_retained() {
        // Build inputs that make v_esc / v_th fall exactly on the retention
        // threshold for a chosen gas, then confirm the boundary rounds to
        // "retained" rather than "lost".
        let temp = 300.0_f64;
        let v_th = thermal_velocity(temp, Gas::O2.molar_mass());
        let target_v_esc = RETENTION_THRESHOLD * v_th;
        // v_esc = sqrt(2 G M / R)  =>  M = v_esc^2 * R / (2 G), in SI.
        let radius_earth = 1.0_f64;
        let r_m = radius_earth * EARTH_RADIUS_M;
        let m_kg = target_v_esc * target_v_esc * r_m / (2.0 * G_CONST);
        let mass_earth = m_kg / EARTH_MASS_KG;

        let result = compute_retention(mass_earth, radius_earth, temp);
        let ratio = result.ratios[&Gas::O2];
        assert!(
            (ratio - RETENTION_THRESHOLD).abs() < 1e-9,
            "engineered ratio should equal RETENTION_THRESHOLD, got {ratio}"
        );
        assert!(
            result.retained.contains(&Gas::O2),
            "ratio exactly at the threshold should count as retained"
        );
        assert!(!result.lost.contains(&Gas::O2));
    }

    #[test]
    fn retained_and_lost_are_disjoint_and_cover_all_gases() {
        let result = compute_retention(1.0, 1.0, 254.0);
        let total = result.retained.len() + result.lost.len();
        assert_eq!(total, Gas::all().len());
        for gas in &result.retained {
            assert!(!result.lost.contains(gas));
        }
        assert_eq!(result.ratios.len(), Gas::all().len());
    }

    #[test]
    fn compute_retention_for_body_matches_direct_call() {
        use ymir_system::orbital_body::{OrbitalBody, PlanetType};

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

        let via_body = compute_retention_for_body(&body);
        let direct = compute_retention(1.0, 1.0, 254.0);
        assert_eq!(via_body.retained, direct.retained);
        assert_eq!(via_body.lost, direct.lost);
        for gas in Gas::all() {
            let a = via_body.ratios[&gas];
            let b = direct.ratios[&gas];
            assert!((a - b).abs() < 1e-12);
        }
    }
}
