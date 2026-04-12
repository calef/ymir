//! Configurable weight schemas for biome boundary blending and transition tuning.
//!
//! Defines [`BiomeTransitions`], a Markov-style affinity table keyed by
//! (current biome, neighbor biome) pairs, plus the hand-tuned Phase 2
//! defaults returned by [`default_transitions`]. Data-derived weights are
//! deferred to Phase 5; see design doc section 5.6.

use crate::palette::{Biome, BiomePalette};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Serialized form of a single weight entry. JSON (and most other formats)
/// can't represent non-string map keys directly, so [`BiomeTransitions`]
/// round-trips the weight map as a `Vec<WeightEntry>`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct WeightEntry {
    current: Biome,
    neighbor: Biome,
    weight: f64,
}

fn serialize_weights<S>(weights: &HashMap<(Biome, Biome), f64>, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let entries: Vec<WeightEntry> = weights
        .iter()
        .map(|((current, neighbor), w)| WeightEntry {
            current: *current,
            neighbor: *neighbor,
            weight: *w,
        })
        .collect();
    entries.serialize(s)
}

fn deserialize_weights<'de, D>(d: D) -> Result<HashMap<(Biome, Biome), f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let entries: Vec<WeightEntry> = Vec::deserialize(d)?;
    Ok(entries
        .into_iter()
        .map(|e| ((e.current, e.neighbor), e.weight))
        .collect())
}

/// Default self-affinity applied when a biome has no explicit entry in the
/// table. A tile "prefers" to stay itself under minor neighbor pressure.
const DEFAULT_SELF_AFFINITY: f64 = 1.0;

/// A Markov-style transition weight table.
///
/// For every (current_biome, neighbor_biome) pair, `weights[(current, neighbor)]`
/// is an unnormalized affinity score. Higher means the current tile is more
/// likely to adopt (or keep) that biome when surrounded by `neighbor_biome`
/// tiles. The smoothing pass computes an argmax over per-candidate scores;
/// exact normalization is handled there (see [`crate::markov`]).
///
/// Only pairs within the same palette should have non-trivial weights. The
/// smoother should never propose transitions to biomes outside the declared
/// palette, and unknown pairs return 0 from [`Self::weight`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BiomeTransitions {
    /// The palette this table is tuned for. Queries for pairs outside the
    /// palette return 0.
    pub palette: BiomePalette,
    /// Affinity for ordered (current, neighbor) pairs. Missing entries
    /// imply 0. The serialized form is a list of entries rather than a
    /// map because JSON object keys must be strings.
    #[serde(
        serialize_with = "serialize_weights",
        deserialize_with = "deserialize_weights"
    )]
    pub weights: HashMap<(Biome, Biome), f64>,
}

impl BiomeTransitions {
    /// Construct a transitions table for `palette` with an empty weight map.
    pub fn empty(palette: BiomePalette) -> Self {
        Self {
            palette,
            weights: HashMap::new(),
        }
    }

    /// Look up the affinity for the (current, neighbor) pair.
    ///
    /// Returns 0 if either biome is outside the table's declared palette,
    /// or if the pair has no entry in `weights`.
    pub fn weight(&self, current: Biome, neighbor: Biome) -> f64 {
        if !self.palette.contains(current) || !self.palette.contains(neighbor) {
            return 0.0;
        }
        self.weights
            .get(&(current, neighbor))
            .copied()
            .unwrap_or(0.0)
    }

    /// Self-affinity (a tile "prefers" to stay the same). Defaults to
    /// [`DEFAULT_SELF_AFFINITY`] (1.0) if not explicitly set in the table,
    /// and returns 0 for biomes outside the declared palette.
    pub fn self_affinity(&self, biome: Biome) -> f64 {
        if !self.palette.contains(biome) {
            return 0.0;
        }
        self.weights
            .get(&(biome, biome))
            .copied()
            .unwrap_or(DEFAULT_SELF_AFFINITY)
    }

    /// Insert a symmetric affinity: sets both `(a, b)` and `(b, a)` to
    /// `weight`. Intended for use by the hand-tuned defaults below; public
    /// so that tests and Phase 5 data loaders can reuse it.
    pub fn insert_symmetric(&mut self, a: Biome, b: Biome, weight: f64) {
        self.weights.insert((a, b), weight);
        self.weights.insert((b, a), weight);
    }

