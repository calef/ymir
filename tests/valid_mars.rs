//! Mars validation integration test (VALID-02).
//!
//! Drives the full `ymir generate --star Mars --seed 1` pipeline through the
//! compiled binary, then loads the persisted artifacts and asserts Mars-like
//! outputs:
//!
//! 1. Mars-like biome palette (no forests, grasslands, oceans, wetlands);
//!    every tile lives in the `MarsLike` palette (MartianDustPlain,
//!    MartianBedrock, MartianPolarIce, ImpactRegolith).
//! 2. Mean surface temperature below 230 K.
//! 3. CO2-dominated atmosphere (ThinCO2 or ThickCO2).
//!
//! The Mars atmosphere is embedded in `skeleton.bin` (via `SkeletonWorld`)
//! rather than being written to a separate `atmosphere.bin`, so this test
//! loads the skeleton and reads `skeleton.atmosphere.class` from it.

use std::process::Command;

use ymir_atmosphere::composition::AtmosphereClass;
use ymir_biome::{Biome, BiomeMap};
use ymir_climate::ClimateMap;
use ymir_storage::load_bin;
use ymir_surface::SkeletonWorld;

/// Run the full Mars pipeline and assert Mars-like output thresholds.
///
/// Invokes the compiled `ymir` binary (resolved via `CARGO_BIN_EXE_ymir`) in
/// a fresh tempdir so the test exercises the same CLI surface users see.
#[test]
fn mars_validation_passes_basic_thresholds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let world_dir = tmp.path().join("mars_world");

    let bin = env!("CARGO_BIN_EXE_ymir");
    let output = Command::new(bin)
        .arg("generate")
        .arg("--star")
        .arg("Mars")
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

    // Mars skips per-planet atmosphere.bin; atmosphere lives inside
    // SkeletonWorld. Load it from skeleton.bin.
    let skeleton: SkeletonWorld =
        load_bin(world_dir.join("skeleton.bin")).expect("load skeleton.bin");
    let climate: ClimateMap = load_bin(world_dir.join("climate.bin")).expect("load climate.bin");
    let biomes: BiomeMap = load_bin(world_dir.join("biomes.bin")).expect("load biomes.bin");

    // --- Assertion 1: Mars-like palette, no Earth-like variants. -----------
    let hist = biomes.histogram();
    let forbidden: &[Biome] = &[
        Biome::TropicalRainforest,
        Biome::TemperateForest,
        Biome::BorealForest,
        Biome::Grassland,
        Biome::Savanna,
        Biome::Wetland,
        Biome::CoastalShallow,
        Biome::Ocean,
    ];
    for (biome, count) in &hist {
        assert!(
            !forbidden.contains(biome),
            "Mars should not produce Earth-like biome {biome:?} ({count} tiles); histogram: {hist:?}"
        );
    }

    let mars_palette: &[Biome] = &[
        Biome::MartianDustPlain,
        Biome::MartianBedrock,
        Biome::MartianPolarIce,
        Biome::ImpactRegolith,
    ];
    for (biome, count) in &hist {
        assert!(
            mars_palette.contains(biome),
            "Mars produced non-MarsLike biome {biome:?} ({count} tiles); histogram: {hist:?}"
        );
    }

    // --- Assertion 2: mean surface temperature below 230 K. ----------------
    let temps = &climate.temperature.per_tile_k;
    assert!(!temps.is_empty(), "temperature field is empty");
    let mean_t = temps.iter().sum::<f64>() / temps.len() as f64;
    assert!(
        mean_t < 230.0,
        "expected Mars mean surface T < 230 K, got {mean_t:.2} K"
    );

    // --- Assertion 3: CO2-dominated atmosphere. ----------------------------
    let class = skeleton.atmosphere.class;
    assert!(
        matches!(class, AtmosphereClass::ThinCO2 | AtmosphereClass::ThickCO2),
        "expected CO2-dominated atmosphere (ThinCO2 or ThickCO2), got {class:?}"
    );
}
