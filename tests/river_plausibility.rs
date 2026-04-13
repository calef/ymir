//! River plausibility integration test (VALID-06).
//!
//! Generates an Earth world, selects a region seed tile that contains a
//! continent (non-zero land fraction), runs `ymir detail` on that tile, loads
//! the persisted `region_NNNN.bin`, and asserts three plausibility invariants:
//!
//! (a) No uphill flow: for every hex with `flow_accumulation > 0`, its
//!     downstream hex has `filled_elevation_m` less than or equal to the
//!     hex's own. Planchon-Darboux fills pits to exactly the outlet lip, so
//!     strictly-less is too strong for lake-interior hexes; we use `<=`.
//!
//! (b) Every flow path terminates in a lake or at the region boundary within
//!     `n` steps, where `n` is the region's hex count. Walks that fall off
//!     the grid (`downstream = None`) or land in a lake (`is_lake = true`)
//!     count as terminated.
//!
//! (c) At least one hex exceeds the river-accumulation threshold (50.0), so
//!     the region actually contains river-like drainage and isn't just flat
//!     land or open ocean.

use std::fs::File;
use std::io::BufReader;
use std::process::Command;
use std::time::Instant;

use tempfile::TempDir;
use ymir_detail::RegionalDetail;
use ymir_storage::load_bin;
use ymir_surface::skeleton::SkeletonWorld;

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

/// Run `ymir detail --world <dir> --region <tile> --radius 1`.
fn run_detail(world_dir: &std::path::Path, tile: u32) {
    let bin = env!("CARGO_BIN_EXE_ymir");
    let output = Command::new(bin)
        .arg("detail")
        .arg("--world")
        .arg(world_dir)
        .arg("--region")
        .arg(tile.to_string())
        .arg("--radius")
        .arg("1")
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

/// Pick a skeleton tile whose own elevation is comfortably above sea level.
///
/// Scans tiles in index order (starting at 0), picking the first tile whose
/// elevation exceeds `min_elev_m`. Subdivision-5 Earth has ~10k tiles and
/// ~29% land, so a low-index land tile is easy to find.
fn first_continent_tile(skeleton: &SkeletonWorld, min_elev_m: f64) -> Option<u32> {
    skeleton
        .elevation
        .elevations_m
        .iter()
        .enumerate()
        .find(|&(_, &e)| e > min_elev_m)
        .map(|(i, _)| i as u32)
}

#[test]
fn detail_region_rivers_flow_downhill() {
    let dir = TempDir::new().expect("tempdir");
    let world_dir = dir.path().join("world");

    let t0 = Instant::now();
    generate_world(&world_dir);

    // Load the skeleton so we can pick a region seed tile that's on a
    // continent rather than open ocean.
    let skeleton_path = world_dir.join("skeleton.bin");
    let skeleton: SkeletonWorld = bincode::deserialize_from(BufReader::new(
        File::open(&skeleton_path).expect("open skeleton.bin"),
    ))
    .expect("deserialize skeleton.bin");

    // Require at least 200 m of parent-tile elevation so the seed tile is
    // solidly on land; this biases the detail region toward interior land
    // rather than a coastal ocean-heavy mix.
    let target_tile =
        first_continent_tile(&skeleton, 200.0).expect("no land tile found in Earth @ seed 42");

    run_detail(&world_dir, target_tile);
    let wall_time = t0.elapsed();

    // Load the persisted region bincode blob.
    let region_path = world_dir
        .join("detail")
        .join(format!("region_{target_tile:04}.bin"));
    let region: RegionalDetail =
        load_bin(&region_path).unwrap_or_else(|e| panic!("load {}: {e}", region_path.display()));

    let n = region.hex_grid.cells.len();
    let flow = &region.flow;
    assert_eq!(flow.downstream.len(), n, "downstream length mismatch");
    assert_eq!(
        flow.flow_accumulation.len(),
        n,
        "flow_accumulation length mismatch"
    );
    assert_eq!(
        flow.filled_elevation_m.len(),
        n,
        "filled_elevation_m length mismatch"
    );
    assert_eq!(flow.is_lake.len(), n, "is_lake length mismatch");

    // Compute the hex-level land fraction using DET-03's detail elevation; we
    // want a meaningful amount of land in the region, not just the seed tile.
    let land_hexes = region
        .elevation
        .per_hex_m
        .iter()
        .filter(|&&e| e > 0.0)
        .count();
    let land_fraction = land_hexes as f64 / n as f64;
    assert!(
        land_fraction > 0.05,
        "region {target_tile} has too little land for a meaningful river test \
         (land_fraction = {land_fraction:.4}, hexes = {n})",
    );

    // --- (a) No uphill flow. ------------------------------------------------
    //
    // Planchon-Darboux fills pits to exactly the outlet lip, so lake-interior
    // hexes legitimately drain to a neighbour at the same filled elevation;
    // we use `<=` rather than strict `<`.
    let mut uphill_checked = 0usize;
    for (i, down) in flow.downstream.iter().enumerate() {
        if flow.flow_accumulation[i] <= 0.0 {
            continue;
        }
        if let Some(d) = down {
            let d = *d as usize;
            assert!(
                flow.filled_elevation_m[d] <= flow.filled_elevation_m[i],
                "hex {i} flows uphill to {d}: {} -> {} (acc = {})",
                flow.filled_elevation_m[i],
                flow.filled_elevation_m[d],
                flow.flow_accumulation[i],
            );
            uphill_checked += 1;
        }
    }
    assert!(
        uphill_checked > 0,
        "no hexes had both positive accumulation and an interior downstream; \
         can't validate the no-uphill invariant"
    );

    // --- (b) Every flow path terminates in <= n steps. ---------------------
    //
    // A path terminates when it lands on a lake hex, walks off the region
    // (`downstream = None`), or exceeds `n` steps (which would indicate a
    // cycle and is a test failure).
    for start in 0..n {
        let mut cur = start;
        let mut steps = 0usize;
        // A hex that starts inside a lake is already terminated.
        if flow.is_lake[cur] {
            continue;
        }
        while let Some(d) = flow.downstream[cur] {
            cur = d as usize;
            steps += 1;
            if flow.is_lake[cur] {
                break;
            }
            assert!(
                steps <= n,
                "flow path from {start} exceeded {n} steps (cycle suspected)"
            );
        }
    }

    // --- (c) At least one basin above the river threshold exists. ---------
    let threshold: f32 = 50.0;
    let basin_count = flow
        .flow_accumulation
        .iter()
        .filter(|&&a| a > threshold)
        .count();
    assert!(
        basin_count > 0,
        "no hex exceeded flow accumulation threshold {threshold} in region \
         {target_tile} (land fraction {land_fraction:.4}, {n} hexes)"
    );

    // The test body never fails silently past this point; emit a one-line
    // summary so CI logs show what was actually validated.
    eprintln!(
        "VALID-06 OK: tile={target_tile} hexes={n} land_fraction={land_fraction:.4} \
         basins_above_{threshold}={basin_count} uphill_checks={uphill_checked} \
         wall_time={wall_time:.2?}",
    );
}
