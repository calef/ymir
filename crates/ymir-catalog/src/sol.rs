//! Hardcoded Sol (Sun) stellar catalog data for Phase 2 validation.
//!
//! Provides a [`StarContext`] matching the Sun's published parameters so
//! downstream stages can be validated against known Earth-like and Mars-like
//! conditions. Used by the CLI when the user passes `--star Earth`, `--star
//! Sol`, `--star Sun`, or `--star Mars`.
//!
//! Habitable zone boundaries are computed by [`StarContext::from_params`]
//! using the Kopparapu et al. (2013) parametric model. For T_eff = 5778 K and
//! L = 1.0 L_sun this produces conservative HZ bounds of roughly 0.99-1.70 AU,
//! consistent with published values.

use crate::star_context::{SpectralClass, SpectralType, StarContext};

/// Factory returning a [`StarContext`] for the Sun using well-established
/// values. Distance is 0 pc (heliocentric reference frame).
///
/// Catalog ID uses the placeholder "HD 0" since the Sun has no Henry Draper
/// number.
pub fn sol_context() -> StarContext {
    StarContext::from_params(
        "HD 0",
        Some("Sol".to_string()),
        SpectralType {
            class: SpectralClass::G,
            subtype: 2,
            luminosity_class: "V".to_string(),
        },
        5778.0, // T_eff (K)
        1.0,    // luminosity (L_sun)
        0.0,    // metallicity [Fe/H]
        1.0,    // mass (M_sun)
        1.0,    // radius (R_sun)
        4.6,    // age (Gyr)
        0.0,    // distance (pc); heliocentric reference
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sol_context_basic_fields() {
        let sol = sol_context();
        assert_eq!(sol.catalog_id, "HD 0");
        assert_eq!(sol.name.as_deref(), Some("Sol"));
        assert_eq!(sol.spectral_type.to_string(), "G2V");
        assert!((sol.effective_temp - 5778.0).abs() < 1e-9);
        assert!((sol.luminosity - 1.0).abs() < 1e-9);
        assert!((sol.metallicity - 0.0).abs() < 1e-9);
        assert!((sol.mass - 1.0).abs() < 1e-9);
        assert!((sol.radius - 1.0).abs() < 1e-9);
        assert!((sol.age - 4.6).abs() < 1e-9);
        assert!((sol.distance - 0.0).abs() < 1e-9);
    }

    #[test]
    fn sol_hz_brackets_earth() {
        // Earth's semi-major axis (1.0 AU) must fall inside the conservative HZ.
        let sol = sol_context();
        let hz_inner = *sol.hz_inner.inner();
        let hz_outer = *sol.hz_outer.inner();
        assert!(
            hz_inner < 1.0,
            "HZ inner ({hz_inner}) should be less than Earth SMA (1.0 AU)"
        );
        assert!(
            hz_outer > 1.0,
            "HZ outer ({hz_outer}) should be greater than Earth SMA (1.0 AU)"
        );
        // Spot-check against Kopparapu+ 2013 published values (~0.99, ~1.70).
        assert!(
            (hz_inner - 0.99).abs() < 0.03,
            "HZ inner for Sol expected ~0.99 AU, got {hz_inner}"
        );
        assert!(
            (hz_outer - 1.70).abs() < 0.05,
            "HZ outer for Sol expected ~1.70 AU, got {hz_outer}"
        );
    }

    #[test]
    fn sol_hz_inner_less_than_outer() {
        let sol = sol_context();
        assert!(*sol.hz_inner.inner() < *sol.hz_outer.inner());
        assert!(*sol.hz_inner_optimistic.inner() < *sol.hz_inner.inner());
        assert!(*sol.hz_outer_optimistic.inner() > *sol.hz_outer.inner());
    }

    #[test]
    fn sol_serde_round_trip() {
        let sol = sol_context();
        let json = serde_json::to_string(&sol).expect("serialize");
        let back: StarContext = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.catalog_id, sol.catalog_id);
        assert_eq!(back.name, sol.name);
        assert!((back.hz_inner - sol.hz_inner).abs() < 1e-12);
        assert!((back.hz_outer - sol.hz_outer).abs() < 1e-12);
    }
}
