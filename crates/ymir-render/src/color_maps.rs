//! Color map definitions for mapping scalar fields (elevation, temperature,
//! moisture) to pixel colors.
//!
//! Phase 1 ships three colormaps — elevation, temperature, and moisture —
//! each built from a small set of anchor stops with piecewise-linear
//! interpolation in RGB space.

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

/// Anchor stops for the temperature colormap, in (Kelvin, RGB) form.
///
/// Covers the range roughly from frozen (180 K) to scorching (330 K).
/// Colours progress from polar blue-white through cool blue, mild green,
/// warm tan, and up to hot orange-red.
const TEMP_STOPS: &[(f64, [u8; 3])] = &[
    (180.0, [200, 220, 255]), // polar ice / cryo
    (220.0, [100, 150, 220]), // sub-freezing cold
    (260.0, [60, 120, 180]),  // cool (near-freezing)
    (280.0, [80, 180, 100]),  // temperate mild
    (295.0, [200, 190, 90]),  // warm subtropical
    (310.0, [230, 120, 40]),  // hot tropics
    (330.0, [200, 40, 20]),   // extreme heat
];

/// Map a surface temperature in Kelvin to an RGB triple using a thermal
/// colour ramp: polar blue-white through temperate green to tropical orange-red.
/// Values outside the anchor range clamp to the nearest endpoint.
pub fn temperature_to_rgb(temp_k: f64) -> [u8; 3] {
    if temp_k.is_nan() {
        return TEMP_STOPS[0].1;
    }
    if temp_k <= TEMP_STOPS[0].0 {
        return TEMP_STOPS[0].1;
    }
    let last = *TEMP_STOPS.last().expect("at least one stop");
    if temp_k >= last.0 {
        return last.1;
    }
    for window in TEMP_STOPS.windows(2) {
        let (t_lo, c_lo) = window[0];
        let (t_hi, c_hi) = window[1];
        if temp_k >= t_lo && temp_k <= t_hi {
            let frac = (temp_k - t_lo) / (t_hi - t_lo);
            return lerp_rgb(c_lo, c_hi, frac);
        }
    }
    last.1
}

/// Anchor stops for the moisture colormap, in (humidity index [0,1], RGB) form.
///
/// Dry is sandy yellow-orange; humid is deep blue-green.
const MOISTURE_STOPS: &[(f64, [u8; 3])] = &[
    (0.0, [220, 180, 80]),  // arid desert
    (0.2, [180, 160, 60]),  // semi-arid
    (0.4, [100, 170, 80]),  // sub-humid
    (0.65, [40, 150, 90]),  // humid
    (0.85, [20, 120, 140]), // very humid / tropical
    (1.0, [10, 60, 160]),   // saturated / coastal ocean
];

/// Map a tile moisture index in `[0, 1]` to an RGB triple using a green-blue
/// ramp: sandy yellow for arid through green for sub-humid and deep blue for
/// saturated/ocean tiles. Values outside `[0, 1]` clamp to the endpoints.
pub fn moisture_to_rgb(moisture: f64) -> [u8; 3] {
    if moisture.is_nan() {
        return MOISTURE_STOPS[0].1;
    }
    if moisture <= MOISTURE_STOPS[0].0 {
        return MOISTURE_STOPS[0].1;
    }
    let last = *MOISTURE_STOPS.last().expect("at least one stop");
    if moisture >= last.0 {
        return last.1;
    }
    for window in MOISTURE_STOPS.windows(2) {
        let (m_lo, c_lo) = window[0];
        let (m_hi, c_hi) = window[1];
        if moisture >= m_lo && moisture <= m_hi {
            let frac = (moisture - m_lo) / (m_hi - m_lo);
            return lerp_rgb(c_lo, c_hi, frac);
        }
    }
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

    // --- temperature_to_rgb ---

    #[test]
    fn temperature_extremes_clamp() {
        assert_eq!(temperature_to_rgb(0.0), TEMP_STOPS[0].1);
        assert_eq!(temperature_to_rgb(1000.0), TEMP_STOPS.last().unwrap().1);
    }

    #[test]
    fn temperature_anchor_points_match() {
        for &(t, c) in TEMP_STOPS {
            assert_eq!(temperature_to_rgb(t), c, "anchor at {t} K");
        }
    }

    #[test]
    fn temperature_nan_returns_first_stop() {
        assert_eq!(temperature_to_rgb(f64::NAN), TEMP_STOPS[0].1);
    }

    #[test]
    fn temperature_cold_is_bluer_than_hot() {
        let cold = temperature_to_rgb(210.0);
        let hot = temperature_to_rgb(320.0);
        // Cold end: blue channel should dominate; hot end: red should dominate.
        assert!(cold[2] > cold[0], "cold pixel should be blue-dominant");
        assert!(hot[0] > hot[2], "hot pixel should be red-dominant");
    }

    #[test]
    fn temperature_interpolation_within_segment_box() {
        for window in TEMP_STOPS.windows(2) {
            let (t_lo, c_lo) = window[0];
            let (t_hi, c_hi) = window[1];
            let mid = 0.5 * (t_lo + t_hi);
            let c = temperature_to_rgb(mid);
            for ch in 0..3 {
                let lo = c_lo[ch].min(c_hi[ch]);
                let hi = c_lo[ch].max(c_hi[ch]);
                assert!(
                    c[ch] >= lo && c[ch] <= hi,
                    "temp channel {ch} out of [{lo},{hi}] at {mid} K: {}",
                    c[ch]
                );
            }
        }
    }

    // --- moisture_to_rgb ---

    #[test]
    fn moisture_extremes_clamp() {
        assert_eq!(moisture_to_rgb(-1.0), MOISTURE_STOPS[0].1);
        assert_eq!(moisture_to_rgb(2.0), MOISTURE_STOPS.last().unwrap().1);
    }

    #[test]
    fn moisture_anchor_points_match() {
        for &(m, c) in MOISTURE_STOPS {
            assert_eq!(moisture_to_rgb(m), c, "anchor at moisture {m}");
        }
    }

    #[test]
    fn moisture_nan_returns_first_stop() {
        assert_eq!(moisture_to_rgb(f64::NAN), MOISTURE_STOPS[0].1);
    }

    #[test]
    fn moisture_dry_is_warmer_than_wet() {
        let dry = moisture_to_rgb(0.0);
        let wet = moisture_to_rgb(1.0);
        // Dry is sandy yellow-orange: red+green > blue. Wet is blue-dominant.
        assert!(
            (dry[0] as u16 + dry[1] as u16) > (dry[2] as u16 * 2),
            "dry pixel should have warm (red+green) tone"
        );
        assert!(wet[2] > wet[0], "wet pixel should be blue-dominant");
    }

    #[test]
    fn moisture_interpolation_within_segment_box() {
        for window in MOISTURE_STOPS.windows(2) {
            let (m_lo, c_lo) = window[0];
            let (m_hi, c_hi) = window[1];
            let mid = 0.5 * (m_lo + m_hi);
            let c = moisture_to_rgb(mid);
            for ch in 0..3 {
                let lo = c_lo[ch].min(c_hi[ch]);
                let hi = c_lo[ch].max(c_hi[ch]);
                assert!(
                    c[ch] >= lo && c[ch] <= hi,
                    "moisture channel {ch} out of [{lo},{hi}] at {mid}: {}",
                    c[ch]
                );
            }
        }
    }
}
