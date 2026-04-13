//! Tidally locked validation integration test (VALID-03).
//!
//! Drives the full pipeline through the compiled binary against a known
//! tidally-locked configuration, then asserts the climate field shows the
//! expected radial signature:
//!
//! 1. The chosen body is actually tidally locked (`body.tidal_locked == true`).
//! 2. Substellar-point temperature exceeds antistellar temperature by a wide
//!    margin (>50 K threshold; in practice the gap is several hundred K for
//!    this airless target).
//! 3. Temperature varies more across longitude (radial distance from the
//!    substellar point) than across latitude. This is the defining signature
//!    of a tidally-locked climate: zoning is radial, not latitudinal.
//!
//! Target: `--star "Tau Ceti" --seed 1 --planet 0`. With Tau Ceti's mass
//! (0.783 M_sun) and age (5.8 Gyr), the tidal-lock distance is ~0.093 AU.
//! The placement algorithm with seed=1 generates a rocky body at ~0.055 AU,
//! inside that threshold, and `is_tidally_locked` returns true. The pipeline
//! then routes through the airless tidally-locked branches of temperature,
//! wind, moisture, and biome classification.

use std::f64::consts::PI;
use std::process::Command;

use ymir_climate::ClimateMap;
use ymir_storage::load_bin;
use ymir_surface::skeleton::SkeletonWorld;

/// Compute the great-circle angular distance in radians between two points
/// on the unit sphere given in (latitude, longitude) radians.
fn angular_distance(lat_a: f64, lon_a: f64, lat_b: f64, lon_b: f64) -> f64 {
    let cos_d = lat_a.sin() * lat_b.sin() + lat_a.cos() * lat_b.cos() * (lon_a - lon_b).cos();
    cos_d.clamp(-1.0, 1.0).acos()
}

/// Find the tile closest to the given (lat_rad, lon_rad) on the sphere.
/// Skeleton tiles store lat/lon in degrees, so we convert before comparing.
fn nearest_tile(skeleton: &SkeletonWorld, target_lat_rad: f64, target_lon_rad: f64) -> usize {
    let mut best_idx = 0;
    let mut best_d = f64::INFINITY;
    for (i, t) in skeleton.grid.tiles.iter().enumerate() {
        let d = angular_distance(
            t.lat.to_radians(),
            t.lon.to_radians(),
            target_lat_rad,
            target_lon_rad,
        );
        if d < best_d {
            best_d = d;
            best_idx = i;
        }
    }
    best_idx
}

