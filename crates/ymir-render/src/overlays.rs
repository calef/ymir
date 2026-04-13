//! Compositable overlay layers for rendering additional data on top of base maps.
//!
//! The primary overlay in Phase 1 is the **confidence overlay**, which desaturates
//! pixels to visualize per-body provenance. Full saturation indicates
//! observationally-grounded values; grayscale indicates fully-derived (simulated)
//! values; partial desaturation indicates a mix.
//!
//! # Phase 1 limitation
//!
//! The skeleton, climate, and biome stages carry a single aggregate
//! [`ymir_core::Source`] tag for the entire grid (one `Derived` node covers all
//! tiles). Per-tile confidence tracking is deferred to Phase 2. The Phase 1
//! overlay therefore applies a **uniform body-level wash** to every pixel inside
//! the Mollweide ellipse, keyed to how many of the upstream per-field stages
//! (stellar, orbital_body, atmosphere) have at least one `Observed` field.

use image::RgbImage;
use ymir_core::provenance::ProvenanceReport;

/// Coarse confidence level for a body's upstream data.
///
/// Derived from the provenance report by checking whether any per-field stage
/// (stellar, orbital_body, atmosphere) contributed at least one
/// [`ymir_core::Source::Observed`] field.
///
/// In Phase 1 this maps to a **body-level** verdict applied uniformly to every
/// pixel of the rendered map. Per-tile confidence tracking is Phase 2+.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfidenceLevel {
    /// All sampled upstream fields are `Source::Assumed` or `Source::Derived`.
    /// No real observational data contributed.
    FullyDerived,
    /// Some upstream fields are `Source::Observed` but not all.
    /// Real data constrains part of the pipeline.
    Mixed,
    /// Every sampled upstream field is `Source::Observed`.
    /// The output is maximally grounded in real measurements.
    FullyObserved,
}

/// Extract a [`ConfidenceLevel`] from a [`ProvenanceReport`].
///
/// Examines the `stellar`, `orbital_body`, and `atmosphere` stages (the three
/// per-field stages that carry `{ value, source }` leaves). Returns:
///
/// - [`ConfidenceLevel::FullyObserved`] if every examined stage has at least
///   one `Observed` field and **zero** `Derived`/`Assumed` fields.
/// - [`ConfidenceLevel::Mixed`] if at least one stage has any `Observed` fields
///   alongside some `Derived` or `Assumed` fields, or if some stages are fully
///   observed while others are not.
/// - [`ConfidenceLevel::FullyDerived`] if no stage has any `Observed` fields
///   at all.
///
/// Stages not present in the report (e.g. when `--skip-climate` was used)
/// are ignored.
pub fn confidence_level_from_report(report: &ProvenanceReport) -> ConfidenceLevel {
    let perfield_stages = ["stellar", "orbital_body", "atmosphere"];

    let mut total_observed = 0usize;
    let mut total_nonobserved = 0usize;

    for stage_name in &perfield_stages {
        if let Some(sp) = report.stages.get(*stage_name) {
            total_observed += sp.counts.observed;
            total_nonobserved += sp.counts.derived + sp.counts.assumed;
        }
    }

    if total_observed == 0 {
        ConfidenceLevel::FullyDerived
    } else if total_nonobserved == 0 {
        ConfidenceLevel::FullyObserved
    } else {
        ConfidenceLevel::Mixed
    }
}

/// Saturation scale factors for each [`ConfidenceLevel`].
///
/// `1.0` = full saturation (no change), `0.0` = complete grayscale.
/// These constants determine the visual weight of the overlay.
const SAT_FULLY_OBSERVED: f32 = 1.0;
const SAT_MIXED: f32 = 0.55;
const SAT_FULLY_DERIVED: f32 = 0.15;

/// Return the saturation scale factor [0, 1] for a given [`ConfidenceLevel`].
///
/// Used internally by [`apply_confidence_overlay`] and exposed for callers that
/// need the scalar directly (e.g. GUI legend rendering).
pub fn saturation_scale(level: ConfidenceLevel) -> f32 {
    match level {
        ConfidenceLevel::FullyObserved => SAT_FULLY_OBSERVED,
        ConfidenceLevel::Mixed => SAT_MIXED,
        ConfidenceLevel::FullyDerived => SAT_FULLY_DERIVED,
    }
}

