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
- **Status:** pending
- **Depends on:** INFRA-01
- **Blocked:** no
- **Assignee:**

GitHub Actions workflow: fmt check, clippy, test, doc build. Deferred until GitHub remote exists.

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
- **Status:** in-progress
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
- **Status:** ready
- **Depends on:** CLIM-04, BIOME-04
- **Blocked:** no
- **Assignee:**

Because ymir-storage cannot depend on ymir-climate / ymir-biome per the crate dependency rules, add generic save/load helpers in ymir-storage (`save_bin<T: Serialize>(path, &t)`, `load_bin<T: DeserializeOwned>(path) -> T`) and keep the concrete climate.bin / biomes.bin writes in the binary crate. Update `stages_computed` vocabulary in the manifest ("climate", "biomes") and extend `ymir info` to summarize climate stats (mean T, moisture coverage) and biome histogram.

### REND-02: Biome-colored Mollweide
- **Crate:** ymir-render
- **Status:** ready
- **Depends on:** BIOME-04
- **Blocked:** no
- **Assignee:**

Add `render_biome_mollweide(&SkeletonWorld, &BiomeMap, cfg)` producing a PNG where each tile is colored by biome (static color table per Biome variant). Keep the elevation renderer; this is an additional entry point. Tests: dimensions, determinism, recognizable palette-level distinctions (Earth-like shows greens/blues/tans; Mars-like shows reds/browns).

### CLI-04: Extend ymir generate for climate and biomes
- **Crate:** ymir (binary)
- **Status:** pending
- **Depends on:** CLIM-04, BIOME-04, REND-02, STOR-02
- **Blocked:** no
- **Assignee:**

After the skeleton stage, compute `ClimateMap` and `BiomeMap`. Persist climate.bin, biomes.bin. Emit `preview_biome.png` alongside the elevation preview. Update `stages_computed`. Add `--skip-climate` / `--skip-biomes` flags so users can stop at any stage. Update the stdout summary with climate + biome histogram.

### CLI-03: ymir regenerate command
- **Crate:** ymir (binary)
- **Status:** pending
- **Depends on:** CLI-04, CORE-04, CORE-05
- **Blocked:** no
- **Assignee:**

`ymir regenerate --world PATH --overrides FILE` applies a per-stage override JSON, marks dirty stages via the dependency graph, recomputes only dirty stages, and rewrites the manifest with the new `stages_computed` list and overrides_file pointer. Tests: override at stage 3 (atmosphere) recomputes atmosphere + skeleton + climate + biomes but leaves star/system bits unchanged; byte comparison of unchanged stage artifacts.

### VALID-01: Earth validation test
- **Crate:** ymir (binary, integration tests)
- **Status:** pending
- **Depends on:** CLI-04, CAT-03
- **Blocked:** no
- **Assignee:**

Integration test: `ymir generate --star Earth --seed 1` produces mean surface T within 5 K of 288 K, ocean tiles ~70% of surface (given Earth continental_fraction override), biome histogram includes forest/grassland/desert/tundra/ocean. Pure assertions, no UI.

### VALID-02: Mars validation test
- **Crate:** ymir (binary, integration tests)
- **Status:** pending
- **Depends on:** CLI-04, CAT-03
- **Blocked:** no
- **Assignee:**

Integration test: `ymir generate --star Mars --seed 1` produces the Mars-like abiotic biome palette (no forests/grasslands), mean T below 230 K, CO2-dominated atmosphere.

### VALID-03: Tidally locked validation test
- **Crate:** ymir (binary, integration tests)
- **Status:** pending
- **Depends on:** CLI-04
- **Blocked:** no
- **Assignee:**

Integration test: on a synthetic tidally-locked body, substellar-point temperature is significantly higher than antistellar temperature; biome map shows radial zoning rather than latitudinal banding.

### VALID-04: Override progression test
- **Crate:** ymir (binary, integration tests)
- **Status:** pending
- **Depends on:** CLI-03
- **Blocked:** no
- **Assignee:**

Integration test: generate world A, save; apply an atmosphere-stage override; regenerate; confirm atmosphere, skeleton, climate, biome artifacts changed but star/system artifacts are byte-identical.