#[test]
fn tidally_locked_shows_radial_temperature_gradient() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let world_dir = tmp.path().join("locked_world");

    let bin = env!("CARGO_BIN_EXE_ymir");
    let output = Command::new(bin)
        .arg("generate")
        .arg("--star")
        .arg("Tau Ceti")
        .arg("--seed")
        .arg("1")
        .arg("--planet")
        .arg("0")
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

    let skeleton: SkeletonWorld =
        load_bin(world_dir.join("skeleton.bin")).expect("load skeleton.bin");
    let climate: ClimateMap = load_bin(world_dir.join("climate.bin")).expect("load climate.bin");

    // --- Assertion 1: body is actually tidally locked. -------------------
    //
    // If this fires, the catalog / placement / tidal-lock threshold has
    // shifted and we need to re-pick a target. See the NOTE under VALID-03.
    assert!(
        *skeleton.body.tidal_locked.inner(),
        "target body should be tidally locked; got tidal_locked=false at {:.4} AU around {}",
        skeleton.body.semi_major_axis.inner(),
        skeleton
            .atmosphere
            .composition
            .inner()
            .keys()
            .map(|g| format!("{g:?}"))
            .collect::<Vec<_>>()
            .join(","),
    );

    // --- Assertion 2: substellar point >> antistellar point. -------------
    //
    // `ClimateConfig::default()` places the substellar point at
    // (lat=0, lon=0). The antistellar point is then at (lat=0, lon=PI).
    let sub_idx = nearest_tile(&skeleton, 0.0, 0.0);
    let anti_idx = nearest_tile(&skeleton, 0.0, PI);

    let t_sub = climate.temperature.per_tile_k[sub_idx];
    let t_anti = climate.temperature.per_tile_k[anti_idx];
    let delta = t_sub - t_anti;

    assert!(
        delta > 50.0,
        "expected substellar-antistellar gap > 50 K; got T_sub={t_sub:.1} K, \
         T_anti={t_anti:.1} K, delta={delta:.1} K"
    );

    // --- Assertion 3: radial zoning dominates latitudinal banding. --------
    //
    // For a tidally locked world, temperature should correlate strongly with
    // angular distance from the substellar point (a radial signature) and
    // weakly with latitude alone. We measure that by comparing:
    //
    //   - mean |dT| over pairs at the same latitude but opposite longitudes
    //     (spans the radial axis),
    //   - mean |dT| over pairs at the same longitude but opposite latitudes
    //     (spans the latitudinal axis only).
    //
    // The radial-axis mean should be larger. Uses a fixed grid of sample
    // latitudes/longitudes for determinism; no RNG.
    let sample_lats_deg = [-60.0_f64, -30.0, 0.0, 30.0, 60.0];
    let sample_lons_deg = [-150.0_f64, -120.0, -60.0, 60.0, 120.0, 150.0];

    // Equal-latitude pairs: (lat, lon) vs (lat, lon + 180 deg).
    let mut eq_lat_sum = 0.0;
    let mut eq_lat_count = 0usize;
    for &lat_deg in &sample_lats_deg {
        for &lon_deg in &sample_lons_deg {
            let lat = lat_deg.to_radians();
            let lon_a = lon_deg.to_radians();
            let lon_b = (lon_deg + 180.0).to_radians();
            let ia = nearest_tile(&skeleton, lat, lon_a);
            let ib = nearest_tile(&skeleton, lat, lon_b);
            if ia == ib {
                continue;
            }
            let dt =
                (climate.temperature.per_tile_k[ia] - climate.temperature.per_tile_k[ib]).abs();
            eq_lat_sum += dt;
            eq_lat_count += 1;
        }
    }
    assert!(
        eq_lat_count > 0,
        "failed to collect any equal-latitude pairs"
    );
    let eq_lat_mean = eq_lat_sum / eq_lat_count as f64;

    // Equal-longitude pairs: (lat, lon) vs (-lat, lon). Skip lat=0.
    let mut eq_lon_sum = 0.0;
    let mut eq_lon_count = 0usize;
    for &lat_deg in &sample_lats_deg {
        if lat_deg.abs() < 1e-6 {
            continue;
        }
        for &lon_deg in &sample_lons_deg {
            let lat_a = lat_deg.to_radians();
            let lat_b = (-lat_deg).to_radians();
            let lon = lon_deg.to_radians();
            let ia = nearest_tile(&skeleton, lat_a, lon);
            let ib = nearest_tile(&skeleton, lat_b, lon);
            if ia == ib {
                continue;
            }
            let dt =
                (climate.temperature.per_tile_k[ia] - climate.temperature.per_tile_k[ib]).abs();
            eq_lon_sum += dt;
            eq_lon_count += 1;
        }
    }
    assert!(
        eq_lon_count > 0,
        "failed to collect any equal-longitude pairs"
    );
    let eq_lon_mean = eq_lon_sum / eq_lon_count as f64;

    assert!(
        eq_lat_mean > eq_lon_mean,
        "expected radial (equal-lat) temperature spread to exceed latitudinal \
         (equal-lon) spread for a tidally locked world; got equal-lat mean |dT| \
         = {eq_lat_mean:.1} K, equal-lon mean |dT| = {eq_lon_mean:.1} K"
    );

    // For the tidally locked model specifically, the substellar side uses a
    // cos(angle) scaling against a high solar irradiance while the nightside
    // clamps to an antistellar floor. The radial axis should win by a large
    // margin, not a hairline. Require at least 2x separation; the actual
    // observed ratio for this Tau Ceti case is ~25x.
    assert!(
        eq_lat_mean > 2.0 * eq_lon_mean,
        "radial spread should dominate latitudinal spread by at least 2x; \
         got equal-lat={eq_lat_mean:.1} K, equal-lon={eq_lon_mean:.1} K"
    );
}