/// Desaturate a single RGB pixel by a scale factor in `[0.0, 1.0]`.
///
/// `scale = 1.0` leaves the pixel unchanged; `scale = 0.0` converts it to
/// full grayscale (luminance-weighted). Intermediate values blend between the
/// original and its grayscale version.
///
/// The luminance weights `(0.2126, 0.7152, 0.0722)` follow the ITU-R BT.709
/// standard for linear light, which is a reasonable approximation for the
/// 8-bit sRGB values stored in our PNG images.
#[inline]
pub fn desaturate_pixel(pixel: [u8; 3], scale: f32) -> [u8; 3] {
    let scale = scale.clamp(0.0, 1.0);
    if (scale - 1.0).abs() < f32::EPSILON {
        return pixel;
    }

    let r = pixel[0] as f32;
    let g = pixel[1] as f32;
    let b = pixel[2] as f32;

    // BT.709 luma
    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

    let nr = (luma + (r - luma) * scale).round().clamp(0.0, 255.0) as u8;
    let ng = (luma + (g - luma) * scale).round().clamp(0.0, 255.0) as u8;
    let nb = (luma + (b - luma) * scale).round().clamp(0.0, 255.0) as u8;

    [nr, ng, nb]
}

/// Apply the confidence overlay to an [`RgbImage`] in-place.
///
/// Every pixel is desaturated according to [`saturation_scale`] for the given
/// [`ConfidenceLevel`]. Off-map pixels (matching `off_map_color`) are left
/// unchanged so the background stays solid.
///
/// # Phase 1 limitation
///
/// This applies a **uniform wash** to every on-map pixel. Per-tile or per-hex
/// confidence gradients require per-tile provenance tracking, which is deferred
/// to Phase 2.
pub fn apply_confidence_overlay(
    img: &mut RgbImage,
    level: ConfidenceLevel,
    off_map_color: [u8; 3],
) {
    let scale = saturation_scale(level);
    if (scale - 1.0).abs() < f32::EPSILON {
        // FullyObserved: nothing to do.
        return;
    }

    for pixel in img.pixels_mut() {
        if pixel.0 == off_map_color {
            continue;
        }
        pixel.0 = desaturate_pixel(pixel.0, scale);
    }
}

/// Render a confidence overlay image from a base [`RgbImage`].
///
/// Clones `base` and applies the overlay for `level`. The `off_map_color`
/// should match the background used in the base renderer so off-map pixels
/// are left unmodified.
///
/// This is the primary entry point for callers that already have a rendered
/// base image (elevation or biome) and want to composite the confidence
/// wash on top.
pub fn render_confidence_overlay(
    base: &RgbImage,
    level: ConfidenceLevel,
    off_map_color: [u8; 3],
) -> RgbImage {
    let mut img = base.clone();
    apply_confidence_overlay(&mut img, level, off_map_color);
    img
}

/// Derive [`ConfidenceLevel`] from a report and render a confidence overlay
/// on top of `base` in one step.
///
/// Equivalent to calling [`confidence_level_from_report`] then
/// [`render_confidence_overlay`].
pub fn render_confidence_from_report(
    base: &RgbImage,
    report: &ProvenanceReport,
    off_map_color: [u8; 3],
) -> RgbImage {
    let level = confidence_level_from_report(report);
    render_confidence_overlay(base, level, off_map_color)
}

/// Sentinel off-map background color matching the biome renderer's constant.
///
/// Used as the default `off_map_color` when compositing onto a biome base image.
pub const BIOME_OFF_MAP_BG: [u8; 3] = [10, 10, 12];

