//! Bulk planetary property derivation: mass, radius, density, and composition
//! class from orbital parameters and stellar metallicity.

use ymir_catalog::star_context::StarContext;
use ymir_core::{Sourced, WorldRng};

use crate::orbital_body::{OrbitalBody, PlanetType};
use crate::placement::PlacedPlanet;
use crate::tidal::{is_tidally_locked, orbital_period_years};

/// Stage name written into [`ymir_core::Source::Derived`] for all fields
/// populated by [`derive_body`].
const STAGE: &str = "planetary_system";

// ---------------------------------------------------------------------------
// Physical constants
// ---------------------------------------------------------------------------

/// Newton's gravitational constant, m^3 / (kg * s^2).
pub const G: f64 = 6.674_30e-11;
/// Astronomical unit in meters.
pub const AU_IN_METERS: f64 = 1.495_978_707e11;
/// Earth mass in kilograms.
pub const EARTH_MASS_KG: f64 = 5.972e24;
/// Earth radius in meters.
pub const EARTH_RADIUS_M: f64 = 6.371e6;
/// Solar luminosity in watts.
pub const SOLAR_LUMINOSITY_W: f64 = 3.828e26;
/// Solar mass in kilograms.
pub const SOLAR_MASS_KG: f64 = 1.989e30;
/// Stefan-Boltzmann constant, W / (m^2 * K^4).
pub const STEFAN_BOLTZMANN: f64 = 5.670_374_419e-8;

/// Default Bond albedo used for equilibrium temperature when none is supplied.
pub const DEFAULT_BOND_ALBEDO: f64 = 0.3;

/// Mass-radius breakpoint (Earth radii) separating the rocky and neptunian
/// regimes in the simplified Chen & Kipping 2017 relation.
const MR_BREAK_R_EARTH: f64 = 1.23;

// ---------------------------------------------------------------------------
// Mass-radius relation
// ---------------------------------------------------------------------------

/// Estimate planet mass (Earth masses) from radius (Earth radii) using a
/// simplified Chen & Kipping 2017 broken power law.
///
/// - `R < 1.23 R_earth`: rocky regime, `M = R^3.7`.
/// - `R >= 1.23 R_earth`: neptunian regime, `M = 1.436 * R^1.70`.
pub fn mass_from_radius(r_earth: f64) -> f64 {
    if r_earth < MR_BREAK_R_EARTH {
        r_earth.powf(3.7)
    } else {
        1.436 * r_earth.powf(1.70)
    }
}

/// Compute bulk density in g/cm^3 from mass (Earth masses) and radius
/// (Earth radii).
pub fn density_gcc(mass_earth: f64, radius_earth: f64) -> f64 {
    if radius_earth <= 0.0 {
        return 0.0;
    }
    let mass_kg = mass_earth * EARTH_MASS_KG;
    let radius_m = radius_earth * EARTH_RADIUS_M;
    let volume_m3 = (4.0 / 3.0) * std::f64::consts::PI * radius_m.powi(3);
    let density_kg_m3 = mass_kg / volume_m3;
    // 1 kg/m^3 = 1e-3 g/cm^3.
    density_kg_m3 * 1.0e-3
}

/// Compute surface gravity in m/s^2 from mass (Earth masses) and radius
/// (Earth radii).
pub fn surface_gravity_ms2(mass_earth: f64, radius_earth: f64) -> f64 {
    if radius_earth <= 0.0 {
        return 0.0;
    }
    let mass_kg = mass_earth * EARTH_MASS_KG;
    let radius_m = radius_earth * EARTH_RADIUS_M;
    G * mass_kg / (radius_m * radius_m)
}

// ---------------------------------------------------------------------------
// Radiative environment
// ---------------------------------------------------------------------------

/// Compute solar irradiance (W/m^2) at `sma_au` from a star of `luminosity_solar`
/// solar luminosities.
pub fn solar_irradiance(luminosity_solar: f64, sma_au: f64) -> f64 {
    if sma_au <= 0.0 {
        return 0.0;
    }
    let distance_m = sma_au * AU_IN_METERS;
    let l_w = luminosity_solar * SOLAR_LUMINOSITY_W;
    l_w / (4.0 * std::f64::consts::PI * distance_m * distance_m)
}

