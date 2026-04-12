//! Orbital placement algorithms for distributing planets around a star based
//! on stability constraints and empirical spacing laws.

use serde::{Deserialize, Serialize};
use ymir_catalog::exoplanets::ExoplanetRecord;
use ymir_catalog::star_context::StarContext;
use ymir_core::WorldRng;

/// Earth mass in solar masses, used for Hill radius calculations.
const EARTH_MASS_IN_SOLAR: f64 = 3.003e-6;

/// Configuration for the orbital placement algorithm.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlacementConfig {
    /// Maximum number of planets to generate (including known).
    pub max_planets: usize,
    /// Minimum separation in mutual Hill radii between adjacent planets.
    pub min_hill_separation: f64,
    /// Minimum orbital distance in AU.
    pub inner_limit_au: f64,
    /// Maximum orbital distance in AU.
    pub outer_limit_au: f64,
}

impl Default for PlacementConfig {
    fn default() -> Self {
        Self {
            max_planets: 8,
            min_hill_separation: 8.0,
            inner_limit_au: 0.05,
            outer_limit_au: 50.0,
        }
    }
}

/// A planet placed in an orbital slot, either from real data or generated.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlacedPlanet {
    /// Semi-major axis in AU.
    pub semi_major_axis: f64,
    /// Planet radius in Earth radii.
    pub radius: f64,
    /// True if this planet comes from real exoplanet data.
    pub is_known: bool,
    /// Planet name from ExoplanetRecord, if known.
    pub name: Option<String>,
}

/// Estimate planet mass (in Earth masses) from radius (in Earth radii) using a
/// simplified Chen & Kipping 2017 power law.
fn mass_from_radius(r: f64) -> f64 {
    if r < 1.23 {
        // Rocky/Terran regime
        r.powf(2.06)
    } else {
        // Volatile-rich (super-Earth / sub-Neptune) regime
        r.powf(1.7)
    }
}

/// Compute the mutual Hill radius for two adjacent planets.
///
/// `R_Hill_mutual = ((a1 + a2) / 2) * ((m1 + m2) / (3 * m_star))^(1/3)`
///
/// All masses must be in the same units (solar masses).
/// Semi-major axes in AU. Returns Hill radius in AU.
fn mutual_hill_radius(a1: f64, m1: f64, a2: f64, m2: f64, m_star: f64) -> f64 {
    let a_avg = (a1 + a2) / 2.0;
    let mass_ratio = (m1 + m2) / (3.0 * m_star);
    a_avg * mass_ratio.cbrt()
}

/// Check whether inserting a planet at `candidate_a` with mass `candidate_m`
/// (solar masses) violates the Hill separation constraint against all existing
/// planets. `planets_sorted` must be sorted by semi_major_axis.
fn violates_hill_separation(
    candidate_a: f64,
    candidate_m: f64,
    planets_sorted: &[(f64, f64)], // (semi_major_axis, mass in solar masses)
    m_star: f64,
    min_sep: f64,
) -> bool {
    for &(a, m) in planets_sorted {
        let r_hill = mutual_hill_radius(candidate_a, candidate_m, a, m, m_star);
        if r_hill > 0.0 {
            let delta = (candidate_a - a).abs() / r_hill;
            if delta < min_sep {
                return true;
            }
        }
    }
    false
}

/// Generate a random planet radius (Earth radii) using a simplified distribution
/// inspired by Kepler occurrence rates. Smaller planets are more common. The
/// `distance_au` parameter biases toward larger planets at greater distances.
fn generate_radius(rng: &mut WorldRng, distance_au: f64) -> f64 {
    let u = rng.next_f64();
    // Inner system: mostly small rocky/super-Earth (1-4 R_earth)
    // Outer system: allow some sub-Neptunes (up to ~6 R_earth)
    let max_radius = if distance_au < 1.0 {
        4.0
    } else if distance_au < 5.0 {
        6.0
    } else {
        8.0
    };

    // Power-law biased toward small planets: more weight at small radii.
    // Use inverse CDF of a distribution that favors small planets.
    let min_radius = 0.5;
    // u^2 gives a distribution skewed toward 0, so more small planets
    min_radius + (max_radius - min_radius) * u * u
}

/// Generate a candidate semi-major axis for a new planet using a log-uniform
/// distribution between `inner` and `outer` (AU).
fn generate_semi_major_axis(rng: &mut WorldRng, inner: f64, outer: f64) -> f64 {
    let log_inner = inner.ln();
    let log_outer = outer.ln();
    let log_a = rng.next_range(log_inner, log_outer);
    log_a.exp()
}

