//! Per-hex humidity refined from the global moisture field via a first-pass
//! orographic lift / rain-shadow model.
//!
//! The global [`ClimateMap`] (Stage 5) provides one humidity and one wind
//! vector per skeleton tile. Detail-scale terrain within a tile introduces
//! relief small enough that the global pass cannot resolve it, but large
//! enough to materially redistribute local precipitation: windward slopes
//! pull moisture out of rising air, leeward slopes bake in descending dry
//! air. [`DetailMoisture`] refines the per-hex humidity using each hex's
//! upwind/downwind elevation gradient in the [`HexGrid`].
//!
//! # Algorithm
//!
//! For each hex `h`:
//!
//! 1. Look up the baseline humidity `H_0 = climate.moisture.per_tile[h.parent_tile]`
//!    and the prevailing wind `W = climate.wind.per_tile[h.parent_tile]`.
//! 2. If `|W|` is below an epsilon (calm tile, airless world), skip
//!    orographic adjustment and set `H = clamp(H_0, 0, saturation_ceiling)`.
//! 3. Project the six neighbour directions onto the unit wind vector
//!    `\hat w` using tangent-plane geometry at `h`'s (lat, lon). The
//!    neighbour whose direction most closely aligns with `+\hat w` is the
//!    **downwind** hex; the neighbour most aligned with `-\hat w` is the
//!    **upwind** hex.
//! 4. Convert the elevation differences to kilometres using the body's
//!    radius (angular distance × radius gives arc length):
//!
//!    - `dh_up_km = (h.elev - upwind.elev) / 1000` (positive if air rose
//!      into this hex)
//!    - `dh_down_km = (upwind.elev - h.elev) / 1000` (positive if we sit
//!      on the leeward side of higher terrain upwind)
//!
//! 5. Apply lift when the air climbed onto this hex:
//!
//!    ```text
//!    H *= 1 + lift_coefficient * dh_up_km       (dh_up_km > 0)
//!    ```
//!
//! 6. Apply shadow when the hex is leeward of higher upwind terrain AND
//!    the downwind hex is lower still (descending dry air):
//!
//!    ```text
//!    H *= 1 - shadow_coefficient * dh_down_km   (dh_down_km > 0 and
//!                                                downwind.elev < h.elev)
//!    ```
//!
//! 7. Clamp the result to `[0, saturation_ceiling]`.
//!
//! This is a single-pass local approximation; it does not propagate
//! moisture deficits multiple hexes downwind. That refinement is a future
//! stage (the full advection solve belongs with CLIM-04).
//!
//! # Units
//!
//! Humidity is unitless relative humidity in `[0, 1]`, matching the
//! convention of [`ymir_climate::MoistureField`]. Elevations are in
//! metres. The `lift_coefficient` and `shadow_coefficient` are expressed
//! in per-km-of-elevation-change units so defaults generalise across
//! bodies with different relief scales.

use crate::DetailElevation;
use crate::hex_grid::HexGrid;
use crate::region::RegionSpec;
use serde::{Deserialize, Serialize};
use ymir_climate::ClimateMap;
use ymir_surface::skeleton::SkeletonWorld;

/// Minimum wind speed (m/s) below which a tile is treated as calm and
/// no orographic adjustment is applied. Matches the order of magnitude
/// of the wind-field's airless-world zero value.
const CALM_WIND_EPS: f64 = 1.0e-6;

/// Configuration for the detail-level orographic moisture refinement.
///
/// `lift_coefficient` and `shadow_coefficient` are expressed in per-km of
/// upwind rise (lift) or per-km of leeward fall (shadow). A 1 km ridge
/// upwind therefore multiplies the windward humidity by
/// `1 + lift_coefficient` and the leeward humidity by
/// `1 - shadow_coefficient`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct DetailMoistureConfig {
    /// Fractional humidity boost per km of upwind elevation rise.
    pub lift_coefficient: f64,
    /// Fractional humidity reduction per km of leeward elevation fall.
    pub shadow_coefficient: f64,
    /// Upper bound on per-hex humidity after adjustment. Typically 1.0
    /// (fully saturated relative humidity).
    pub saturation_ceiling: f64,
}

