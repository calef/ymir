# Ymir Task Tracker

## How to Use This File

Tasks are grouped by phase and crate. Each task has a fixed-format header block for machine-grepability and a freeform body for context.

### Task Template

```
### CRATE-NN: Short title
- **Crate:** ymir-crate
- **Status:** ready
- **Depends on:** CRATE-XX, OTHER-YY
- **Blocked:** no
- **Assignee:** (empty, "human", or "agent")

Description and context. Link to design doc sections. Notes on approach.
```

### Status Values

| Status | Meaning |
|--------|---------|
| `ready` | All dependencies are `done`, not blocked, can be picked up |
| `in-progress` | Someone (human or agent) is actively working on it |
| `done` | Implemented, tested, merged to main |
| `blocked` | Cannot proceed; see **Blocked** field for reason |
| `pending` | Dependencies not yet `done`; will become `ready` automatically |

### Rules

- A `pending` task becomes `ready` when all its **Depends on** tasks are `done` and **Blocked** is `no`.
- When blocking a task, set **Status** to `blocked` and **Blocked** to a description of what's needed (e.g., "needs human decision on override granularity").
- When unblocking, set **Blocked** back to `no` and **Status** to `ready` (or `pending` if dependencies aren't met).
- Keep IDs stable. Don't renumber tasks after creation.
- Prefix IDs with a short crate tag: CORE, CAT, SYS, ATMO, SURF, CLIM, BIOME, DET, REND, STOR, CLI, INFRA.

---

## Phase 0: Workspace Setup

### INFRA-01: Initialize Cargo workspace with stub crates
- **Crate:** (all)
- **Status:** done
- **Depends on:** (none)
- **Blocked:** no
- **Assignee:** agent

Set up workspace Cargo.toml, all 10 library crate stubs with correct inter-crate dependencies, binary crate stub. Every crate has src/lib.rs (or main.rs) with module declarations. `cargo check` passes on the full workspace.

### INFRA-02: Set up CI workflow
- **Crate:** (infra)
- **Status:** done
- **Depends on:** INFRA-01
- **Blocked:** no
- **Assignee:** claude

GitHub Actions workflow: fmt check, clippy, test, doc build. Deferred until GitHub remote exists.

NOTE: `.github/workflows/ci.yml` written with four jobs (fmt, clippy, test, doc) all gated by `RUSTFLAGS=-D warnings` and `RUSTDOCFLAGS=-D warnings`. Uses `dtolnay/rust-toolchain@stable` and `Swatinem/rust-cache@v2`. Runs on push and PR to `main`. Tests use `--release` to keep the VALID-* + regenerate integration tests tractable (each runs the full pipeline at subdivision 5; debug mode is ~70 s per test, release is ~10-15 s). While writing this, two rustdoc lints tripped under `-D warnings` and were fixed in place: a private intra-doc link in `weight_schema.rs::self_affinity` and a redundant explicit link target in `noise.rs`. Workflow will activate on first push to a GitHub remote; dormant until then.

### INFRA-03: Create .gitignore and repo boilerplate
- **Crate:** (infra)
- **Status:** done
- **Depends on:** (none)
- **Blocked:** no
- **Assignee:** agent

.gitignore (Rust standard), LICENSE-MIT, LICENSE-APACHE, CITATION.cff skeleton.

---

## Phase 1: Star-to-Skeleton MVP

### CORE-01: Implement Sourced<T> and Source enum
- **Crate:** ymir-core
- **Status:** done
- **Depends on:** INFRA-01
- **Blocked:** no
- **Assignee:** agent

`Source` enum (Derived, Observed, Assumed) and `Sourced<T>` wrapper with constructor methods and serde support. Per design doc section 4.1. Include property-based tests via proptest.

### CORE-02: Seeded PRNG module
- **Crate:** ymir-core
- **Status:** done
- **Depends on:** INFRA-01
- **Blocked:** no
- **Assignee:** agent

PCG-based PRNG wrapper using `rand_pcg`. Seeded, deterministic, with helpers for deriving child seeds from (parent_seed, context_string). Ensures reproducibility per design doc determinism goal.

### CORE-03: Shared traits for cross-crate abstractions
- **Crate:** ymir-core
- **Status:** done
- **Depends on:** INFRA-01
- **Blocked:** no
- **Assignee:** agent

Define trait interfaces used across crates: `GeoTile` (lat, lon, elevation), `PipelineStage` (input/output types, dirty tracking). Per open question #13 resolution.

### CORE-04: Override file parsing and validation
- **Crate:** ymir-core
- **Status:** done
- **Depends on:** CORE-01
- **Blocked:** no
- **Assignee:** agent

Parse override JSON files per the schema in design doc section 4.1. Validate structure, report errors. Per-stage override application (not per-field in Phase 1).

### CORE-05: Dependency graph and dirty tracking
- **Crate:** ymir-core
- **Status:** done
- **Depends on:** CORE-01, CORE-03
- **Blocked:** no
- **Assignee:** agent

Stage-level dependency graph. When an override is applied at stage N, stages N through 7 are marked dirty. Supports the `regenerate` workflow. Per design doc section 4.1 override rules.

### CAT-01: StarContext struct and habitable zone calculation
- **Crate:** ymir-catalog
- **Status:** done
- **Depends on:** CORE-01
- **Blocked:** no
- **Assignee:** agent

`StarContext` struct with all fields from design doc section 5.1. Kopparapu et al. (2013, 2014) HZ calculation. Unit tests against published values (Sun HZ inner = 0.99 AU). Hardcoded Tau Ceti data for Phase 1 (no Parquet ingest).

### CAT-02: Exoplanet record types
- **Crate:** ymir-catalog
- **Status:** done
- **Depends on:** CORE-01
- **Blocked:** no
- **Assignee:** agent

`ExoplanetRecord` struct for known exoplanet data from NASA Exoplanet Archive. Hardcoded Tau Ceti system data for Phase 1. Struct design should support future ingest from real catalog data.

### SYS-01: Orbital placement algorithm
- **Crate:** ymir-system
- **Status:** done
- **Depends on:** CORE-01, CORE-02, CAT-01
- **Blocked:** no
- **Assignee:** agent

Planet placement using Kepler occurrence rates, Hill sphere spacing (delta >= 8 R_Hill), known exoplanet anchoring. Per design doc section 5.2.

### SYS-02: Bulk property derivation
- **Crate:** ymir-system
- **Status:** done
- **Depends on:** CORE-01, SYS-01
- **Blocked:** no
- **Assignee:** agent

Mass-radius relationships (Chen & Kipping 2017), density, surface gravity, equilibrium temperature, solar irradiance. Tidal lock check. `OrbitalBody` struct per design doc section 5.2.

### ATMO-01: Atmosphere retention (Jeans escape)
- **Crate:** ymir-atmosphere
- **Status:** done
- **Depends on:** CORE-01, SYS-02
- **Blocked:** no
- **Assignee:** agent

NOTE: Retention threshold tuned to 10 (not 6 as in design doc) because equilibrium temp gives lower thermal velocities than exospheric temp; 10 calibrates correctly against Solar System test cases.

Escape velocity vs thermal velocity for each molecule. Determines which species a planet retains. Unit tests: Earth retains N2/O2/H2O, loses H2/He. Mars retains CO2, loses H2O over Gyr. Per design doc section 5.3.

### ATMO-02: Composition model and greenhouse effect
- **Crate:** ymir-atmosphere
- **Status:** done
- **Depends on:** ATMO-01
- **Blocked:** no
- **Assignee:** agent

NOTE: Greenhouse tau coefficients calibrated to A=10.0 (CO2/H2O term) and B=0.5 (base scattering) instead of design doc's 0.2/0.05. Those spec values cannot simultaneously satisfy Earth calibration (~288K at 1 bar) and Venus (>600K at 90 bar). Calibrated values hit both targets.

Simplified composition model (Terran vs SuperEarth vs metallicity influence). Greenhouse factor from CO2/H2O optical depth. `AtmosphereModel` struct. Earth calibration test: inputs produce ~288K surface temp within 5K. Per design doc section 5.3.

### SURF-01: Geodesic grid generation
- **Crate:** ymir-surface
- **Status:** done
- **Depends on:** CORE-01, CORE-03
- **Blocked:** no
- **Assignee:** agent

Icosahedral geodesic grid with configurable subdivision level. Tile neighbor computation. Tests: all tiles have 5 or 6 neighbors, total tiles = 10*4^N + 2, symmetric neighbor relationships. Per design doc section 5.4.

### SURF-02: Tectonic plate generation
- **Crate:** ymir-surface
- **Status:** done
- **Depends on:** CORE-02, SURF-01
- **Blocked:** no
- **Assignee:** agent

Poisson-seeded flood fill for plate assignment. Boundary classification (convergent/divergent/transform). Plate count scaled by surface area and internal heat. Tests: every tile in exactly one plate, convergent boundaries have positive elevation bias. Per design doc section 5.4.

### SURF-03: Heightmap generation
- **Crate:** ymir-surface
- **Status:** done
- **Depends on:** SURF-02, ATMO-02
- **Blocked:** no
- **Assignee:** agent

NOTE: SkeletonWorld omits a dedicated SkeletonTile view; per-tile elevation lives in ElevationMap and per-tile plate assignment in TectonicData. Unified tile view deferred until a downstream stage needs it.

Tectonic bias field + spherical fractal noise. Noise amplitude scaled by gravity. Mountain height cap from yield strength / (density * g). Tests: lower-gravity planet has higher max elevation. `SkeletonWorld` struct. Per design doc section 5.4.

### STOR-01: World manifest serialization
- **Crate:** ymir-storage
- **Status:** done
- **Depends on:** CORE-01
- **Blocked:** no
- **Assignee:** agent

Serialize/deserialize world manifest (star context, body params, config, seed, overrides). JSON manifest + binary skeleton data via bincode. World directory layout per design doc section 6.2.

### REND-01: Mollweide projection to PNG
- **Crate:** ymir-render
- **Status:** done
- **Depends on:** SURF-03
- **Blocked:** no
- **Assignee:** agent

Software rasterizer using the `image` crate. Mollweide equal-area projection. Color by elevation for Phase 1. Output PNG file. No wgpu; that's Phase 6.

### CLI-01: `ymir generate` command
- **Crate:** ymir (binary)
- **Status:** done
- **Depends on:** CAT-01, SYS-01, SYS-02, ATMO-01, ATMO-02, SURF-03, STOR-01, REND-01
- **Blocked:** no
- **Assignee:** agent

NOTE: AtmosphereModel.composition switched from HashMap to BTreeMap so bincode-serialized skeleton.bin is byte-deterministic across runs. Purely mechanical change; Gas already implements Ord.

Wire up the full pipeline via clap CLI. `ymir generate --star "Tau Ceti" --seed 42 --output worlds/tau_ceti_42/` produces skeleton data on disk and Mollweide PNG. Per design doc section 7.5.

### SYS-03: Rocky/volatile branch selection in placement
- **Crate:** ymir-system
- **Status:** done
- **Depends on:** SYS-01, SYS-02
- **Blocked:** no
- **Assignee:** agent

Tau Ceti e, h, and f all have RV-only minimum-mass data. Current pipeline feeds those into the M→R inverse, which lands on the volatile branch, producing three 2.24 R_earth sub-Neptunes. Real-world consensus treats e and f as rocky candidates. Add a rocky-branch override: when the catalogued minimum mass is below the rocky/volatile threshold (~4 M_earth), force the rocky branch even though the inverse would pick volatile. Keep the volatile branch for objects where catalog mass is clearly above threshold. Phase 2 scope.

### CLI-02: `ymir info` command
- **Crate:** ymir (binary)
- **Status:** done
- **Depends on:** STOR-01
- **Blocked:** no
- **Assignee:** agent

Print summary of a generated world: star properties, planet properties, atmosphere, provenance stats. Per design doc section 7.5.

---

## Phase 2: Climate, biomes, and override system

Design reference: `ymir-design.md` sections 5.5, 5.6, 11 (Phase 2). Deliverable per design doc: globe PNGs with biome coloring that look recognizably different for Earth-like vs Mars-like vs tidally locked worlds; `ymir regenerate` recomputes only dirty stages after an override is applied.

### CAT-03: Hardcoded Earth and Mars catalog data
- **Crate:** ymir-catalog
- **Status:** done
- **Depends on:** CAT-01, CAT-02
- **Blocked:** no
- **Assignee:** agent

NOTE: `earth_body()` and `mars_body()` live in the binary crate (`src/main.rs`) rather than `ymir-catalog`, because putting them in catalog would require a catalog→system dependency that violates the crate dependency graph. `sol_context()` stays in catalog.

Add `earth_context()` / `mars_context()` (Sol StarContext variants) and `earth_body()` / `mars_body()` helpers that produce `OrbitalBody` values consistent with real observational data. These feed the Phase 2 validation tests. Needed because the CLI's star lookup only knows Tau Ceti, and validation cases require precisely-tuned Earth and Mars params. Wire into the binary crate's star lookup so `--star Earth` / `--star Mars` work.

### CLIM-01: Temperature field
- **Crate:** ymir-climate
- **Status:** done
- **Depends on:** ATMO-02, SURF-03
- **Blocked:** no
- **Assignee:** agent

NOTE: `ymir-system` added as dev-dep to ymir-climate so tests can construct `SkeletonWorld` via `OrbitalBody`. No runtime dep added; layering preserved.

`TemperatureField { per_tile_k: Vec<f64> }`. Two branches per design doc section 5.5: rotating planets use `T_base(lat) - lapse_rate * elevation` with cos(lat) irradiance scaling; tidally locked planets use substellar-angle irradiance. Lapse rate derived from atmosphere (scale with pressure; dry adiabatic fallback). Tests: Earth inputs give ~288 K mean surface temp, polar < equatorial, tidally locked substellar point > antistellar.

### BIOME-01: Biome palette enums
- **Crate:** ymir-biome
- **Status:** done
- **Depends on:** ATMO-02
- **Blocked:** no
- **Assignee:** agent

NOTE: `ymir-atmosphere` added as direct dep to ymir-biome because `palette_for` consumes `AtmosphereClass`. Layering stays acyclic: biome → atmosphere → system → catalog → core.

`Biome` enum with variants spanning all palette types (forest, grassland, desert, tundra, ocean, bedrock, dust_plain, polar_co2_ice, venus_crust, titan_methane_sea, etc.). `BiomePalette` enum selecting which subset is available, with `palette_for(atmo_class) -> &'static [Biome]`. Per design doc section 5.6 palette switch.

### CLIM-02: Moisture field
- **Crate:** ymir-climate
- **Status:** done
- **Depends on:** CLIM-01
- **Blocked:** no
- **Assignee:** agent

`MoistureField { per_tile: Vec<f64> }` (relative humidity or g/kg). Two-component model: latitude-banded circulation (or radial from substellar point if tidally locked) + wind-transported moisture with orographic precipitation bias. Clausius-Clapeyron capacity scaling with atmospheric P and T. Dry worlds (no liquid water retained) return zero everywhere. Tests: zero moisture on dry world, poles drier than mid-latitudes on Earth-like, windward slopes wetter than leeward.

### CLIM-03: Wind model
- **Crate:** ymir-climate
- **Status:** done
- **Depends on:** CLIM-01
- **Blocked:** no
- **Assignee:** agent

`WindField { per_tile: Vec<WindVector>, cell_count: u32 }`. Rotating planets: latitude-dependent prevailing winds (trade/westerly/polar easterly). Cell count scaled by rotation rate. Tidally locked: radial outflow from substellar at surface. Feeds CLIM-02 orographic transport.

### CLIM-04: ClimateMap and SkeletonWorld integration
- **Crate:** ymir-climate
- **Status:** done
- **Depends on:** CLIM-01, CLIM-02, CLIM-03
- **Blocked:** no
- **Assignee:** agent

`ClimateMap { temperature, moisture, wind }` struct. `ClimateMap::build(&SkeletonWorld) -> Self` runs all three fields in the right order (temperature → wind → moisture, since moisture needs wind). Serializable. Tests: build is deterministic; round-trips through bincode.

### BIOME-02: Whittaker classification
- **Crate:** ymir-biome
- **Status:** done
- **Depends on:** BIOME-01
- **Blocked:** no
- **Assignee:** agent

NOTE: Dependency on CLIM-04 relaxed — BIOME-02 takes raw (temperature_k, humidity, palette) and returns a Biome. The ClimateMap composite is not required; BIOME-04 will wire things together.

Temperature × moisture → Biome lookup, returning only biomes in the active palette. Earth-palette lookup calibrated against a standard Whittaker diagram. Abiotic palettes use simpler physical-state maps. Tests: Earth-palette (hot, wet) → tropical forest; (cold, dry) → tundra; Mars-palette maps (any, any) to a small abiotic set.

### BIOME-03: Markov smoothing
- **Crate:** ymir-biome
- **Status:** done
- **Depends on:** BIOME-02
- **Blocked:** no
- **Assignee:** agent

Markov transition smoothing pass over tile neighbors. Weight file schema (TOML/JSON) with transition probabilities between biomes. Phase 2 ships hand-tuned weights per palette (data-derived weights are Phase 5). Tests: smoothing reduces lone-tile islands; preserves large contiguous regions; idempotent-ish after enough iterations.

### BIOME-04: BiomeMap integration
- **Crate:** ymir-biome
- **Status:** done
- **Depends on:** BIOME-03
- **Blocked:** no
- **Assignee:** agent

`BiomeMap { per_tile: Vec<Biome> }` struct. `BiomeMap::build(&SkeletonWorld, &ClimateMap) -> Self` runs Whittaker + smoothing. Serializable. Tests: determinism, serde round-trip, every tile assigned a biome in the active palette.

### STOR-02: Climate and biome persistence
- **Crate:** ymir-storage
- **Status:** done
- **Depends on:** CLIM-04, BIOME-04
- **Blocked:** no
- **Assignee:** agent

Because ymir-storage cannot depend on ymir-climate / ymir-biome per the crate dependency rules, add generic save/load helpers in ymir-storage (`save_bin<T: Serialize>(path, &t)`, `load_bin<T: DeserializeOwned>(path) -> T`) and keep the concrete climate.bin / biomes.bin writes in the binary crate. Update `stages_computed` vocabulary in the manifest ("climate", "biomes") and extend `ymir info` to summarize climate stats (mean T, moisture coverage) and biome histogram.

### REND-02: Biome-colored Mollweide
- **Crate:** ymir-render
- **Status:** done
- **Depends on:** BIOME-04
- **Blocked:** no
- **Assignee:** agent

Add `render_biome_mollweide(&SkeletonWorld, &BiomeMap, cfg)` producing a PNG where each tile is colored by biome (static color table per Biome variant). Keep the elevation renderer; this is an additional entry point. Tests: dimensions, determinism, recognizable palette-level distinctions (Earth-like shows greens/blues/tans; Mars-like shows reds/browns).

NOTE: Implemented as two new modules in ymir-render: `biome_palette` (exhaustive `match` on `Biome` so adding a variant breaks the build) and `biome_mollweide` (reuses the existing Mollweide inverse + nearest-tile dot-product search from `globe_renderer`). Off-map background is a fixed deep near-black `[10, 10, 12]` chosen so it can't collide with any biome color; not a config knob. Synthetic-world tests stamp `BiomeMap` directly rather than running the full climate pipeline, keeping the renderer tests hermetic and fast. `BiomeRenderConfig::default()` is 2048x1024 (Phase 1 preview size).

### CLI-04: Extend ymir generate for climate and biomes
- **Crate:** ymir (binary)
- **Status:** done
- **Depends on:** CLIM-04, BIOME-04, REND-02, STOR-02
- **Blocked:** no
- **Assignee:** agent

After the skeleton stage, compute `ClimateMap` and `BiomeMap`. Persist climate.bin, biomes.bin. Emit `preview_biome.png` alongside the elevation preview. Update `stages_computed`. Add `--skip-climate` / `--skip-biomes` flags so users can stop at any stage. Update the stdout summary with climate + biome histogram.

NOTE: `--skip-climate` implies `--skip-biomes` enforced in code (biomes require climate). Manifest save moved to the end of `run_generate` so `stages_computed` reflects the full set actually computed. Earth smoke (`--star Earth --seed 1`) lands at mean T 288.0 K with an Earth-like biome mix (Grassland 23.7%, Savanna 22.9%, HotDesert 16.8%, Tundra 16.5%, BorealForest 6.5%); no calibration deviations. Tau Ceti planet 1 (`--planet 1`) resolves to Tau Ceti h at 0.243 AU (Venus-regime, mean T 1060 K, LavaPlain-dominated) rather than Tau Ceti e, which reflects the current placement ordering.

### CLI-03: ymir regenerate command
- **Crate:** ymir (binary)
- **Status:** done
- **Depends on:** CLI-04, CORE-04, CORE-05
- **Blocked:** no
- **Assignee:** agent

`ymir regenerate --world PATH --overrides FILE` applies a per-stage override JSON, marks dirty stages via the dependency graph, recomputes only dirty stages, and rewrites the manifest with the new `stages_computed` list and overrides_file pointer. Tests: override at stage 3 (atmosphere) recomputes atmosphere + skeleton + climate + biomes but leaves star/system bits unchanged; byte comparison of unchanged stage artifacts.

NOTE (implementation): `run_generate` was refactored into stage-helper functions: `compute_upstream` (stages 1-3 together: star lookup, placement/fixed-body, atmosphere derivation), `compute_skeleton`, `compute_climate`, `compute_biomes`, plus render helpers (`render_elevation_preview`, `render_biome_preview`). Each compute helper accepts a `StageOverrides` and merges any JSON override into the computed stage output via deep-merge (`merge_json` + `apply_json_override`). `run_regenerate` (new) loads the manifest, parses the override file via CORE-04, computes dirty stages via `dirty_stages_from_overrides` (which drives `PipelineDirtyState::mark_override_at`), then for each stage either recomputes (if dirty) or loads the persisted artifact. The manifest is rewritten with `overrides_file` set to the override file path. Clean stages stay byte-identical on disk because the code path never touches their files. Byte-identity of `skeleton.bin` across a climate-only override is exercised by `regenerate::regenerate_climate_override_leaves_skeleton_byte_identical`.

Phase-2 caveat: stages 1-3 (stellar, system, atmosphere) live only inside `skeleton.bin` in the current persistence layout — there are no separate `stellar.bin` or `system.bin` files. So "byte-identical upstream artifacts" collapses to "skeleton.bin byte-identical" when no upstream override is applied. Overrides at stages 5-6 (climate, biome) leave skeleton.bin untouched; overrides at stages 2-4 rebuild skeleton.bin. When only climate or biome are dirty, the upstream is loaded from `skeleton.bin` rather than recomputed, preserving byte-identity by construction.

### VALID-01: Earth validation test
- **Crate:** ymir (binary, integration tests)
- **Status:** done
- **Depends on:** CLI-04, CAT-03
- **Blocked:** no
- **Assignee:** agent

Integration test: `ymir generate --star Earth --seed 1` produces mean surface T within 5 K of 288 K, ocean tiles ~70% of surface (given Earth continental_fraction override), biome histogram includes forest/grassland/desert/tundra/ocean. Pure assertions, no UI.

NOTE (implementation): `tests/valid_earth.rs` drives the compiled binary via `CARGO_BIN_EXE_ymir` into a tempdir, then loads `climate.bin` / `biomes.bin` via `ymir_storage::load_bin`. Asserted: mean surface T within 5 K of 288 K (actual 288.0 K at seed 1), and biome histogram includes at least one forest, grassland/savanna, desert, and cold/polar variant. The ocean-fraction and Ocean-biome assertions were deliberately deferred: `earth_body()` in `src/main.rs` does not set a `continental_fraction` override, and the Whittaker classifier excludes Ocean (reserved for the BIOME-04 overlay pathway). Revisit once continental_fraction overrides land in the skeleton stage. Test runtime ~70 s (full pipeline at subdivision 5).

### VALID-02: Mars validation test
- **Crate:** ymir (binary, integration tests)
- **Status:** done
- **Depends on:** CLI-04, CAT-03
- **Blocked:** no
- **Assignee:** agent

Integration test: `ymir generate --star Mars --seed 1` produces the Mars-like abiotic biome palette (no forests/grasslands), mean T below 230 K, CO2-dominated atmosphere.

NOTE (implementation): `tests/valid_mars.rs` drives the compiled binary via `CARGO_BIN_EXE_ymir` into a tempdir, then loads artifacts via `ymir_storage::load_bin`. Mars does not persist a separate `atmosphere.bin`; the atmosphere lives inside `SkeletonWorld`, so the test loads `skeleton.bin` and reads `skeleton.atmosphere.class`. Actual Mars smoke (seed=1): `AtmosphereClass::ThinCO2` at 0.0006 bar, climate mean T 211.0 K (min 133.3, max 222.2), biome histogram 100% `MartianBedrock` across 10242 tiles. The brief's ~222 K expected mean reflects the atmosphere-level effective surface temp; the per-tile field pulls the mean down via lapse-rate cooling at negative elevations (mean elevation -2995 m) plus cos(lat) latitude scaling, consistent with the temperature model. No new dev-deps needed: the binary crate's `[dependencies]` are visible to integration tests. Test runtime ~70 s.

### VALID-03: Tidally locked validation test
- **Crate:** ymir (binary, integration tests)
- **Status:** done
- **Depends on:** CLI-04
- **Blocked:** no
- **Assignee:** agent

Integration test: on a synthetic tidally-locked body, substellar-point temperature is significantly higher than antistellar temperature; biome map shows radial zoning rather than latitudinal banding.

NOTE: Implemented via Option A (full CLI invocation) as `tests/valid_tidally_locked.rs`. Picked `--star "Tau Ceti" --seed 1 --planet 0`: placement generates a planet at 0.055 AU, well inside Tau Ceti's ~0.093 AU lock distance (star mass 0.783 M_sun, age 5.8 Gyr), so `is_tidally_locked` returns true and the body lands in the airless tidally-locked regime (atmosphere class None, climate T range 50 K to ~905 K per CLI summary). Test asserts: (1) `skeleton.body.tidal_locked == true`, (2) T at equator/substellar tile exceeds T at equator/antistellar tile by >50 K (actual gap is ~850 K since the nightside clamps to `TIDAL_LOCKED_MIN_K = 50` K while substellar approaches local equilibrium near 905 K), (3) mean |dT| between equal-latitude / opposite-longitude tile pairs exceeds 2x the mean |dT| between equal-longitude / opposite-latitude pairs (radial zoning dominates latitudinal banding). Test runtime ~9 s in release, ~70 s in debug.

### VALID-04: Override progression test
- **Crate:** ymir (binary, integration tests)
- **Status:** done
- **Depends on:** CLI-03
- **Blocked:** no
- **Assignee:** agent

Integration test: generate world A, save; apply an atmosphere-stage override; regenerate; confirm atmosphere, skeleton, climate, biome artifacts changed but star/system artifacts are byte-identical.

NOTE (implementation): Implemented as `tests/valid_override_progression.rs` using a **climate-stage override** (Option A) rather than an atmosphere override, for two reasons. First, the Phase 2 persistence layout bundles stellar + system + atmosphere into `skeleton.bin` (there is no standalone `stellar.bin` / `system.bin`), so the brief's "star/system artifacts byte-identical while atmosphere/skeleton change" cannot be expressed directly; an atmosphere override rebuilds skeleton.bin along with everything downstream. Second, a climate override cleanly verifies the dirty-propagation contract: `skeleton.bin` and `preview.png` stay byte-identical, while `climate.bin`, `biomes.bin`, and `preview_biome.png` all change. To force a real content change (the merge path replaces arrays wholesale, but an empty `{"climate": {}}` would round-trip identically), the test loads the freshly-generated `ClimateMap`, shifts every per-tile temperature by +50 K, and writes the shifted map as the climate override JSON. Earth smoke (seed=1, subdivision=5): skeleton.bin = 1_768_717 bytes byte-identical before/after; climate.bin = 245_836 bytes before and after (same length, different bytes); biomes.bin and preview_biome.png both differ as expected since +50 K pushes many tiles across Whittaker bands. Test runtime ~13 s in release. Asserts also that the reloaded climate.bin carries the shifted temperatures to guard against silent merge regressions.

---

## Phase 3: Regional detail

Per design doc section 5.7: on-demand high-resolution generation for a region of the skeleton. Implements stage 7 of the 7-stage pipeline. Also folds in three deferred pieces from Phase 2 (Earth continental_fraction override, water-biome overlay, unified SkeletonTile view) that were flagged in implementation NOTEs.

### CAT-04: Earth continental_fraction override
- **Crate:** ymir (binary) + ymir-surface
- **Status:** ready
- **Depends on:** CAT-03, SURF-03
- **Blocked:** no
- **Assignee:**

Wire a `continental_fraction` observational override through `earth_body()` in `src/main.rs` and through the skeleton stage so Earth generates with ~29% land. The heightmap currently ignores any such override; extend `HeightmapConfig` (or the equivalent) to accept an optional continental-fraction target and calibrate the elevation threshold to hit it. Also wire Mars to a ~100% land target (it's land-dominated already, but explicit is better). Tests: Earth mean ocean-tile fraction within 5 pp of 0.71; Mars land-tile fraction ≥ 0.98. Update VALID-01's deferred ocean assertion to assert on the actual continental_fraction once BIOME-05 lands.

### BIOME-05: Water-biome overlay
- **Crate:** ymir-biome
- **Status:** ready
- **Depends on:** BIOME-04, CAT-04
- **Blocked:** no
- **Assignee:**

Post-Whittaker overlay pass that paints Ocean, CoastalShallow, Wetland, and AlpineMeadow biomes from skeleton elevation and climate. Rules (first approximation):
- Elevation < sea level → Ocean (deep) or CoastalShallow (shallow / depth < ~200 m).
- Elevation just above sea level with humidity > ~0.7 → Wetland.
- Elevation above the snow line with moderate moisture → AlpineMeadow (Earth-like palette only).
Runs between Whittaker classification and Markov smoothing inside `BiomeMap::build`, or as an additional pass exposed via `BiomeMapConfig`. Non-Earth palettes should leave these biomes unused (they're already in the palette membership tables from BIOME-01). Tests: Earth with continental_fraction 0.29 yields ≥60% Ocean+CoastalShallow tiles; Mars yields 0 water biomes. Update VALID-01 to re-enable the Ocean assertion.

### SURF-04: Unified SkeletonTile view
- **Crate:** ymir-surface
- **Status:** ready
- **Depends on:** SURF-03
- **Blocked:** no
- **Assignee:**

Consolidate `ElevationMap` + `TectonicData` + grid neighbor lookups into a `SkeletonTile` accessor (`world.tile(idx) -> SkeletonTile { lat, lon, elevation_m, plate_id, plate_type, neighbors: &[usize] }`). Phase 2 NOTE deferred this; Phase 3 regional detail needs quick per-tile access across three data sources during region extraction. Implement as a view struct (zero-copy where possible) so it doesn't duplicate storage. Tests: accessor returns consistent values with the underlying maps; neighbor lists match `GeodesicGrid`.

### DET-01: Region specification and PRNG derivation
- **Crate:** ymir-detail
- **Status:** ready
- **Depends on:** CORE-02, SURF-04
- **Blocked:** no
- **Assignee:**

`RegionSpec { tile_index: u32, radius_tiles: u32 }` identifying a contiguous neighborhood of skeleton tiles to generate detail for. Radius-0 is a single tile (not generally useful); radius-1 is a tile plus its ~6 neighbors. Include a deterministic PRNG factory: `detail_rng(world_seed: u64, tile_index: u32) -> PcgRng` using `hash(world_seed, tile_index)` per design doc §5.7. Tests: same (seed, tile_index) yields the same RNG sequence; different inputs diverge.

### DET-02: Hex subgrid generation
- **Crate:** ymir-detail
- **Status:** pending
- **Depends on:** DET-01
- **Blocked:** no
- **Assignee:**

Generate a hex grid covering a `RegionSpec`. Fixed subdivision (e.g. 32×32 hexes per skeleton tile in the initial pass). Handle the gnomonic projection from the skeleton-tile local frame to a planar hex grid; document the distortion bound. Serializable. Tests: hex count matches expected formula; every hex has a well-defined (lat, lon); neighbor relationships are consistent across shared edges between adjacent skeleton tiles.

### DET-03: Fractal-perturbed elevation
- **Crate:** ymir-detail
- **Status:** pending
- **Depends on:** DET-02
- **Blocked:** no
- **Assignee:**

Per-hex elevation = skeleton-tile baseline + FBM noise at detail scale. Reuse `ymir_surface::noise::spherical_fbm` seeded from the detail RNG. Boundary handling: elevation at shared edges between adjacent skeleton tiles must match across the seam (blend from baseline at tile centers to shared value at boundary). Tests: determinism; cross-tile seam continuity (max |dH| across seam < threshold); perturbation magnitude bounded by gravity-scaled cap.

### DET-04: Orographic moisture refinement
- **Crate:** ymir-detail
- **Status:** pending
- **Depends on:** DET-03
- **Blocked:** no
- **Assignee:**

Local humidity = skeleton-tile moisture baseline + orographic lift/shadow from the wind field. Use prevailing wind at the region; advect moisture uphill (saturation → precipitation → downwind dry shadow). Simpler first pass: humidity *= f(upwind elevation gradient). Tests: windward slopes wetter than leeward slopes at equal elevation; humidity stays in [0, saturation_ceiling].

### DET-05: River flow accumulation + lake formation
- **Crate:** ymir-detail
- **Status:** pending
- **Depends on:** DET-03
- **Blocked:** no
- **Assignee:**

Flow-accumulation algorithm on the hex grid using D8-equivalent (D6 for hexes) steepest-descent. Fill local minima into lakes via priority-flood or Planchon-Darboux. Emit per-hex `flow_accumulation: f32` and `is_lake: bool` fields. Rivers are implicit (hexes with accumulation > threshold). Tests: no hex flows uphill; every drainage basin has exactly one outlet (or terminates in a lake); total water volume conserved up to numerical tolerance.

### DET-06: Fine-grained biome refinement
- **Crate:** ymir-detail
- **Status:** pending
- **Depends on:** DET-04, DET-05, BIOME-03
- **Blocked:** no
- **Assignee:**

Classify each hex via the same Whittaker logic (BIOME-02) using refined local T/humidity, then apply Markov smoothing (BIOME-03) at detail scale. Rivers (flow_accumulation > threshold) become distinct river-channel biomes or bias neighbors toward Wetland. Lakes get the water-body biome. Boundary condition: detail biomes at the region edge should agree with the parent skeleton tile's biome within tolerance (same palette, same-family biome). Tests: palette membership; lake hexes are water-body; river hexes follow descending elevation gradient.

### DET-07: RegionalDetail composite
- **Crate:** ymir-detail
- **Status:** pending
- **Depends on:** DET-02, DET-03, DET-04, DET-05, DET-06
- **Blocked:** no
- **Assignee:**

`RegionalDetail { spec, hex_grid, elevation, moisture, flow, biomes }` capstone. `RegionalDetail::build(&SkeletonWorld, &ClimateMap, &BiomeMap, spec, cfg)` orchestrates DET-02..06. Serializable. Tests: determinism; serde round-trip; boundary consistency with parent skeleton tile; same-region regeneration is byte-identical.

### STOR-03: Per-region persistence
- **Crate:** ymir-storage + ymir (binary)
- **Status:** pending
- **Depends on:** DET-07
- **Blocked:** no
- **Assignee:**

Persist regional details to `<world>/detail/region_NNNN.bin` (design doc §6.2). Update manifest to track which regions have been generated (e.g. `regions_generated: Vec<u32>`). Reuse `save_bin`/`load_bin`. Tests: save then load round-trips a `RegionalDetail`; manifest update is idempotent.

### REND-03: Regional detail renderer
- **Crate:** ymir-render
- **Status:** pending
- **Depends on:** DET-07
- **Blocked:** no
- **Assignee:**

Render a `RegionalDetail` to a high-res PNG: hex grid rasterized with biome-colored fill, elevation-shaded (hillshade), river/lake overlay. Separate entry point from the Mollweide renderers. Consider a stereographic or orthographic local projection centered on the region. Tests: output dimensions; biome palette visible; river hexes distinguishable.

### CLI-05: `ymir detail` command
- **Crate:** ymir (binary)
- **Status:** pending
- **Depends on:** DET-07, STOR-03, REND-03
- **Blocked:** no
- **Assignee:**

`ymir detail --world PATH --region TILE_INDEX [--radius N] [--output-image FILE]` generates regional detail for the specified tile, persists to `detail/region_NNNN.bin`, and optionally renders a PNG. Update the manifest's regions-generated list. Stdout summary: hex count, elevation range, biome histogram, river/lake counts. Tests: command succeeds on an Earth world; re-running with the same region produces byte-identical output.

### VALID-05: Regional determinism test
- **Crate:** ymir (binary, integration tests)
- **Status:** pending
- **Depends on:** CLI-05
- **Blocked:** no
- **Assignee:**

Integration test: generate an Earth world, run `ymir detail --region 100 --region 200 --region 300`, re-run in a fresh tempdir, assert all three region bin files are byte-identical across runs. Proves `hash(world_seed, tile_index)` seeding is stable.

### VALID-06: River plausibility test
- **Crate:** ymir (binary, integration tests)
- **Status:** pending
- **Depends on:** CLI-05
- **Blocked:** no
- **Assignee:**

Integration test: generate an Earth world, pick a region known to contain a continent (non-zero land fraction), run `ymir detail`. Assert: (a) for every hex with `flow_accumulation > 0`, the downstream hex has strictly lower elevation — no uphill flow; (b) each hex's flow path terminates in a lake or at the region boundary within N steps; (c) at least one basin (flow accumulation > threshold) exists.

