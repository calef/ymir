//! Coherent noise functions (simplex, fractal Brownian motion) for terrain
//! and detail generation.
//!
//! This module wraps the [`noise`](https://docs.rs/noise) crate's
//! [`OpenSimplex`] sampler and exposes a spherical FBM
//! helper. Callers pass latitude/longitude in radians; the point is lifted
//! to a 3D unit vector before being evaluated so the noise field is
//! naturally continuous across the date line and at the poles.

use noise::{NoiseFn, OpenSimplex};
use serde::{Deserialize, Serialize};

/// Fractal Brownian motion sampled on the unit sphere via layered
/// [`OpenSimplex`] noise.
///
/// Each octave scales the input frequency by `lacunarity^i` and the output
/// amplitude by `gain^i`. The per-octave samples are summed and normalized
/// to an approximate `[-1, 1]` range so callers can use the output as a
/// unitless displacement before rescaling into physical units.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SphericalFbm {
    /// Seed used to configure the underlying simplex noise.
    seed: u64,
    /// Number of noise octaves summed together.
    octaves: u32,
    /// Per-octave frequency multiplier (typical: ~2.0).
    lacunarity: f64,
    /// Per-octave amplitude multiplier (typical: ~0.5).
    gain: f64,
    /// Frequency of the first octave.
    base_frequency: f64,
}

impl SphericalFbm {
    /// Create a new spherical FBM sampler.
    ///
    /// Parameters use the usual FBM conventions: `lacunarity` > 1 shrinks
    /// feature size per octave, and `gain` < 1 attenuates amplitude per
    /// octave. `base_frequency` controls the spatial scale of the largest
    /// features on the unit sphere.
    pub fn new(seed: u64, octaves: u32, lacunarity: f64, gain: f64, base_frequency: f64) -> Self {
        Self {
            seed,
            octaves: octaves.max(1),
            lacunarity,
            gain,
            base_frequency,
        }
    }

    /// Evaluate the FBM sum at a point on the sphere given latitude and
    /// longitude in radians.
    ///
    /// The result is normalized by the geometric sum of octave amplitudes
    /// so it stays in a roughly `[-1, 1]` range regardless of octave count.
    pub fn sample(&self, lat_rad: f64, lon_rad: f64) -> f64 {
        let cos_lat = lat_rad.cos();
        let x = cos_lat * lon_rad.cos();
        let y = cos_lat * lon_rad.sin();
        let z = lat_rad.sin();

        // Use a distinct OpenSimplex instance per octave, derived from the
        // base seed, so octaves are statistically independent.
        let mut total = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = self.base_frequency;
        let mut amp_sum = 0.0;

        for i in 0..self.octaves {
            let octave_seed = self
                .seed
                .wrapping_add(0x9E37_79B9_7F4A_7C15_u64.wrapping_mul(i as u64 + 1));
            // OpenSimplex's seed parameter is u32; fold the u64 down.
            let simplex = OpenSimplex::new((octave_seed ^ (octave_seed >> 32)) as u32);
            let v = simplex.get([x * frequency, y * frequency, z * frequency]);
            total += v * amplitude;
            amp_sum += amplitude;
            amplitude *= self.gain;
            frequency *= self.lacunarity;
        }

        if amp_sum > 0.0 {
            // OpenSimplex output is roughly in [-0.5, 0.5]; rescale to [-1, 1].
            (total / amp_sum) * 2.0
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn determinism_same_seed_same_inputs() {
        let fbm = SphericalFbm::new(42, 4, 2.0, 0.5, 1.5);
        let a = fbm.sample(0.5, 1.2);
        let b = fbm.sample(0.5, 1.2);
        assert_eq!(a, b);
    }

    #[test]
    fn determinism_across_instances() {
        let a = SphericalFbm::new(42, 4, 2.0, 0.5, 1.5);
        let b = SphericalFbm::new(42, 4, 2.0, 0.5, 1.5);
        for (lat, lon) in [(0.0, 0.0), (0.3, -0.7), (-1.2, 2.5), (1.5, 3.0)] {
            assert_eq!(a.sample(lat, lon), b.sample(lat, lon));
        }
    }

    #[test]
    fn different_seeds_produce_different_output() {
        let a = SphericalFbm::new(1, 4, 2.0, 0.5, 1.5);
        let b = SphericalFbm::new(2, 4, 2.0, 0.5, 1.5);
        // Compare at a handful of points; they should not all coincide.
        let mut any_different = false;
        for (lat, lon) in [(0.0, 0.0), (0.3, -0.7), (-1.2, 2.5), (1.5, 3.0)] {
            if (a.sample(lat, lon) - b.sample(lat, lon)).abs() > 1e-9 {
                any_different = true;
                break;
            }
        }
        assert!(
            any_different,
            "distinct seeds should produce different output"
        );
    }

    #[test]
    fn differs_across_points() {
        let fbm = SphericalFbm::new(7, 4, 2.0, 0.5, 1.5);
        let a = fbm.sample(0.0, 0.0);
        let b = fbm.sample(0.5, 1.0);
        assert!(
            (a - b).abs() > 1e-6,
            "expected distinct samples at different points"
        );
    }

    #[test]
    fn output_within_reasonable_bounds() {
        // Normalized FBM should stay within [-1, 1] (with some margin for
        // the coarse rescaling factor).
        use std::f64::consts::{PI, TAU};
        let fbm = SphericalFbm::new(123, 6, 2.0, 0.5, 2.0);
        let mut max_abs = 0.0f64;
        for i in 0..50 {
            for j in 0..50 {
                let lat = -PI / 2.0 + (i as f64) * (PI / 49.0);
                let lon = -PI + (j as f64) * (TAU / 49.0);
                max_abs = max_abs.max(fbm.sample(lat, lon).abs());
            }
        }
        assert!(max_abs <= 1.2, "max |sample| = {max_abs}, expected <= 1.2");
        assert!(
            max_abs > 0.05,
            "max |sample| = {max_abs}, expected non-trivial magnitude"
        );
    }

    #[test]
    fn no_nan_or_infinite() {
        use std::f64::consts::PI;
        let fbm = SphericalFbm::new(999, 5, 2.1, 0.55, 1.3);
        for lat in [-PI / 2.0, -0.8, 0.0, 0.8, PI / 2.0] {
            for lon in [-PI, -1.0, 0.0, 1.0, PI] {
                let v = fbm.sample(lat, lon);
                assert!(v.is_finite(), "non-finite sample at ({lat}, {lon}): {v}");
            }
        }
    }
}
