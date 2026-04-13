//! Provenance dump validation integration test (VALID-08).
//!
//! Drives the full `ymir generate --star Earth --seed 1` pipeline through the
//! compiled binary, then:
//!
//! 1. Asserts `provenance.json` exists and parses as valid JSON.
//! 2. Asserts the six expected pipeline stage keys are present.
//! 3. Checks per-stage Observed/Derived/Assumed counts against a golden
//!    histogram reflecting the Earth fixture's provenance:
//!    - `stellar`: Sol via `from_params` → 7 Assumed (catalog scalars) + 5
//!      Derived (HZ bounds + UV flux), 0 Observed.
//!    - `orbital_body`: hand-filled Earth body → 13 Observed (all scalars
//!      including `continental_fraction`), 0 Derived/Assumed.
//!    - `atmosphere`: fully derived → 7 Derived, 0 Observed/Assumed.
//!    - `skeleton`, `climate`, `biome`: aggregate tags → 1 Derived each.
//! 4. Asserts specific known-Observed fields in `orbital_body` carry the
//!    expected reference/instrument metadata.
//! 5. Regenerates with an atmosphere `surface_pressure` override and asserts
//!    the overridden field transitioned from Derived → Observed in the
//!    refreshed `provenance.json`.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Invoke `ymir generate --star Earth --seed 1 --output <dir>` and panic on
/// failure with a diagnostic message.
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

