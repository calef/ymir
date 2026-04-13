//! Integration tests for the `ymir list-stars` and `ymir describe-star`
//! subcommands (CAT-09).
//!
//! Both commands exercise the `Catalog` facade against the committed
//! fixture files at `crates/ymir-catalog/fixtures/` so the tests are
//! hermetic and don't depend on the full 100pc Gaia download.
//!
//! Driven via `CARGO_BIN_EXE_ymir` + `std::process::Command` (matching
//! the house style established by `tests/regenerate.rs`) rather than
//! pulling in `assert_cmd` as a dev-dep.

use std::process::Command;

/// Spawn `ymir` with the given args and return (exit_code, stdout, stderr).
fn run_ymir(args: &[&str]) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_ymir");
    let out = Command::new(bin)
        .args(args)
        .output()
        .expect("failed to exec ymir");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn list_stars_fixture_prints_rows() {
    let (code, stdout, stderr) = run_ymir(&[
        "list-stars",
        "--catalog-dir",
        "crates/ymir-catalog/fixtures",
        "--limit",
        "5",
    ]);
    assert_eq!(code, 0, "exit={code} stderr={stderr}");
    // Header line always present.
    assert!(
        stdout.contains("gaia_id") && stdout.contains("spectral") && stdout.contains("distance_pc"),
        "expected header columns, got:\n{stdout}"
    );
    // Count data rows: total lines minus header + separator.
    let data_rows = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
        .saturating_sub(2);
    assert_eq!(
        data_rows, 5,
        "expected 5 data rows under --limit 5, got {data_rows}:\n{stdout}"
    );
}

#[test]
fn list_stars_spectral_filter() {
    let (code, stdout, stderr) = run_ymir(&[
        "list-stars",
        "--catalog-dir",
        "crates/ymir-catalog/fixtures",
        "--spectral-type",
        "M",
        "--limit",
        "100",
    ]);
    assert_eq!(code, 0, "exit={code} stderr={stderr}");

    // Skip the header + separator, then every remaining non-empty row's
    // spectral column must start with 'M'.
    let mut data_lines = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .skip(2)
        .peekable();
    assert!(data_lines.peek().is_some(), "no data rows:\n{stdout}");
    for line in data_lines {
        // The table is fixed-width: gaia_id column is 20 chars, then a
        // space, then name is 24 chars, then a space, then spectral (8).
        // Some common names contain spaces ("Proxima Centauri") so we
        // cannot use whitespace splitting. Slice the spectral column by
        // its byte offset instead.
        let spectral_start = 20 + 1 + 24 + 1;
        assert!(
            line.len() > spectral_start,
            "row shorter than expected column layout: '{line}'"
        );
        let spectral = line[spectral_start..]
            .split_whitespace()
            .next()
            .unwrap_or("");
        assert!(
            spectral.starts_with('M'),
            "spectral filter leaked non-M row: spectral='{spectral}' line='{line}'"
        );
    }
}

#[test]
fn describe_star_sol() {
    let (code, stdout, stderr) = run_ymir(&[
        "describe-star",
        "Sol",
        "--catalog-dir",
        "crates/ymir-catalog/fixtures",
    ]);
    assert_eq!(code, 0, "exit={code} stderr={stderr}");
    // Sol-specific: name and the HZ label must both appear.
    assert!(
        stdout.contains("Sol") || stdout.contains("Sun"),
        "expected Sol/Sun in output:\n{stdout}"
    );
    assert!(
        stdout.contains("L_sun"),
        "expected L_sun in output:\n{stdout}"
    );
    assert!(
        stdout.contains("Habitable Zone"),
        "expected Habitable Zone section:\n{stdout}"
    );
}

#[test]
fn describe_star_fixture_hit() {
    let (code, stdout, stderr) = run_ymir(&[
        "describe-star",
        "Epsilon Eridani",
        "--catalog-dir",
        "crates/ymir-catalog/fixtures",
    ]);
    assert_eq!(code, 0, "exit={code} stderr={stderr}");
    // Epsilon Eridani's Gaia source_id must appear in the header line.
    assert!(
        stdout.contains("5164707970261890560"),
        "expected Epsilon Eridani's Gaia ID in:\n{stdout}"
    );
    // HZ bounds must be non-zero (both inner and outer).
    let inner_line = stdout
        .lines()
        .find(|l| l.contains("Inner:"))
        .expect("HZ Inner line");
    let outer_line = stdout
        .lines()
        .find(|l| l.contains("Outer:"))
        .expect("HZ Outer line");
    let parse_au = |s: &str| -> f64 {
        s.split_whitespace()
            .find_map(|t| t.parse::<f64>().ok())
            .expect("no numeric token in HZ line")
    };
    let inner = parse_au(inner_line);
    let outer = parse_au(outer_line);
    assert!(
        inner > 0.0 && outer > 0.0 && inner < outer,
        "HZ bounds invalid: inner={inner} outer={outer}"
    );
}

#[test]
fn describe_star_unknown() {
    let (code, stdout, stderr) = run_ymir(&[
        "describe-star",
        "ThisStarDoesNotExist",
        "--catalog-dir",
        "crates/ymir-catalog/fixtures",
    ]);
    assert_ne!(
        code, 0,
        "unknown star should exit non-zero; stdout={stdout}"
    );
    assert!(
        stderr.contains("No match"),
        "expected 'No match' in stderr, got: {stderr}"
    );
}
