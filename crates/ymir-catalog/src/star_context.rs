//! The `StarContext` type representing a fully characterized star with its
//! spectral class, luminosity, metallicity, and known planetary companions.
//!
//! Includes habitable zone calculations using the Kopparapu et al. (2013, 2014)
//! parametric model.

use serde::{Deserialize, Serialize};
use std::fmt;
use ymir_core::Sourced;

use crate::catalog_index::StarInfo;
use crate::exoplanet::ExoplanetRecord;

/// Gaia DR3 release date used to tag observed catalog-derived values.
/// The third data release was published on 2022-06-13.
pub const GAIA_DR3_RELEASE_DATE: &str = "2022-06-13";

/// Tau Ceti stellar-parameter reference used for [`Sourced::observed`] tags.
const TAU_CETI_REF: &str = "Teixeira+ 2009; Tuomi+ 2013";
const TAU_CETI_INSTRUMENT: &str = "HARPS + UCLES";

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
/// from the input catalog measurements. Every scalar field is wrapped in
/// [`Sourced<f64>`] so downstream stages and provenance reports can tell
/// observed catalog values apart from derived, assumed, or overridden ones.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StarContext {
    /// Catalog identifier, e.g. "Gaia DR3 4472832130942575872"
    pub catalog_id: String,
    /// Common name if known, e.g. "Tau Ceti"
    pub name: Option<String>,
    /// Spectral classification
    pub spectral_type: SpectralType,

    // Catalog measurements with per-field provenance.
    /// Effective temperature in Kelvin
    pub effective_temp: Sourced<f64>,
    /// Luminosity in solar luminosities
    pub luminosity: Sourced<f64>,
    /// Metallicity as [Fe/H]
    pub metallicity: Sourced<f64>,
    /// Mass in solar masses
    pub mass: Sourced<f64>,
    /// Radius in solar radii
    pub radius: Sourced<f64>,
    /// Estimated age in Gyr
    pub age: Sourced<f64>,
    /// Distance from Sol in parsecs
    pub distance: Sourced<f64>,

    // Derived habitable zone boundaries (AU)
    /// Conservative inner HZ edge (moist greenhouse), AU
    pub hz_inner: Sourced<f64>,
    /// Conservative outer HZ edge (maximum greenhouse), AU
    pub hz_outer: Sourced<f64>,
    /// Optimistic inner HZ edge (recent Venus), AU
    pub hz_inner_optimistic: Sourced<f64>,
    /// Optimistic outer HZ edge (early Mars), AU
    pub hz_outer_optimistic: Sourced<f64>,
    /// UV flux at HZ midpoint in relative solar units
    pub uv_flux_hz: Sourced<f64>,

    /// Known exoplanets from the NASA Exoplanet Archive (populated by CAT-02)
    pub known_exoplanets: Vec<String>,
}

/// Default stage tag written into [`ymir_core::Source::Derived`] for all derived HZ
/// bounds and the UV flux estimate.
const STAR_CTX_STAGE: &str = "stellar_context";

