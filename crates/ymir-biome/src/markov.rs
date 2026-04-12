//! Markov-chain biome transition smoothing for spatially coherent biome boundaries.
//!
//! The smoother iterates over a tile's neighbors and updates each tile's
//! biome toward the most probable neighbor-conditioned biome per a transition
//! weight table (see [`crate::weight_schema`]). Phase 2 ships hand-tuned
//! weights per palette; data-derived weights are Phase 5.
//!
//! # Neighbor topology
//!
//! To keep `ymir-biome`'s dependency graph clean, the smoother takes a
//! pre-built neighbor table as `&[Vec<usize>]` rather than depending on
//! `ymir-surface` directly. Each entry `neighbor_lists[i]` is the list of
//! tile indices adjacent to tile `i`. `BIOME-04` will adapt a
//! `GeodesicGrid`'s topology to this shape at call time.

use crate::palette::{Biome, BiomePalette, palette_for};
use crate::weight_schema::BiomeTransitions;
use serde::{Deserialize, Serialize};

/// Configuration for the Markov smoothing pass.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SmoothingConfig {
    /// Number of smoothing passes. 1–3 is typical for Phase 2.
    pub iterations: u32,
    /// Fractional threshold above which a tile flips to the argmax candidate.
    ///
    /// A flip is only applied when the argmax candidate differs from the
    /// current biome AND its score, normalized by `argmax_score + current_score`,
    /// exceeds this value. Values in the range (0.5, 1.0) mean the winner must
    /// beat the current biome by that margin; 0.5 means any win triggers a
    /// flip. Typical values are 0.55–0.7.
    pub flip_threshold: f64,
}

impl Default for SmoothingConfig {
    fn default() -> Self {
        Self {
            iterations: 2,
            flip_threshold: 0.6,
        }
    }
}

/// Apply one or more Markov smoothing passes to a tile biome vector.
///
/// For each pass:
/// 1. For each tile, gather its neighbors from `neighbor_lists`.
/// 2. For each candidate new biome `b_new` in the active palette, compute a
///    score: `self_affinity(b_new)` if `b_new == current`, plus the sum over
///    each neighbor of `transition_weight(b_new, neighbor_biome)`.
/// 3. The tile picks the argmax score. If the argmax is not the current biome
///    AND its score, normalized by `argmax_score + current_score`, exceeds
///    `cfg.flip_threshold`, flip; otherwise keep.
/// 4. All proposed flips are applied synchronously using a double buffer:
///    read from `prev`, write to `next`, then swap. This prevents one tile's
///    flip from cascading through its neighbors in the same pass.
///
/// # Panics
///
/// Panics if `biomes.len() != neighbor_lists.len()`.
///
/// # Integration
///
/// `BIOME-04` will build `neighbor_lists` from a `GeodesicGrid` at call time
/// before invoking this function.
pub fn smooth_biomes(
    neighbor_lists: &[Vec<usize>],
    biomes: &mut Vec<Biome>,
    transitions: &BiomeTransitions,
    cfg: &SmoothingConfig,
) {
    assert_eq!(
        biomes.len(),
        neighbor_lists.len(),
        "biome vector length must match neighbor_lists length"
    );

    if cfg.iterations == 0 || biomes.is_empty() {
        return;
    }

    let palette = transitions.palette;
    let candidates: &[Biome] = palette.biomes();
    // NoSurface palettes have only one biome and nothing to smooth.
    if candidates.len() <= 1 {
        return;
    }

    let mut prev: Vec<Biome> = biomes.clone();
    let mut next: Vec<Biome> = biomes.clone();

    for _ in 0..cfg.iterations {
        for (tile_idx, neighbors) in neighbor_lists.iter().enumerate() {
            let current = prev[tile_idx];
            let (argmax_biome, argmax_score, current_score) =
                score_candidates(current, neighbors, &prev, candidates, transitions);

            if argmax_biome == current {
                next[tile_idx] = current;
                continue;
            }
            // If the current biome is outside the palette, its self-affinity
            // is 0 and any in-palette winner should flip it. Otherwise
            // require the winner to beat the current biome by `flip_threshold`
            // (expressed as a fraction of their combined score).
            let denom = argmax_score + current_score;
            let flip = if denom <= 0.0 {
                argmax_score > 0.0
            } else {
                (argmax_score / denom) > cfg.flip_threshold
            };
            if flip {
                next[tile_idx] = argmax_biome;
            } else {
                next[tile_idx] = current;
            }
        }
        std::mem::swap(&mut prev, &mut next);
    }

    *biomes = prev;
}

