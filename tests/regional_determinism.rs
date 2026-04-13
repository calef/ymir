//! Regional determinism integration test (VALID-05).
//!
//! Proves that `ymir detail` seeds its PRNG via `hash(world_seed,
//! tile_index)` so that two fresh runs of the same world + the same set of
//! region tile indices produce byte-identical `region_NNNN.bin` files.
//!
//! This is the integration-level counterpart to DET-01's unit test: it
//! exercises the full CLI surface users see (`ymir generate` followed by
//! repeated `ymir detail --region N` invocations), on two independent
//! tempdirs, and compares the persisted bytes.

use std::fs;
use std::process::Command;

use tempfile::TempDir;

/// Run `ymir generate --star Earth --seed 42 --output <dir>` against the
/// compiled binary.
fn generate_world(world_dir: &std::path::Path) {
    let bin = env!("CARGO_BIN_EXE_ymir");
    let output = Command::new(bin)
        .arg("generate")
        .arg("--star")
        .arg("Earth")
        .arg("--seed")
        .arg("42")
        .arg("--output")
        .arg(world_dir)
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

/// Run `ymir detail --world <dir> --region <tile>` for a single tile.
fn run_detail(world_dir: &std::path::Path, tile: u32) {
    let bin = env!("CARGO_BIN_EXE_ymir");
    let output = Command::new(bin)
        .arg("detail")
        .arg("--world")
        .arg(world_dir)
        .arg("--region")
        .arg(tile.to_string())
        .output()
        .expect("failed to exec ymir detail");

    assert!(
        output.status.success(),
        "ymir detail --region {tile} failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Two independent tempdirs, same seed, same set of tile indices → byte-
/// identical `region_NNNN.bin` files.
///
/// We use three tile indices spanning the low/mid/high range of the default
/// subdivision-5 icosphere (10242 tiles) to cover a mix of positions. The
/// CLI accepts one `--region` per invocation, so we run `ymir detail` three
/// times per world.
#[test]
fn regional_detail_byte_identical_across_runs() {
    let tiles: [u32; 3] = [100, 200, 300];

    let dir_a = TempDir::new().expect("tempdir a");
    let world_a = dir_a.path().join("world");
    generate_world(&world_a);
    for tile in tiles {
        run_detail(&world_a, tile);
    }

    let dir_b = TempDir::new().expect("tempdir b");
    let world_b = dir_b.path().join("world");
    generate_world(&world_b);
    for tile in tiles {
        run_detail(&world_b, tile);
    }

    for tile in tiles {
        let name = format!("region_{tile:04}.bin");
        let path_a = world_a.join("detail").join(&name);
        let path_b = world_b.join("detail").join(&name);
        let bytes_a =
            fs::read(&path_a).unwrap_or_else(|e| panic!("read {}: {e}", path_a.display()));
        let bytes_b =
            fs::read(&path_b).unwrap_or_else(|e| panic!("read {}: {e}", path_b.display()));
        assert_eq!(
            bytes_a.len(),
            bytes_b.len(),
            "region {tile}: file sizes differ ({} vs {})",
            bytes_a.len(),
            bytes_b.len(),
        );
        assert_eq!(
            bytes_a,
            bytes_b,
            "region {tile}: bytes differ between runs (len {})",
            bytes_a.len(),
        );
    }
}
