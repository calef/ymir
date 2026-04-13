//! Earth validation integration test (VALID-01).
//!
//! Drives the full `ymir generate --star Earth --seed 1` pipeline through the
//! compiled binary, then loads the persisted `skeleton.bin`, `climate.bin`,
//! and `biomes.bin` and asserts Earth-like outputs:
//!
//! 1. Mean surface temperature within 5 K of 288 K.
//! 2. Ocean-tile fraction (elevation < 0 m) within 5 percentage points of
//!    0.71, given the Earth `continental_fraction = 0.29` override wired
//!    through CAT-04.
//! 3. A plausible biome mix: at least one forest variant, one
//!    grassland/savanna variant, one desert variant, and one cold/polar
//!    variant.
//! 4. Biome-level water fraction: with BIOME-05's water overlay landed, the
//!    biome histogram should report ≥50% Ocean+CoastalShallow tiles (the
//!    overlay paints sub-sea-level tiles directly; Markov smoothing may
//!    round a few coastal tiles back to land so this is set below the
//!    skeleton-level 0.71 target).

use std::fs::File;
use std::io::BufReader;
use std::process::Command;

use ymir_biome::{Biome, BiomeMap};
use ymir_climate::ClimateMap;
use ymir_storage::load_bin;
use ymir_surface::skeleton::SkeletonWorld;

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

    // Load the persisted skeleton, climate, and biome artifacts directly;
    // this avoids parsing CLI output and matches how downstream tooling will
    // consume the world directory.
    let skeleton_path = world_dir.join("skeleton.bin");
    let skeleton_file = File::open(&skeleton_path).expect("open skeleton.bin");
    let skeleton: SkeletonWorld =
        bincode::deserialize_from(BufReader::new(skeleton_file)).expect("deserialize skeleton.bin");
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

    // --- Assertion 2: ocean-tile fraction is within 5 percentage points of
    // 0.71 (Earth's observed ocean fraction). This relies on the
    // continental_fraction = 0.29 override wired through CAT-04 and the
    // heightmap calibration pass that uses it.
    let elevations = &skeleton.elevation.elevations_m;
    assert!(!elevations.is_empty(), "elevation field is empty");
    let ocean = elevations.iter().filter(|&&e| e < 0.0).count();
    let ocean_fraction = ocean as f64 / elevations.len() as f64;
    assert!(
        (ocean_fraction - 0.71).abs() <= 0.05,
        "ocean-tile fraction out of Earth-like band (0.66..=0.76): \
         got {ocean_fraction:.4} ({ocean}/{})",
        elevations.len()
    );

    // --- Assertion 3: biome mix covers forests, open land, desert, cold. --
    //
    // With BIOME-05 landed, the water overlay also paints Ocean and
    // CoastalShallow tiles; we check those separately below.
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

    // --- Assertion 4: biome-level water fraction (BIOME-05 overlay). ------
    //
    // The water overlay paints Ocean and CoastalShallow for every
    // sub-sea-level tile before the Markov smoother runs. On Earth @ seed 1
    // the skeleton's ocean-tile fraction is 0.71; the biome histogram should
    // report a ≥50% water fraction after smoothing (some coastal tiles may
    // be nibbled by Markov in either direction, so we leave headroom).
    let water_tiles = biomes
        .per_tile
        .iter()
        .filter(|b| matches!(b, Biome::Ocean | Biome::CoastalShallow))
        .count();
    let water_fraction = water_tiles as f64 / biomes.per_tile.len() as f64;
    assert!(
        water_fraction >= 0.50,
        "biome-level water fraction (Ocean+CoastalShallow) should be ≥0.50, \
         got {water_fraction:.4} ({water_tiles}/{}); histogram: {hist:?}",
        biomes.per_tile.len()
    );
}