    /// Insert a self-affinity entry for `biome`.
    pub fn insert_self(&mut self, biome: Biome, weight: f64) {
        self.weights.insert((biome, biome), weight);
    }
}

/// Default transition tables, one per palette, hand-tuned for Phase 2.
///
/// Weight philosophy:
/// - Self-affinity is high (3.0) so tiles resist flipping under minor pressure.
/// - "Similar" biome pairs get moderate affinity (1.5–2.0).
/// - "Dissimilar" pairs get near-zero affinity (0.0–0.2).
/// - At least 8–12 populated pairs per non-trivial palette.
///
/// The [`BiomePalette::NoSurface`] palette has only one biome and returns an
/// empty weight map.
pub fn default_transitions(palette: BiomePalette) -> BiomeTransitions {
    match palette {
        BiomePalette::EarthLike => earth_like_transitions(),
        BiomePalette::VenusLike => venus_like_transitions(),
        BiomePalette::MarsLike => mars_like_transitions(),
        BiomePalette::TitanLike => titan_like_transitions(),
        BiomePalette::Airless => airless_transitions(),
        BiomePalette::NoSurface => BiomeTransitions::empty(BiomePalette::NoSurface),
    }
}

fn earth_like_transitions() -> BiomeTransitions {
    use Biome::*;
    let mut t = BiomeTransitions::empty(BiomePalette::EarthLike);

    // Self-affinity: all palette members resist flipping.
    for &b in BiomePalette::EarthLike.biomes() {
        t.insert_self(b, 3.0);
    }

    // --- Similar pairs: moderate affinity (1.5 - 2.0) ---
    t.insert_symmetric(TemperateForest, BorealForest, 1.8);
    t.insert_symmetric(TemperateForest, Grassland, 1.6);
    t.insert_symmetric(BorealForest, Tundra, 1.6);
    t.insert_symmetric(Grassland, Savanna, 1.7);
    t.insert_symmetric(Grassland, ColdDesert, 1.5);
    t.insert_symmetric(HotDesert, Savanna, 1.7);
    t.insert_symmetric(HotDesert, ColdDesert, 1.5);
    t.insert_symmetric(Tundra, IceSheet, 1.8);
    t.insert_symmetric(Tundra, ColdDesert, 1.5);
    t.insert_symmetric(Ocean, CoastalShallow, 2.0);
    t.insert_symmetric(CoastalShallow, Wetland, 1.6);
    t.insert_symmetric(TropicalRainforest, Savanna, 1.6);
    t.insert_symmetric(TropicalRainforest, Wetland, 1.5);
    t.insert_symmetric(Wetland, TemperateForest, 1.5);
    t.insert_symmetric(AlpineMeadow, Tundra, 1.6);
    t.insert_symmetric(AlpineMeadow, BorealForest, 1.5);

    // --- Dissimilar pairs: near-zero affinity (0.0 - 0.2) ---
    t.insert_symmetric(TropicalRainforest, IceSheet, 0.0);
    t.insert_symmetric(TropicalRainforest, Tundra, 0.0);
    t.insert_symmetric(HotDesert, Tundra, 0.0);
    t.insert_symmetric(HotDesert, IceSheet, 0.0);
    t.insert_symmetric(IceSheet, Ocean, 0.1);
    t.insert_symmetric(IceSheet, TemperateForest, 0.1);

    t
}

fn venus_like_transitions() -> BiomeTransitions {
    use Biome::*;
    let mut t = BiomeTransitions::empty(BiomePalette::VenusLike);

    for &b in BiomePalette::VenusLike.biomes() {
        t.insert_self(b, 3.0);
    }

    // Similar pairs.
    t.insert_symmetric(VenusPlains, VenusHighlands, 1.8);
    t.insert_symmetric(VenusPlains, LavaPlain, 1.7);
    t.insert_symmetric(VenusHighlands, SulfuricHighlands, 1.8);
    t.insert_symmetric(LavaPlain, VenusHighlands, 1.5);
    t.insert_symmetric(ChemicalSediment, VenusPlains, 1.5);
    t.insert_symmetric(ChemicalSediment, SulfuricHighlands, 1.6);

    // Dissimilar pairs.
    t.insert_symmetric(LavaPlain, SulfuricHighlands, 0.2);
    t.insert_symmetric(LavaPlain, ChemicalSediment, 0.1);

    t
}

