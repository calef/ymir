//! Earth validation integration test (VALID-01).
//!
//! Drives the full `ymir generate --star Earth --seed 1` pipeline through the
//! compiled binary, then loads the persisted `climate.bin` and `biomes.bin`
//! and asserts Earth-like outputs:
//!
//! 1. Mean surface temperature within 5 K of 288 K.
//! 2. A plausible biome mix: at least one forest variant, one
//!    grassland/savanna variant, one desert variant, and one cold/polar
//!    variant.
//!
//! Ocean tile fraction ("~70%") is NOT asserted here: the Earth path in
//! `earth_body()` does not yet set a `continental_fraction` override and the
//! Whittaker classifier does not emit `Ocean` without the BIOME-04 overlay.
//! See the NOTE under VALID-01 in TASKS.md.

use std::process::Command;

use ymir_biome::{Biome, BiomeMap};
use ymir_climate::ClimateMap;
use ymir_storage::load_bin;

/// Run the full Earth pipeline and assert Earth-like output thresholds.
///
/// This test invokes the compiled `ymir` binary (resolved via
/// `CARGO_BIN_EXE_ymir`) in a fresh tempdir, so it exercises the same CLI
/// surface users see. It is slow (several seconds) but there are only a
/// handful of VALID-* tests total.
#[test]
fn earth_validation_passes_basic_thresholds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let world_dir = tmp.path().join("earth_world");

    let bin = env!("CARGO_BIN_EXE_ymir");
    let output = Command::new(bin)
        .arg("generate")
        .arg("--star")
        .arg("Earth")
        .arg("--seed")
        .arg("1")
        .arg("--output")
        .arg(&world_dir)
        .output()
        .expect("failed to exec ymir binary");

    assert!(
        output.status.success(),
        "ymir generate failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // Load the persisted climate and biome artifacts directly; this avoids
    // parsing CLI output and matches how downstream tooling will consume the
    // world directory.
    let climate: ClimateMap = load_bin(world_dir.join("climate.bin")).expect("load climate.bin");
    let biomes: BiomeMap = load_bin(world_dir.join("biomes.bin")).expect("load biomes.bin");

    // --- Assertion 1: mean surface temperature within 5 K of 288 K. -------
    let temps = &climate.temperature.per_tile_k;
    assert!(!temps.is_empty(), "temperature field is empty");
    let mean_t = temps.iter().sum::<f64>() / temps.len() as f64;
    assert!(
        (283.0..=293.0).contains(&mean_t),
        "mean surface T out of Earth-like band (283..=293 K): {mean_t:.2} K"
    );

    // --- Assertion 2: biome mix covers forests, open land, desert, cold. --
    //
    // The Whittaker lookup excludes Ocean (reserved for the BIOME-04 overlay
    // pathway, which requires a continental_fraction override that Earth's
    // body does not yet set). We intentionally do NOT assert Ocean here. See
    // the TASKS.md NOTE under VALID-01.
    let hist = biomes.histogram();
    let present: std::collections::HashSet<Biome> = biomes.per_tile.iter().copied().collect();

    let has_forest = present.contains(&Biome::TropicalRainforest)
        || present.contains(&Biome::TemperateForest)
        || present.contains(&Biome::BorealForest);
    assert!(
        has_forest,
        "expected at least one forest variant in Earth biome mix; histogram: {hist:?}"
    );

    let has_grassland = present.contains(&Biome::Grassland) || present.contains(&Biome::Savanna);
    assert!(
        has_grassland,
        "expected a Grassland or Savanna variant in Earth biome mix; histogram: {hist:?}"
    );

    let has_desert = present.contains(&Biome::HotDesert) || present.contains(&Biome::ColdDesert);
    assert!(
        has_desert,
        "expected a HotDesert or ColdDesert variant in Earth biome mix; histogram: {hist:?}"
    );

    let has_cold = present.contains(&Biome::Tundra) || present.contains(&Biome::IceSheet);
    assert!(
        has_cold,
        "expected a Tundra or IceSheet variant in Earth biome mix; histogram: {hist:?}"
    );
}
