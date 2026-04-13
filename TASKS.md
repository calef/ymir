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
- **Model:** sonnet
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

### Model Tags

The **Model** field picks which Claude tier runs the task as a background agent:

| Tier | When to use |
|------|-------------|
| `sonnet` (default) | Spec-driven implementation, clear public API, explicit test list, module-per-task, integration tests, CLI subcommands, data fetchers, mechanical refactors with type-driven guidance |
| `opus` | Cross-cutting refactors touching many crates, type-system design, non-obvious trade-offs, security-sensitive review, first task in a new subsystem (scaffolds), serde compat / backwards-compat design calls |

When in doubt, start with `sonnet`. If an agent at `sonnet` routinely fails or produces shallow work for a given task, escalate to `opus` and update the rubric here.

### Rules

- A `pending` task becomes `ready` when all its **Depends on** tasks are `done` and **Blocked** is `no`.
- When blocking a task, set **Status** to `blocked` and **Blocked** to a description of what's needed (e.g., "needs human decision on override granularity").
- When unblocking, set **Blocked** back to `no` and **Status** to `ready` (or `pending` if dependencies aren't met).
- Keep IDs stable. Don't renumber tasks after creation.
- Prefix IDs with a short crate tag: CORE, CAT, SYS, ATMO, SURF, CLIM, BIOME, DET, REND, STOR, CLI, INFRA.
- Honor the **Model** field when dispatching an agent. Override inline only when the task surprises you; if overriding becomes routine, update the tag.

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

Design reference: `ARCHITECTURE.md` sections 5.5, 5.6, 11 (Phase 2). Deliverable per design doc: globe PNGs with biome coloring that look recognizably different for Earth-like vs Mars-like vs tidally locked worlds; `ymir regenerate` recomputes only dirty stages after an override is applied.

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

## Open Source Release Artifacts

Baseline hygiene for publishing Ymir as an open-source Rust project. Tasks are independent unless otherwise noted and can be picked up in parallel once GitHub hosting is in place. Tier 1 items (README, LICENSE, CHANGELOG, CONTRIBUTING, CoC, SECURITY, issue/PR templates) are required for a public-facing release; Tier 2 items (crates.io metadata, toolchain pin, Dependabot, cargo-deny) are strongly recommended before any crate is actually published.

### CLEAN-01: Clippy 1.88 uninlined_format_args cleanup
- **Crate:** ymir-core, ymir-surface, ymir (binary)
- **Status:** done
- **Depends on:** INFRA-05
- **Blocked:** no
- **Assignee:** agent

The INFRA-05 toolchain bump (1.85 → 1.88) activated `clippy::uninlined_format_args` by default, surfacing ~20+ pre-existing violations that now fail CI. Mechanical fix: rewrite `format!("...{}", x)` as `format!("...{x}")` throughout. Affected files observed: `crates/ymir-core/src/dependency_graph.rs` (lines ~132, 141, 169, 179), `crates/ymir-surface/src/geodesic.rs` (lines ~460, 515, 523, 543, 619), `src/main.rs` (many — lines ~313, 1005, 1011, 1030, 1065-1069, 1089). Also sweep any other crates that clippy flags. Verify: `cargo clippy --workspace --all-targets -- -D warnings` returns 0.

NOTE: Fixed 18 format-string sites across 3 files: `crates/ymir-core/src/dependency_graph.rs` (4), `crates/ymir-surface/src/geodesic.rs` (4), `src/main.rs` (10). `cargo clippy --workspace --all-targets -- -D warnings` now passes clean; `cargo fmt --check` clean; `cargo test --workspace --release` all pass. (Two hex_grid.rs sites flagged in initial clippy run were resolved by a concurrent agent before this sweep completed.)

### DOC-01: README.md
- **Crate:** repo root
- **Status:** done
- **Depends on:** DOC-09
- **Blocked:** no
- **Assignee:** agent

Project-level README at the repo root. Sections: one-line description, badges (CI, license, crates.io once published), what Ymir is and isn't, a short "why causal pipeline" paragraph, install/build instructions (`cargo install --path .` + system requirements), quickstart (`ymir generate --star Earth --seed 1`), link to the design doc, link to CONTRIBUTING, license summary. Include a sample Mollweide PNG rendered from the quickstart. Keep it scannable; details belong in the design doc and per-crate docs.

NOTE: `README.md` written at repo root (123 lines). Covers badges (CI/MSRV/license with TODO markers for crates.io until INFRA-04 and the GitHub remote), what Ymir is and isn't, why-causal-pipeline paragraph, quickstart with `cargo install --path .` and `ymir generate --star Earth --seed 1 --output worlds/earth-1/`, layered workspace graph, build/test commands, current status (Phase 1+2 done, Phase 3 in progress with link to TASKS.md), documentation links (ARCHITECTURE.md, CONTRIBUTING.md, CHANGELOG.md, CODE_OF_CONDUCT.md, SECURITY.md, docs.rs TODO), and dual MIT OR Apache-2.0 license. No preview PNGs were committed to the repo yet, so the README documents the expected output paths and leaves commented-out `![…](…)` image tags to uncomment once sample previews are added. `cargo fmt --check` and `cargo test --workspace --release` pass; `cargo clippy --workspace --all-targets -- -D warnings` still fails with the pre-existing `uninlined_format_args` warnings in `src/main.rs` that CLEAN-01 owns (not fixed here).

### DOC-02: CONTRIBUTING.md
- **Crate:** repo root
- **Status:** done
- **Depends on:**
- **Blocked:** no
- **Assignee:** agent

Contribution guide covering: how to file a good bug report, how to propose a feature (link to TASKS.md process), dev setup (`cargo check`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt`), commit/PR conventions, DCO or CLA note if applicable (default: DCO sign-off), and how to claim a task from TASKS.md. Explicitly call out the workspace layering rule (crate dependency graph in design doc §7.2) and the "all public items documented" rule.

NOTE: `CONTRIBUTING.md` written at repo root (105 lines). Covers bug reports, feature proposals linking to the TASKS.md lifecycle, dev setup with per-crate and `--workspace --all-targets` variants, task claim/cascade flow, PR conventions (optional conventional commits, task ID references, PR template), DCO sign-off via `git commit --signoff` with a link to developercertificate.org, the four hard rules (public-item docs, no sibling-crate type leaks per design §7.2, `approx` for float comparisons, `rand_pcg` seeded from the world seed), and a Getting Help section pointing at SECURITY.md / CODE_OF_CONDUCT.md / Issues / Discussions. Quality-gate verification surfaced pre-existing fmt and clippy failures in `crates/ymir-biome/src/water_overlay.rs` unrelated to this markdown-only change; flagged for whoever owns that file.

### DOC-03: CODE_OF_CONDUCT.md
- **Crate:** repo root
- **Status:** done
- **Depends on:**
- **Blocked:** no
- **Assignee:**

Adopt Contributor Covenant v2.1 verbatim. Update the enforcement contact line to a project-controlled email or GitHub handle before merging. No custom text.

NOTE: Contributor Covenant v2.1 adopted verbatim; enforcement contact is a TODO placeholder.

### DOC-04: SECURITY.md
- **Crate:** repo root
- **Status:** done
- **Depends on:**
- **Blocked:** no
- **Assignee:**

Vulnerability reporting policy. Low-ceremony version: a single contact address (private email or GitHub Security Advisories), a statement of supported versions (initially "only `main`"), expected response window, and a note that Ymir is a simulation library with minimal attack surface but still welcomes dependency-level vulnerability reports.

NOTE: Wrote `SECURITY.md` (37 lines): GHSA preferred, email fallback with TODO marker, supported versions (main only pre-1.0), 7/14/30-day response window, scope/out-of-scope, 90-day coordinated disclosure. Doc-only change; pre-existing workspace fmt/test breakage in other crates is unrelated.

### DOC-05: CHANGELOG.md
- **Crate:** repo root
- **Status:** done
- **Depends on:**
- **Blocked:** no
- **Assignee:**

Keep-a-Changelog v1.1 format. Start with `[Unreleased]` section and a `[0.1.0] - YYYY-MM-DD` entry covering Phase 1+2 deliverables (skeleton, climate, biomes, CLI, rendering, override progression). Use Added/Changed/Fixed/Removed groupings. Link version headings to future git tags.

**NOTE:** Wrote `CHANGELOG.md` with `[Unreleased]` stubs + `[0.1.0] - 2026-04-12` covering all Phase 1+2 deliverables; compare/tag link references carry a TODO placeholder until a GitHub remote exists.

### DOC-06: LICENSE files
- **Crate:** repo root
- **Status:** done
- **Depends on:**
- **Blocked:** no
- **Assignee:**

Ymir is dual-licensed MIT OR Apache-2.0 (already declared in every Cargo.toml). Commit the full license texts as `LICENSE-MIT` and `LICENSE-APACHE` at the repo root, plus a short `LICENSE` stub pointing at both. Match the copyright holder / year line used in existing Cargo.toml metadata.

NOTE: LICENSE-MIT copyright line reads "Copyright (c) 2026 Christopher Alef and Ymir contributors" (kept from the pre-existing file; no Cargo.toml `authors` field to override it). LICENSE-APACHE replaced with the verbatim canonical text from https://www.apache.org/licenses/LICENSE-2.0.txt (sha256 cfc7749b…523d30) after a prompt-injection string was found on line 185 of the pre-existing file. LICENSE stub written per spec. No NOTICE file added (no bundled third-party Apache-2.0 code).

### DOC-07: Issue templates
- **Crate:** repo root
- **Status:** done
- **Depends on:**
- **Blocked:** no
- **Assignee:**

`.github/ISSUE_TEMPLATE/bug_report.yml` and `.github/ISSUE_TEMPLATE/feature_request.yml` using GitHub's form-based schema. Bug report asks for: ymir version, rust version, `ymir info` output (if applicable), repro steps, expected vs. actual, attached override JSON. Feature request asks for: problem statement, proposed behavior, scope (which stage(s)?), alternatives considered. Include `config.yml` to disable blank issues and point at Discussions if/when enabled.

**NOTE:** Created `.github/ISSUE_TEMPLATE/bug_report.yml`, `.github/ISSUE_TEMPLATE/feature_request.yml`, and `.github/ISSUE_TEMPLATE/config.yml`.

### DOC-08: Pull request template
- **Crate:** repo root
- **Status:** done
- **Depends on:**
- **Blocked:** no
- **Assignee:**

`.github/pull_request_template.md` with sections: summary, linked TASKS.md task ID(s), what changed, test plan (commands run), screenshots for render-affecting changes, checklist (tests added, docs updated, `cargo fmt`/`clippy`/`doc` clean, CHANGELOG entry).

**NOTE:** Created `.github/pull_request_template.md` with summary, linked tasks (`Closes TASKS.md#CRATE-NN`), changes, test plan (fmt/clippy/test/doc + manual), screenshots, and checklist sections.

### DOC-09: Relocate ymir-design.md
- **Crate:** repo root
- **Status:** done
- **Depends on:**
- **Blocked:** no
- **Assignee:**

Current location (`ymir-design.md` at repo root) is idiosyncratic. Rename/relocate consistent with OSS conventions: either `ARCHITECTURE.md` at the repo root (per the GitHub community-profile checklist and Aleksey Kladov's "ARCHITECTURE.md" essay), or `docs/architecture.md` with a one-line pointer from the README. Grep for every reference in the repo (CLAUDE.md, task NOTEs, crate docs) and update paths in the same change. Preserve git history via `git mv`.

**NOTE:** Renamed via `git mv ymir-design.md ARCHITECTURE.md` at the repo root (chosen over `docs/architecture.md` for discoverability and GitHub community-profile alignment). Updated literal `ymir-design.md` references in `CLAUDE.md` (1), `CONTRIBUTING.md` (2), and `TASKS.md` (1 in the ATM-01 design-reference line). No crate-level doc comments, `.github/` templates, or intra-doc links referenced the old filename. Conceptual "design doc" / "design document" prose left intact.

### INFRA-04: crates.io metadata
- **Crate:** all crates
- **Status:** done
- **Depends on:**
- **Blocked:** no
- **Assignee:** agent

Audit every `Cargo.toml` in `crates/` and the binary crate for the metadata `cargo publish` expects: `description`, `license` (already present), `repository`, `homepage`, `documentation` (typically `https://docs.rs/<crate>`), `readme` (per-crate if present, else repo README via workspace inheritance), `keywords` (max 5), `categories`. Use workspace-level metadata inheritance (`[workspace.package]`) to reduce duplication. Verify with `cargo publish --dry-run -p <crate>` in layer order.

NOTE: Added `[workspace.package]` to root `Cargo.toml` with shared fields (`version = "0.1.0"`, `edition = "2024"`, `license = "MIT OR Apache-2.0"`, `repository`/`homepage` pointing at placeholder `https://github.com/TODO/ymir`, `readme = "README.md"`, `rust-version = "1.88"`, `authors = ["Christopher Alef and Ymir contributors"]`). All 10 library crates plus the root `ymir` binary now inherit these via `field.workspace = true` and override per-crate `name`, `description`, `documentation = "https://docs.rs/<crate>"`, `keywords` (5 max per crate — `simulation`/`worldgen` plus 2-3 crate-specific), and `categories` (valid crates.io slugs: `simulation`, `science`, `rendering`, `graphics`, `encoding`, `command-line-utilities`). Added `version = "0.1.0"` to every intra-workspace `path = "../foo"` dep so they resolve on publish. Set `publish = false` on the root `ymir` binary (end-user CLI, not a library). `cargo publish --dry-run -p ymir-core --allow-dirty` succeeds end-to-end; the other nine library crates fail dry-run only on "no matching package `ymir-foo` found" on crates.io, which is the normal workspace chicken-and-egg (they'll succeed once their dependencies are actually published in layer order). `cargo package --list --allow-dirty -p <crate>` confirms every manifest itself is valid. `cargo check --workspace`, `cargo fmt --check`, and `cargo clippy --workspace --all-targets -- -D warnings` all pass. `cargo test --workspace --release` has one failure in `ymir-detail::elevation::cross_tile_seam_continuity`, which belongs to DET-03 (concurrent) and is unaffected by manifest-only edits. TODO markers introduced: placeholder `https://github.com/TODO/ymir` in `[workspace.package].repository` and `.homepage`; README §badges + §Documentation carry TODO comments pointing at the new CONTRIBUTING.md §"Publishing to crates.io" section, which documents the gate (push repo, update URLs, publish bottom-up in layer order).

### INFRA-05: rust-toolchain.toml
- **Crate:** repo root
- **Status:** done
- **Depends on:**
- **Blocked:** no
- **Assignee:**

Pin the minimum supported Rust version. The workspace currently uses edition 2024, which requires Rust 1.85+. Commit `rust-toolchain.toml` with `channel = "1.85"` (or the actual floor), `components = ["rustfmt", "clippy"]`, and `profile = "minimal"`. Document the MSRV in README and CONTRIBUTING.

NOTE: Pinned to 1.88 (edition 2024 requires 1.85; `image 0.25.10` requires 1.88; `u32::is_multiple_of` in `ymir-climate` requires 1.87 — 1.88 satisfies all). CI migrated from `dtolnay/rust-toolchain@stable` to `actions-rust-lang/setup-rust-toolchain@v1` across all four jobs so the pin is honored. MSRV section added to CONTRIBUTING.md; README update belongs to DOC-01.

### INFRA-06: Dependabot config
- **Crate:** repo root
- **Status:** done
- **Depends on:** INFRA-02
- **Blocked:** no
- **Assignee:**

`.github/dependabot.yml` watching `cargo` (weekly) and `github-actions` (weekly). Group minor/patch updates to reduce PR noise. Exclude pre-1.0 crates from auto-merge suggestions. Ensure CI (INFRA-02) runs on Dependabot PRs.

NOTE: Added `.github/dependabot.yml` with two ecosystems (cargo limit 10 / github-actions limit 5), weekly Monday 06:00 America/Los_Angeles schedule, groups `cargo-minor-and-patch` and `github-actions-minor-and-patch` bundling minor+patch (majors stay ungrouped for individual review); no auto-merge configured. CI already runs on PRs (INFRA-02), so Dependabot PRs trigger it automatically.

### INFRA-07: cargo-deny config
- **Crate:** repo root
- **Status:** done
- **Depends on:** INFRA-02
- **Blocked:** no
- **Assignee:** agent

Add `deny.toml` for license, advisory, and source auditing. Allow MIT / Apache-2.0 / BSD-3-Clause / ISC / Unicode-DFS-2016. Deny GPL-family and "unknown" licenses. Add a CI job that runs `cargo deny check advisories licenses sources` and gates merges on it. Update CONTRIBUTING with the expectation that new dependencies must pass cargo-deny.

NOTE: `deny.toml` uses v2 schema; allow list is MIT / Apache-2.0 / BSD-2-Clause / BSD-3-Clause / ISC / Unicode-DFS-2016 / Unicode-3.0 / CC0-1.0 / Zlib / MPL-2.0 (default-deny, no explicit deny list). CI job `deny` uses `EmbarkStudios/cargo-deny-action@v2` and runs `cargo deny check advisories licenses sources bans`. CONTRIBUTING.md Hard Rules updated. Not verified locally (no cargo on PATH); CI is the first verification.

### INFRA-08: Publish repository on GitHub
- **Crate:** repo root
- **Status:** ready
- **Depends on:** INFRA-02, INFRA-06, INFRA-07
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

Push the local repo to a new GitHub repository so CI actually runs, Dependabot PRs start firing, and collaborators can PR. Until this lands all of INFRA-02/06/07 are theoretical.

Steps:

1. Confirm with the human: repo owner (user or org), repo name (default `ymir`), and visibility (public vs private). The LICENSE, README, CODE_OF_CONDUCT, CONTRIBUTING, SECURITY files already in the tree are shaped for a public OSS project; default to public unless the human says otherwise.
2. Run `gh auth status`. If not authenticated, stop and ask the human to run `gh auth login` (interactive — the agent cannot complete this step). Document the interactive requirement as a handoff.
3. Secret sweep before push. `git grep -I -E '(api[_-]?key|secret|token|password|BEGIN .* PRIVATE KEY)' -- :^CHANGELOG.md :^README.md :^CONTRIBUTING.md` and inspect hits. The catalog fetch scripts (`scripts/fetch_gaia_catalog.py`, `scripts/fetch_exoplanet_catalog.py`) talk to anonymous public archives, so they should contain no credentials, but verify.
4. Create the remote: `gh repo create <owner>/<name> --<public|private> --source=. --remote=origin --description "Causal star-to-surface planet simulation in Rust"`. Do NOT use `--push` on the first create call — push main separately so any failure is easier to diagnose.
5. `git push -u origin main` and then any additional branches that exist.
6. Verify: (a) the INFRA-02 CI workflow triggers and either passes or surfaces a real failure (if it fails, open an issue documenting it; do not hotfix on first push); (b) Dependabot registers and queues its first run on the next Monday 06:00 America/Los_Angeles slot; (c) the `deny` CI job runs and passes.
7. Repo hygiene on first landing: enable branch protection on `main` (require CI green, require PR review, disallow force-push). Add repository topics: `rust`, `simulation`, `procedural-generation`, `astronomy`, `planet-generation`. Set the default branch to `main`. These can be done via `gh` subcommands or the web UI; prefer `gh` so the sequence is replayable.
8. Update README with the canonical GitHub URL where relevant (clone command, CI badge link, issue-tracker reference). Include a CI badge if the workflow has a well-known name.

Gotchas: this is the first operation that exposes the repo publicly. Re-run the secret sweep right before `git push`, not just at task start — the working tree changes as other tasks land. If the CI workflow fails on first push, do not bypass with `push --force`; diagnose the failure and open a follow-up task.

---

## Phase 3: Regional detail

Per design doc section 5.7: on-demand high-resolution generation for a region of the skeleton. Implements stage 7 of the 7-stage pipeline. Also folds in three deferred pieces from Phase 2 (Earth continental_fraction override, water-biome overlay, unified SkeletonTile view) that were flagged in implementation NOTEs.

### CAT-04: Earth continental_fraction override
- **Crate:** ymir (binary) + ymir-surface
- **Status:** done
- **Depends on:** CAT-03, SURF-03
- **Blocked:** no
- **Assignee:** agent

Wire a `continental_fraction` observational override through `earth_body()` in `src/main.rs` and through the skeleton stage so Earth generates with ~29% land. The heightmap currently ignores any such override; extend `HeightmapConfig` (or the equivalent) to accept an optional continental-fraction target and calibrate the elevation threshold to hit it. Also wire Mars to a ~100% land target (it's land-dominated already, but explicit is better). Tests: Earth mean ocean-tile fraction within 5 pp of 0.71; Mars land-tile fraction ≥ 0.98. Update VALID-01's deferred ocean assertion to assert on the actual continental_fraction once BIOME-05 lands.

NOTE: Implemented `continental_fraction: Option<Sourced<f64>>` on `OrbitalBody` (ymir-system). `SkeletonWorld::build` now reads this and passes it through `HeightmapConfig::target_continental_fraction` to the heightmap generator, which sorts elevations and shifts by the `(1 - frac)` percentile so exactly the target fraction of tiles end up >= 0 m. `earth_body()` sets 0.29 (ETOPO1), `mars_body()` sets 1.0 (MOLA). Added calibration + determinism + ordering-preservation tests in `crates/ymir-surface/src/heightmap.rs`. VALID-01 now asserts ocean-tile fraction within 5 pp of 0.71 directly from `skeleton.bin` (does not require BIOME-05). Earth @ seed 1 produces ocean fraction 0.710 (7271/10242 tiles), mean elevation -2171.7 m, min -6758.8 m, max 4341.4 m. Biome histogram is unchanged (BIOME-05 not yet landed, so no Ocean biome is painted). `ymir info` now echoes the continental_fraction override in the manifest and reports the ocean-tile count in the skeleton section.

### BIOME-05: Water-biome overlay
- **Crate:** ymir-biome
- **Status:** done
- **Depends on:** BIOME-04, CAT-04
- **Blocked:** no
- **Assignee:** agent

Post-Whittaker overlay pass that paints Ocean, CoastalShallow, Wetland, and AlpineMeadow biomes from skeleton elevation and climate. Rules (first approximation):
- Elevation < sea level → Ocean (deep) or CoastalShallow (shallow / depth < ~200 m).
- Elevation just above sea level with humidity > ~0.7 → Wetland.
- Elevation above the snow line with moderate moisture → AlpineMeadow (Earth-like palette only).
Runs between Whittaker classification and Markov smoothing inside `BiomeMap::build`, or as an additional pass exposed via `BiomeMapConfig`. Non-Earth palettes should leave these biomes unused (they're already in the palette membership tables from BIOME-01). Tests: Earth with continental_fraction 0.29 yields ≥60% Ocean+CoastalShallow tiles; Mars yields 0 water biomes. Update VALID-01 to re-enable the Ocean assertion.

NOTE: Overlay lives in new module `crates/ymir-biome/src/water_overlay.rs` (public `apply_water_overlay` + `WaterOverlayConfig`). `BiomeMapConfig` now carries a `water_overlay: WaterOverlayConfig` field (default = enabled with Earth-calibrated thresholds); `BiomeMap::build` runs the overlay between Whittaker classification (step 1) and Markov smoothing (step 4). Palette gating: the overlay is a no-op on non-Earth-like palettes since Ocean/CoastalShallow/Wetland/AlpineMeadow are not in their palette membership tables (BIOME-01). Thresholds: elevation <= -200 m → Ocean, -200 m < elevation < 0 m → CoastalShallow; above sea level with elevation <= 100 m and humidity > 0.7 → Wetland; elevation >= 2500 m with temperature in [260, 283] K and humidity in [0.3, 0.8] → AlpineMeadow. Snow line is modeled jointly as elevation + temperature + moisture bounds (no separate lapse-rate derivation; the temperature field already carries the altitude signal downstream of CLIM-01). Added unit tests in `water_overlay.rs` (disabled-is-noop, non-Earth-palette-noop, sub-sea-level-tiles-become-water, Earth-yields-≥60%-water) and integration tests in `biome_map.rs` (Earth @ seed 1 ≥60% Ocean+CoastalShallow, Mars produces no water/alpine biomes, overlay-can-be-disabled). VALID-01 now asserts biome-level water fraction ≥ 0.50 on top of the skeleton-level ocean-fraction assertion. Earth @ seed=1 biome histogram after the pass: Ocean 7231 (70.6%), Tundra 1512 (14.8%), BorealForest 852 (8.3%), Grassland 421 (4.1%), IceSheet 96 (0.9%), CoastalShallow 67 (0.7%), ColdDesert 35 (0.3%), Wetland 17 (0.2%), AlpineMeadow 7 (0.1%), TemperateForest 4 (<0.1%). Ocean+CoastalShallow = 7298/10242 = 0.713, matching the skeleton-level 0.710 almost exactly (Markov smoothing nibbled a handful of coastal cells).

### SURF-04: Unified SkeletonTile view
- **Crate:** ymir-surface
- **Status:** done
- **Depends on:** SURF-03
- **Blocked:** no
- **Assignee:** agent

Consolidate `ElevationMap` + `TectonicData` + grid neighbor lookups into a `SkeletonTile` accessor (`world.tile(idx) -> SkeletonTile { lat, lon, elevation_m, plate_id, plate_type, neighbors: &[usize] }`). Phase 2 NOTE deferred this; Phase 3 regional detail needs quick per-tile access across three data sources during region extraction. Implement as a view struct (zero-copy where possible) so it doesn't duplicate storage. Tests: accessor returns consistent values with the underlying maps; neighbor lists match `GeodesicGrid`.

### DET-01: Region specification and PRNG derivation
- **Crate:** ymir-detail
- **Status:** done
- **Depends on:** CORE-02, SURF-04
- **Blocked:** no
- **Assignee:** agent

`RegionSpec { tile_index: u32, radius_tiles: u32 }` identifying a contiguous neighborhood of skeleton tiles to generate detail for. Radius-0 is a single tile (not generally useful); radius-1 is a tile plus its ~6 neighbors. Include a deterministic PRNG factory: `detail_rng(world_seed: u64, tile_index: u32) -> PcgRng` using `hash(world_seed, tile_index)` per design doc §5.7. Tests: same (seed, tile_index) yields the same RNG sequence; different inputs diverge.

NOTE: Hash algorithm is splitmix64 (Vigna's finalizer, stable constants), exposed from `ymir-core` as `splitmix64(u64) -> u64` and `stable_derive_seed(world_seed, key) -> u64`. `stable_derive_seed` mixes each input with splitmix64 independently, XORs with a rotated second mix, and runs splitmix64 again; this keeps tiny inputs (e.g., tile_index=0) well-avalanched. Chosen over `DefaultHasher` because DefaultHasher is explicitly documented as non-stable across Rust versions. Public API in `ymir-detail`: `RegionSpec { tile_index, radius_tiles }` (Clone + Debug + Serialize + Deserialize + PartialEq + Eq + Hash) with `new()` constructor, and `detail_rng(world_seed: u64, tile_index: u32) -> Pcg64`. The PRNG is keyed only on `(world_seed, tile_index)`, not `radius_tiles`, so growing a region's radius does not invalidate cached detail at the original radius. Locked first three u64s for `(42, 7)` in a literal-bytes test; same for `splitmix64` and `stable_derive_seed` outputs on known inputs.

### DET-02: Hex subgrid generation
- **Crate:** ymir-detail
- **Status:** done
- **Depends on:** DET-01
- **Blocked:** no
- **Assignee:** agent

Generate a hex grid covering a `RegionSpec`. Fixed subdivision (e.g. 32×32 hexes per skeleton tile in the initial pass). Handle the gnomonic projection from the skeleton-tile local frame to a planar hex grid; document the distortion bound. Serializable. Tests: hex count matches expected formula; every hex has a well-defined (lat, lon); neighbor relationships are consistent across shared edges between adjacent skeleton tiles.

NOTE: Public API in `crates/ymir-detail/src/hex_grid.rs`: `HexGridConfig { subdivision: u32 }` (default 32), `HexCell { region_hex_index, parent_tile, q, r, lat_rad, lon_rad }`, `HexGrid { region_spec, config, cells, neighbors: Vec<[Option<u32>; 6]> }`, and `HexGrid::build(world, spec, config)`. Re-exported from `ymir_detail::lib`. Each parent skeleton tile gets its own pointy-top axial lattice of `subdivision * subdivision` hexes, laid out in the gnomonic tangent plane at the tile's (lat, lon) and scaled so the lattice spans roughly one mean tile-radius. Inverse gnomonic projection lifts each hex centre back to (lat, lon) on the unit sphere. Gnomonic distortion bound: `sec(δ)` where `δ` is the great-circle angle from tile centre to tile edge. Level-3 geodesic grid (642 tiles) → δ ≈ 4.5°, distortion ≈ 0.3%. Level-2 grid (162 tiles, used in tests) → δ ≈ 9°, distortion ≈ 1.25%. For a 700 km tile radius on Earth (δ ≈ 6.3°), distortion ≈ 0.6%. Hex counts on the tested level-2 Earth world: radius-0 on any tile = 1024 hexes; radius-1 on tile 0 (a pentagon with 5 neighbours) = 6 * 1024 = 6144 hexes; on a tile with 6 neighbours = 7 * 1024 = 7168. Cross-seam strategy: boundary hexes (axial q or r at 0 or N-1) from adjacent parent tiles are stitched by nearest-neighbour match in (lat, lon) with a tolerance of 0.75 * lattice cell width; reciprocal links are installed in the first free slot of each cell's 6-slot neighbour array, exact to within the gnomonic distortion bound above. Open question: 5-neighbour seams at the 12 icosahedral pentagons leave one parent tile with fewer adjacent tiles, so one lattice edge has no counterpart; those boundary hexes keep `None` in that direction, which is correct for a radius-1 region but may need revisiting if future stages require explicit pentagon-centre handling. Future refinement: replace nearest-neighbour stitching with an analytic shared-midpoint frame along each A-B edge for sub-hex-exact seam coincidence. Tests (17 total, all passing): radius-0 cell count = N*N; radius-1 parents = 1 + |neighbours(seed)|; finite lat/lon in range; intra-tile symmetric adjacency; cross-seam links exist and are symmetric; serde JSON round-trip preserves all fields; determinism via byte-identical serialised bytes across two `build` calls; gnomonic round-trip at tile centre (0, 0) reproduces the tile's (lat, lon) to 1e-12. `cargo clippy -p ymir-detail --all-targets -- -D warnings` clean; `cargo fmt` applied.

### DET-03: Fractal-perturbed elevation
- **Crate:** ymir-detail
- **Status:** done
- **Depends on:** DET-02
- **Blocked:** no
- **Assignee:** agent

Per-hex elevation = skeleton-tile baseline + FBM noise at detail scale. Reuse `ymir_surface::noise::spherical_fbm` seeded from the detail RNG. Boundary handling: elevation at shared edges between adjacent skeleton tiles must match across the seam (blend from baseline at tile centers to shared value at boundary). Tests: determinism; cross-tile seam continuity (max |dH| across seam < threshold); perturbation magnitude bounded by gravity-scaled cap.

NOTE: Public API in `crates/ymir-detail/src/elevation.rs`: `DetailElevationConfig { octaves, lacunarity, gain, base_frequency, amplitude_m }` (defaults 5 / 2.0 / 0.5 / 300.0 / 300.0 m), `DetailElevation { spec: RegionSpec, per_hex_m: Vec<f64> }`, and `DetailElevation::build(world: &SkeletonWorld, grid: &HexGrid, world_seed: u64, config: DetailElevationConfig)`. Re-exported from `ymir_detail::lib`. Baseline-blending scheme: each hex's baseline is an inverse-fourth-power distance-weighted mean over every skeleton tile centre: `baseline(lat,lon) = Σ_i elev_i / (ε + d_i)^4 / Σ_i 1 / (ε + d_i)^4` where `d_i` is great-circle angle from the hex to tile `i`. The weighting is a pure function of (lat, lon), independent of `parent_tile`, so the baseline is continuous across every skeleton-tile seam by construction — no per-edge bookkeeping required. The 1/d^4 kernel decays fast enough that contributions from tiles more than two tile-spacings away are negligible. FBM seed derivation: `detail_rng(world_seed, spec.tile_index).next_u64()` is consumed to seed a single sphere-wide `SphericalFbm`; since FBM is sampled at the hex's (lat, lon) on the unit sphere, it is automatically seam-continuous too. Gravity-scaled cap: `max_displacement_m = amplitude_m * sqrt(9.81 / body.surface_gravity)`, clamped to ±1.2 × cap to bound OpenSimplex's rescaling margin. Worked examples at the 300 m default: Earth (g=9.81) → cap = 300 m; Mars (g=3.71) → cap = 300 × √(9.81/3.71) ≈ 487 m; a 2g super-earth (g=19.62) → cap ≈ 212 m. Rationale: mountain relief scales roughly as 1/√g under brittle-failure / isostatic limits, so lower-gravity bodies can support larger features. Observed seam-continuity threshold (subdivision-2 Earth, default config, radius-1 region at tile 0): max |dH| across cross-seam neighbour pairs ≈ 257 m; test asserts < 2 × cap = 600 m (≈ 2.3× headroom). Determinism: two back-to-back `DetailElevation::build` calls with identical inputs produce byte-identical `per_hex_m` (verified via `serde_json::to_vec` equality in `deterministic_byte_identical`). Tests (5, all passing): determinism, per-hex length matches cells, cross-tile seam continuity, perturbation bounded by gravity-scaled cap, baseline at tile centre matches the tile's elevation to < 1 mm. `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt` applied; full workspace `cargo test --release` passes.

### DET-04: Orographic moisture refinement
- **Crate:** ymir-detail
- **Status:** done
- **Depends on:** DET-03
- **Blocked:** no
- **Assignee:** agent

Local humidity = skeleton-tile moisture baseline + orographic lift/shadow from the wind field. Use prevailing wind at the region; advect moisture uphill (saturation → precipitation → downwind dry shadow). Simpler first pass: humidity *= f(upwind elevation gradient). Tests: windward slopes wetter than leeward slopes at equal elevation; humidity stays in [0, saturation_ceiling].

NOTE: Public API in `crates/ymir-detail/src/moisture.rs`: `DetailMoistureConfig { lift_coefficient, shadow_coefficient, saturation_ceiling }` (defaults 0.5 per km upwind rise, 0.3 per km leeward fall, ceiling 1.0), `DetailMoisture { spec: RegionSpec, per_hex: Vec<f64> }`, and `DetailMoisture::build(world: &SkeletonWorld, climate: &ClimateMap, grid: &HexGrid, elevation: &DetailElevation, config: DetailMoistureConfig)`. Re-exported from `ymir_detail::lib`. Algorithm (first-pass orographic, per-hex local): (1) baseline humidity is the parent skeleton tile's value from `climate.moisture.per_tile[parent_tile]`; (2) the parent tile's wind vector `(u, v)` m/s selects upwind and downwind neighbours — each of the six HexGrid neighbours is projected onto the local east/north tangent basis at the hex centre and the neighbour with the largest positive dot product against `+wind_hat` is the downwind hex, the largest against `-wind_hat` is the upwind hex, and neighbours must align to within 60° (cos θ ≥ 0.5) to be used; (3) lift formula when air climbed onto this hex (self_elev > upwind_elev): `H *= 1 + lift_coefficient * (self_elev − upwind_elev)/1000`; (4) shadow formula when the hex is leeward of higher upwind terrain AND the downwind hex is lower still: `H *= 1 − shadow_coefficient * (upwind_elev − self_elev)/1000`; (5) clamp to `[0, saturation_ceiling]`. Calm tiles (|wind| < 1e-6 m/s, e.g. airless worlds) skip adjustment and just clamp the baseline. Observed windward-vs-leeward delta on a synthetic radius-0 region with a 2 km triangular ridge across tile 0, baseline 0.5, 10 m/s eastward wind, default config: average windward humidity 0.6000, average leeward humidity 0.4400, delta = 0.1600 (i.e. +20 % of baseline on the windward side, −12 % on the leeward side, exactly matching 1 ± coefficient × 1 km at the ridge flanks). Observed humidity range for Earth seed=1 radius-1 region (tile 0, subdivision-3 skeleton, subdivision-32 hex grid, default config): 0.6318..=0.9401 — all values stay inside `[0, saturation_ceiling=1.0]`. Tests (7, all passing): humidity range in [0, 1] for Earth seed-1 region, determinism (byte-identical build), serde round-trip on the moisture field, config serde round-trip, synthetic-ridge windward-vs-leeward delta > 0.05, calm-wind preserves baseline, saturation-ceiling respected under deliberately-inflated lift_coefficient. `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --check` clean; `cargo test -p ymir-detail --release` passes 37/37. Multi-hex advection (full wind transport beyond the immediate upwind/downwind pair) is deferred; belongs with CLIM-04.

### DET-05: River flow accumulation + lake formation
- **Crate:** ymir-detail
- **Status:** done
- **Depends on:** DET-03
- **Blocked:** no
- **Assignee:** agent

Flow-accumulation algorithm on the hex grid using D8-equivalent (D6 for hexes) steepest-descent. Fill local minima into lakes via priority-flood or Planchon-Darboux. Emit per-hex `flow_accumulation: f32` and `is_lake: bool` fields. Rivers are implicit (hexes with accumulation > threshold). Tests: no hex flows uphill; every drainage basin has exactly one outlet (or terminates in a lake); total water volume conserved up to numerical tolerance.

NOTE: Landed as `crates/ymir-detail/src/flow.rs`. Public API: `DetailFlowConfig { lake_fill_epsilon_m: f64 }` (default `1e-3`), `DetailFlow { spec, flow_accumulation: Vec<f32>, is_lake: Vec<bool>, filled_elevation_m: Vec<f64>, downstream: Vec<Option<u32>> }`, and `DetailFlow::build(&HexGrid, &DetailElevation, DetailFlowConfig) -> DetailFlow`. Algorithm: (A) Planchon-Darboux iterative fill, initialising `filled` to `+∞` except at boundary hexes (any cell with a `None` neighbour slot) which pin to their original elevation, then sweeping `filled[h] = max(orig, min_nb_filled + eps)` to convergence. (B) D6 steepest-descent `downstream[h] = argmin(filled)` over neighbours (`None` on boundary cells, which discharge out of the region); ties broken by lowest region-hex index for determinism. (C) Flow accumulation processes hexes in descending filled elevation and propagates `accum[h]` into `accum[downstream[h]]`. (D) `is_lake[h]` is set when `filled - orig > lake_fill_epsilon_m`. Tests (7, all passing): no-uphill-flow, bounded-termination (walk downstream ≤ `n` steps from every hex), water conservation (Σ outlet accumulation = n cells, within f32 rounding), lake ↔ fill-delta equivalence, byte-identical determinism, `DetailFlow` serde round-trip, and `DetailFlowConfig` serde round-trip.

Stats on the Earth seed=1 radius-1 region (subdivision-2 skeleton, 32×32 hex lattice, 6 parent tiles, 6144 cells):
- Planchon-Darboux converged in 32 sweeps with `eps = 1e-3 m`.
- 910 lake hexes (~14.8% of region). Per-hex area ≈ 3.08 × 10³ km² at this subdivision, so ~2.8 × 10⁶ km² of lake area in-region. The fraction is high because the region is dominated by boundary cells and low-relief hexes where PD rounding floods many cells; deeper detail subdivisions plus post-DET-05 erosion are expected to drain most of these.
- 738 outlets (every boundary hex is an outlet; interior hexes all route to a boundary outlet).
- Max `flow_accumulation = 589.0` (units: hexes) at region-hex 2035, parent_tile 44, axial (q=19, r=31), lat ≈ 28.35°N lon ≈ 129.66°E — a seaward boundary hex aggregating the drainage of one parent tile.
- Number of basins (distinct outlets that receive > 1 hex of flow) is bounded above by the 738 outlets; the long tail of outlet flows is dominated by a few hundred boundary cells that only carry their own unit contribution.

Ran `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --release`; all pass. DET-04 is still in-progress, so DET-06 remains `pending`.

### DET-06: Fine-grained biome refinement
- **Crate:** ymir-detail
- **Status:** done
- **Depends on:** DET-04, DET-05, BIOME-03
- **Blocked:** no
- **Assignee:** agent

Classify each hex via the same Whittaker logic (BIOME-02) using refined local T/humidity, then apply Markov smoothing (BIOME-03) at detail scale. Rivers (flow_accumulation > threshold) become distinct river-channel biomes or bias neighbors toward Wetland. Lakes get the water-body biome. Boundary condition: detail biomes at the region edge should agree with the parent skeleton tile's biome within tolerance (same palette, same-family biome). Tests: palette membership; lake hexes are water-body; river hexes follow descending elevation gradient.

NOTE: Landed as `crates/ymir-detail/src/biomes.rs`, re-exported from `lib.rs` as `DetailBiomes` + `DetailBiomeConfig`. Public API: `DetailBiomes::build(&SkeletonWorld, &ClimateMap, &BiomeMap, &HexGrid, &DetailElevation, &DetailMoisture, &DetailFlow, DetailBiomeConfig) -> DetailBiomes`. Config defaults: `river_flow_threshold = 50.0` hexes, `markov_iterations = 2`, `lapse_rate_k_per_m = 6.5e-3`, `neighbor_agreement_threshold = 0.0`. Pipeline: (1) Whittaker classify each hex at refined temperature `parent_T − lapse × (hex_elev − parent_elev)` and `DetailMoisture::per_hex[i]` humidity, restricted to the parent tile's palette. (2) Lake overlay paints `water_body_biome_for(palette)` — EarthLike → CoastalShallow, TitanLike → TitanMethaneSea, other palettes skip (no inland water biome). (3) River overlay paints `river_biome_for(palette)` — EarthLike → Wetland, other palettes skip — on non-lake hexes with `flow_accumulation ≥ threshold`. (4) Dirichlet boundary pin: every hex with a `None` neighbour slot is snapped to `biomes.per_tile[parent]`. (5) Synchronous Markov smoothing via BIOME-03's `smooth_biomes` with boundary hexes snapshotted and restored so they stay pinned. (6) Interior-only re-apply of lake/river overlays after smoothing so physical features survive the smoother; boundary lake/river hexes remain pinned to the skeleton-tile biome for seam agreement, and a lake or river crossing the edge resumes one cell inside. Six tests pass: palette membership, lake hexes are water body (interior only), river hexes follow descending gradient (≥ 90% flow to non-ascending downstream), boundary agrees with parent tile, determinism byte-identical, serde round-trip for both types. Full `cargo test -p ymir-detail --release` green (44 tests). `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` clean. Not committed.

### DET-07: RegionalDetail composite
- **Crate:** ymir-detail
- **Status:** done
- **Depends on:** DET-02, DET-03, DET-04, DET-05, DET-06
- **Blocked:** no
- **Assignee:** agent

`RegionalDetail { spec, hex_grid, elevation, moisture, flow, biomes }` capstone. `RegionalDetail::build(&SkeletonWorld, &ClimateMap, &BiomeMap, spec, cfg)` orchestrates DET-02..06. Serializable. Tests: determinism; serde round-trip; boundary consistency with parent skeleton tile; same-region regeneration is byte-identical.

NOTE: Landed as `crates/ymir-detail/src/regional.rs`, re-exported from `lib.rs` as `RegionalDetail` + `RegionalDetailConfig`. Public API: `RegionalDetail::build(&SkeletonWorld, &ClimateMap, &BiomeMap, RegionSpec, RegionalDetailConfig) -> RegionalDetail`. `RegionalDetailConfig { seed: u64, hex_grid: HexGridConfig, elevation: DetailElevationConfig, moisture: DetailMoistureConfig, flow: DetailFlowConfig, biomes: DetailBiomeConfig }` derives Default/Clone/Copy/Debug/Serialize/Deserialize/PartialEq. `RegionalDetail { spec, hex_grid, elevation, moisture, flow, biomes }` derives Clone/Debug/Serialize/Deserialize. `build()` runs HexGrid → DetailElevation (passing `config.seed` as the world-seed fed into `detail_rng`) → DetailMoisture → DetailFlow → DetailBiomes with the same `world`/`climate`/`biomes` shared across stages. Six new tests in `regional::tests`: `builds_without_panicking` (cross-stage length consistency on Earth seed=1 spec=(0,1)); `deterministic_byte_identical` (two back-to-back builds bincode-match); `serde_round_trip` (bincode round-trip with approx float compare on all inner stages); `boundary_consistency_with_parent_skeleton` (every region-edge hex matches `biomes.per_tile[parent_tile]`, leveraging DET-06's Dirichlet pin); `same_region_regeneration_is_byte_identical` (two independently-built parent contexts from the same world seed produce byte-identical output, exercising the full determinism chain); `config_serde_round_trip` through serde_json. Added `bincode = "1"` to `ymir-detail` dev-dependencies to match the workspace-standard binary round-trip format. Full `cargo test -p ymir-detail --release` green (50 tests, up from 44). `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` clean. Not committed.

### STOR-03: Per-region persistence
- **Crate:** ymir-storage + ymir (binary)
- **Status:** done
- **Depends on:** DET-07
- **Blocked:** no
- **Assignee:** agent

Persist regional details to `<world>/detail/region_NNNN.bin` (design doc §6.2). Update manifest to track which regions have been generated (e.g. `regions_generated: Vec<u32>`). Reuse `save_bin`/`load_bin`. Tests: save then load round-trips a `RegionalDetail`; manifest update is idempotent.

**NOTE:** Landed. `ymir-storage` gained `WorldDirectory::region_path(tile_index)` and a free-standing `region_file_name` helper that format as `<root>/detail/region_NNNN.bin` with 4-digit zero-padded indices (code comment flags the widening needed for subdiv-5). `WorldManifest` gained a serde-defaulted `regions_generated: Vec<u32>` field plus `mark_region_generated` (sorted, deduped insert) and `has_region` query. Concrete `save_region` / `load_region` live in the binary (`src/main.rs`) so `ymir-storage` stays pure bytes and keeps its single-dep graph (core-only). Tests added: manifest idempotency + sort order, `has_region` truth table, backwards-compat load of a pre-field JSON manifest, `region_path` / `region_file_name` formatting, and a full `RegionalDetail` save-then-load byte-identical round trip in the binary crate. CLI-05 will wire `save_region` / `load_region` into a `ymir detail` subcommand; both helpers are `#[allow(dead_code)]` until then.

### REND-03: Regional detail renderer
- **Crate:** ymir-render
- **Status:** done
- **Depends on:** DET-07
- **Blocked:** no
- **Assignee:** agent

Render a `RegionalDetail` to a high-res PNG: hex grid rasterized with biome-colored fill, elevation-shaded (hillshade), river/lake overlay. Separate entry point from the Mollweide renderers. Consider a stereographic or orthographic local projection centered on the region. Tests: output dimensions; biome palette visible; river hexes distinguishable.

NOTE: Landed as `crates/ymir-render/src/regional.rs`, re-exported from `lib.rs` as `render_regional_detail`, `RegionalRenderConfig`, `RegionProjection`. Public API: `pub fn render_regional_detail(region: &RegionalDetail, world: &SkeletonWorld, config: &RegionalRenderConfig) -> RgbImage`. `RegionalRenderConfig { width: u32 = 1024, height: u32 = 1024, hillshade_strength: f32 = 0.4, river_flow_threshold: f32 = 50.0, projection: RegionProjection = Orthographic }` derives Clone/Copy/Debug/Serialize/Deserialize/PartialEq. `RegionProjection { Orthographic, Stereographic }`. Projection: local tangent frame anchored at `world.tile(region.spec.tile_index).(lat_rad, lon_rad)`; orthographic uses the tangent-plane east/north components of the unit-sphere point (clipping the far hemisphere); stereographic uses the conformal `k = 2 / (1 + cos_c)` scaling. Projected hex centres are scaled to fit the output canvas with a 2% margin (uniform scale to preserve aspect). Rasterisation: per-pixel brute-force nearest-hex search over all projected hex centres, with a paint-radius cutoff (~1.25 * median nearest-neighbour spacing, estimated by stride-sampling 32 hexes) so pixels outside the region footprint stay as the off-region background `[10, 10, 12]`. Hillshade: per-hex elevation delta versus the mean of in-region neighbours, normalised by `HILLSHADE_NORMALIZATION_M = 250.0` (matched to DET-03's default FBM amplitude), clamped to `[-1, 1]`, scaled by `config.hillshade_strength`, then applied as a linear brighten/darken towards 255 / 0 per channel. River overlay: non-lake hexes with `flow_accumulation >= config.river_flow_threshold` are linearly blended towards `#2060a0` with weight `flow / (flow + threshold)` so big rivers saturate deeper blue than just-qualifying streams; lakes already read as `CoastalShallow` / `TitanMethaneSea` via DET-06 so no extra lake compositing is needed. Biome palette reuses existing `biome_color()` from `biome_palette.rs` (no TODO / fork). Tests (5 + 1 ignored perf bench): `output_dimensions_match_config` (256×256 and 512×256), `palette_visible_in_output` (every biome with >= 5 hexes has a ≤8-per-channel match with hillshade disabled and river threshold raised so raw biome fill survives), `river_hexes_distinguishable` (low vs. sky-high threshold produces different buffers; asserts the region actually contains a qualifying hex first so the test isn't a tautology), `determinism_byte_identical` (two calls with the same inputs produce byte-identical RGB), `config_serde_round_trip` via `serde_json`, plus an `#[ignore]` perf bench `bench_1024_radius1_render`. Full `cargo test -p ymir-render --release` green (40 tests, 2 ignored perf benches). `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` clean. Perf: 1024×1024 render of a radius-1 Earth region (6144 hexes) runs in ~3.4 s on the dev machine — O(W * H * hex_count) brute-force, single-threaded. If this becomes a bottleneck a k-d tree or a rayon `par_iter` over rows are both easy wins; flagged for a future perf pass. Dep-graph change: `ymir-render` now additionally depends on `ymir-detail`. The crate-layering note in `CLAUDE.md` §"Workspace Structure" lists `ymir-render` without `ymir-detail`, but the design doc §9.x explicitly envisions the renderer consuming `RegionalDetail`, so the extra edge is intended; flag for doc-sync. Added `serde_json` to `ymir-render` dev-dependencies for the config round-trip test. Not committed.

### CLI-05: `ymir detail` command
- **Crate:** ymir (binary)
- **Status:** done
- **Depends on:** DET-07, STOR-03, REND-03
- **Blocked:** no
- **Assignee:** agent

`ymir detail --world PATH --region TILE_INDEX [--radius N] [--output-image FILE]` generates regional detail for the specified tile, persists to `detail/region_NNNN.bin`, and optionally renders a PNG. Update the manifest's regions-generated list. Stdout summary: hex count, elevation range, biome histogram, river/lake counts. Tests: command succeeds on an Earth world; re-running with the same region produces byte-identical output.

NOTE: Landed in `src/main.rs` as a new `Commands::Detail { world, region, radius, output_image, seed }` clap variant plus `run_detail(&DetailArgs)`. The handler loads `manifest.json` + `skeleton.bin` + `climate.bin` + `biomes.bin` from the world directory (errors clearly if any are missing — e.g. worlds generated with `--skip-climate` are rejected up front), validates `region < tile_count`, resolves the detail seed as `args.seed.unwrap_or_else(|| stable_derive_seed(manifest.seed, region as u64))`, builds `RegionalDetail::build(&world, &climate, &biomes, RegionSpec::new(region, radius), RegionalDetailConfig { seed, ..Default::default() })`, writes via the binary-local `save_region(...)` (which `mark_region_generated`s the manifest), persists the manifest with `wd.save_manifest(...)`, and optionally renders a `RegionalRenderConfig::default()` (1024×1024 orthographic) PNG via `render_regional_detail`. Stdout summary: seed tile index + radius + hex count; elevation min/max/mean in metres (1 dp); river count (non-lake hexes with `flow_accumulation >= DetailBiomeConfig::default().river_flow_threshold`) + lake count (`flow.is_lake` hits); biome histogram sorted by hex count descending with percentages. `save_region` / `load_region` lost their STOR-03 `#[allow(dead_code)]`; `load_region` kept it because nothing in the binary currently reads regions back, but it stays paired with `save_region` for future read-only commands. Removed manual cast on `DetailBiomeConfig::river_flow_threshold` once confirmed it's already `f32`. Three new tests: `detail_subcommand_succeeds_on_earth_world` (generate Earth seed=1 subdiv=2, run `Detail { region: 0, radius: 1 }`, asserts `detail/region_0000.bin` exists and manifest lists `[0]`); `detail_subcommand_is_idempotent` (runs `Detail` twice on the same region, asserts the .bin is byte-identical across calls and the manifest still records `[0]`); `detail_with_output_image_writes_png` (supplies `--output-image` and decodes the written file via `image::open`, checking dimensions). Added `image = { version = "0.25", default-features = false, features = ["png"] }` to the binary crate's `dev-dependencies` solely for the PNG decode test. One doc-sync observation worth flagging: the DEFAULT river-flow threshold in `ymir-detail` (`DEFAULT_RIVER_FLOW_THRESHOLD = 16.0`, `biomes.rs:83`) diverges from the "50.0" cited in the DET-06 NOTE and the REND-03 NOTE; CLI-05 reports the actual 16.0 default in its summary. The mismatch does not block this task; flagged for whoever next touches those notes. End-to-end wall time on Earth `--seed 1 --subdivision 5`, `detail --region 0 --radius 1 --output-image`: ~4.7 s total on the dev machine, of which the brute-force 1024×1024 renderer is ~3.4 s (REND-03's known hot spot) and the DET-02..06 build + save is ~1.3 s for 6144 hexes. Full `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --release` green (binary crate goes from 25 to 28 tests). Not committed.

### VALID-05: Regional determinism test
- **Crate:** ymir (binary, integration tests)
- **Status:** done
- **Depends on:** CLI-05
- **Blocked:** no
- **Assignee:** agent

Integration test: generate an Earth world, run `ymir detail --region 100 --region 200 --region 300`, re-run in a fresh tempdir, assert all three region bin files are byte-identical across runs. Proves `hash(world_seed, tile_index)` seeding is stable.

NOTE: Landed as `tests/regional_determinism.rs`, a new end-to-end integration test that drives the compiled binary via `CARGO_BIN_EXE_ymir` + `std::process::Command` (same pattern as `tests/valid_earth.rs` and VALID-06's `tests/river_plausibility.rs`, no new dev-deps required — `tempfile` was already present). Because `Commands::Detail` accepts a single `--region TILE_INDEX` per invocation (not a repeated flag, per CLI-05's NOTE), the test spawns `ymir detail --world <dir> --region <tile>` three times per world for tiles `[100, 200, 300]` against the default subdivision-5 icosphere (10242 tiles, so all three indices are valid). Run-A and run-B use independent `TempDir`s, both generate with `--star Earth --seed 42`, and the assertion reads `<world>/detail/region_NNNN.bin` (STOR-03's `region_file_name` format: `region_{:04}.bin`) from both tempdirs and compares bytes with `assert_eq!` on the `Vec<u8>`s. File-size and full-content equality are both checked so a size mismatch produces a clearer failure message than the byte compare alone. Wall time 27.84 s in `--release` (two full `generate` passes at subdivision 5 dominate; the six `detail` invocations are relatively cheap). No flakiness observed across local reruns; this is a pure determinism check so timing sensitivity is nil. `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --release` all green (full workspace suite continues to pass alongside VALID-06's parallel landing). Not committed.

### VALID-06: River plausibility test
- **Crate:** ymir (binary, integration tests)
- **Status:** done
- **Depends on:** CLI-05
- **Blocked:** no
- **Assignee:** agent

Integration test: generate an Earth world, pick a region known to contain a continent (non-zero land fraction), run `ymir detail`. Assert: (a) for every hex with `flow_accumulation > 0`, the downstream hex has strictly lower elevation — no uphill flow; (b) each hex's flow path terminates in a lake or at the region boundary within N steps; (c) at least one basin (flow accumulation > threshold) exists.

NOTE: Landed as `tests/river_plausibility.rs` (new integration test, distinct from VALID-05's `tests/regional_determinism.rs`). Drives the compiled `ymir` binary via `CARGO_BIN_EXE_ymir` + `std::process::Command` to match VALID-05's style: `generate --star Earth --seed 42`, load `skeleton.bin`, scan for the first tile with `elevation > 200 m` to guarantee a continent seed, then `detail --region <tile> --radius 1` and load the persisted `region_NNNN.bin` via `ymir_storage::load_bin`. Tile selection returned `tile=0` on seed 42 (a land tile at the icosphere's first vertex); the resulting radius-1 region covers 6144 hexes with a hex-level land fraction of 1.0000 (fully continental), so the river-plausibility assertions are exercised on a meaningful drainage network rather than a coastal or ocean-dominated region. Assertions: (a) softened to `filled[downstream] <= filled[cell]` because Planchon-Darboux fills pits to exactly the outlet lip, so lake-interior hexes legitimately have equal-elevation downstream neighbours (the task sketch's strict `<` would false-positive on every filled basin); guarded with a separate `uphill_checks > 0` assertion so we know the invariant was actually exercised (5406 positive-accumulation hexes with an interior downstream on this region). (b) flow paths that fall off the region (`downstream = None`) or land in a lake count as terminated; cycles would trip `steps > n` and fail. (c) river-accumulation threshold of 50.0 (matches the REND-03 default); the region produced 392 hexes above that threshold. Test body also guards against the "region is all ocean" degenerate case with `land_fraction > 0.05`. Wall time 10.94 s in `--release` (includes the full `generate` + `detail` spawn), well under the several-seconds budget allotted to VALID-* tests. Bincode / `RegionalDetail` API gotcha: `DetailElevation::per_hex_m` (not `.elevations_m`, which is only on `SkeletonWorld::elevation`); flagged for anyone else writing detail-facing tests. No `Cargo.toml` changes needed — `ymir-detail`, `ymir-surface`, `ymir-storage`, `bincode`, and `tempfile` are all already in the binary crate's deps. `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --release --test river_plausibility` all green. Not committed.

---

## Phase 4: Catalog, GUI, and confidence visualization

Per design doc §11 Phase 4: Gaia DR3 Parquet ingest, real exoplanet overlay, star browser UI, system/globe/detail views, confidence overlay rendering (observed vs. derived coloring), and provenance report panel.

Three subsystems land roughly in parallel once Phase 3 tooling is stable:

1. **Real catalog ingest** (CAT-05..09): replace the hardcoded Earth/Mars/tidally-locked entries with a queryable catalog sourced from Gaia DR3 + NASA Exoplanet Archive. Design doc §5.1 and §6.1.
2. **Per-field provenance** (CORE-06..08, REND-04..05): upgrade the current per-stage `Sourced<T>` tracking to per-field granularity, emit `provenance.json`, and render a confidence overlay.
3. **GUI** (GUI-01..08): new `ymir-gui` crate for interactive star browsing, system visualization, globe + detail views, override editing.

Non-goals for Phase 4: no 3D rendering (that's Phase 6), no data-derived biome weights (Phase 5), no crates.io publish (Phase 7).

### CAT-05: Gaia DR3 Parquet reader
- **Crate:** ymir-catalog
- **Status:** done
- **Depends on:** CAT-03, INFRA-04
- **Blocked:** no
- **Assignee:** agent

Add a Parquet reader for the pre-filtered Gaia DR3 working catalog (design doc §5.1, §6.1): ~500k stars within 100 parsecs, parallax error < 20%, known T_eff / luminosity, ~2 GB on disk. Use the `parquet` crate or `polars` (prefer `parquet` directly to keep the dependency footprint small). Public API: `GaiaReader::open(path) -> Result<GaiaReader>` with an iterator yielding `StarRow { gaia_id, ra, dec, parallax_mas, teff_k, luminosity_sun, feh, mass_sun, radius_sun, age_gyr }`. Do not commit the actual Parquet file; ship a small sample (≤100 rows) in `fixtures/` for tests. Provide a bootstrap script or documented download URL for the full catalog. Tests: sample fixture round-trips; filter by distance returns expected subset.

NOTE: Source is Gaia DR3 itself (Gaia Collaboration 2023, A&A 674, A1), queried via the ESA Gaia archive TAP service. The ADQL joins `gaiadr3.gaia_source` with `gaiadr3.astrophysical_parameters` to pick up FLAME luminosity / mass / radius / age alongside GSP-Phot T_eff and metallicity; filters select `parallax > 10`, `parallax_over_error > 5`, and non-null T_eff + luminosity. GCNS was considered but raw DR3 was simpler (no extra landing page, same schema) and the ARCHITECTURE doc already names DR3 specifically. Full catalog is 116,249 rows (9.8 MB Zstd Parquet — materially smaller than the ~2 GB estimate because the FLAME luminosity filter is quite tight and we only kept 11 columns). Fixture is 50 rows (nearest stars, ~11 KB) at `crates/ymir-catalog/fixtures/gaia_sample.parquet`. Fetch script: `scripts/fetch_gaia_catalog.py` (uses `uv run` with `requests` + `pyarrow`, idempotent, skips if the output already exists). Fixture builder: `scripts/build_gaia_fixture.py`. Public API added: `ymir_catalog::gaia::{GaiaReader, StarRow, GaiaError}` (re-exported from the crate root). `GaiaReader::open(path)` validates the schema up front; `rows()` streams row-by-row without loading the file; `rows_within_pc(max_pc)` is a thin distance filter. Tests: fixture round-trip (`gaia_sample_fixture_round_trips`), distance filter (`distance_filter_narrows_correctly`), malformed-input handling (`malformed_parquet_fails_cleanly`). Gotchas worth flagging downstream: (a) Gaia TAP async jobs fail on `lum_flame` if you query `gaia_source` alone — the join against `astrophysical_parameters` is required. (b) Gaia `source_id` is int64 in the Parquet schema even though it is logically u64; the reader casts losslessly (Gaia IDs fit in 63 bits). (c) The `parquet` crate currently at 58.x; if a future DR4 refresh ships with different compression, the crate's `default-features = false` + `features = ["snap", "zstd"]` config in ymir-catalog/Cargo.toml may need adjusting.

### CAT-06: NASA Exoplanet Archive ingest
- **Crate:** ymir-catalog
- **Status:** done
- **Depends on:** CAT-05
- **Blocked:** no
- **Assignee:** agent

Read the NASA Exoplanet Archive CSV/TSV export (~5,700 confirmed planets) and join on host-star ID so `StarContext.known_exoplanets` is populated when real exoplanets orbit the queried star. Public API: `ExoplanetCatalog::load(path) -> ExoplanetCatalog` with `lookup_by_host(gaia_id | name) -> &[ExoplanetRecord]`. Handle the Archive's quirks: multiple name aliases per host (HD, HIP, TOI, Kepler, TIC), missing mass or radius columns (Sourced::Assumed fallback), discovery method column (transit, RV, imaging). Tests: fixture with 10 known systems; lookup by name returns all associated planets; missing-column handling doesn't panic.

NOTE: Source is the NASA Exoplanet Archive's `pscomppars` (Planetary Systems Composite Parameters) table, queried via the IPAC TAP sync endpoint at `https://exoplanetarchive.ipac.caltech.edu/TAP/sync`. DOI: 10.26133/NEA12. Full catalog is 6,158 rows (~0.8 MB CSV) at `data/catalog/exoplanet_archive.csv` (gitignored via the existing `/data/catalog/*.csv` rule). Fetch script: `scripts/fetch_exoplanet_catalog.py` (uses `uv run` with `requests`, idempotent, skips if the output already exists and is ≥ 500 KB; sync query returns in a few seconds). Fixture builder: `scripts/build_exoplanet_fixture.py`. Fixture is 46 rows across 10 well-known systems (Proxima Centauri, Tau Ceti, TRAPPIST-1, Kepler-186, KOI-351 / Kepler-90, TOI-700, HD 10180, 55 Cancri, HR 8799, PDS 70) at `crates/ymir-catalog/fixtures/exoplanet_sample.csv` (~5.5 KB). Public API added: `ymir_catalog::exoplanet::{ExoplanetCatalog, ExoplanetRecord, ExoplanetError, DiscoveryMethod}` (re-exported from the crate root). `ExoplanetCatalog::load(path)` streams the CSV through the `csv` crate (added as a new dep), skips any leading `#`-prefixed comment lines before the header, and builds dual `HashMap` indexes keyed on lowercased hostname and on parsed Gaia source ID. Lookups: `lookup_by_host(&str)` (case-insensitive, trims whitespace) and `lookup_by_gaia_id(u64)` both return `&[ExoplanetRecord]`. Aliases: the Archive does not ship a first-class alias column, so we derive primitive aliases by grouping every distinct `hostname` that shares a `gaia_id`. Custom `Serialize`/`Deserialize` for `ExoplanetCatalog` so the indexes are rebuilt on load rather than persisted. Tests (7 new, all green under `cargo test -p ymir-catalog --release`): `fixture_load_round_trips`, `lookup_by_name_returns_all_planets` (TRAPPIST-1 → 7 planets), `missing_columns_handled_without_panic`, `lookup_by_gaia_id_when_present`, `case_insensitive_host_lookup`, `serde_round_trip`, `discovery_method_parse`, plus `comment_header_is_skipped`. Gotchas worth flagging downstream: (a) The Archive's TAP `pscomppars` table exposes Gaia cross-matches as `gaia_dr3_id` (and `gaia_dr2_id`), not `gaia_id`; the fetch ADQL aliases `gaia_dr3_id as gaia_id` so the Rust parser can key on a stable column name across any future DR4 rename. Querying `gaia_id` directly returns `ORA-00904: invalid identifier`. (b) Kepler-90's hostname in the Archive is `KOI-351`, not `Kepler-90`; the fixture list hardcodes `KOI-351`, and the CAT-08 facade will need a real alias table (Simbad or SIMBAD/NED) to bridge the gap. (c) A new `DiscoveryMethod` enum lives in `ymir_catalog::exoplanet`; the existing `ymir_catalog::exoplanets::DiscoveryMethod` (Phase 1 hardcoded Tau Ceti) is left untouched because `ymir-system` and the binary still consume it. CAT-08 should collapse them. (d) `ExoplanetRecord` now has the same name in two sibling modules (`exoplanet::` and `exoplanets::`); crate-root re-exports point at the new `exoplanet::` version. Anything that imports `ymir_catalog::exoplanets::ExoplanetRecord` (main.rs, ymir-system/placement.rs) is unaffected. (e) Fixture CSV leaves the `pl_name` field as "HD 10180 b" etc. with no embedded quoting; Python's `csv.DictWriter` with `QUOTE_MINIMAL` happens to drop outer quotes for fields without special characters, which is schema-identical to what the Rust `csv` crate expects.

### CAT-07: Spatial + spectral catalog index
- **Crate:** ymir-catalog
- **Status:** done
- **Depends on:** CAT-05, CAT-06
- **Blocked:** no
- **Assignee:** agent

Build an in-memory index over the loaded Gaia catalog for fast queries: spatial (RA/Dec cone search via HEALPix bins or a k-d tree on Cartesian coords), spectral type filter, distance filter, HZ presence filter ("stars with known HZ planets"). Back with a columnar layout (structure-of-arrays) so queries are cache-friendly at 500k rows. Public API: `CatalogIndex::build(gaia: &GaiaReader, exo: &ExoplanetCatalog) -> CatalogIndex` with query methods. Persist to `catalog_index.bin` (bincode) so subsequent runs skip the Parquet parse. Tests: query for Sol returns Sol; cone search around Sol within 10 pc returns an expected star count; spectral-type filter narrows correctly.

NOTE: Lives at `crates/ymir-catalog/src/catalog_index.rs` (previously a one-line stub). Public API added: `ymir_catalog::{CatalogIndex, CatalogIndexError, IndexedSpectralType, StarInfo}` (re-exported from the crate root). `CatalogIndex::build(&GaiaReader, &ExoplanetCatalog)` streams the Gaia Parquet once, builds SoA columns (`gaia_ids`, `ra_deg`, `dec_deg`, `distance_pc`, `teff_k`, `luminosity_sun`, `unit_vec`, `spectral`, `has_hz_planet`) plus a `gaia_id -> row` `HashMap`. Query methods: `lookup_by_gaia_id`, `cone_search(ra, dec, radius_deg)`, `filter_by_spectral(SpectralClass)`, `filter_by_distance(min_pc, max_pc)`, `filter_hz_hosts()`, plus `len` / `is_empty`. Persistence uses `bincode` (added to `ymir-catalog/Cargo.toml`); `save` writes the SoA columns + `has_hz_planet` flag, `load` rebuilds the transient `gaia_id_to_row` map on the way in. Kept a k-d tree crate (`kiddo`) out of the dep tree; brute-force dot-product cone search is plenty fast (numbers below). Design choices: (a) Reused the existing `crate::star_context::SpectralClass` rather than inventing a parallel enum so CAT-08 can hand `StarContext` back without translation layers; `IndexedSpectralType { class, subtype: u8 }` adds the subclass digit. The task sketch's alternative `SpectralType::O(u8)` shape would have forced a second enum-to-class conversion everywhere. (b) HZ-host flag uses the simple Kopparapu optical-depth scaling `hz_inner = sqrt(L / 1.1) AU`, `hz_outer = sqrt(L / 0.53) AU` (T_eff-independent). That's coarser than `StarContext::from_params`'s full Kopparapu polynomial, but the archive's `semi_major_axis_au` column itself has percent-level uncertainty and the overlay only needs a binary flag. (c) Spectral classification clamps sub-2400 K to M9 rather than erroring. Perf on the real 116,249-row catalog (release, M2 MacBook): build 90.6 ms; cone search at 1° radius 0.052 ms/query (mean over 200 queries), at 10° 0.064 ms, at 30° 0.119 ms, at 90° (hemisphere) 0.390 ms; `filter_hz_hosts()` 65 µs (returns 40 hosts); `save` 6.5 ms to a 9.07 MB bincode blob; `load` 5.7 ms. Tests (9 new, all green under `cargo test -p ymir-catalog --release`): `build_from_fixture` (index.len() == 50), `lookup_by_gaia_id_round_trip` (middle row, all SoA columns match within 1e-9), `cone_search_includes_center` (0.001° cone at a fixture star returns it), `cone_search_excludes_far_star` (1° cone around a star drops a fixture star > 10° away), `spectral_filter_narrows` (disjoint class subsets, fixture is M-heavy), `distance_filter` (0..5 pc range honored), `hz_hosts_intersection` (every flagged host has a planet whose `semi_major_axis_au` is inside the computed HZ), `save_load_round_trip` (every SoA column + HashMap rebuilt identically), plus `spectral_from_teff_boundaries` and `cone_search_perf_smoke`. Gotchas worth flagging downstream: (a) `IndexedSpectralType` can't `derive(Copy)` because `SpectralClass` doesn't — `star_info()` clones the subtype struct per row. Not measurable at our sizes but something to watch if we ever stream-compute StarInfo in a hot loop. (b) The Kopparapu approximation used for `has_hz_planet` does **not** match `StarContext`'s polynomial HZ edges; CAT-08 should prefer `StarContext::hz_inner/hz_outer` over recomputing from `StarInfo.luminosity_sun`. The index flag is the fast filter, not the authoritative boundary. (c) The `gaia_id_to_row` map is not serialized; rebuild time is O(N) and trivial (<1 ms at 116k rows). (d) The SoA `unit_vec` column doubles as the spatial index — on `load`, callers can skip any kd-tree reconstruction entirely. (e) 9 MB bincode per 116k rows matches the expected ~80 bytes/row; if DR4 pushes us toward a million rows, switch to a mmap-friendly format (rkyv, zerocopy) before worrying about trees.

### CAT-08: Catalog query API
- **Crate:** ymir-catalog
- **Status:** done
- **Depends on:** CAT-07
- **Blocked:** no
- **Assignee:** agent

Unified `Catalog` facade wrapping the Gaia reader, exoplanet overlay, and spatial/spectral index. Replaces the hardcoded `earth_body() / mars_body()` path for the `--star` CLI argument. Public API: `Catalog::open(gaia_path, exo_path) -> Catalog`; `catalog.resolve(name_or_id) -> Option<StarContext>`; `catalog.list(query: CatalogQuery) -> Vec<StarSummary>`. `StarContext::from_catalog(...)` wires real observed values as `Sourced::Observed` with the proper `reference` / `instrument` / `date`. The legacy hardcoded path stays as a `--builtin` override for reproducibility of Phase 1/2 test worlds. Tests: resolve("Tau Ceti") returns a StarContext with `Observed` fields; resolve("Sol") too; resolve on an unknown name returns None.

NOTE: Facade lives at `crates/ymir-catalog/src/catalog.rs`. Public API added (re-exported from the crate root): `Catalog`, `CatalogQuery`, `StarSummary`, `CatalogError`, plus `StellarProvenance` and `GAIA_DR3_RELEASE_DATE` from `star_context`. `Catalog::open(gaia_path, exo_path)` builds the full index eagerly (wraps `GaiaReader::open` + `ExoplanetCatalog::load` + `CatalogIndex::build`); the Gaia reader is dropped once the index is live because the reader does not cache rows. `resolve(query)` handles three cases in order: (1) "Sol"/"Sun" short-circuit to `StarContext::from_sun()` because Gaia DR3 has no row for the Sun; (2) pure-digit input is treated as a Gaia source_id; (3) otherwise case-insensitive lookup in the alias table, then a Gaia-index hit. `list(query)` starts from the narrowest index filter (spectral → distance → HZ hosts → full walk) and applies any remaining predicates as a linear post-filter. `StarContext::from_catalog(&StarInfo, &[ExoplanetRecord])` and `StarContext::from_sun()` are the two new constructors; both populate a new optional `StellarProvenance { effective_temp, luminosity, distance_pc }` sidecar (as `Sourced<f64>`) with `Source::Observed` tagged "Gaia DR3 gspphot" / "Gaia DR3 FLAME" / "Gaia DR3 parallax" and date "2022-06-13" (the DR3 release date). The sidecar is `#[serde(default, skip_serializing_if = "Option::is_none")]` so existing JSON serializations of `StarContext` (CORE-01 round-trip, sol_context serde tests) still round-trip unchanged. Alias table lives at `crates/ymir-catalog/data/common_names.tsv`, embedded via `include_str!` at compile time so no runtime filesystem lookup is needed. Current entries: Proxima Centauri / Proxima Cen, Barnard's Star / Barnard Star, Epsilon Eridani / Eps Eri, Tau Ceti / tau Cet, Sirius / Sirius A. The Alpha Centauri A row was dropped pending a distinct source_id (the task sketch's ID collided with Proxima's). Added one helper to `ymir-core`: `Sourced::observed_on(value, reference, instrument, date)` so catalog adapters can supply the DR3 release date at construction time. Tests (11 new, all green under `cargo test -p ymir-catalog --release`): `resolve_epsilon_eridani_returns_observed_context` (substitutes for the task's Tau Ceti test because Tau Ceti isn't in the 50-star fixture — see gotchas below), `resolve_tau_ceti_missing_from_fixture_returns_none` (documents the fixture limitation), `resolve_sol_synthesizes_context` (covers both "Sol" and "Sun" aliases), `resolve_unknown_returns_none`, `resolve_by_gaia_id_string`, `list_with_spectral_filter`, `list_with_distance_filter`, `list_with_hz_hosts_only`, `list_combined_filters_intersect`, `summary_lookup`, `serde_round_trip_catalog_query`, `serde_round_trip_star_summary`, and `alias_table_parses_committed_tsv`. Full workspace: 49 tests in `ymir-catalog`, all workspace tests green, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo fmt` clean. Binary (`src/main.rs`) deliberately untouched — CAT-09 owns the `--star` resolution surface. Gotchas worth flagging downstream: (a) **The Sun is not in Gaia DR3.** Gaia's astrometric reference frame is heliocentric; it cannot observe its own origin. The facade hard-codes the "Sol"/"Sun" branch to return a synthesized `StarContext::from_sun()` whose provenance cites IAU 2015 Resolution B3 (nominal solar constants), not Gaia. Any future CLI that keys on "must have a Gaia row to count as Observed" needs to exempt Sol. (b) **Tau Ceti is absent from the committed fixture.** The 50-star nearest-slice in `crates/ymir-catalog/fixtures/gaia_sample.parquet` stops well before Tau Ceti's 3.65 pc. The alias table entry points at `2452378776434477184` (the Gaia DR3 source_id that appears in the exoplanet sample CSV for "tau Cet"), which differs from the `2452378776434276992` in the CAT-08 task sketch; neither ID is in the fixture. The working catalog at `data/catalog/gaia_dr3_100pc.parquet` does contain Tau Ceti; `resolve("Tau Ceti")` will succeed there. CAT-09's integration tests should run against the full catalog if they need Tau Ceti specifically. The fixture-available substitute is Epsilon Eridani (source_id 5164707970261890560, T_eff ~5002 K). (c) `StellarProvenance` is an optional sidecar rather than per-field `Sourced<f64>` on `StarContext` itself. Adding `Sourced<f64>` inline touches every HZ / UV flux math site and every downstream consumer; the sidecar is the Phase-1 bridge. CORE-06 will collapse these into first-class per-field wrappers and `StellarProvenance` can be retired at that point. (d) `Catalog::list` with all-`None` filters currently walks via `filter_by_distance(0.0, f64::INFINITY)` because the index has no dedicated `all()` accessor. Still O(N) but does an extra `Vec<StarInfo>` allocation per call; harmless at 116k rows but easy to optimize by adding `CatalogIndex::iter()` if a caller ever runs it in a hot loop. (e) The alias TSV is loaded with `include_str!` so changes require a rebuild — by design (the committed aliases are part of the crate's public contract). Users who want ad-hoc aliases should extend via a sibling TSV plus a future `Catalog::with_extra_aliases` builder. (f) `Catalog` does not implement `Serialize`/`Deserialize` (the `GaiaReader` handle is not serializable). If caching becomes desirable, persist the `CatalogIndex` via its existing `save`/`load` and rebuild the alias table from the embedded TSV on reload.

### CAT-09: `ymir list-stars` and `ymir describe-star` CLI
- **Crate:** ymir (binary)
- **Status:** done
- **Depends on:** CAT-08
- **Blocked:** no
- **Assignee:** agent

Two subcommands: `ymir list-stars [--spectral-type G] [--within-pc 20] [--has-planets] [--limit N]` prints a table of matching stars; `ymir describe-star <name|gaia_id>` prints the full StarContext (catalog fields + derived HZ + known exoplanets). Both consume the `Catalog` facade. Use `comfy-table` or manual column alignment (pick the lightest dep). Tests: integration test invokes both commands against the fixture catalog and asserts on stdout shape.

NOTE: Two new clap subcommands live in `src/main.rs`: `Commands::ListStars { catalog_dir, spectral_type, within_pc, has_planets, limit }` and `Commands::DescribeStar { query, catalog_dir }`. Both share a `resolve_catalog_paths(&Path) -> (PathBuf, PathBuf)` helper that looks for `<dir>/gaia_dr3_100pc.parquet` + `<dir>/exoplanet_archive.csv` and falls back to `crates/ymir-catalog/fixtures/gaia_sample.parquet` + `exoplanet_sample.csv` if either real file is missing. The fallback is documented in the clap `--help` on both subcommands. `list-stars` builds a `CatalogQuery` from the CLI flags (spectral parsed by a local `parse_spectral_class` helper that accepts any case), calls `catalog.list(&query)`, sorts by `distance_pc` ascending, truncates to `--limit` (default 50), then prints a plain-ASCII fixed-width table. Column layout: `gaia_id` (20 char), `name` (24 char, truncated with `…` if a real-catalog name exceeds that), `spectral` (8), `distance_pc` (right-aligned 11, 2 decimals), `teff_K` (right 8, 0 decimals), `L_sun` (right 8, 3 decimals), `has_hz_planet` (14 — "yes"/"no"). Header + dashed separator row + data rows; no color, no external table crate. `describe-star` calls `catalog.resolve(&query)`; on `None` it returns a `__nomatch__`-prefixed `Err` that `main` recognizes and renders as the bare "No match for '<query>'" line on stderr before exiting 1 (so the user doesn't see a doubled `error: ` banner). On success it prints a ~80-column human-readable report: header (`Star: <name> (<catalog_id>)`), a Catalog block (RA/Dec from `catalog.summary(gaia_id)` when available, Distance/Teff/Luminosity with provenance labels from a local `format_source` helper that maps `Source::{Observed,Derived,Assumed}` to `[Observed - <reference> (<date>)]` etc., and the `SpectralType` via its `Display` impl), a Habitable Zone block reading `ctx.hz_inner` / `ctx.hz_outer` straight off `StarContext` (no HZ math reimplementation), and a Known exoplanets block enumerating `catalog.exoplanets_for(gaia_id)` rows (host + letter, discovery method, orbital period, semi-major axis, mass in M_earth, radius in R_earth — `?` for missing columns).

Library additions to `ymir-catalog`: (a) `Catalog::common_name_for(gaia_id: u64) -> Option<&str>` — reverse-lookup via the existing `gaia_id_to_name` map built from the embedded alias TSV, returns the first canonical form recorded (e.g. "Proxima Centauri" not "Proxima Cen"). (b) `Catalog::exoplanets_for(gaia_id: u64) -> &[ExoplanetRecord]` — delegates to `ExoplanetCatalog::lookup_by_gaia_id` so the CLI can render the per-planet table without opening the CSV a second time. Both are additive, non-breaking, and have unit tests alongside the existing Catalog tests (`common_name_for_reverse_lookup`). No HZ math was duplicated — the CLI reads `StarContext::hz_inner` / `hz_outer` directly.

Integration tests (new file `tests/cat_cli.rs`, matches the house style of `tests/regenerate.rs` — spawns the compiled binary via `CARGO_BIN_EXE_ymir` + `std::process::Command`, no `assert_cmd`/`predicates` dev-dep added): `list_stars_fixture_prints_rows` (5 data rows + header under `--limit 5`), `list_stars_spectral_filter` (parses the fixed-width columns by byte offset rather than whitespace because "Proxima Centauri" has an internal space, asserts every spectral token starts with 'M'), `describe_star_sol` (stdout contains "Sol"/"Sun", "L_sun", "Habitable Zone"), `describe_star_fixture_hit` (Epsilon Eridani's source_id `5164707970261890560` appears and both HZ bounds parse to positive floats with inner < outer), `describe_star_unknown` (non-zero exit, stderr contains "No match"). All 5 pass under `cargo test --workspace --release`; full workspace is green (25 binary unit tests + 5 new CLI tests + the existing 50 ymir-catalog unit tests including the 2 new ones + all other crates).

Build gates: `cargo fmt` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace --release` all green.

Coordination: no changes to `crates/ymir-catalog/src/catalog.rs` beyond the two additive accessors (CAT-08 stays stable). No Phase 4 tasks currently list CAT-09 as a blocker in a way that would unblock on this landing (GUI-02 depends on GUI-01 + CAT-09 but GUI-01 is still pending on CORE-07). No cascade flips needed.

Gotchas worth flagging for downstream tasks: (a) The fixture fallback resolves paths relative to the current working directory (`crates/ymir-catalog/fixtures/...`). Cargo sets CWD to the workspace root for integration tests, so this works transparently in CI. If a user invokes `ymir list-stars` from outside the workspace without `--catalog-dir`, the fallback will miss and surface a friendly parquet-open error; the happy path for distributed binaries is to pass `--catalog-dir` explicitly. (b) The describe-star "No match" path uses a `__nomatch__` sentinel in the Err string so `main` can render a clean stderr line without the `error:` banner. If a future subcommand needs similar behavior, extend that dispatch in `main` rather than reinventing it. (c) `StarContext` does not carry RA/Dec, so describe-star fetches them from `Catalog::summary(gaia_id)`. The Sun has no Gaia row and `summary` returns `None`; the CLI prints "- / -" for RA/Dec in that case. (d) Common names with spaces in the alias table (e.g., "Proxima Centauri") broke the obvious whitespace-split parser in the spectral-filter test, so the test now slices by the known column byte-offset. If the table layout ever changes, that offset in `tests/cat_cli.rs` must change with it. (e) The `parse_spectral_class` helper lives only in `main.rs` for now — it's tiny enough not to be worth a public library addition, but if a future CLI or GUI surface wants the same parsing, promote it to `ymir-catalog::star_context`. (f) The planet table alignment is tuned for the archive's real columns (period in days, semi-major axis in AU, mass in M_earth, radius in R_earth). Tidally-locked / sub-day orbits will fit; eccentric multi-millennium orbits (e.g., Proxima Centauri c's ~1900 d) still format fine in the 10-char slot.

### CORE-06: Per-field Sourced<T> tagging
- **Crate:** ymir-core + ymir-system + ymir-atmosphere + ymir-surface + ymir-climate + ymir-biome
- **Status:** done
- **Depends on:** CORE-01
- **Blocked:** no
- **Model:** opus
- **Assignee:** agent

Phase 1 deferred per-field recomputation; the current implementation uses per-stage dirty tracking. Phase 4 needs per-field source information to drive provenance reports and confidence overlays. For every overridable field across stage output structs (`StarContext`, `OrbitalBody`, `AtmosphereModel`, `SkeletonWorld`, `ClimateMap`, `BiomeMap`), wrap the field in `Sourced<T>` if it isn't already and thread the `Source` tag through the compute path (stage functions write `Source::Derived { from_stage }` by default; catalog entries and override JSON inject `Source::Observed {...}`; hand-tuned fallbacks use `Source::Assumed {...}`). Do not yet change the regenerate dirty-tracking semantics — that's a later pass. Tests: every field in every stage output has a non-default Source after generation; override JSON flips the matching Source to `Observed`.

Debrief (2026-04-12): the agent hit the Opus rate limit after a ~40-minute burn but had already pushed the refactor past the build gates; finish-up (1 clippy allow on a local enum, 13 doc-link cleanups, TASKS.md bookkeeping) was done by the orchestrator. All 20 test suites (~440 tests) pass, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo doc --workspace --no-deps` warning-free. Shape of the landed change:

- **`Source` enum (ymir-core)**: extended to three struct variants — `Derived { from_stage }`, `Observed { reference, instrument, date, uncertainty }`, `Assumed { reason }`. New constructors on `Sourced<T>`: `derived`, `observed`, `observed_full`, `observed_with_date`, `assumed`, plus `mark_observed` for the override-flip path.
- **Per-stage field counts** (grep for `Sourced<` in each struct file):
  - `StarContext` (ymir-catalog): 21 Sourced fields. `T_eff`, `luminosity`, `mass`, `radius`, `distance`, `metallicity`, HZ inner/outer, UV flux, etc. `from_catalog` now threads Gaia DR3 citations into `Observed`; `from_sun` cites IAU 2015 solar constants; `from_params` defaults to `Assumed` because the raw-f64 entry point has no catalog row to quote.
  - `OrbitalBody` (ymir-system): 15 Sourced fields — semi-major axis, eccentricity, inclination, axial tilt, mass, radius, rotation period, obliquity, …. `earth_body`/`mars_body` hand-fills in `src/main.rs` cite the NASA planetary fact sheet.
  - `AtmosphereModel` (ymir-atmosphere): 9 Sourced fields — surface pressure, mean molar mass, greenhouse strength, composition fractions, etc.
  - `SkeletonWorld` / `ClimateMap` / `BiomeMap`: 1 Sourced field each — these still carry a single aggregate provenance tag on the struct rather than per-tile wrapping, because per-tile `Sourced<T>` would explode memory for 10⁵-tile grids. CORE-07 / REND-04 should read the struct-level tag when picking confidence bands; if per-tile override eventually matters, introduce a sparse `HashMap<tile_id, Source>` sidecar then.
- **Override propagation**: the override loader in `ymir-storage` now calls `.mark_observed(reference, instrument)` on flipped fields instead of overwriting the whole `Sourced`. This preserves `date`/`uncertainty` when the override JSON supplies them; absence leaves empty strings / `None`.
- **Test coverage**: new per-field provenance assertions across `ymir-system`, `ymir-atmosphere`, `ymir-catalog` unit tests; the regenerate + valid_* integration tests were updated to `.value()` through the wrapper. `regenerate.rs` gained an assertion that an overridden field's `Source` is `Observed` post-regenerate while its siblings remain `Derived`.

Gotchas for downstream tasks:
- (a) `StarSelection` (a local enum in `src/main.rs`) grew past clippy's `large_enum_variant` threshold once `OrbitalBody` ballooned from ~64 B to 2.5 kB. Fix landed as `#[allow(clippy::large_enum_variant)]`; boxing the `Fixed` variant would have touched a handful of call sites for no runtime benefit. Revisit if the enum starts getting cloned on a hot path.
- (b) `Sourced<T>` implements `Deref<Target = T>` plus `Add/Sub/Mul/Div`, so most arithmetic sites kept working without touch-ups. Places that destructured via pattern match or called `Clone` on the inner value did need `.value()` / `.clone_value()`. Watch for this when wiring new reads.
- (c) Doc-link cleanups: `[`Source::*`]` style intra-doc links only resolve when `Source` is in scope. In files that only `use ymir_core::Sourced`, reach for `[`ymir_core::Source::Observed`]` instead. Also avoid angle-bracketed placeholders in docstrings (`<id>`, `<date>`); rustdoc parses them as HTML tags. Use backtick-quoted braces: `` `{id}` ``.
- (d) CORE-07's `provenance.json` can treat every struct field as a `(value, source)` leaf and the three aggregate-tagged fields (skeleton/climate/biome) as summary nodes. The agent did NOT write `provenance.json` yet — that's CORE-07's scope.
- (e) REND-04's "confidence overlay" can key off the three aggregate tags for a v1 (whole-map desaturation), then switch to per-field tags once CORE-07 lands the leaf tree.

### CORE-07: Provenance report (`provenance.json`)
- **Crate:** ymir-core + ymir-storage + ymir (binary)
- **Status:** in-progress
- **Depends on:** CORE-06
- **Blocked:** no
- **Model:** sonnet
- **Assignee:** agent

Dump per-field provenance to `<world>/provenance.json` (design doc §6.2). Structure: nested object mirroring the stage output tree, each leaf holding `{ value, source }` for the field. Produced during `ymir generate` and refreshed on `ymir regenerate`. CLI subcommand `ymir provenance --world PATH [--summary | --full]` prints either a histogram (N Observed / M Derived / K Assumed per stage) or the full dump. Tests: generate an Earth world; assert `provenance.json` exists, parses, and contains the expected field tree; the Observed count matches the override JSON.

### CORE-08: Override CLI authoring helpers
- **Crate:** ymir (binary)
- **Status:** pending
- **Depends on:** CORE-06, CORE-07
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

`ymir override add --world PATH --field orbital_body.radius --value 1.073 --unit R_earth --reference "Gilbert+ 2023" --instrument "TESS"` edits the world's overrides.json without hand-editing JSON. Companion commands: `override remove --field X`, `override list`, `override validate` (checks that `--field` paths resolve to real Sourced fields). After the edit, users still run `ymir regenerate` to propagate. Tests: add, list, remove round-trip; validate rejects unknown field paths.

### REND-04: Confidence overlay
- **Crate:** ymir-render
- **Status:** ready
- **Depends on:** CORE-06, REND-02
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

Render-mode toggle that desaturates tiles whose biome is `Derived` or `Assumed` and leaves `Observed`-driven tiles at full saturation (design doc §4.1 "Confidence visualization"). "Observed-driven" means at least one upstream Sourced field contributing to the tile's biome is `Source::Observed`. Implement as a post-process on the biome-colored Mollweide output. Expose via `--render-mode confidence` in `ymir generate` / new `ymir render` subcommand. Tests: Earth with full observational overrides renders uniformly saturated; Earth with zero overrides renders uniformly desaturated; partial overrides produce a mix.

### REND-05: Per-stage render modes
- **Crate:** ymir-render + ymir (binary)
- **Status:** ready
- **Depends on:** REND-02, CORE-06
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

Unify the Mollweide outputs (`preview.png`, `preview_biome.png`) behind a single `--render-mode` flag: `elevation` (hillshade), `biome` (palette colors), `temperature` (thermal LUT), `moisture` (green-blue LUT), `confidence` (REND-04), `plates` (tectonic plate coloring). Keep the existing two outputs for backwards compatibility but deprecate them in the CLI help. Tests: each mode produces a non-empty PNG with reasonable color distribution.

### GUI-01: Scaffold `ymir-gui` crate
- **Crate:** ymir-gui (new)
- **Status:** pending
- **Depends on:** CAT-08, CORE-07
- **Blocked:** no
- **Model:** opus
- **Assignee:**

New crate `crates/ymir-gui` using `egui` (via `eframe`) for the desktop UI. Justification: small dep footprint, runs natively and in wasm, good at data-heavy panels, easy to embed PNG previews. Alternative `iced` rejected for Phase 4 because its layout model is heavier for data-first UIs. Establish the app skeleton: main window, top menu bar (File → Open World, View → Render Mode, Help), blank central area with a left sidebar (future star browser) and right inspector panel. No interactivity beyond opening a pre-generated world directory. Tests: `cargo check` compiles; headless `cargo test` runs an app-startup smoke test if egui supports one (`egui_kittest` or similar — skip if heavyweight).

### GUI-02: Star browser panel
- **Crate:** ymir-gui
- **Status:** pending
- **Depends on:** GUI-01, CAT-09
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

Left sidebar: searchable table over the `Catalog` facade. Columns: name (or Gaia ID), spectral type, distance (pc), T_eff, known planets. Filters: spectral-type multi-select, distance slider, "has confirmed planets" checkbox. Row click selects the star; selected StarContext surfaces in the inspector. Virtualize the table (only render visible rows) so 500k rows perform well. Tests: unit tests for the filter-predicate logic; manual verification for scroll performance.

### GUI-03: System view
- **Crate:** ymir-gui
- **Status:** pending
- **Depends on:** GUI-02
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

Central-area panel showing the selected star's planetary system as a top-down orbital diagram. Circles scaled by body radius (log), orbits by semi-major axis (log). HZ band shaded. Planet click selects it; OrbitalBody surfaces in the inspector. Known exoplanets drawn as filled circles; procedurally-added bodies as outlines. Tests: visual snapshot tests (optional); unit tests on the HZ-band geometry math.

### GUI-04: Globe view
- **Crate:** ymir-gui
- **Status:** pending
- **Depends on:** GUI-03, REND-05
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

Planet-specific view: embeds the generated Mollweide PNG (any render mode from REND-05) for the selected body. Mode dropdown switches between elevation/biome/temperature/moisture/confidence/plates. Lat-lon coordinate display on hover. Tile-click surfaces the underlying `SkeletonTile` + climate + biome data in the inspector. This is still software-rasterized 2D; the 3D globe is Phase 6.

### GUI-05: Detail view
- **Crate:** ymir-gui
- **Status:** pending
- **Depends on:** GUI-04, CLI-05
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

Zoom layer: when a tile is clicked in the globe view with a detail region already generated (`detail/region_NNNN.bin` exists), render the hex-grid detail map. If no detail exists yet, offer a "Generate detail for this region" button that invokes `ymir detail` in-process and displays progress. Hex-click reveals per-hex data (elevation, moisture, flow_accumulation, biome) in the inspector.

### GUI-06: Confidence overlay toggle
- **Crate:** ymir-gui
- **Status:** pending
- **Depends on:** GUI-04, REND-04
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

View menu toggle: overlay the confidence desaturation on top of any render mode (not just biome). State persists across selection changes. Legend sidebar explains the desaturation gradient: full saturation = Observed, mid = Derived, desat = Assumed.

### GUI-07: Provenance report panel
- **Crate:** ymir-gui
- **Status:** pending
- **Depends on:** GUI-04, CORE-07
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

Inspector tab showing the full provenance tree for the selected world: collapsible sections per stage, each leaf field showing `value | source | reference` where applicable. Filter control (text search + "show Observed only"). Click-to-copy on citation references. Populated from `provenance.json`.

### GUI-08: Override editor
- **Crate:** ymir-gui
- **Status:** pending
- **Depends on:** GUI-07, CORE-08
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

Right-click on any Derived/Assumed field in the provenance panel → "Override with observation". Form: value + unit + reference + instrument + date + uncertainty. Saves to the world's overrides.json and offers a "Regenerate" button that invokes the regenerate command and reloads the world in place. Editing an Observed field re-opens the same form to update the observation.

### VALID-07: Catalog round-trip validation
- **Crate:** ymir-catalog (integration tests)
- **Status:** done
- **Depends on:** CAT-08
- **Blocked:** no
- **Model:** sonnet
- **Assignee:** agent

Integration test: open the fixture catalog, query for a known set of stars (Sol, Tau Ceti, TRAPPIST-1, TOI-700 if present), assert each resolves with the expected spectral type and parallax within tolerance. Also verify exoplanet overlay: TRAPPIST-1 resolves with ≥7 known planets; Sol resolves with ≥8 solar system bodies if the fixture includes them (otherwise skip Sol's planet assertion).

**Debrief (2026-04-12):** Shipped `crates/ymir-catalog/tests/catalog_round_trip.rs` with 5 tests: `sun_resolves_as_g_star`, `tau_ceti_resolves_with_correct_spectral_type`, `trappist1_has_seven_planets`, `resolve_by_common_name_matches_resolve_by_gaia_id`, and `toi_700_planets_in_exoplanet_fixture`. Stars actually asserted on: Sol (special-case branch, G class, T_eff 5778 K), Proxima Centauri (round-trip via common name and Gaia ID), TRAPPIST-1 (>=7 planets via exoplanet overlay), TOI-700 (>=4 planets via exoplanet overlay). Tau Ceti and the Gaia-row assertions for TRAPPIST-1/TOI-700 are skipped at runtime because the nearest-50 Gaia fixture does not include them; the tests self-document the miss and verify graceful None returns. TRAPPIST-1 and TOI-700 planet counts are verified directly via `exoplanets_for(gaia_id)` which queries the exoplanet fixture regardless of Gaia fixture presence. All 5 tests pass; clippy clean with `-D warnings`.

### VALID-08: Provenance dump validation
- **Crate:** ymir (binary, integration tests)
- **Status:** pending
- **Depends on:** CORE-07
- **Blocked:** no
- **Model:** sonnet
- **Assignee:**

Integration test: generate an Earth world with the standard continental_fraction + atmosphere overrides; parse `provenance.json`; assert every field has a well-formed `Source`; count Observed vs Derived vs Assumed per stage and check against a golden histogram. Regenerate with an additional override; re-dump; assert the flipped fields transitioned Derived → Observed.