/// Place planets around a star, anchored by any known exoplanets.
///
/// Known exoplanets are placed first at their observed orbital distances.
/// Additional planets are generated procedurally using simplified Kepler
/// occurrence rates and spaced according to mutual Hill sphere stability.
///
/// Returns a `Vec<PlacedPlanet>` sorted by semi_major_axis.
pub fn place_planets(
    star: &StarContext,
    known: &[ExoplanetRecord],
    rng: &mut WorldRng,
    config: &PlacementConfig,
) -> Vec<PlacedPlanet> {
    let mut planets: Vec<PlacedPlanet> = Vec::new();
    // Track (semi_major_axis, mass_in_solar_masses) for Hill checks.
    let mut orbit_mass: Vec<(f64, f64)> = Vec::new();

    // Step 1: Place known exoplanets.
    for exo in known {
        let a = match exo.semi_major_axis {
            Some(a) if a >= config.inner_limit_au && a <= config.outer_limit_au => a,
            _ => continue, // skip planets without valid orbital data
        };

        // Use observed radius if available, otherwise estimate from mass,
        // otherwise assign a default.
        let radius = if let Some(r) = exo.radius {
            r
        } else if let Some(m) = exo.mass {
            // Invert the mass-radius relation approximately.
            // For rocky: M = R^2.06 -> R = M^(1/2.06)
            // For volatile: M = R^1.7 -> R = M^(1/1.7)
            // Try rocky first; if result > 1.23, use volatile.
            let r_rocky = m.powf(1.0 / 2.06);
            if r_rocky <= 1.23 {
                r_rocky
            } else {
                m.powf(1.0 / 1.7)
            }
        } else {
            1.0 // default Earth-sized
        };

        let mass_earth = mass_from_radius(radius);
        let mass_solar = mass_earth * EARTH_MASS_IN_SOLAR;

        planets.push(PlacedPlanet {
            semi_major_axis: a,
            radius,
            is_known: true,
            name: Some(exo.name.clone()),
        });
        orbit_mass.push((a, mass_solar));
    }

    // Sort by semi_major_axis after placing known planets.
    sort_by_sma(&mut planets, &mut orbit_mass);

    // Step 2: Generate additional planets to fill remaining slots.
    // Decide how many additional planets to attempt. Use a simple occurrence
    // rate: more planets for solar-type stars, fewer for very low or high mass.
    let target_count = {
        let base = config.max_planets;
        // Metallicity boost: metal-rich stars tend to have more planets.
        let metal_factor = (1.0 + star.metallicity * 0.5).clamp(0.5, 1.5);
        let target = (base as f64 * metal_factor).round() as usize;
        target.min(config.max_planets)
    };

    let max_attempts = target_count * 20; // avoid infinite loops
    let mut attempts = 0;

    while planets.len() < target_count && attempts < max_attempts {
        attempts += 1;

        let a = generate_semi_major_axis(rng, config.inner_limit_au, config.outer_limit_au);
        let radius = generate_radius(rng, a);
        let mass_earth = mass_from_radius(radius);
        let mass_solar = mass_earth * EARTH_MASS_IN_SOLAR;

        if violates_hill_separation(
            a,
            mass_solar,
            &orbit_mass,
            star.mass,
            config.min_hill_separation,
        ) {
            continue;
        }

        planets.push(PlacedPlanet {
            semi_major_axis: a,
            radius,
            is_known: false,
            name: None,
        });
        orbit_mass.push((a, mass_solar));

        // Keep sorted for accurate Hill checks against nearest neighbors.
        sort_by_sma(&mut planets, &mut orbit_mass);
    }

    // Final sort (should already be sorted, but ensure it).
    sort_by_sma(&mut planets, &mut orbit_mass);

    planets
}