/// Sentinel off-map background color matching the elevation renderer's default.
pub const ELEVATION_OFF_MAP_BG: [u8; 3] = [0, 0, 0];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;
    use ymir_core::Source;
    use ymir_core::provenance::{ProvenanceReport, StageCounts, StageProvenance};

    // Helper: build a StageProvenance with given counts.
    fn stage_prov(observed: usize, derived: usize, assumed: usize) -> StageProvenance {
        StageProvenance {
            counts: StageCounts {
                observed,
                derived,
                assumed,
            },
            fields: serde_json::Value::Null,
        }
    }

    fn fully_observed_report() -> ProvenanceReport {
        let mut r = ProvenanceReport::new();
        r.stages.insert("stellar".into(), stage_prov(7, 0, 0));
        r.stages.insert("orbital_body".into(), stage_prov(5, 0, 0));
        r.stages.insert("atmosphere".into(), stage_prov(3, 0, 0));
        r
    }

    fn fully_derived_report() -> ProvenanceReport {
        let mut r = ProvenanceReport::new();
        r.stages.insert("stellar".into(), stage_prov(0, 7, 0));
        r.stages.insert("orbital_body".into(), stage_prov(0, 5, 0));
        r.stages.insert("atmosphere".into(), stage_prov(0, 3, 0));
        r
    }

    fn mixed_report() -> ProvenanceReport {
        let mut r = ProvenanceReport::new();
        // stellar has some observed, orbital_body is fully derived
        r.stages.insert("stellar".into(), stage_prov(3, 4, 0));
        r.stages.insert("orbital_body".into(), stage_prov(0, 5, 0));
        r.stages.insert("atmosphere".into(), stage_prov(0, 3, 0));
        r
    }

    // --- confidence_level_from_report ---

    #[test]
    fn fully_observed_report_gives_fully_observed() {
        let r = fully_observed_report();
        assert_eq!(
            confidence_level_from_report(&r),
            ConfidenceLevel::FullyObserved
        );
    }

    #[test]
    fn fully_derived_report_gives_fully_derived() {
        let r = fully_derived_report();
        assert_eq!(
            confidence_level_from_report(&r),
            ConfidenceLevel::FullyDerived
        );
    }

    #[test]
    fn mixed_report_gives_mixed() {
        let r = mixed_report();
        assert_eq!(confidence_level_from_report(&r), ConfidenceLevel::Mixed);
    }

    #[test]
    fn empty_report_is_fully_derived() {
        // No stages at all: no observed fields → FullyDerived.
        let r = ProvenanceReport::new();
        assert_eq!(
            confidence_level_from_report(&r),
            ConfidenceLevel::FullyDerived
        );
    }

    #[test]
    fn report_with_only_aggregate_stages_is_fully_derived() {
        // Aggregate stages (skeleton/climate/biome) are not examined.
        let mut r = ProvenanceReport::new();
        let source = Source::Observed {
            reference: "ref".into(),
            instrument: "inst".into(),
            date: "".into(),
            uncertainty: None,
        };
        r.add_aggregate_stage("skeleton", &source);
        r.add_aggregate_stage("climate", &source);
        // No per-field stages → zero observed fields → FullyDerived.
        assert_eq!(
            confidence_level_from_report(&r),
            ConfidenceLevel::FullyDerived
        );
    }

    // --- saturation_scale ---

    #[test]
    fn saturation_scale_ordering() {
        // FullyObserved should have highest saturation.
        assert!(
            saturation_scale(ConfidenceLevel::FullyObserved)
                > saturation_scale(ConfidenceLevel::Mixed)
        );
        assert!(
            saturation_scale(ConfidenceLevel::Mixed)
                > saturation_scale(ConfidenceLevel::FullyDerived)
        );
    }

    #[test]
    fn saturation_scale_observed_is_one() {
        assert!((saturation_scale(ConfidenceLevel::FullyObserved) - 1.0).abs() < f32::EPSILON);
    }

    // --- desaturate_pixel ---

    #[test]
    fn desaturate_scale_one_is_identity() {
        let px = [100u8, 150, 200];
        assert_eq!(desaturate_pixel(px, 1.0), px);
    }

    #[test]
    fn desaturate_scale_zero_is_grayscale() {
        let px = [255u8, 0, 0]; // pure red
        let gray = desaturate_pixel(px, 0.0);
        // BT.709: luma ≈ 0.2126 * 255 ≈ 54
        assert_eq!(gray[0], gray[1]);
        assert_eq!(gray[1], gray[2]);
    }

    #[test]
    fn desaturate_gray_input_unchanged() {
        let px = [128u8, 128, 128];
        let result = desaturate_pixel(px, 0.3);
        // A neutral gray has no saturation to desaturate; all channels stay equal.
        assert_eq!(result[0], result[1]);
        assert_eq!(result[1], result[2]);
    }

    #[test]
    fn desaturate_scale_clamps_out_of_range() {
        let px = [200u8, 100, 50];
        let over = desaturate_pixel(px, 1.5);
        let under = desaturate_pixel(px, -0.5);
        // Equivalent to scale 1.0 and 0.0 respectively.
        assert_eq!(over, desaturate_pixel(px, 1.0));
        assert_eq!(under, desaturate_pixel(px, 0.0));
    }

    // --- apply_confidence_overlay ---

    #[test]
    fn overlay_fully_observed_is_noop() {
        let mut img = RgbImage::new(4, 4);
        for px in img.pixels_mut() {
            px.0 = [100, 150, 200];
        }
        let original_raw = img.as_raw().clone();
        apply_confidence_overlay(&mut img, ConfidenceLevel::FullyObserved, [0, 0, 0]);
        assert_eq!(img.as_raw(), &original_raw);
    }

    #[test]
    fn overlay_respects_off_map_color() {
        let off_map = [10u8, 10, 12];
        let on_map = [20u8, 100, 60]; // TemperateForest-ish
        let mut img = RgbImage::from_pixel(4, 4, Rgb(off_map));
        img.put_pixel(2, 2, Rgb(on_map));

        apply_confidence_overlay(&mut img, ConfidenceLevel::FullyDerived, off_map);

        // Off-map pixels must stay unchanged.
        assert_eq!(img.get_pixel(0, 0).0, off_map);
        // On-map pixel must have been desaturated.
        let result = img.get_pixel(2, 2).0;
        assert_ne!(result, on_map, "on-map pixel should have been desaturated");
    }

    #[test]
    fn overlay_fully_derived_reduces_saturation() {
        let off_map = [0u8, 0, 0];
        let on_map = [20u8, 120, 60]; // green (TemperateForest-ish)
        let mut img = RgbImage::new(2, 2);
        for px in img.pixels_mut() {
            px.0 = on_map;
        }

        apply_confidence_overlay(&mut img, ConfidenceLevel::FullyDerived, off_map);

        // After full desaturation, all channels should be much closer together.
        let result = img.get_pixel(0, 0).0;
        let [r, g, b] = result.map(|c| c as i32);
        let spread = (r - g).abs().max((g - b).abs()).max((r - b).abs());
        let orig = on_map.map(|c| c as i32);
        let orig_spread = (orig[0] - orig[1])
            .abs()
            .max((orig[1] - orig[2]).abs())
            .max((orig[0] - orig[2]).abs());
        assert!(
            spread < orig_spread,
            "desaturated spread {spread} should be less than original {orig_spread}"
        );
    }

    // --- render_confidence_overlay / render_confidence_from_report ---

    #[test]
    fn render_confidence_overlay_does_not_mutate_base() {
        let base = RgbImage::from_pixel(8, 8, Rgb([50u8, 120, 60]));
        let base_raw = base.as_raw().clone();
        let _ = render_confidence_overlay(&base, ConfidenceLevel::FullyDerived, [0, 0, 0]);
        assert_eq!(base.as_raw(), &base_raw, "base image must not be mutated");
    }

    #[test]
    fn render_confidence_from_report_matches_manual() {
        let base = RgbImage::from_pixel(8, 8, Rgb([50u8, 120, 60]));
        let report = fully_derived_report();

        let via_report = render_confidence_from_report(&base, &report, [0, 0, 0]);
        let level = confidence_level_from_report(&report);
        let manual = render_confidence_overlay(&base, level, [0, 0, 0]);

        assert_eq!(via_report.as_raw(), manual.as_raw());
    }

    #[test]
    fn fully_observed_overlay_produces_identical_image() {
        let base = RgbImage::from_pixel(8, 8, Rgb([50u8, 120, 60]));
        let base_raw = base.as_raw().clone();
        let report = fully_observed_report();
        let result = render_confidence_from_report(&base, &report, [0, 0, 0]);
        assert_eq!(
            result.as_raw(),
            &base_raw,
            "fully-observed overlay should leave image unchanged"
        );
    }

    #[test]
    fn fully_derived_overlay_produces_desaturated_image() {
        let on_map = [20u8, 120, 60];
        let off_map = [10u8, 10, 12];
        let mut base = RgbImage::from_pixel(8, 8, Rgb(on_map));
        // Set corners to off-map color.
        base.put_pixel(0, 0, Rgb(off_map));
        base.put_pixel(7, 0, Rgb(off_map));
        base.put_pixel(0, 7, Rgb(off_map));
        base.put_pixel(7, 7, Rgb(off_map));

        let report = fully_derived_report();
        let result = render_confidence_from_report(&base, &report, off_map);

        // Interior (on-map) pixels should be desaturated.
        let [r, g, b] = result.get_pixel(4, 4).0.map(|c| c as i32);
        let spread = (r - g).abs().max((g - b).abs()).max((r - b).abs());
        let orig = on_map.map(|c| c as i32);
        let orig_spread = (orig[0] - orig[1])
            .abs()
            .max((orig[1] - orig[2]).abs())
            .max((orig[0] - orig[2]).abs());
        assert!(spread < orig_spread);

        // Corner (off-map) pixels must be unchanged.
        assert_eq!(result.get_pixel(0, 0).0, off_map);
        assert_eq!(result.get_pixel(7, 7).0, off_map);
    }
}