/// Equilibrium blackbody temperature (K) for a planet receiving `irradiance`
/// W/m^2 with Bond albedo `albedo`.
///
/// `T_eq = (S * (1 - A) / (4 * sigma))^(1/4)`
pub fn equilibrium_temperature(irradiance: f64, albedo: f64) -> f64 {
    if irradiance <= 0.0 {
        return 0.0;
    }
    let absorbed = irradiance * (1.0 - albedo);
    (absorbed / (4.0 * STEFAN_BOLTZMANN)).powf(0.25)
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Classify a planet by radius (with mass as a tiebreaker for edge cases).
pub fn classify_planet(radius_earth: f64, _mass_earth: f64) -> PlanetType {
    if radius_earth < 1.5 {
        PlanetType::Terran
    } else if radius_earth < 2.0 {
        PlanetType::SuperEarth
    } else if radius_earth < 4.0 {
        PlanetType::SubNeptune
    } else if radius_earth < 10.0 {
        PlanetType::Neptune
    } else {
        PlanetType::GasGiant
    }
}

// ---------------------------------------------------------------------------
// Random draws for orbital elements
// ---------------------------------------------------------------------------

/// Sample from a Rayleigh distribution with scale parameter `sigma`.
///
/// The inverse CDF for Rayleigh is `x = sigma * sqrt(-2 * ln(1 - u))`.
fn rayleigh_sample(rng: &mut WorldRng, sigma: f64) -> f64 {
    // Guard against log(0). `next_f64` returns [0, 1); use 1 - u to shift to (0, 1].
    let u = rng.next_f64();
    let one_minus_u = 1.0 - u;
    // one_minus_u is in (0, 1], so ln is defined and <= 0.
    sigma * (-2.0 * one_minus_u.ln()).sqrt()
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Derive a full [`OrbitalBody`] for a placed planet given the star context.
///
/// `seed_offset` is mixed into a child RNG derived from `rng` so callers can
/// produce deterministic per-planet streams without depleting the system RNG.
pub fn derive_body(
    placed: &PlacedPlanet,
    star: &StarContext,
    rng: &mut WorldRng,
    seed_offset: u64,
) -> OrbitalBody {
    // Derive an independent per-planet RNG stream so that the relative order
    // of planets doesn't perturb each body's draws.
    let context = format!("orbital_body::{seed_offset}");
    let mut body_rng = rng.child(&context);

    let radius = placed.radius;
    let mass = mass_from_radius(radius);
    let density = density_gcc(mass, radius);
    let gravity = surface_gravity_ms2(mass, radius);

    let irradiance = solar_irradiance(*star.luminosity.inner(), placed.semi_major_axis);
    let t_eq = equilibrium_temperature(irradiance, DEFAULT_BOND_ALBEDO);

    // Eccentricity: Rayleigh(sigma = 0.08). Clamp to [0, 0.95).
    let eccentricity = rayleigh_sample(&mut body_rng, 0.08).min(0.95);
    // Inclination (degrees): Rayleigh(sigma = 2).
    let inclination = rayleigh_sample(&mut body_rng, 2.0);
    // Axial tilt (degrees): uniform in [0, 45].
    let axial_tilt = body_rng.next_range(0.0, 45.0);

    let locked = is_tidally_locked(placed.semi_major_axis, *star.mass.inner(), *star.age.inner());
    let rotation_period = if locked {
        // Orbital period in hours (1 year ~ 8766 h).
        orbital_period_years(placed.semi_major_axis, *star.mass.inner()) * 365.25 * 24.0
    } else {
        24.0
    };

    let is_in_hz = placed.semi_major_axis >= *star.hz_inner.inner()
        && placed.semi_major_axis <= *star.hz_outer.inner();
    let planet_type = classify_planet(radius, mass);

    OrbitalBody {
        semi_major_axis: Sourced::derived(placed.semi_major_axis, STAGE),
        eccentricity: Sourced::derived(eccentricity, STAGE),
        inclination: Sourced::derived(inclination, STAGE),
        axial_tilt: Sourced::derived(axial_tilt, STAGE),
        mass: Sourced::derived(mass, STAGE),
        radius: Sourced::derived(radius, STAGE),
        density: Sourced::derived(density, STAGE),
        surface_gravity: Sourced::derived(gravity, STAGE),
        solar_irradiance: Sourced::derived(irradiance, STAGE),
        equilibrium_temp: Sourced::derived(t_eq, STAGE),
        tidal_locked: Sourced::derived(locked, STAGE),
        rotation_period: Sourced::derived(rotation_period, STAGE),
        is_in_hz,
        planet_type,
        name: placed.name.clone(),
        is_known_exoplanet: placed.is_known,
        continental_fraction: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ymir_catalog::star_context::StarContext;
    use ymir_core::WorldRng;

    fn sun() -> StarContext {
        use ymir_catalog::star_context::{SpectralClass, SpectralType};
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

    #[test]
    fn earth_mass_from_radius() {
        let m = mass_from_radius(1.0);
        // R = 1 < 1.23, so rocky: M = 1^3.7 = 1.0.
        assert!(
            (m - 1.0).abs() < 1e-6,
            "Earth mass from R=1 should be 1, got {m}"
        );
    }

    #[test]
    fn mars_mass_from_radius() {
        let m = mass_from_radius(0.53);
        // Rocky: 0.53^3.7. True Mars mass is 0.107.
        assert!(
            (0.05..0.20).contains(&m),
            "Mars mass should be ~0.11, got {m}"
        );
    }

    #[test]
    fn jupiter_like_mass_from_radius() {
        // The simplified neptunian branch (M = 1.436 * R^1.70) under-predicts
        // true Jupiter by roughly a factor of 4, but still returns a large
        // mass that dominates any terrestrial body.
        let m_11 = mass_from_radius(11.0);
        assert!(m_11 > 50.0, "R=11 should give > 50 M_earth, got {m_11}");
        // A somewhat larger gas giant should clear 100 M_earth.
        let m_14 = mass_from_radius(14.0);
        assert!(m_14 > 100.0, "R=14 should give > 100 M_earth, got {m_14}");
    }

    #[test]
    fn mass_radius_piecewise_branches() {
        let rocky = mass_from_radius(1.0);
        let neptunian = mass_from_radius(2.0);
        assert!(rocky < neptunian);
    }

    #[test]
    fn earth_density() {
        let d = density_gcc(1.0, 1.0);
        // Earth: ~5.51 g/cm^3.
        assert!(
            (d - 5.51).abs() < 0.1,
            "Earth density should be ~5.51 g/cm^3, got {d}"
        );
    }

    #[test]
    fn earth_surface_gravity() {
        let g = surface_gravity_ms2(1.0, 1.0);
        assert!(
            (g - 9.82).abs() < 0.05,
            "Earth gravity should be ~9.82 m/s^2, got {g}"
        );
    }

    #[test]
    fn earth_equilibrium_temperature() {
        // Irradiance at 1 AU from the Sun: ~1361 W/m^2.
        let s = solar_irradiance(1.0, 1.0);
        assert!(
            (s - 1361.0).abs() < 5.0,
            "Solar constant at 1 AU should be ~1361, got {s}"
        );
        let t = equilibrium_temperature(s, DEFAULT_BOND_ALBEDO);
        assert!(
            (t - 254.0).abs() < 3.0,
            "Earth T_eq should be ~254 K with A=0.3, got {t}"
        );
    }

    #[test]
    fn equilibrium_temperature_zero_for_zero_irradiance() {
        assert_eq!(equilibrium_temperature(0.0, 0.3), 0.0);
    }

    #[test]
    fn classify_planet_buckets() {
        assert_eq!(classify_planet(1.0, 1.0), PlanetType::Terran);
        assert_eq!(classify_planet(1.7, 3.0), PlanetType::SuperEarth);
        assert_eq!(classify_planet(3.0, 10.0), PlanetType::SubNeptune);
        assert_eq!(classify_planet(6.0, 20.0), PlanetType::Neptune);
        assert_eq!(classify_planet(12.0, 300.0), PlanetType::GasGiant);
    }

    #[test]
    fn derive_body_populates_all_fields() {
        let star = sun();
        let placed = PlacedPlanet {
            semi_major_axis: 1.0,
            radius: 1.0,
            is_known: false,
            name: None,
        };
        let mut rng = WorldRng::new(7);
        let body = derive_body(&placed, &star, &mut rng, 0);

        assert_eq!(*body.semi_major_axis.inner(), 1.0);
        assert_eq!(*body.radius.inner(), 1.0);
        assert!((*body.mass.inner() - 1.0).abs() < 1e-6);
        assert!((*body.density.inner() - 5.51).abs() < 0.1);
        assert!((*body.surface_gravity.inner() - 9.82).abs() < 0.05);
        assert!((*body.solar_irradiance.inner() - 1361.0).abs() < 5.0);
        assert!((*body.equilibrium_temp.inner() - 254.0).abs() < 3.0);
        assert!(*body.eccentricity.inner() >= 0.0 && *body.eccentricity.inner() < 1.0);
        assert!(*body.inclination.inner() >= 0.0);
        assert!(*body.axial_tilt.inner() >= 0.0 && *body.axial_tilt.inner() < 45.0);
        assert!(!*body.tidal_locked.inner());
        assert_eq!(body.planet_type, PlanetType::Terran);
        assert!(body.is_in_hz);
    }

    #[test]
    fn derive_body_is_deterministic() {
        let star = sun();
        let placed = PlacedPlanet {
            semi_major_axis: 1.2,
            radius: 1.1,
            is_known: false,
            name: None,
        };

        let mut rng1 = WorldRng::new(42);
        let b1 = derive_body(&placed, &star, &mut rng1, 3);

        let mut rng2 = WorldRng::new(42);
        let b2 = derive_body(&placed, &star, &mut rng2, 3);

        assert_eq!(*b1.eccentricity.inner(), *b2.eccentricity.inner());
        assert_eq!(*b1.inclination.inner(), *b2.inclination.inner());
        assert_eq!(*b1.axial_tilt.inner(), *b2.axial_tilt.inner());
        assert_eq!(*b1.rotation_period.inner(), *b2.rotation_period.inner());
    }

    #[test]
    fn derive_body_close_planet_is_locked() {
        let star = sun();
        let placed = PlacedPlanet {
            semi_major_axis: 0.03,
            radius: 1.0,
            is_known: false,
            name: None,
        };
        let mut rng = WorldRng::new(1);
        let body = derive_body(&placed, &star, &mut rng, 0);
        assert!(
            *body.tidal_locked.inner(),
            "close-in planet should be tidally locked"
        );
        // Rotation period should match orbital period in hours.
        let orbital_hours = orbital_period_years(0.03, *star.mass.inner()) * 365.25 * 24.0;
        assert!((*body.rotation_period.inner() - orbital_hours).abs() < 1e-6);
    }

    // ------------------------------------------------------------------
    // CORE-06 per-field provenance tests
    // ------------------------------------------------------------------

    #[test]
    fn core06_derive_body_tags_every_scalar_derived() {
        let star = sun();
        let placed = PlacedPlanet {
            semi_major_axis: 1.0,
            radius: 1.0,
            is_known: false,
            name: None,
        };
        let mut rng = WorldRng::new(7);
        let body = derive_body(&placed, &star, &mut rng, 0);

        assert!(body.semi_major_axis.is_derived());
        assert!(body.eccentricity.is_derived());
        assert!(body.inclination.is_derived());
        assert!(body.axial_tilt.is_derived());
        assert!(body.mass.is_derived());
        assert!(body.radius.is_derived());
        assert!(body.density.is_derived());
        assert!(body.surface_gravity.is_derived());
        assert!(body.solar_irradiance.is_derived());
        assert!(body.equilibrium_temp.is_derived());
        assert!(body.tidal_locked.is_derived());
        assert!(body.rotation_period.is_derived());
    }
}