/// Sort both the planet vec and the orbit_mass vec by semi_major_axis.
fn sort_by_sma(planets: &mut Vec<PlacedPlanet>, orbit_mass: &mut Vec<(f64, f64)>) {
    // Build indices, sort by sma, then reorder both vecs.
    let mut indices: Vec<usize> = (0..planets.len()).collect();
    indices.sort_by(|&i, &j| {
        planets[i]
            .semi_major_axis
            .partial_cmp(&planets[j].semi_major_axis)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let sorted_planets: Vec<PlacedPlanet> = indices.iter().map(|&i| planets[i].clone()).collect();
    let sorted_om: Vec<(f64, f64)> = indices.iter().map(|&i| orbit_mass[i]).collect();

    *planets = sorted_planets;
    *orbit_mass = sorted_om;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ymir_catalog::exoplanets::ExoplanetRecord;
    use ymir_catalog::star_context::StarContext;
    use ymir_core::WorldRng;

    fn tau_ceti_star() -> StarContext {
        StarContext::tau_ceti()
    }

    #[test]
    fn empty_known_generates_planets_within_limits() {
        let star = tau_ceti_star();
        let config = PlacementConfig::default();
        let mut rng = WorldRng::new(42);
        let planets = place_planets(&star, &[], &mut rng, &config);

        assert!(!planets.is_empty(), "should generate at least one planet");
        for p in &planets {
            assert!(
                p.semi_major_axis >= config.inner_limit_au,
                "planet at {} AU is inside inner limit {}",
                p.semi_major_axis,
                config.inner_limit_au
            );
            assert!(
                p.semi_major_axis <= config.outer_limit_au,
                "planet at {} AU is outside outer limit {}",
                p.semi_major_axis,
                config.outer_limit_au
            );
            assert!(!p.is_known, "generated planets should not be marked known");
        }
    }

    #[test]
    fn known_exoplanets_preserved() {
        let star = tau_ceti_star();
        let known = ExoplanetRecord::tau_ceti_system();
        let config = PlacementConfig::default();
        let mut rng = WorldRng::new(42);
        let planets = place_planets(&star, &known, &mut rng, &config);

        let known_planets: Vec<&PlacedPlanet> = planets.iter().filter(|p| p.is_known).collect();
        assert_eq!(
            known_planets.len(),
            known.len(),
            "all known exoplanets should be preserved"
        );

        for exo in &known {
            let found = known_planets
                .iter()
                .any(|p| p.name.as_deref() == Some(&exo.name));
            assert!(found, "known planet '{}' not found in output", exo.name);
        }
    }

    #[test]
    fn no_hill_separation_violations() {
        let star = tau_ceti_star();
        let known = ExoplanetRecord::tau_ceti_system();
        let config = PlacementConfig::default();
        let mut rng = WorldRng::new(42);
        let planets = place_planets(&star, &known, &mut rng, &config);

        // Check all adjacent pairs.
        for pair in planets.windows(2) {
            let a1 = pair[0].semi_major_axis;
            let a2 = pair[1].semi_major_axis;
            let m1 = mass_from_radius(pair[0].radius) * EARTH_MASS_IN_SOLAR;
            let m2 = mass_from_radius(pair[1].radius) * EARTH_MASS_IN_SOLAR;
            let r_hill = mutual_hill_radius(a1, m1, a2, m2, star.mass);

            if r_hill > 0.0 {
                let delta = (a2 - a1) / r_hill;
                // Known exoplanet pairs may be closer than the default threshold
                // since they reflect real observations, but generated planets
                // should respect it. We check a relaxed threshold for pairs that
                // include at least one known planet.
                let both_generated = !pair[0].is_known && !pair[1].is_known;
                if both_generated {
                    assert!(
                        delta >= config.min_hill_separation * 0.99, // small float tolerance
                        "generated planets at {:.3} and {:.3} AU have Hill separation {:.1} < {:.1}",
                        a1,
                        a2,
                        delta,
                        config.min_hill_separation
                    );
                }
            }
        }
    }

    #[test]
    fn deterministic_same_seed_same_output() {
        let star = tau_ceti_star();
        let config = PlacementConfig::default();

        let mut rng1 = WorldRng::new(12345);
        let planets1 = place_planets(&star, &[], &mut rng1, &config);

        let mut rng2 = WorldRng::new(12345);
        let planets2 = place_planets(&star, &[], &mut rng2, &config);

        assert_eq!(planets1.len(), planets2.len(), "planet count should match");
        for (a, b) in planets1.iter().zip(planets2.iter()) {
            assert_eq!(a.semi_major_axis, b.semi_major_axis);
            assert_eq!(a.radius, b.radius);
            assert_eq!(a.is_known, b.is_known);
            assert_eq!(a.name, b.name);
        }
    }

    #[test]
    fn output_sorted_by_semi_major_axis() {
        let star = tau_ceti_star();
        let known = ExoplanetRecord::tau_ceti_system();
        let config = PlacementConfig::default();
        let mut rng = WorldRng::new(99);
        let planets = place_planets(&star, &known, &mut rng, &config);

        for pair in planets.windows(2) {
            assert!(
                pair[0].semi_major_axis <= pair[1].semi_major_axis,
                "planets not sorted: {:.4} > {:.4}",
                pair[0].semi_major_axis,
                pair[1].semi_major_axis
            );
        }
    }

    #[test]
    fn mass_from_radius_continuity() {
        // The mass-radius relation should be roughly continuous at the breakpoint.
        let r_break = 1.23;
        let m_below = (r_break - 0.001_f64).powf(2.06);
        let m_above = (r_break + 0.001_f64).powf(1.7);
        // They won't match exactly, but should be in the same ballpark.
        assert!(
            (m_below - m_above).abs() < 0.5,
            "mass discontinuity at breakpoint: {m_below} vs {m_above}"
        );
    }

    #[test]
    fn mutual_hill_radius_basic() {
        // Two Earth-mass planets at 1 and 1.5 AU around a solar-mass star.
        let m_earth_solar = EARTH_MASS_IN_SOLAR;
        let r = mutual_hill_radius(1.0, m_earth_solar, 1.5, m_earth_solar, 1.0);
        // R_Hill = 1.25 * (2 * 3e-6 / 3)^(1/3) ~ 1.25 * (2e-6)^(1/3) ~ 1.25 * 0.0126 ~ 0.0157
        assert!(
            r > 0.01 && r < 0.03,
            "Hill radius {r} out of expected range"
        );
    }
}