impl StarContext {
    /// Construct a `StarContext` from raw catalog measurements.
    ///
    /// Computes all derived fields (habitable zone boundaries, UV flux
    /// estimate). Every input scalar is tagged [`ymir_core::Source::Assumed`] with a
    /// `"StarContext::from_params hand-filled parameter"` rationale because
    /// the raw-f64 entry point has no catalog row to cite; real observed
    /// values flow through [`Self::from_catalog`] or [`Self::from_sun`]. All
    /// HZ / UV derived fields are tagged [`ymir_core::Source::Derived`] from the
    /// `"stellar_context"` stage.
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
        let assumed = "StarContext::from_params hand-filled parameter";
        Self::from_sourced(
            catalog_id,
            name,
            spectral_type,
            Sourced::assumed(effective_temp, assumed),
            Sourced::assumed(luminosity, assumed),
            Sourced::assumed(metallicity, assumed),
            Sourced::assumed(mass, assumed),
            Sourced::assumed(radius, assumed),
            Sourced::assumed(age, assumed),
            Sourced::assumed(distance, assumed),
        )
    }

    /// Construct a `StarContext` from already-tagged [`Sourced<f64>`] inputs.
    ///
    /// This is the per-field entry point used by [`Self::from_catalog`] and
    /// [`Self::from_sun`]. Derived HZ / UV fields are recomputed from the
    /// inner `f64` values and tagged [`ymir_core::Source::Derived`].
    #[allow(clippy::too_many_arguments)]
    pub fn from_sourced(
        catalog_id: impl Into<String>,
        name: Option<String>,
        spectral_type: SpectralType,
        effective_temp: Sourced<f64>,
        luminosity: Sourced<f64>,
        metallicity: Sourced<f64>,
        mass: Sourced<f64>,
        radius: Sourced<f64>,
        age: Sourced<f64>,
        distance: Sourced<f64>,
    ) -> Self {
        let t_eff = *effective_temp.inner();
        let l_sun = *luminosity.inner();
        let hz_inner_raw = flux_to_au(l_sun, s_eff(&MOIST_GREENHOUSE, t_eff));
        let hz_outer_raw = flux_to_au(l_sun, s_eff(&MAX_GREENHOUSE, t_eff));
        let hz_inner_optimistic_raw = flux_to_au(l_sun, s_eff(&RECENT_VENUS, t_eff));
        let hz_outer_optimistic_raw = flux_to_au(l_sun, s_eff(&EARLY_MARS, t_eff));

        // Rough UV flux estimate at HZ midpoint, scaled relative to Sol.
        // UV output scales roughly as T_eff^4 (Stefan-Boltzmann), and flux falls
        // as 1/d^2. We evaluate at the midpoint of the conservative HZ.
        let hz_midpoint = (hz_inner_raw + hz_outer_raw) / 2.0;
        let uv_flux_raw = if hz_midpoint > 0.0 {
            l_sun / (hz_midpoint * hz_midpoint)
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
            hz_inner: Sourced::derived(hz_inner_raw, STAR_CTX_STAGE),
            hz_outer: Sourced::derived(hz_outer_raw, STAR_CTX_STAGE),
            hz_inner_optimistic: Sourced::derived(hz_inner_optimistic_raw, STAR_CTX_STAGE),
            hz_outer_optimistic: Sourced::derived(hz_outer_optimistic_raw, STAR_CTX_STAGE),
            uv_flux_hz: Sourced::derived(uv_flux_raw, STAR_CTX_STAGE),
            known_exoplanets: Vec::new(),
        }
    }

    /// Build a [`StarContext`] from a real Gaia DR3 catalog row joined against
    /// the NASA Exoplanet Archive.
    ///
    /// Uses the indexed spectral class / subtype from [`StarInfo`] for the
    /// Harvard classification and tags `effective_temp`, `luminosity`, and
    /// `distance` as [`ymir_core::Source::Observed`] citing Gaia DR3. Fields Gaia does
    /// not provide (metallicity, mass, radius, age) use sensible
    /// [`ymir_core::Source::Assumed`] defaults; a future pass can thread through the
    /// FLAME nullable columns if real observational values are available.
    pub fn from_catalog(info: &StarInfo, planets: &[ExoplanetRecord]) -> Self {
        let spectral_type = SpectralType {
            class: info.spectral.class.clone(),
            subtype: info.spectral.subtype,
            luminosity_class: "V".to_string(),
        };
        let catalog_id = format!("Gaia DR3 {}", info.gaia_id);
        let common_name = planets.first().map(|p| p.host_name.clone());

        // Phase-1 fallbacks for fields not carried by the current Gaia
        // index. Mean-main-sequence values scaled by class; intentionally
        // generic because the Kopparapu HZ math below only reads the
        // T_eff + luminosity, both of which ARE observed.
        let no_gaia = "Gaia DR3 index does not expose this field";

        let mut ctx = Self::from_sourced(
            catalog_id,
            common_name,
            spectral_type,
            Sourced::observed_on(
                info.teff_k,
                "Gaia DR3 gspphot",
                "Gaia BP/RP",
                GAIA_DR3_RELEASE_DATE,
            ),
            Sourced::observed_on(
                info.luminosity_sun,
                "Gaia DR3 FLAME",
                "Gaia BP/RP + astrometry",
                GAIA_DR3_RELEASE_DATE,
            ),
            Sourced::assumed(0.0, no_gaia),
            Sourced::assumed(1.0, no_gaia),
            Sourced::assumed(1.0, no_gaia),
            Sourced::assumed(5.0, no_gaia),
            Sourced::observed_on(
                info.distance_pc,
                "Gaia DR3 parallax",
                "Gaia astrometry",
                GAIA_DR3_RELEASE_DATE,
            ),
        );

        ctx.known_exoplanets = planets
            .iter()
            .map(|p| {
                if p.planet_letter.is_empty() {
                    p.host_name.clone()
                } else {
                    format!("{} {}", p.host_name, p.planet_letter)
                }
            })
            .collect();

        ctx
    }

    /// Synthesized [`StarContext`] for the Sun.
    ///
    /// Gaia DR3 does not contain a row for the Sun (Gaia cannot observe
    /// its own reference frame). When the Catalog facade resolves "Sol"
    /// or "Sun" it returns this record instead, tagged with
    /// [`ymir_core::Source::Observed`] referencing solar constants from IAU 2015
    /// Resolution B3 (nominal solar values).
    pub fn from_sun() -> Self {
        let iau = "IAU 2015 Resolution B3";
        let iau_instr = "nominal solar constants";
        let iau_date = "2015-08-13";
        Self::from_sourced(
            "Sol",
            Some("Sol".to_string()),
            SpectralType {
                class: SpectralClass::G,
                subtype: 2,
                luminosity_class: "V".to_string(),
            },
            Sourced::observed_on(5778.0, iau, iau_instr, iau_date),
            Sourced::observed_on(1.0, iau, iau_instr, iau_date),
            Sourced::observed_on(0.0, iau, iau_instr, iau_date),
            Sourced::observed_on(1.0, iau, iau_instr, iau_date),
            Sourced::observed_on(1.0, iau, iau_instr, iau_date),
            Sourced::observed_on(
                4.6,
                "Bonanno & Fröhlich 2015",
                "helioseismology + isochrones",
                "2015-01-01",
            ),
            Sourced::observed_on(
                0.0,
                "Heliocentric reference frame",
                "definitional",
                iau_date,
            ),
        )
    }

    /// Factory method returning a `StarContext` for Tau Ceti (HD 10700) using
    /// well-established observational values.
    pub fn tau_ceti() -> Self {
        Self::from_sourced(
            "HD 10700",
            Some("Tau Ceti".to_string()),
            SpectralType {
                class: SpectralClass::G,
                subtype: 8,
                luminosity_class: "V".to_string(),
            },
            Sourced::observed(5344.0, TAU_CETI_REF, TAU_CETI_INSTRUMENT),
            Sourced::observed(0.488, TAU_CETI_REF, TAU_CETI_INSTRUMENT),
            Sourced::observed(-0.55, TAU_CETI_REF, TAU_CETI_INSTRUMENT),
            Sourced::observed(0.783, TAU_CETI_REF, TAU_CETI_INSTRUMENT),
            Sourced::observed(0.793, TAU_CETI_REF, TAU_CETI_INSTRUMENT),
            Sourced::observed(5.8, TAU_CETI_REF, TAU_CETI_INSTRUMENT),
            Sourced::observed(
                3.65,
                "Hipparcos parallax + Gaia DR2 refinement",
                "Hipparcos astrometry",
            ),
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
            (*sol.hz_inner.inner() - 0.99).abs() < 0.02,
            "Sun conservative inner HZ should be ~0.99 AU, got {}",
            *sol.hz_inner.inner()
        );
    }

    #[test]
    fn sun_hz_conservative_outer() {
        let sol = sun();
        // For the Sun, max greenhouse S_eff_sun = 0.3438, so d = sqrt(1/0.3438) ~ 1.705 AU
        assert!(
            (*sol.hz_outer.inner() - 1.70).abs() < 0.05,
            "Sun conservative outer HZ should be ~1.70 AU, got {}",
            *sol.hz_outer.inner()
        );
    }

    #[test]
    fn tau_ceti_hz_sanity() {
        let tc = StarContext::tau_ceti();
        let hi = *tc.hz_inner.inner();
        let ho = *tc.hz_outer.inner();
        assert!(hi > 0.0, "HZ inner must be positive");
        assert!(ho > 0.0, "HZ outer must be positive");
        assert!(hi < ho, "inner ({hi}) must be less than outer ({ho})");
        // Tau Ceti is a G8V star with L=0.488; HZ should be roughly 0.5-1.2 AU range
        assert!(
            hi > 0.4 && hi < 1.0,
            "Tau Ceti inner HZ looks unreasonable: {hi}"
        );
        assert!(
            ho > 0.8 && ho < 1.5,
            "Tau Ceti outer HZ looks unreasonable: {ho}"
        );
    }

    #[test]
    fn tau_ceti_optimistic_wider_than_conservative() {
        let tc = StarContext::tau_ceti();
        assert!(
            *tc.hz_inner_optimistic.inner() < *tc.hz_inner.inner(),
            "optimistic inner should be closer to star than conservative inner"
        );
        assert!(
            *tc.hz_outer_optimistic.inner() > *tc.hz_outer.inner(),
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
            let hi = *star.hz_inner.inner();
            let ho = *star.hz_outer.inner();
            assert!(hi < ho, "At T_eff={t_eff}: inner ({hi}) >= outer ({ho})");
            let hio = *star.hz_inner_optimistic.inner();
            let hoo = *star.hz_outer_optimistic.inner();
            assert!(
                hio < hoo,
                "At T_eff={t_eff}: optimistic inner ({hio}) >= optimistic outer ({hoo})"
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

    // ------------------------------------------------------------------
    // CORE-06 per-field provenance tests
    // ------------------------------------------------------------------

    #[test]
    fn core06_from_params_tags_every_field() {
        // Every overridable field on a from_params StarContext must carry a
        // non-default Source (Assumed for inputs, Derived for HZ/UV).
        let sol = sun();
        assert!(sol.effective_temp.is_assumed());
        assert!(sol.luminosity.is_assumed());
        assert!(sol.metallicity.is_assumed());
        assert!(sol.mass.is_assumed());
        assert!(sol.radius.is_assumed());
        assert!(sol.age.is_assumed());
        assert!(sol.distance.is_assumed());
        assert!(sol.hz_inner.is_derived());
        assert!(sol.hz_outer.is_derived());
        assert!(sol.hz_inner_optimistic.is_derived());
        assert!(sol.hz_outer_optimistic.is_derived());
        assert!(sol.uv_flux_hz.is_derived());
    }

    #[test]
    fn core06_from_sun_tags_every_scalar_observed() {
        let sun = StarContext::from_sun();
        assert!(sun.effective_temp.is_observed());
        assert!(sun.luminosity.is_observed());
        assert!(sun.metallicity.is_observed());
        assert!(sun.mass.is_observed());
        assert!(sun.radius.is_observed());
        assert!(sun.age.is_observed());
        assert!(sun.distance.is_observed());
        assert!(sun.hz_inner.is_derived());
        assert!(sun.hz_outer.is_derived());
    }

    #[test]
    fn core06_tau_ceti_tags_every_scalar_observed() {
        let tc = StarContext::tau_ceti();
        assert!(tc.effective_temp.is_observed());
        assert!(tc.luminosity.is_observed());
        assert!(tc.metallicity.is_observed());
        assert!(tc.mass.is_observed());
        assert!(tc.radius.is_observed());
        assert!(tc.age.is_observed());
        assert!(tc.distance.is_observed());
        assert!(tc.hz_inner.is_derived());
    }
}
