//! Deterministic PRNG management using PCG for reproducible planet generation.

use rand::prelude::*;
use rand_pcg::Pcg64;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Mix a 64-bit value using the splitmix64 finalizer.
///
/// This is the finalization step of Sebastiano Vigna's splitmix64 PRNG. It is
/// a bijective avalanche function with well-documented constants and is stable
/// across compiler versions, making it suitable for deriving reproducible child
/// seeds that must survive refactors and toolchain upgrades.
#[inline]
pub fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Derive a stable 64-bit seed from `(world_seed, key)`.
///
/// Uses splitmix64 mixing to combine the inputs into a single seed. Unlike
/// `DefaultHasher`, the output is guaranteed stable across Rust compiler
/// versions, so seeds derived through this function remain reproducible as
/// long as the inputs are unchanged.
#[inline]
pub fn stable_derive_seed(world_seed: u64, key: u64) -> u64 {
    // Two-stage mixing: mix each input individually, then combine and re-mix.
    // This preserves entropy when either input is small (e.g., tile_index=0).
    let a = splitmix64(world_seed);
    let b = splitmix64(key ^ 0xD1B5_4A32_D192_ED03);
    splitmix64(a ^ b.rotate_left(32))
}

/// A seeded, deterministic PRNG based on PCG64.
///
/// `WorldRng` supports deriving independent child RNGs from a context string,
/// so different pipeline stages get reproducible but non-overlapping sequences.
#[derive(Debug, Clone)]
pub struct WorldRng {
    rng: Pcg64,
    seed: u64,
}

impl WorldRng {
    /// Create a new `WorldRng` from a seed.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Pcg64::seed_from_u64(seed),
            seed,
        }
    }

    /// Derive a child RNG by hashing the current seed with a context string.
    ///
    /// This produces a deterministic but independent stream for the given context.
    pub fn child(&mut self, context: &str) -> WorldRng {
        let mut hasher = DefaultHasher::new();
        self.seed.hash(&mut hasher);
        context.hash(&mut hasher);
        let child_seed = hasher.finish();
        WorldRng::new(child_seed)
    }

    /// Return a uniform `f64` in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        self.rng.r#gen::<f64>()
    }

    /// Return a uniform `f64` in [min, max).
    pub fn next_range(&mut self, min: f64, max: f64) -> f64 {
        self.rng.gen_range(min..max)
    }

    /// Return a random `u64`.
    pub fn next_u64(&mut self) -> u64 {
        self.rng.r#gen::<u64>()
    }

    /// Return a sample from a normal distribution with the given mean and standard deviation.
    pub fn next_gaussian(&mut self, mean: f64, stddev: f64) -> f64 {
        use rand::distributions::Distribution;
        let dist = rand::distributions::Standard;
        // Box-Muller transform using two uniform samples
        let u1: f64 = loop {
            let v: f64 = dist.sample(&mut self.rng);
            if v > 0.0 {
                break v;
            }
        };
        let u2: f64 = dist.sample(&mut self.rng);
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        mean + stddev * z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn determinism_same_seed_same_sequence() {
        let mut a = WorldRng::new(42);
        let mut b = WorldRng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn child_independence_different_contexts() {
        let mut rng = WorldRng::new(42);
        let mut c1 = rng.child("terrain");
        let mut c2 = rng.child("climate");
        let seq1: Vec<u64> = (0..20).map(|_| c1.next_u64()).collect();
        let seq2: Vec<u64> = (0..20).map(|_| c2.next_u64()).collect();
        assert_ne!(seq1, seq2);
    }

    #[test]
    fn child_determinism_same_context() {
        let mut rng1 = WorldRng::new(42);
        let mut rng2 = WorldRng::new(42);
        let mut c1 = rng1.child("terrain");
        let mut c2 = rng2.child("terrain");
        for _ in 0..100 {
            assert_eq!(c1.next_u64(), c2.next_u64());
        }
    }

    #[test]
    fn next_range_stays_in_bounds() {
        let mut rng = WorldRng::new(99);
        for _ in 0..10_000 {
            let v = rng.next_range(0.0, 1.0);
            assert!((0.0..1.0).contains(&v), "value out of range: {v}");
        }
    }

    #[test]
    fn splitmix64_is_stable() {
        // Lock the output for known inputs so future refactors are caught.
        assert_eq!(splitmix64(0), 0xE220_A839_7B1D_CDAF);
        assert_eq!(splitmix64(1), 0x910A_2DEC_8902_5CC1);
        assert_eq!(splitmix64(42), 0xBDD7_3226_2FEB_6E95);
    }

    #[test]
    fn stable_derive_seed_is_deterministic() {
        assert_eq!(stable_derive_seed(42, 7), stable_derive_seed(42, 7));
        assert_ne!(stable_derive_seed(42, 7), stable_derive_seed(42, 8));
        assert_ne!(stable_derive_seed(42, 7), stable_derive_seed(43, 7));
        // Zero inputs must still produce non-zero, well-mixed output.
        assert_ne!(stable_derive_seed(0, 0), 0);
        assert_ne!(stable_derive_seed(0, 0), stable_derive_seed(0, 1));
    }

    #[test]
    fn stable_derive_seed_literal_locks_algorithm() {
        // Lock the exact output so future refactors can't silently change
        // the derived seed. If this test fails, either the algorithm
        // changed or the constants did; both are breaking changes.
        assert_eq!(stable_derive_seed(42, 0), 0x4028_00AF_28D7_8446);
        assert_eq!(stable_derive_seed(42, 1), 0xD315_7293_454D_A88D);
        assert_eq!(stable_derive_seed(0xDEAD_BEEF, 1234), 0x4156_C7AE_CAC1_74AE);
    }

    #[test]
    fn gaussian_mean_is_close() {
        let mut rng = WorldRng::new(123);
        let target_mean = 5.0;
        let n = 10_000;
        let sum: f64 = (0..n).map(|_| rng.next_gaussian(target_mean, 1.0)).sum();
        let sample_mean = sum / n as f64;
        assert!(
            (sample_mean - target_mean).abs() < 0.1,
            "sample mean {sample_mean} too far from {target_mean}"
        );
    }
}
