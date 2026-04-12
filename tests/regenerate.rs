//! Integration tests for the `ymir regenerate` subcommand (CLI-03).
//!
//! Tests that:
//! 1. A climate-stage override leaves `skeleton.bin` byte-identical while
//!    recomputing climate and biomes (plus updating the manifest with an
//!    `overrides_file` pointer).
//! 2. An atmosphere-stage override propagates through downstream stages,
//!    producing a new skeleton when the merged atmosphere differs from the
//!    derived one (and leaves the manifest updated in all cases).
//!
//! Both tests drive the compiled `ymir` binary (resolved via
//! `CARGO_BIN_EXE_ymir`) so they exercise the same CLI surface users see.

use std::fs;
use std::path::Path;
use std::process::Command;

/// Run `ymir generate --star Earth --seed 1 --output <dir>` and panic on
/// failure, returning the captured output for diagnostics if needed.
fn run_generate_earth(out: &Path) {
    let bin = env!("CARGO_BIN_EXE_ymir");
    let output = Command::new(bin)
        .args(["generate", "--star", "Earth", "--seed", "1", "--output"])
        .arg(out)
        .output()
        .expect("failed to exec ymir generate");
    assert!(
        output.status.success(),
        "ymir generate failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Run `ymir regenerate --world <dir> --overrides <file>` and panic on
/// failure.
fn run_regenerate(world: &Path, overrides: &Path) {
    let bin = env!("CARGO_BIN_EXE_ymir");
    let output = Command::new(bin)
        .args(["regenerate", "--world"])
        .arg(world)
        .arg("--overrides")
        .arg(overrides)
        .output()
        .expect("failed to exec ymir regenerate");
    assert!(
        output.status.success(),
        "ymir regenerate failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// A climate-only override must not touch stages 1-4. We assert that
/// `skeleton.bin` and `preview.png` are byte-identical before and after
/// `ymir regenerate`, and that the manifest records the override file.
#[test]
fn regenerate_climate_override_leaves_skeleton_byte_identical() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let world_dir = tmp.path().join("earth_world");
    run_generate_earth(&world_dir);

    let skeleton_before = fs::read(world_dir.join("skeleton.bin")).expect("read skeleton");
    let preview_before = fs::read(world_dir.join("preview.png")).expect("read preview");

    // A no-op climate override (empty object) is enough to exercise the
    // "only stages 5-7 dirty" path. The merge is a deep-merge into the
    // computed ClimateMap, so an empty object leaves the map functionally
    // unchanged; what matters here is the dirty-stage plumbing.
    let override_path = tmp.path().join("override_climate.json");
    fs::write(
        &override_path,
        r#"{
            "version": "1.0",
            "target_star": "Sol",
            "target_planet": "Earth",
            "overrides": { "climate": {} }
        }"#,
    )
    .expect("write override file");

    run_regenerate(&world_dir, &override_path);

    // Skeleton and elevation preview must be untouched.
    let skeleton_after = fs::read(world_dir.join("skeleton.bin")).expect("read skeleton");
    let preview_after = fs::read(world_dir.join("preview.png")).expect("read preview");
    assert_eq!(
        skeleton_before, skeleton_after,
        "skeleton.bin changed after a climate-only override (should be clean)"
    );
    assert_eq!(
        preview_before, preview_after,
        "preview.png changed after a climate-only override (should be clean)"
    );

    // Manifest should now point at the override file.
    let manifest_json = fs::read_to_string(world_dir.join("manifest.json")).expect("read manifest");
    assert!(
        manifest_json.contains("override_climate.json"),
        "manifest missing overrides_file pointer; contents:\n{manifest_json}"
    );
}

/// An atmosphere-stage override marks stages 3-7 dirty. Skeleton must be
/// rebuilt (its file may change or stay the same depending on whether the
/// merged atmosphere equals the computed atmosphere), and the manifest
/// must record the override file. We focus on plumbing: every expected
/// artifact exists post-regenerate, and the manifest reflects the override.
#[test]
fn regenerate_atmosphere_override_updates_manifest_and_artifacts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let world_dir = tmp.path().join("earth_world");
    run_generate_earth(&world_dir);

    // Change surface_pressure in the computed atmosphere. This isn't a
    // physically consistent change (greenhouse + moisture wouldn't track)
    // but it's enough to exercise the "stages 3-7 dirty" path and prove
    // the override is actually applied to the stage output.
    let override_path = tmp.path().join("override_atmo.json");
    fs::write(
        &override_path,
        r#"{
            "version": "1.0",
            "target_star": "Sol",
            "target_planet": "Earth",
            "overrides": {
                "atmosphere": {
                    "surface_pressure": 0.5
                }
            }
        }"#,
    )
    .expect("write override file");

    run_regenerate(&world_dir, &override_path);

    // All expected artifacts still present.
    for name in [
        "manifest.json",
        "skeleton.bin",
        "climate.bin",
        "biomes.bin",
        "preview.png",
        "preview_biome.png",
    ] {
        assert!(
            world_dir.join(name).exists(),
            "missing artifact after regenerate: {name}"
        );
    }

    // Manifest records the override file path.
    let manifest_json = fs::read_to_string(world_dir.join("manifest.json")).expect("read manifest");
    assert!(
        manifest_json.contains("override_atmo.json"),
        "manifest missing overrides_file pointer; contents:\n{manifest_json}"
    );

    // Load skeleton and confirm the overridden surface_pressure was applied.
    let skeleton: ymir_surface::skeleton::SkeletonWorld =
        ymir_storage::load_bin(world_dir.join("skeleton.bin")).expect("load skeleton");
    assert!(
        (skeleton.atmosphere.surface_pressure - 0.5).abs() < 1e-9,
        "atmosphere surface_pressure should reflect override (0.5), got {}",
        skeleton.atmosphere.surface_pressure
    );
}
