//! Override progression integration test (VALID-04).
//!
//! Exercises the `generate → regenerate (with override)` flow and verifies the
//! dirty-propagation contract: a stage-N override rebuilds stage N and every
//! downstream stage, while every upstream artifact stays byte-identical on
//! disk.
//!
//! Phase 2 layout caveat (see NOTE under CLI-03 in TASKS.md): stages 1-3
//! (stellar, system, atmosphere) do not have separate `.bin` files; they live
//! inside `skeleton.bin`. So the cleanest way to demonstrate upstream
//! byte-identity is to override at the climate stage (stage 5), which leaves
//! `skeleton.bin` and `preview.png` untouched and rebuilds `climate.bin`,
//! `biomes.bin`, and `preview_biome.png`.
//!
//! To force a real climate change (rather than a no-op merge), the test
//! loads the freshly-generated `ClimateMap`, shifts every per-tile
//! temperature upward by a large amount, and writes the shifted map as the
//! climate override. The merge path in `run_regenerate` replaces
//! `climate.temperature.per_tile_k` wholesale, so the new `climate.bin`
//! carries shifted temperatures and the downstream biome classification
//! moves across Whittaker bands on many tiles.

use std::process::Command;

use ymir_climate::ClimateMap;
use ymir_storage::load_bin;

/// Temperature shift in Kelvin applied to every tile in the override.
/// Large enough that many tiles cross Whittaker temperature bands, guaranteeing
/// a different `biomes.bin` and `preview_biome.png`.
const TEMPERATURE_SHIFT_K: f64 = 50.0;

#[test]
fn override_progression_preserves_upstream_byte_identity() {
    let bin = env!("CARGO_BIN_EXE_ymir");
    let tmp = tempfile::tempdir().expect("tempdir");
    let world_dir = tmp.path().join("earth_world");

    // 1. Generate a baseline Earth world.
    let gen_output = Command::new(bin)
        .args(["generate", "--star", "Earth", "--seed", "1", "--output"])
        .arg(&world_dir)
        .output()
        .expect("failed to exec ymir generate");
    assert!(
        gen_output.status.success(),
        "ymir generate failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
        gen_output.status,
        String::from_utf8_lossy(&gen_output.stdout),
        String::from_utf8_lossy(&gen_output.stderr),
    );

    // 2. Snapshot every artifact's bytes for pre/post comparison.
    let skeleton_before = std::fs::read(world_dir.join("skeleton.bin")).expect("read skeleton.bin");
    let climate_before = std::fs::read(world_dir.join("climate.bin")).expect("read climate.bin");
    let biomes_before = std::fs::read(world_dir.join("biomes.bin")).expect("read biomes.bin");
    let preview_before = std::fs::read(world_dir.join("preview.png")).expect("read preview.png");
    let biome_png_before =
        std::fs::read(world_dir.join("preview_biome.png")).expect("read preview_biome.png");

    // 3. Build a climate override that shifts every per-tile temperature by
    //    `TEMPERATURE_SHIFT_K`. Overrides in `ymir regenerate` merge the
    //    supplied JSON into the freshly-computed stage output, with wholesale
    //    replacement for arrays; replacing `temperature.per_tile_k` drives
    //    climate.bin to a new value and shifts biomes downstream.
    let mut climate: ClimateMap =
        load_bin(world_dir.join("climate.bin")).expect("load climate.bin");
    for t in climate.temperature.per_tile_k.iter_mut() {
        *t += TEMPERATURE_SHIFT_K;
    }
    let climate_json = serde_json::to_value(&climate).expect("serialize climate to json");

    let override_doc = serde_json::json!({
        "version": "1.0",
        "target_star": "Sol",
        "target_planet": "Earth",
        "overrides": {
            "climate": climate_json
        }
    });
    let override_path = tmp.path().join("override_climate.json");
    std::fs::write(
        &override_path,
        serde_json::to_string(&override_doc).expect("serialize override"),
    )
    .expect("write override file");

    // 4. Regenerate with the override.
    let regen_output = Command::new(bin)
        .args(["regenerate", "--world"])
        .arg(&world_dir)
        .arg("--overrides")
        .arg(&override_path)
        .output()
        .expect("failed to exec ymir regenerate");
    assert!(
        regen_output.status.success(),
        "ymir regenerate failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
        regen_output.status,
        String::from_utf8_lossy(&regen_output.stdout),
        String::from_utf8_lossy(&regen_output.stderr),
    );

    // 5. Re-snapshot and assert the dirty-propagation contract.
    let skeleton_after = std::fs::read(world_dir.join("skeleton.bin")).expect("read skeleton.bin");
    let climate_after = std::fs::read(world_dir.join("climate.bin")).expect("read climate.bin");
    let biomes_after = std::fs::read(world_dir.join("biomes.bin")).expect("read biomes.bin");
    let preview_after = std::fs::read(world_dir.join("preview.png")).expect("read preview.png");
    let biome_png_after =
        std::fs::read(world_dir.join("preview_biome.png")).expect("read preview_biome.png");

    // Upstream (stages 1-4) byte-identical. skeleton.bin bundles stellar +
    // system + atmosphere + skeleton in Phase 2, and preview.png is the
    // elevation render derived only from the (unchanged) skeleton.
    assert_eq!(
        skeleton_before, skeleton_after,
        "skeleton.bin must be byte-identical after a climate-stage override"
    );
    assert_eq!(
        preview_before, preview_after,
        "preview.png must be byte-identical after a climate-stage override"
    );

    // Downstream (stages 5-7) differ: climate changed, biomes recomputed from
    // shifted temperatures, biome PNG re-rendered from shifted biomes.
    assert_ne!(
        climate_before, climate_after,
        "climate.bin must differ after applying a climate override"
    );
    assert_ne!(
        biomes_before, biomes_after,
        "biomes.bin must differ (it is downstream of the climate override)"
    );
    assert_ne!(
        biome_png_before, biome_png_after,
        "preview_biome.png must differ (it is downstream of the climate override)"
    );

    // Manifest now records the override file.
    let manifest_text =
        std::fs::read_to_string(world_dir.join("manifest.json")).expect("read manifest.json");
    assert!(
        manifest_text.contains("overrides_file"),
        "manifest must carry an overrides_file pointer after regenerate; contents:\n{manifest_text}"
    );
    assert!(
        manifest_text.contains("override_climate.json"),
        "manifest must reference the override file by name; contents:\n{manifest_text}"
    );

    // Sanity-check the new climate.bin actually reflects the shifted values;
    // this guards against a silent merge-regression where the override is
    // parsed but not applied.
    let climate_reloaded: ClimateMap =
        load_bin(world_dir.join("climate.bin")).expect("reload climate.bin");
    assert_eq!(
        climate_reloaded.temperature.per_tile_k.len(),
        climate.temperature.per_tile_k.len(),
        "tile count must match between pre-override and post-regenerate climate"
    );
    // Every tile should match the shifted value (within f64 roundtrip jitter).
    for (i, (got, want)) in climate_reloaded
        .temperature
        .per_tile_k
        .iter()
        .zip(climate.temperature.per_tile_k.iter())
        .enumerate()
    {
        assert!(
            (got - want).abs() < 1e-9,
            "tile {i} temperature after regenerate ({got}) does not match override ({want})"
        );
    }
}
