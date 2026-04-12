//! Deterministic PRNG management using PCG for reproducible planet generation.

use rand::prelude::*;
use rand_pcg::Pcg64;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

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