impl Default for DetailMoistureConfig {
    fn default() -> Self {
        Self {
            lift_coefficient: 0.5,
            shadow_coefficient: 0.3,
            saturation_ceiling: 1.0,
        }
    }
}

/// Per-hex refined humidity field in `[0, saturation_ceiling]` for every
/// cell in a [`HexGrid`].
///
/// `per_hex[i]` matches the ordering of `HexGrid::cells[i]`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetailMoisture {
    /// The region this moisture field was generated for.
    pub spec: RegionSpec,
    /// Relative humidity per hex (unitless, `[0, saturation_ceiling]`).
    pub per_hex: Vec<f64>,
}

impl DetailMoisture {
    /// Build refined moisture for every hex in `grid` by applying a
    /// first-pass orographic lift/shadow adjustment to the parent
    /// skeleton tile's baseline humidity.
    ///
    /// See the module docs for the full algorithm and formula.
    pub fn build(
        world: &SkeletonWorld,
        climate: &ClimateMap,
        grid: &HexGrid,
        elevation: &DetailElevation,
        config: DetailMoistureConfig,
    ) -> Self {
        assert_eq!(
            elevation.per_hex_m.len(),
            grid.cells.len(),
            "DetailElevation::per_hex_m length must match HexGrid::cells length"
        );
        assert_eq!(
            climate.moisture.per_tile.len(),
            world.tile_count(),
            "ClimateMap::moisture must be sized to world.tile_count()"
        );
        assert_eq!(
            climate.wind.per_tile.len(),
            world.tile_count(),
            "ClimateMap::wind must be sized to world.tile_count()"
        );

        let saturation_ceiling = config.saturation_ceiling.max(0.0);
        let mut per_hex = Vec::with_capacity(grid.cells.len());

        for (idx, cell) in grid.cells.iter().enumerate() {
            let parent = cell.parent_tile as usize;
            let baseline = climate.moisture.per_tile[parent];
            let wind = climate.wind.per_tile[parent];
            let wind_speed = wind.speed();

            let mut humidity = baseline;

            if wind_speed >= CALM_WIND_EPS {
                // Wind unit vector in tangent-plane (east, north).
                let wu = wind.u as f64 / wind_speed;
                let wv = wind.v as f64 / wind_speed;

                // Find the upwind and downwind neighbour by tangent-plane
                // alignment with ±(wu, wv).
                let self_elev = elevation.per_hex_m[idx];
                let (upwind_idx, upwind_align) = best_aligned_neighbor(grid, cell, idx, -wu, -wv);
                let (downwind_idx, downwind_align) = best_aligned_neighbor(grid, cell, idx, wu, wv);

                // Only adjust when we have a neighbour aligned at least
                // somewhat with the wind direction (cos θ ≥ 0.5, i.e.
                // within 60° of the wind). Otherwise the gradient
                // estimate is dominated by cross-wind neighbours.
                const ALIGN_MIN: f64 = 0.5;

                let upwind_elev = upwind_idx
                    .filter(|_| upwind_align >= ALIGN_MIN)
                    .map(|i| elevation.per_hex_m[i]);
                let downwind_elev = downwind_idx
                    .filter(|_| downwind_align >= ALIGN_MIN)
                    .map(|i| elevation.per_hex_m[i]);

                if let Some(up) = upwind_elev {
                    let dh_up_km = (self_elev - up) / 1000.0;
                    if dh_up_km > 0.0 {
                        // Air climbed onto this hex → orographic lift.
                        humidity *= 1.0 + config.lift_coefficient * dh_up_km;
                    } else if let Some(down) = downwind_elev {
                        // Leeward position: upwind is higher, and the
                        // air continues to descend toward downwind.
                        let dh_down_km = (-dh_up_km).max(0.0); // (up - self) km
                        if dh_down_km > 0.0 && down < self_elev {
                            humidity *= 1.0 - config.shadow_coefficient * dh_down_km;
                        }
                    }
                }
            }

            humidity = humidity.clamp(0.0, saturation_ceiling);
            per_hex.push(humidity);
        }

        DetailMoisture {
            spec: grid.region_spec.clone(),
            per_hex,
        }
    }
}

