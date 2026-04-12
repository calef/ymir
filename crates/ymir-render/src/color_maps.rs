//! Color map definitions for mapping scalar fields (elevation, temperature,
//! moisture) to pixel colors.
//!
//! Phase 1 ships a single elevation colormap built from a small set of anchor
//! stops with piecewise-linear interpolation in RGB space.

/// Anchor stops for the elevation colormap, in (meters, RGB) form. The list
/// must remain sorted by ascending elevation; values outside the range clamp
/// to the nearest endpoint.
const COLOR_STOPS: &[(f64, [u8; 3])] = &[
    (-4000.0, [10, 20, 60]),   // ocean deep
    (0.0, [100, 180, 220]),    // ocean shallow / sea level
    (100.0, [220, 200, 140]),  // coastal beige
    (500.0, [80, 140, 60]),    // lowland green
    (2000.0, [120, 90, 50]),   // highland brown
    (5000.0, [160, 160, 160]), // mountain gray
    (9000.0, [240, 240, 240]), // peak white
];

/// Map an elevation in meters to an RGB triple using the Phase 1 hypsometric
/// palette: dark navy at abyssal depths through navy/light-blue ocean to a
/// beige coast, green lowlands, brown highlands, gray mountains, and white
/// peaks. Values outside the anchor range clamp to the nearest endpoint.
pub fn elevation_to_rgb(elevation_m: f64) -> [u8; 3] {
    if elevation_m.is_nan() {
        return COLOR_STOPS[0].1;
    }

    // Below the lowest stop: clamp to the deepest color.
    if elevation_m <= COLOR_STOPS[0].0 {
        return COLOR_STOPS[0].1;
    }
    // Above the highest stop: clamp to the peak color.
    let last = *COLOR_STOPS.last().expect("at least one stop");
    if elevation_m >= last.0 {
        return last.1;
    }

    // Find the bracketing pair.
    for window in COLOR_STOPS.windows(2) {
        let (e_lo, c_lo) = window[0];
        let (e_hi, c_hi) = window[1];
        if elevation_m >= e_lo && elevation_m <= e_hi {
            let t = (elevation_m - e_lo) / (e_hi - e_lo);
            return lerp_rgb(c_lo, c_hi, t);
        }
    }

    // Unreachable given the clamps above.
    last.1
}

fn lerp_rgb(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        lerp_u8(a[0], b[0], t),
        lerp_u8(a[1], b[1], t),
        lerp_u8(a[2], b[2], t),
    ]
}

fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
    let af = a as f64;
    let bf = b as f64;
    (af + (bf - af) * t).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extremes_clamp_to_endpoints() {
        assert_eq!(elevation_to_rgb(-100_000.0), [10, 20, 60]);
        assert_eq!(elevation_to_rgb(100_000.0), [240, 240, 240]);
    }

    #[test]
    fn anchor_points_match_exactly() {
        for &(e, c) in COLOR_STOPS {
            assert_eq!(elevation_to_rgb(e), c, "anchor at {e} m");
        }
    }

    #[test]
    fn ocean_floor_color() {
        // Well below the deep-ocean anchor clamps to dark navy.
        assert_eq!(elevation_to_rgb(-10_000.0), [10, 20, 60]);
    }

    #[test]
    fn sea_level_is_shallow_blue() {
        assert_eq!(elevation_to_rgb(0.0), [100, 180, 220]);
    }

    #[test]
    fn interpolation_is_within_endpoint_box() {
        // For each segment, sample an interior point and verify each channel
        // lies between the segment endpoints (no overshoot / no reversal).
        for window in COLOR_STOPS.windows(2) {
            let (e_lo, c_lo) = window[0];
            let (e_hi, c_hi) = window[1];
            let mid = 0.5 * (e_lo + e_hi);
            let c = elevation_to_rgb(mid);
            for ch in 0..3 {
                let lo = c_lo[ch].min(c_hi[ch]);
                let hi = c_lo[ch].max(c_hi[ch]);
                assert!(
                    c[ch] >= lo && c[ch] <= hi,
                    "channel {ch} out of [{lo}, {hi}] at {mid} m: {}",
                    c[ch]
                );
            }
        }
    }

    #[test]
    fn smoothness_at_segment_boundaries() {
        // Values just below and just above each anchor should be very close
        // to the anchor itself (the two adjacent ramps both terminate there).
        for &(e, c) in COLOR_STOPS {
            for delta in [-0.5, 0.5] {
                let probe = elevation_to_rgb(e + delta);
                for ch in 0..3 {
                    let diff = (probe[ch] as i32 - c[ch] as i32).abs();
                    assert!(
                        diff <= 2,
                        "channel {ch} jumped by {diff} at anchor {e}: {} vs {}",
                        probe[ch],
                        c[ch]
                    );
                }
            }
        }
    }

    #[test]
    fn nan_returns_deepest_color() {
        assert_eq!(elevation_to_rgb(f64::NAN), [10, 20, 60]);
    }
}