fn mars_like_transitions() -> BiomeTransitions {
    use Biome::*;
    let mut t = BiomeTransitions::empty(BiomePalette::MarsLike);

    for &b in BiomePalette::MarsLike.biomes() {
        t.insert_self(b, 3.0);
    }

    t.insert_symmetric(MartianDustPlain, MartianBedrock, 1.7);
    t.insert_symmetric(MartianDustPlain, ImpactRegolith, 1.8);
    t.insert_symmetric(MartianBedrock, ImpactRegolith, 1.6);
    t.insert_symmetric(MartianPolarIce, MartianDustPlain, 1.5);
    t.insert_symmetric(MartianPolarIce, MartianBedrock, 1.2);

    // Dissimilar.
    t.insert_symmetric(MartianPolarIce, ImpactRegolith, 0.2);

    t
}

fn titan_like_transitions() -> BiomeTransitions {
    use Biome::*;
    let mut t = BiomeTransitions::empty(BiomePalette::TitanLike);

    for &b in BiomePalette::TitanLike.biomes() {
        t.insert_self(b, 3.0);
    }

    t.insert_symmetric(TitanMethaneSea, TitanDunes, 1.6);
    t.insert_symmetric(TitanDunes, TitanIceHighland, 1.7);
    t.insert_symmetric(TitanIceHighland, TitanCryovolcanic, 1.8);
    t.insert_symmetric(TitanCryovolcanic, TitanDunes, 1.5);
    t.insert_symmetric(TitanMethaneSea, TitanIceHighland, 1.2);

    // Dissimilar.
    t.insert_symmetric(TitanMethaneSea, TitanCryovolcanic, 0.2);

    t
}

fn airless_transitions() -> BiomeTransitions {
    use Biome::*;
    let mut t = BiomeTransitions::empty(BiomePalette::Airless);

    for &b in BiomePalette::Airless.biomes() {
        t.insert_self(b, 3.0);
    }

    t.insert_symmetric(BareRegolith, ImpactCrater, 1.8);
    t.insert_symmetric(BareRegolith, ThermalFracture, 1.6);
    t.insert_symmetric(BareRegolith, SolarBleached, 1.7);
    t.insert_symmetric(ImpactCrater, ThermalFracture, 1.5);
    t.insert_symmetric(ThermalFracture, SolarBleached, 1.5);

    // Dissimilar.
    t.insert_symmetric(ImpactCrater, SolarBleached, 0.2);

    t
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_PALETTES: &[BiomePalette] = &[
        BiomePalette::EarthLike,
        BiomePalette::VenusLike,
        BiomePalette::MarsLike,
        BiomePalette::TitanLike,
        BiomePalette::Airless,
        BiomePalette::NoSurface,
    ];

    #[test]
    fn default_transitions_exist_for_every_palette() {
        for p in ALL_PALETTES {
            let t = default_transitions(*p);
            assert_eq!(t.palette, *p, "palette mismatch for {p:?}");
            if *p == BiomePalette::NoSurface {
                assert!(
                    t.weights.is_empty(),
                    "NoSurface should have an empty weight table"
                );
            } else {
                assert!(
                    !t.weights.is_empty(),
                    "palette {p:?} should have non-empty weights"
                );
            }
        }
    }

    #[test]
    fn weight_table_respects_palette() {
        let earth = default_transitions(BiomePalette::EarthLike);
        // VenusPlains is not in EarthLike, so any query should be 0.
        assert_eq!(earth.weight(Biome::VenusPlains, Biome::Ocean), 0.0);
        assert_eq!(earth.weight(Biome::Ocean, Biome::VenusPlains), 0.0);
        assert_eq!(earth.weight(Biome::VenusPlains, Biome::VenusPlains), 0.0);
        assert_eq!(earth.self_affinity(Biome::VenusPlains), 0.0);
    }

    #[test]
    fn self_affinity_default() {
        // An empty table reports the default 1.0 for any in-palette biome.
        let t = BiomeTransitions::empty(BiomePalette::EarthLike);
        assert_eq!(t.self_affinity(Biome::Ocean), 1.0);
        // Out-of-palette returns 0.
        assert_eq!(t.self_affinity(Biome::VenusPlains), 0.0);
    }

    #[test]
    fn transitions_serde_round_trip() {
        let t = default_transitions(BiomePalette::EarthLike);
        let s = serde_json::to_string(&t).expect("serialize");
        let back: BiomeTransitions = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(back.palette, t.palette);
        assert_eq!(back.weights, t.weights);
    }
}