/// Write an overrides JSON file that bumps `atmosphere.surface_pressure` to
/// 2.5 bar and invoke `ymir regenerate`.
fn run_regenerate_with_atmosphere_override(world_dir: &Path, override_path: &Path) {
    // The override patches `surface_pressure` as a bare scalar; `merge_json`
    // in the CLI will re-wrap it as `{ value, source: Observed{...} }`.
    let override_doc = serde_json::json!({
        "version": "1.0",
        "target_star": "Sol",
        "target_planet": "Earth",
        "overrides": {
            "atmosphere": {
                "surface_pressure": 2.5
            }
        }
    });
    std::fs::write(
        override_path,
        serde_json::to_string_pretty(&override_doc).expect("serialize override"),
    )
    .expect("write override file");

    let bin = env!("CARGO_BIN_EXE_ymir");
    let output = Command::new(bin)
        .args(["regenerate", "--world"])
        .arg(world_dir)
        .arg("--overrides")
        .arg(override_path)
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

/// Load `provenance.json` from `world_dir` and return it as a parsed
/// [`serde_json::Value`] (the raw JSON object).  The test intentionally reads
/// the file as untyped JSON so assertions catch shape regressions that a
/// typed deserialize might silently swallow.
fn load_provenance_json(world_dir: &Path) -> Value {
    let path = world_dir.join("provenance.json");
    assert!(
        path.exists(),
        "provenance.json not found in {}",
        world_dir.display()
    );
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read provenance.json: {e}"));
    serde_json::from_str(&contents)
        .unwrap_or_else(|e| panic!("provenance.json is not valid JSON: {e}"))
}

/// Count Observed/Derived/Assumed leaves in the `fields` subtree of a stage
/// entry.  Mirrors the logic in `ProvenanceReport::tally_json`: any object
/// with exactly the keys `value` + `source` is treated as a `Sourced<T>`
/// leaf; everything else is recursed into.
fn count_sources(v: &Value) -> (usize, usize, usize) {
    let mut observed = 0usize;
    let mut derived = 0usize;
    let mut assumed = 0usize;
    walk_sources(v, &mut observed, &mut derived, &mut assumed);
    (observed, derived, assumed)
}

fn walk_sources(v: &Value, observed: &mut usize, derived: &mut usize, assumed: &mut usize) {
    match v {
        Value::Object(map) => {
            if map.len() == 2 && map.contains_key("value") && map.contains_key("source") {
                // Sourced<T> leaf.
                if let Some(Value::Object(src_map)) = map.get("source") {
                    if src_map.contains_key("Observed") {
                        *observed += 1;
                    } else if src_map.contains_key("Derived") {
                        *derived += 1;
                    } else if src_map.contains_key("Assumed") {
                        *assumed += 1;
                    }
                }
                // Do not recurse into value/source subtree.
                return;
            }
            for child in map.values() {
                walk_sources(child, observed, derived, assumed);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                walk_sources(item, observed, derived, assumed);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Test 1: golden histogram for a baseline Earth world
// ---------------------------------------------------------------------------

/// Generate an Earth world and assert the provenance.json golden histogram.
///
/// Earth stellar context uses `sol_context()` (the `from_params` path), so
/// the 7 catalog scalar fields (effective_temp, luminosity, metallicity, mass,
/// radius, age, distance) are tagged `Assumed` and the 5 derived fields
/// (hz_inner, hz_outer, hz_inner_optimistic, hz_outer_optimistic, uv_flux_hz)
/// are tagged `Derived`. The hand-filled `earth_body()` tags all 13 physical
/// scalars `Observed`. `AtmosphereModel::derive` always tags its 7 output
/// scalars `Derived`. Aggregate stages (skeleton, climate, biome) each emit a
/// single `Derived` summary node.
#[test]
fn provenance_golden_histogram_earth() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let world_dir = tmp.path().join("earth_prov");
    run_generate_earth(&world_dir);

    let prov = load_provenance_json(&world_dir);

    // Top-level must be an object with a "stages" key.
    let stages = prov
        .get("stages")
        .and_then(Value::as_object)
        .expect("provenance.json must have a top-level 'stages' object");

    // All six pipeline stages must be present.
    for stage_name in &[
        "stellar",
        "orbital_body",
        "atmosphere",
        "skeleton",
        "climate",
        "biome",
    ] {
        assert!(
            stages.contains_key(*stage_name),
            "missing expected stage '{stage_name}' in provenance.json"
        );
    }

    // --- stellar: 7 Assumed (catalog scalars) + 5 Derived (HZ/UV) -----------
    //
    // Sol goes through `from_params`, which tags the 7 numeric inputs Assumed
    // and derives 5 habitable-zone / UV fields.
    {
        let stage = &stages["stellar"];
        let counts = stage.get("counts").expect("stellar stage missing 'counts'");
        let observed = counts["observed"].as_u64().unwrap_or(0);
        let derived = counts["derived"].as_u64().unwrap_or(0);
        let assumed = counts["assumed"].as_u64().unwrap_or(0);

        assert_eq!(
            observed, 0,
            "stellar (Sol/from_params): expected 0 Observed, got {observed}"
        );
        assert_eq!(
            derived, 5,
            "stellar (Sol/from_params): expected 5 Derived (HZ bounds + uv_flux_hz), got {derived}"
        );
        assert_eq!(
            assumed, 7,
            "stellar (Sol/from_params): expected 7 Assumed (catalog scalars), got {assumed}"
        );

        // Cross-check that counts agree with the fields subtree.
        let fields = stage.get("fields").expect("stellar stage missing 'fields'");
        let (f_obs, f_der, f_asm) = count_sources(fields);
        assert_eq!(
            f_obs, 0,
            "stellar fields walker: expected 0 Observed, found {f_obs}"
        );
        assert_eq!(
            f_der, 5,
            "stellar fields walker: expected 5 Derived, found {f_der}"
        );
        assert_eq!(
            f_asm, 7,
            "stellar fields walker: expected 7 Assumed, found {f_asm}"
        );
    }

    // --- orbital_body: 13 Observed, 0 Derived, 0 Assumed --------------------
    //
    // `earth_body()` hand-fills all 13 scalars with `Sourced::observed` citing
    // "IAU / NASA planetary fact sheet". This includes `continental_fraction`.
    {
        let stage = &stages["orbital_body"];
        let counts = stage
            .get("counts")
            .expect("orbital_body stage missing 'counts'");
        let observed = counts["observed"].as_u64().unwrap_or(0);
        let derived = counts["derived"].as_u64().unwrap_or(0);
        let assumed = counts["assumed"].as_u64().unwrap_or(0);

        assert_eq!(
            observed, 13,
            "orbital_body (Earth): expected 13 Observed, got {observed}"
        );
        assert_eq!(
            derived, 0,
            "orbital_body (Earth): expected 0 Derived, got {derived}"
        );
        assert_eq!(
            assumed, 0,
            "orbital_body (Earth): expected 0 Assumed, got {assumed}"
        );
    }

    // --- atmosphere: 7 Derived, 0 Observed, 0 Assumed -----------------------
    //
    // `AtmosphereModel::derive` always tags its 7 `Sourced<T>` scalars
    // (surface_pressure, composition, greenhouse_factor, effective_surface_temp,
    // scale_height, moisture_capacity, uv_surface_flux) as Derived from the
    // "atmosphere" stage.
    {
        let stage = &stages["atmosphere"];
        let counts = stage
            .get("counts")
            .expect("atmosphere stage missing 'counts'");
        let observed = counts["observed"].as_u64().unwrap_or(0);
        let derived = counts["derived"].as_u64().unwrap_or(0);
        let assumed = counts["assumed"].as_u64().unwrap_or(0);

        assert_eq!(
            observed, 0,
            "atmosphere (Earth): expected 0 Observed, got {observed}"
        );
        assert_eq!(
            derived, 7,
            "atmosphere (Earth): expected 7 Derived, got {derived}"
        );
        assert_eq!(
            assumed, 0,
            "atmosphere (Earth): expected 0 Assumed, got {assumed}"
        );
    }

    // --- skeleton / climate / biome: 1 Derived each (aggregate nodes) -------
    for stage_name in &["skeleton", "climate", "biome"] {
        let stage = &stages[*stage_name];
        let counts = stage
            .get("counts")
            .unwrap_or_else(|| panic!("{stage_name} stage missing 'counts'"));
        let observed = counts["observed"].as_u64().unwrap_or(0);
        let derived = counts["derived"].as_u64().unwrap_or(0);
        let assumed = counts["assumed"].as_u64().unwrap_or(0);

        assert_eq!(
            observed, 0,
            "{stage_name}: expected 0 Observed, got {observed}"
        );
        assert_eq!(
            derived, 1,
            "{stage_name}: expected 1 Derived (aggregate node), got {derived}"
        );
        assert_eq!(
            assumed, 0,
            "{stage_name}: expected 0 Assumed, got {assumed}"
        );

        // Aggregate stage fields must have a "source" key at the top level
        // with a "Derived" variant.
        let fields = stage
            .get("fields")
            .unwrap_or_else(|| panic!("{stage_name} stage missing 'fields'"));
        let src = fields
            .get("source")
            .unwrap_or_else(|| panic!("{stage_name} fields missing top-level 'source' key"));
        assert!(
            src.get("Derived").is_some(),
            "{stage_name} aggregate source must be a 'Derived' variant, got: {src}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 2: known-Observed field metadata for orbital_body
// ---------------------------------------------------------------------------

/// Assert that specific known-Observed fields in `orbital_body` carry the
/// expected reference and instrument strings from `earth_body()`.
///
/// This guards against regressions where provenance metadata is stripped or
/// changed silently.
#[test]
fn provenance_observed_field_metadata_earth() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let world_dir = tmp.path().join("earth_meta");
    run_generate_earth(&world_dir);

    let prov = load_provenance_json(&world_dir);
    let stages = prov["stages"].as_object().expect("stages object");
    let body_fields = stages["orbital_body"]
        .get("fields")
        .expect("orbital_body fields");

    // Check a representative set of scalar fields for the expected citation.
    let expected_ref = "IAU / NASA planetary fact sheet";
    let expected_instr = "Earth";

    for field_name in &[
        "semi_major_axis",
        "mass",
        "radius",
        "surface_gravity",
        "rotation_period",
    ] {
        let field = body_fields
            .get(*field_name)
            .unwrap_or_else(|| panic!("orbital_body.{field_name} missing from fields"));

        // The field must be a Sourced<T> leaf: { value, source }.
        let source = field
            .get("source")
            .unwrap_or_else(|| panic!("orbital_body.{field_name} has no 'source' key"));
        let obs = source
            .get("Observed")
            .unwrap_or_else(|| panic!("orbital_body.{field_name} source is not 'Observed'"));

        let reference = obs["reference"].as_str().unwrap_or_else(|| {
            panic!("orbital_body.{field_name} Observed.reference is not a string")
        });
        let instrument = obs["instrument"].as_str().unwrap_or_else(|| {
            panic!("orbital_body.{field_name} Observed.instrument is not a string")
        });

        assert_eq!(
            reference, expected_ref,
            "orbital_body.{field_name}: expected reference = {expected_ref:?}, got {reference:?}"
        );
        assert_eq!(
            instrument, expected_instr,
            "orbital_body.{field_name}: expected instrument = {expected_instr:?}, got {instrument:?}"
        );
    }

    // continental_fraction is also Observed (Earth hypsometric curve).
    let cf = body_fields
        .get("continental_fraction")
        .expect("orbital_body.continental_fraction missing from fields");
    // continental_fraction is Option<Sourced<f64>>; when Some it serialises
    // directly as the Sourced<f64> leaf (not nested in an Option wrapper).
    let cf_source = cf
        .get("source")
        .expect("orbital_body.continental_fraction has no 'source' key");
    assert!(
        cf_source.get("Observed").is_some(),
        "orbital_body.continental_fraction source must be Observed, got: {cf_source}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Derived → Observed transition after atmosphere override
// ---------------------------------------------------------------------------

/// Regenerate with an `atmosphere.surface_pressure` override and assert that
/// the `surface_pressure` field transitions from `Derived` to `Observed` in
/// the refreshed `provenance.json`, while the overall Observed count for the
/// atmosphere stage increases by 1 (and Derived decreases by 1).
#[test]
fn provenance_override_flips_derived_to_observed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let world_dir = tmp.path().join("earth_flip");
    run_generate_earth(&world_dir);

    // Baseline: atmosphere has 7 Derived, 0 Observed.
    let prov_before = load_provenance_json(&world_dir);
    let stages_before = prov_before["stages"].as_object().expect("stages");
    let atmo_before = &stages_before["atmosphere"];
    let counts_before = atmo_before.get("counts").expect("counts");
    assert_eq!(
        counts_before["derived"].as_u64().unwrap_or(0),
        7,
        "baseline atmosphere Derived count should be 7"
    );
    assert_eq!(
        counts_before["observed"].as_u64().unwrap_or(0),
        0,
        "baseline atmosphere Observed count should be 0"
    );

    // Regenerate with a surface_pressure override.
    let override_path = tmp.path().join("override_atmo.json");
    run_regenerate_with_atmosphere_override(&world_dir, &override_path);

    // Post-override: surface_pressure should be Observed; total Derived drops
    // from 7 to 6 and Observed rises from 0 to 1.
    let prov_after = load_provenance_json(&world_dir);
    let stages_after = prov_after["stages"].as_object().expect("stages after");
    let atmo_after = &stages_after["atmosphere"];
    let counts_after = atmo_after.get("counts").expect("counts after");

    assert_eq!(
        counts_after["observed"].as_u64().unwrap_or(0),
        1,
        "after atmosphere override: expected 1 Observed field (surface_pressure), \
         got {}",
        counts_after["observed"]
    );
    assert_eq!(
        counts_after["derived"].as_u64().unwrap_or(0),
        6,
        "after atmosphere override: expected 6 Derived fields (7 - 1 flipped), \
         got {}",
        counts_after["derived"]
    );
    assert_eq!(
        counts_after["assumed"].as_u64().unwrap_or(0),
        0,
        "after atmosphere override: expected 0 Assumed fields"
    );

    // Verify the surface_pressure field itself carries an Observed tag citing
    // "user override".
    let fields_after = atmo_after.get("fields").expect("atmosphere fields after");
    let sp = fields_after
        .get("surface_pressure")
        .expect("atmosphere.surface_pressure missing from fields after override");
    let sp_source = sp
        .get("source")
        .expect("atmosphere.surface_pressure has no 'source' key after override");
    let obs = sp_source
        .get("Observed")
        .expect("atmosphere.surface_pressure source is not Observed after override");
    assert_eq!(
        obs["reference"].as_str().unwrap_or(""),
        "user override",
        "surface_pressure override reference should be 'user override'"
    );
    assert_eq!(
        obs["instrument"].as_str().unwrap_or(""),
        "ymir CLI --override",
        "surface_pressure override instrument should be 'ymir CLI --override'"
    );

    // Verify the overridden value was actually applied (2.5 bar).
    let sp_value = sp["value"]
        .as_f64()
        .expect("atmosphere.surface_pressure value is not a number");
    assert!(
        (sp_value - 2.5).abs() < 1e-9,
        "surface_pressure after override should be 2.5, got {sp_value}"
    );

    // All other stages should be unaffected at the provenance level.
    // Skeleton is dirty (atmosphere changed it) but it still emits exactly
    // 1 Derived aggregate node.
    let skeleton_after = &stages_after["skeleton"];
    assert_eq!(
        skeleton_after["counts"]["derived"].as_u64().unwrap_or(0),
        1,
        "skeleton aggregate Derived count should remain 1 after atmosphere override"
    );
}