/// Find the neighbour of `cell` (if any) whose unit-direction in the
/// local tangent plane is most aligned with the target direction
/// `(tu, tv)` (east, north components of a unit vector).
///
/// Returns `(neighbour_index, cos_alignment)` where `cos_alignment` is
/// the dot product of the neighbour's unit direction with `(tu, tv)`.
/// If `cell` has no neighbours, returns `(None, -1.0)`.
fn best_aligned_neighbor(
    grid: &HexGrid,
    cell: &crate::hex_grid::HexCell,
    cell_idx: usize,
    tu: f64,
    tv: f64,
) -> (Option<usize>, f64) {
    let (east, north) = tangent_basis(cell.lat_rad, cell.lon_rad);
    let center = latlon_rad_to_xyz(cell.lat_rad, cell.lon_rad);

    let mut best: Option<usize> = None;
    let mut best_align = f64::NEG_INFINITY;

    for slot in grid.neighbors[cell_idx].iter().flatten() {
        let nb = &grid.cells[*slot as usize];
        let nb_xyz = latlon_rad_to_xyz(nb.lat_rad, nb.lon_rad);
        // Tangent-plane displacement from center to neighbour.
        let dx = [
            nb_xyz[0] - center[0],
            nb_xyz[1] - center[1],
            nb_xyz[2] - center[2],
        ];
        // Project onto east/north basis.
        let e = dx[0] * east[0] + dx[1] * east[1] + dx[2] * east[2];
        let n = dx[0] * north[0] + dx[1] * north[1] + dx[2] * north[2];
        let len = (e * e + n * n).sqrt();
        if len < 1.0e-12 {
            continue;
        }
        let eh = e / len;
        let nh = n / len;
        let align = eh * tu + nh * tv;
        if align > best_align {
            best_align = align;
            best = Some(*slot as usize);
        }
    }

    (best, best_align)
}

/// East/north tangent basis at (lat, lon) on the unit sphere. Matches
/// the convention used by [`crate::hex_grid`] and
/// [`ymir_climate::wind`].
fn tangent_basis(lat_rad: f64, lon_rad: f64) -> ([f64; 3], [f64; 3]) {
    let cos_lat = lat_rad.cos();
    let sin_lat = lat_rad.sin();
    let cos_lon = lon_rad.cos();
    let sin_lon = lon_rad.sin();
    let east = [-sin_lon, cos_lon, 0.0];
    let north = [-sin_lat * cos_lon, -sin_lat * sin_lon, cos_lat];
    (east, north)
}