/// Score every candidate biome for a given tile and return the argmax.
///
/// Returns `(argmax_biome, argmax_score, current_score)` where
/// `current_score` is the score of the tile's existing biome (used as the
/// "stay" baseline during the flip-threshold comparison). If the current
/// biome is outside the candidate palette, `current_score` is 0.
fn score_candidates(
    current: Biome,
    neighbors: &[usize],
    prev: &[Biome],
    candidates: &[Biome],
    transitions: &BiomeTransitions,
) -> (Biome, f64, f64) {
    let mut best_biome = current;
    let mut best_score = f64::NEG_INFINITY;
    let mut current_score = 0.0;

    for &candidate in candidates {
        let mut score = 0.0;
        if candidate == current {
            score += transitions.self_affinity(candidate);
        }
        for &n_idx in neighbors {
            let n_biome = prev[n_idx];
            score += transitions.weight(candidate, n_biome);
        }
        if candidate == current {
            current_score = score;
        }
        if score > best_score {
            best_score = score;
            best_biome = candidate;
        }
    }

    (best_biome, best_score, current_score)
}

/// Convenience helper: choose the palette that matches `biomes`, falling back
/// to the palette derived from `atmosphere_class` when a tile sits outside
/// any known palette. Used by `BIOME-04`; not needed here but exposed so
/// callers can pick a palette without re-implementing the logic.
#[doc(hidden)]
pub fn palette_from_atmosphere(
    class: ymir_atmosphere::composition::AtmosphereClass,
) -> BiomePalette {
    palette_for(class)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weight_schema::default_transitions;

    /// Build a trivial "ring" neighbor topology where each tile is adjacent
    /// to the tile before and after it (wrapping). Good enough to exercise
    /// the smoothing algorithm without pulling in ymir-surface.
    fn ring_neighbors(n: usize) -> Vec<Vec<usize>> {
        (0..n).map(|i| vec![(i + n - 1) % n, (i + 1) % n]).collect()
    }

    /// Build a "cluster" topology: two dense groups of tiles connected by
    /// one bridge. Lets us test that large regions are preserved.
    fn two_cluster_neighbors(half: usize) -> Vec<Vec<usize>> {
        let n = half * 2;
        let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
        // Inside each cluster, every tile is connected to every other tile
        // in that cluster.
        for cluster in 0..2 {
            let base = cluster * half;
            for i in 0..half {
                for j in 0..half {
                    if i != j {
                        out[base + i].push(base + j);
                    }
                }
            }
        }
        // Bridge: tile 0 of cluster 0 connects to tile 0 of cluster 1.
        out[0].push(half);
        out[half].push(0);
        out
    }

    #[test]
    fn smoothing_converts_lone_islands() {
        // 12 tiles on a ring, all TemperateForest except one Tundra.
        let n = 12;
        let neighbors = ring_neighbors(n);
        let mut biomes = vec![Biome::TemperateForest; n];
        biomes[5] = Biome::Tundra;

        let transitions = default_transitions(BiomePalette::EarthLike);
        let cfg = SmoothingConfig {
            iterations: 3,
            flip_threshold: 0.6,
        };
        smooth_biomes(&neighbors, &mut biomes, &transitions, &cfg);

        assert_ne!(
            biomes[5],
            Biome::Tundra,
            "lone Tundra tile should be smoothed away"
        );
    }

    #[test]
    fn smoothing_preserves_large_regions() {
        // Half TemperateForest, half HotDesert in dense clusters.
        let half = 16;
        let n = half * 2;
        let neighbors = two_cluster_neighbors(half);
        let mut biomes = vec![Biome::TemperateForest; n];
        for b in biomes.iter_mut().take(n).skip(half) {
            *b = Biome::HotDesert;
        }
        let original = biomes.clone();

        let transitions = default_transitions(BiomePalette::EarthLike);
        let cfg = SmoothingConfig {
            iterations: 3,
            flip_threshold: 0.6,
        };
        smooth_biomes(&neighbors, &mut biomes, &transitions, &cfg);

        let unchanged = biomes
            .iter()
            .zip(original.iter())
            .filter(|(a, b)| a == b)
            .count();
        let ratio = unchanged as f64 / n as f64;
        assert!(
            ratio >= 0.8,
            "large regions should be preserved (>=80% unchanged), got {ratio}"
        );
    }

    #[test]
    fn smoothing_stays_in_palette() {
        // Earth-like ring with a few off-palette VenusPlains tiles mixed in.
        let n = 20;
        let neighbors = ring_neighbors(n);
        let mut biomes = vec![Biome::TemperateForest; n];
        biomes[2] = Biome::VenusPlains;
        biomes[7] = Biome::VenusPlains;
        biomes[13] = Biome::VenusPlains;

        let transitions = default_transitions(BiomePalette::EarthLike);
        let cfg = SmoothingConfig {
            iterations: 3,
            flip_threshold: 0.6,
        };
        smooth_biomes(&neighbors, &mut biomes, &transitions, &cfg);

        let palette = BiomePalette::EarthLike;
        for (i, b) in biomes.iter().enumerate() {
            assert!(
                palette.contains(*b),
                "tile {i} has off-palette biome {b:?} after smoothing"
            );
        }
    }

    #[test]
    fn smoothing_is_deterministic() {
        let n = 24;
        let neighbors = ring_neighbors(n);
        let mut a = vec![Biome::Grassland; n];
        a[3] = Biome::HotDesert;
        a[4] = Biome::HotDesert;
        a[11] = Biome::Tundra;
        a[20] = Biome::Ocean;
        let mut b = a.clone();

        let transitions = default_transitions(BiomePalette::EarthLike);
        let cfg = SmoothingConfig {
            iterations: 3,
            flip_threshold: 0.6,
        };
        smooth_biomes(&neighbors, &mut a, &transitions, &cfg);
        smooth_biomes(&neighbors, &mut b, &transitions, &cfg);
        assert_eq!(a, b);
    }

    #[test]
    fn zero_iterations_is_noop() {
        let neighbors = ring_neighbors(6);
        let mut biomes = vec![
            Biome::Ocean,
            Biome::CoastalShallow,
            Biome::TemperateForest,
            Biome::Grassland,
            Biome::HotDesert,
            Biome::Tundra,
        ];
        let original = biomes.clone();
        let transitions = default_transitions(BiomePalette::EarthLike);
        let cfg = SmoothingConfig {
            iterations: 0,
            flip_threshold: 0.6,
        };
        smooth_biomes(&neighbors, &mut biomes, &transitions, &cfg);
        assert_eq!(biomes, original);
    }

    #[test]
    fn no_surface_palette_is_noop() {
        let neighbors = ring_neighbors(4);
        let mut biomes = vec![Biome::NoSurface; 4];
        let transitions = default_transitions(BiomePalette::NoSurface);
        let cfg = SmoothingConfig::default();
        smooth_biomes(&neighbors, &mut biomes, &transitions, &cfg);
        assert!(biomes.iter().all(|b| *b == Biome::NoSurface));
    }
}
