//! Regional detail specification and deterministic PRNG derivation.
//!
//! A [`RegionSpec`] names a contiguous neighborhood of skeleton tiles to
//! generate detail for. The [`detail_rng`] factory produces a reproducible
//! `Pcg64` keyed on `(world_seed, tile_index)` so each region's detail pass
//! is deterministic and independent from every other region's.

use rand::SeedableRng;
use rand_pcg::Pcg64;
use serde::{Deserialize, Serialize};
use ymir_core::stable_derive_seed;

/// Identifies a region of the skeleton grid to generate detail for.
///
/// A region is the seed tile plus its `radius_tiles`-ring neighborhood on
/// the geodesic skeleton:
///
/// - `radius_tiles = 0` — single tile (primarily useful for tests and
///   worst-case boundary checks; not the expected production configuration).
/// - `radius_tiles = 1` — seed tile plus its ~6 immediate neighbors
///   (~5 at the 12 icosahedral pentagons).
/// - `radius_tiles = N` — N-ring flood-fill starting from the seed tile,
///   i.e. the set of tiles reachable within N neighbor hops.
///
/// The concrete flood-fill is performed by downstream stages (DET-02+) that
/// have access to the skeleton's neighbor graph; `RegionSpec` itself is only
/// the identifier.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RegionSpec {
    /// Index of the seed tile on the geodesic skeleton grid.
    pub tile_index: u32,
    /// Radius of the neighborhood, measured in tile-neighbor hops.
    pub radius_tiles: u32,
}

impl RegionSpec {
    /// Construct a new `RegionSpec` from a seed tile and neighborhood radius.
    pub fn new(tile_index: u32, radius_tiles: u32) -> Self {
        Self {
            tile_index,
            radius_tiles,
        }
    }
}

/// Build a deterministic PRNG for a region's detail pass.
///
/// Derives the PCG64 seed from `hash(world_seed, tile_index)` per design
/// doc §5.7. The underlying mixing function is splitmix64 (see
/// [`ymir_core::stable_derive_seed`]), which is stable across Rust compiler
/// versions, so the same `(world_seed, tile_index)` pair reproduces the
/// same stream across machines and toolchains.
///
/// Note that the seed depends only on `tile_index`, not on `radius_tiles`:
/// two regions sharing a seed tile but with different radii draw from the
/// same stream. This is intentional so that growing a region's radius does
/// not invalidate cached detail at the original radius.
pub fn detail_rng(world_seed: u64, tile_index: u32) -> Pcg64 {
    let seed = stable_derive_seed(world_seed, u64::from(tile_index));
    Pcg64::seed_from_u64(seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    fn first_n(rng: &mut Pcg64, n: usize) -> Vec<u64> {
        (0..n).map(|_| rng.next_u64()).collect()
    }

    #[test]
    fn same_inputs_same_sequence() {
        let mut a = detail_rng(42, 7);
        let mut b = detail_rng(42, 7);
        assert_eq!(first_n(&mut a, 32), first_n(&mut b, 32));
    }

    #[test]
    fn different_seed_diverges_at_first_sample() {
        let mut a = detail_rng(42, 7);
        let mut b = detail_rng(43, 7);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn different_tile_index_diverges_at_first_sample() {
        let mut a = detail_rng(42, 7);
        let mut b = detail_rng(42, 8);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn zero_inputs_are_still_well_mixed() {
        let mut a = detail_rng(0, 0);
        let mut b = detail_rng(0, 1);
        assert_ne!(a.next_u64(), b.next_u64());
        // And the first sample must not itself be zero.
        let mut c = detail_rng(0, 0);
        assert_ne!(c.next_u64(), 0);
    }

    #[test]
    fn cross_run_determinism_literal_bytes() {
        // Lock the exact first-three u64s for a known (seed, tile_index).
        // If this test fails, the derived seed has silently drifted; bump
        // a world-format version before changing these constants.
        let mut rng = detail_rng(42, 7);
        let got: [u64; 3] = [rng.next_u64(), rng.next_u64(), rng.next_u64()];
        let expected: [u64; 3] = [
            0x0368_10D7_0E8A_0C0D,
            0x89F6_7C09_775F_3D85,
            0x5480_DE26_6BC6_3313,
        ];
        assert_eq!(got, expected);
    }

    #[test]
    fn region_spec_serde_round_trip() {
        let spec = RegionSpec::new(4242, 3);
        let json = serde_json::to_string(&spec).expect("serialize");
        let back: RegionSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(spec, back);
        assert_eq!(back.tile_index, 4242);
        assert_eq!(back.radius_tiles, 3);
    }

    #[test]
    fn region_spec_constructor_and_equality() {
        let a = RegionSpec::new(1, 2);
        let b = RegionSpec {
            tile_index: 1,
            radius_tiles: 2,
        };
        assert_eq!(a, b);
    }
}