/// Convert (lat_rad, lon_rad) to unit-sphere xyz.
fn latlon_rad_to_xyz(lat: f64, lon: f64) -> [f64; 3] {
    let cl = lat.cos();
    [cl * lon.cos(), cl * lon.sin(), lat.sin()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DetailElevationConfig;
    use crate::hex_grid::{HexCell, HexGrid, HexGridConfig};
    use std::collections::BTreeMap;
    use ymir_atmosphere::atmosphere_model::AtmosphereModel;
    use ymir_atmosphere::composition::AtmosphereClass;
    use ymir_atmosphere::retention::Gas;
    use ymir_climate::{ClimateConfig, ClimateMap};
    use ymir_core::Sourced;
    use ymir_surface::skeleton::SkeletonWorld;
    use ymir_system::orbital_body::{OrbitalBody, PlanetType};

    fn d(v: f64) -> Sourced<f64> {
        Sourced::derived(v, "test")
    }

    fn earth_body() -> OrbitalBody {
        OrbitalBody {
            semi_major_axis: d(1.0),
            eccentricity: d(0.0167),
            inclination: d(0.0),
            axial_tilt: d(23.4),
            mass: d(1.0),
            radius: d(1.0),
            density: d(5.51),
            surface_gravity: d(9.81),
            solar_irradiance: d(1361.0),
            equilibrium_temp: d(254.0),
            tidal_locked: Sourced::derived(false, "test"),
            rotation_period: d(24.0),
            is_in_hz: true,
            planet_type: PlanetType::Terran,
            name: Some("Earth".into()),
            is_known_exoplanet: false,
            continental_fraction: None,
        }
    }

    fn earth_atmosphere() -> AtmosphereModel {
        let mut composition = BTreeMap::new();
        composition.insert(Gas::N2, 0.78);
        composition.insert(Gas::O2, 0.21);
        composition.insert(Gas::H2O, 0.01);
        AtmosphereModel {
            surface_pressure: d(1.0),
            composition: Sourced::derived(composition, "test"),
            greenhouse_factor: d(288.0 / 254.0),
            effective_surface_temp: d(288.0),
            scale_height: d(8.0),
            moisture_capacity: d(1.0),
            uv_surface_flux: d(0.05),
            class: AtmosphereClass::NitrogenOxygen,
            retained: vec![Gas::N2, Gas::O2, Gas::H2O],
        }
    }

    fn earth_world(seed: u64) -> SkeletonWorld {
        SkeletonWorld::build(earth_body(), earth_atmosphere(), 3, seed)
    }

    fn build_pipeline(
        seed: u64,
        spec: RegionSpec,
    ) -> (SkeletonWorld, ClimateMap, HexGrid, DetailElevation) {
        let world = earth_world(seed);
        let climate = ClimateMap::build(&world, &ClimateConfig::default());
        let grid = HexGrid::build(&world, spec, HexGridConfig::default());
        let elev = DetailElevation::build(&world, &grid, seed, DetailElevationConfig::default());
        (world, climate, grid, elev)
    }

    #[test]
    fn humidity_stays_in_range_for_earth_region() {
        let spec = RegionSpec::new(0, 1);
        let (world, climate, grid, elev) = build_pipeline(1, spec);
        let cfg = DetailMoistureConfig::default();
        let moisture = DetailMoisture::build(&world, &climate, &grid, &elev, cfg);

        assert_eq!(moisture.per_hex.len(), grid.cells.len());
        let mut minv = f64::INFINITY;
        let mut maxv = f64::NEG_INFINITY;
        for &h in &moisture.per_hex {
            assert!(h.is_finite(), "non-finite humidity: {h}");
            assert!(
                (0.0..=cfg.saturation_ceiling + 1.0e-12).contains(&h),
                "humidity {h} outside [0, {}]",
                cfg.saturation_ceiling
            );
            minv = minv.min(h);
            maxv = maxv.max(h);
        }
        // Sanity: the real Earth baseline has non-zero spread.
        assert!(
            maxv > minv,
            "expected some variation in humidity, got {minv}..={maxv}"
        );
        eprintln!("earth-seed1 radius-1 tile-0 humidity range: {minv:.4}..={maxv:.4}");
    }

    #[test]
    fn deterministic_byte_identical() {
        let spec = RegionSpec::new(3, 1);
        let (world_a, climate_a, grid_a, elev_a) = build_pipeline(11, spec.clone());
        let (world_b, climate_b, grid_b, elev_b) = build_pipeline(11, spec);
        let cfg = DetailMoistureConfig::default();

        let a = DetailMoisture::build(&world_a, &climate_a, &grid_a, &elev_a, cfg);
        let b = DetailMoisture::build(&world_b, &climate_b, &grid_b, &elev_b, cfg);

        let sa = serde_json::to_vec(&a).expect("serialize a");
        let sb = serde_json::to_vec(&b).expect("serialize b");
        assert_eq!(sa, sb, "DetailMoisture::build is non-deterministic");
    }

    #[test]
    fn serde_round_trip() {
        let spec = RegionSpec::new(5, 1);
        let (world, climate, grid, elev) = build_pipeline(7, spec);
        let cfg = DetailMoistureConfig::default();
        let moisture = DetailMoisture::build(&world, &climate, &grid, &elev, cfg);

        let json = serde_json::to_string(&moisture).expect("serialize");
        let back: DetailMoisture = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.spec, moisture.spec);
        assert_eq!(back.per_hex.len(), moisture.per_hex.len());
        for (a, b) in back.per_hex.iter().zip(moisture.per_hex.iter()) {
            assert!((a - b).abs() < 1.0e-12, "round-trip delta {a} vs {b}");
        }
    }

    #[test]
    fn config_serde_round_trip() {
        let cfg = DetailMoistureConfig {
            lift_coefficient: 0.4,
            shadow_coefficient: 0.25,
            saturation_ceiling: 0.9,
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: DetailMoistureConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, cfg);
    }

    /// Synthetic ridge test: inject a one-ridge elevation profile into a
    /// radius-0 region so a clear windward / leeward pair exists, then
    /// verify the windward hex is wetter than the leeward hex at the
    /// same natural elevation.
    ///
    /// The ridge runs perpendicular to the local wind vector: hexes on
    /// the upwind side of the ridge see `dh_up > 0` (air climbing), and
    /// hexes on the downwind side see `dh_up < 0` plus a lower downwind
    /// still (rain shadow). We pick two hexes at the same axial `r` on
    /// either side of the ridge and compare their humidities.
    #[test]
    fn windward_wetter_than_leeward() {
        // Build a normal pipeline, then replace the elevation and wind
        // so the geometry is exactly what we want. The parent tile in
        // a radius-0 region is tile 0 by construction.
        let spec = RegionSpec::new(0, 0);
        let (world, mut climate, grid, _elev) = build_pipeline(1, spec.clone());

        // Synthetic ridge along the `r` axis at the lattice centre, with
        // wind pointing in the +q direction (roughly east). Set ridge
        // height to 2000 m at the peak row, 0 m elsewhere.
        let n = grid.config.subdivision as i32;
        let ridge_q = n / 2;
        let mut fake_elev = vec![0.0_f64; grid.cells.len()];
        for (i, cell) in grid.cells.iter().enumerate() {
            // Triangular ridge profile peaking at q == ridge_q.
            let dq = (cell.q - ridge_q).unsigned_abs() as f64;
            let height = (2000.0 - 400.0 * dq).max(0.0);
            fake_elev[i] = height;
        }
        let fake_elevation = DetailElevation {
            spec,
            per_hex_m: fake_elev,
        };

        // Force wind on the parent tile to be purely eastward (strong
        // enough to clear the calm epsilon). Also force a known baseline.
        climate.wind.per_tile[0] = ymir_climate::WindVector { u: 10.0, v: 0.0 };
        let baseline = 0.5;
        climate.moisture.per_tile[0] = baseline;

        let cfg = DetailMoistureConfig::default();
        let moisture = DetailMoisture::build(&world, &climate, &grid, &fake_elevation, cfg);

        // Pick a windward hex (q == ridge_q - 1) and a leeward hex
        // (q == ridge_q + 1) at the same r. We scan all rows and accumulate
        // windward-vs-leeward deltas; at least one row should show the
        // expected signed difference, and the average should be positive.
        let mut windward_sum = 0.0;
        let mut leeward_sum = 0.0;
        let mut row_count = 0u32;
        let mut worst_pair: (f64, f64, i32) = (0.0, 0.0, 0);
        for r in 0..n {
            let windward_idx = find_cell(&grid, 0, ridge_q - 1, r);
            let leeward_idx = find_cell(&grid, 0, ridge_q + 1, r);
            if let (Some(wi), Some(li)) = (windward_idx, leeward_idx) {
                windward_sum += moisture.per_hex[wi];
                leeward_sum += moisture.per_hex[li];
                row_count += 1;
                if moisture.per_hex[wi] - moisture.per_hex[li] > worst_pair.0 - worst_pair.1 {
                    worst_pair = (moisture.per_hex[wi], moisture.per_hex[li], r);
                }
            }
        }
        assert!(
            row_count > 0,
            "expected at least one windward/leeward row pair"
        );
        let avg_windward = windward_sum / row_count as f64;
        let avg_leeward = leeward_sum / row_count as f64;
        assert!(
            avg_windward > avg_leeward,
            "windward avg {avg_windward} should exceed leeward avg {avg_leeward}"
        );
        // Test documentation: record the observed delta so the TASKS.md
        // note can reference a concrete number.
        let delta = avg_windward - avg_leeward;
        eprintln!(
            "windward-leeward avg delta = {delta:.4} (windward {avg_windward:.4}, leeward {avg_leeward:.4}); best single-row pair r={} windward={:.4} leeward={:.4}",
            worst_pair.2, worst_pair.0, worst_pair.1
        );
        assert!(
            delta > 0.05,
            "expected a clear humidity delta (>0.05) across the ridge, got {delta}"
        );
    }

    /// Lookup helper: find the `HexCell` index in `grid` with the given
    /// `(parent_tile, q, r)`. Used by the windward/leeward test.
    fn find_cell(grid: &HexGrid, parent_tile: u32, q: i32, r: i32) -> Option<usize> {
        grid.cells
            .iter()
            .position(|c: &HexCell| c.parent_tile == parent_tile && c.q == q && c.r == r)
    }

    #[test]
    fn calm_wind_preserves_baseline() {
        // Build a region, then force the parent-tile wind to zero and
        // verify every hex's humidity equals the clamped baseline.
        let spec = RegionSpec::new(2, 0);
        let (world, mut climate, grid, elev) = build_pipeline(1, spec);
        for w in climate.wind.per_tile.iter_mut() {
            w.u = 0.0;
            w.v = 0.0;
        }
        let baseline = 0.4;
        for m in climate.moisture.per_tile.iter_mut() {
            *m = baseline;
        }
        let cfg = DetailMoistureConfig::default();
        let moisture = DetailMoisture::build(&world, &climate, &grid, &elev, cfg);
        for (i, &h) in moisture.per_hex.iter().enumerate() {
            assert!(
                (h - baseline).abs() < 1.0e-12,
                "hex {i}: expected {baseline}, got {h}"
            );
        }
    }

    #[test]
    fn saturation_ceiling_respected() {
        // Even with an enormous lift coefficient and a tall synthetic
        // ridge, humidity must not exceed the configured ceiling.
        let spec = RegionSpec::new(0, 0);
        let (world, mut climate, grid, _elev) = build_pipeline(1, spec.clone());
        let n = grid.config.subdivision as i32;
        let mut fake_elev = vec![0.0_f64; grid.cells.len()];
        for (i, cell) in grid.cells.iter().enumerate() {
            fake_elev[i] = 5000.0 * (cell.q as f64 / n as f64);
        }
        let fake_elevation = DetailElevation {
            spec,
            per_hex_m: fake_elev,
        };
        climate.wind.per_tile[0] = ymir_climate::WindVector { u: 10.0, v: 0.0 };
        climate.moisture.per_tile[0] = 0.9;
        let cfg = DetailMoistureConfig {
            lift_coefficient: 10.0,
            shadow_coefficient: 0.3,
            saturation_ceiling: 0.7,
        };
        let moisture = DetailMoisture::build(&world, &climate, &grid, &fake_elevation, cfg);
        for &h in &moisture.per_hex {
            assert!(
                h <= cfg.saturation_ceiling + 1.0e-12,
                "humidity {h} exceeds ceiling {}",
                cfg.saturation_ceiling
            );
            assert!(h >= 0.0, "humidity {h} below zero");
        }
    }
}
