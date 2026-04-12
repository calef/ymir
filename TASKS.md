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
- **Status:** in-progress
- **Depends on:** SURF-03
- **Blocked:** no
- **Assignee:** agent

Software rasterizer using the `image` crate. Mollweide equal-area projection. Color by elevation for Phase 1. Output PNG file. No wgpu; that's Phase 6.

### CLI-01: `ymir generate` command
- **Crate:** ymir (binary)
- **Status:** pending
- **Depends on:** CAT-01, SYS-01, SYS-02, ATMO-01, ATMO-02, SURF-03, STOR-01, REND-01
- **Blocked:** no
- **Assignee:**

Wire up the full pipeline via clap CLI. `ymir generate --star "Tau Ceti" --seed 42 --output worlds/tau_ceti_42/` produces skeleton data on disk and Mollweide PNG. Per design doc section 7.5.

### CLI-02: `ymir info` command
- **Crate:** ymir (binary)
- **Status:** done
- **Depends on:** STOR-01
- **Blocked:** no
- **Assignee:** agent

Print summary of a generated world: star properties, planet properties, atmosphere, provenance stats. Per design doc section 7.5.
