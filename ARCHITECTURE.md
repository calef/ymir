# Ymir — Design Document

**A causal star-to-surface planet simulator grounded in real astronomical data**

**Version:** 1.3 (Draft)
**Date:** 2026-04-11

*"From Ymir's flesh the earth was made, and from his blood the sea, mountains from his bones, trees from his hair, and from his skull the sky." — Grímnismál, Poetic Edda*

---

## 1. Overview

Ymir takes real stellar catalog data (Gaia DR3, NASA Exoplanet Archive), places planets into physically plausible orbits around real stars, then runs a full causal simulation chain — stellar irradiance → atmosphere → climate → tectonics → terrain → biomes — to produce explorable planetary surfaces. Every surface feature is physically derived from the star it orbits, not artistically prescribed.

Any stage's output can be overridden with real observational data — measured orbital parameters from radial velocity surveys, atmospheric composition from JWST transmission spectroscopy, or surface constraints from direct imaging. The pipeline recomputes all downstream stages from the override point, producing a model that blends known observations with physics-derived predictions. As new data arrives, the model improves without regenerating from scratch.

The question the system answers: **"Given this real star — and everything we've observed about its planets — what would the surface actually look like?"**

Written in Rust with GPU-accelerated rendering via wgpu. Designed for agentic code generation with strict typing, comprehensive testing, and modular architecture.

---

## 2. What Makes This Different

No existing tool bridges the full pipeline:

| Tool | What It Does | What It Doesn't |
|------|-------------|----------------|
| SpaceEngine | Procedural universe, galaxy → surface seamless traversal | Surface is generic fractal noise, not derived from stellar context |
| Universe Sandbox | N-body orbital mechanics, basic climate from orbital params | No surface generation, no biomes |
| World Orogen | Tectonics, climate, Köppen biomes on a single planet | No concept of what star it orbits |
| NASA PSG | Rigorous atmospheric spectral modeling | No surface terrain, not interactive |

Ymir fills the gap: the causal chain from real star → derived planet → physically modeled surface, as an integrated system.

---

## 3. Goals and Non-Goals

**Goals:**

- Full causal chain: star properties → planetary properties → surface features
- Observational override system: inject real measurements at any pipeline stage; downstream stages recompute automatically
- Anchored to real stellar data — generate surfaces for planets orbiting actual cataloged stars
- Deterministic: same star + same seed + same overrides → identical planet
- Physically grounded: every parameter derived from upstream physics or real observation, not hand-tuned
- Modular pipeline: each stage independently testable and replaceable
- Designed for agentic code generation

**Non-Goals:**

- Real-time 3D exploration (v1 produces 2D projected maps; 3D is a future phase)
- Galaxy-scale rendering or universe traversal (SpaceEngine exists)
- N-body orbital simulation (Universe Sandbox exists)
- Multiplayer or networked sharing
- Atmospheric spectral modeling (NASA PSG exists)

---

## 4. Causal Pipeline

This is the core of the system. Each stage consumes only the outputs of previous stages. Any stage's output can be replaced with observational data via the override system (§4.1), and all downstream stages recompute from the override point.

```
┌─────────────────────────────────────────────────────┐
│  STAGE 1: STELLAR CONTEXT                           │
│  Real catalog data → star characterization           │
│  Input: Gaia DR3 / Hipparcos / Exoplanet Archive     │
│  Output: StarContext                                 │
│  Override: ★ Always uses real data when available     │
└──────────────────────┬──────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────┐
│  STAGE 2: PLANETARY SYSTEM                           │
│  Derive orbits, masses, bulk properties              │
│  Input: StarContext + seed                           │
│  Output: PlanetarySystem (array of OrbitalBody)      │
│  Override: inject measured orbit, mass, radius        │
└──────────────────────┬──────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────┐
│  STAGE 3: ATMOSPHERE                                 │
│  Composition, pressure, greenhouse effect            │
│  Input: OrbitalBody + StarContext                    │
│  Output: AtmosphereModel                             │
│  Override: inject JWST-detected composition           │
└──────────────────────┬──────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────┐
│  STAGE 4: GLOBAL SKELETON                            │
│  Geodesic grid, tectonics, macro elevation           │
│  Input: OrbitalBody + AtmosphereModel + seed         │
│  Output: SkeletonWorld                               │
│  Override: inject heightmap from radar/imaging data   │
└──────────────────────┬──────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────┐
│  STAGE 5: CLIMATE                                    │
│  Temperature, moisture, wind — physics-driven        │
│  Input: SkeletonWorld + AtmosphereModel + StarContext │
│  Output: ClimateField                                │
│  Override: inject observed temperature map            │
└──────────────────────┬──────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────┐
│  STAGE 6: BIOME CLASSIFICATION                       │
│  Whittaker lookup + Markov smoothing                 │
│  Input: ClimateField + BiomeWeights                  │
│  Output: BiomeMap                                    │
│  Override: inject observed surface classification     │
└──────────────────────┬──────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────┐
│  STAGE 7: REGIONAL DETAIL (on demand)                │
│  High-res hex grid, rivers, local terrain            │
│  Input: SkeletonWorld region + ClimateField + seed   │
│  Output: DetailChunk                                 │
└─────────────────────────────────────────────────────┘
```

### 4.1 Observational Override System

The override system is what makes Ymir a living model rather than a one-shot generator. Every stage's output type wraps its data in a provenance-tagged container that records whether each field was **derived** (computed from upstream), **observed** (injected from real measurements), or **default** (fallback value).

**Design:**

```rust
/// Provenance tag for every value in the pipeline
#[derive(Clone, Debug, Serialize, Deserialize)]
enum Source {
    /// Computed from upstream stage outputs
    Derived { from_stage: &'static str },
    /// Injected from real observational data
    Observed {
        reference: String,    // e.g., "Benneke+ 2024, ApJ, 961, 86"
        instrument: String,   // e.g., "JWST NIRSpec G395H"
        date: String,         // ISO 8601 observation date
        uncertainty: Option<f64>,
    },
    /// Fallback / user-specified with no observational basis
    Assumed { reason: String },
}

/// A value with provenance tracking
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Sourced<T> {
    value: T,
    source: Source,
}

impl<T> Sourced<T> {
    fn derived(value: T, stage: &'static str) -> Self { ... }
    fn observed(value: T, reference: &str, instrument: &str) -> Self { ... }
    fn assumed(value: T, reason: &str) -> Self { ... }
    fn is_observed(&self) -> bool { matches!(self.source, Source::Observed { .. }) }
}
```

**Stage output types use `Sourced<T>` for overridable fields:**

```rust
struct OrbitalBody {
    semi_major_axis: Sourced<f64>,   // Could be: Observed (RV data) or Derived
    mass: Sourced<f64>,              // Could be: Observed (RV) or Derived (mass-radius)
    radius: Sourced<f64>,            // Could be: Observed (transit) or Derived
    eccentricity: Sourced<f64>,
    // ... etc
}

struct AtmosphereModel {
    surface_pressure: Sourced<f64>,
    composition: Sourced<HashMap<Gas, f64>>,  // Observed if JWST data available
    greenhouse_factor: Sourced<f64>,
    effective_surface_temp: Sourced<f64>,
    // ... etc
}
```

**Override rules:**

1. If a field is `Observed`, it is never recomputed — it always takes precedence.
2. If a field is `Derived` and an upstream dependency changes (new override added), all derived fields downstream are invalidated and recomputed.
3. The pipeline tracks a dependency graph: when an override is added at stage N, stages N+1 through 7 are marked dirty and regenerated. Stages 1 through N-1 are untouched.
4. Overrides are stored in the world manifest alongside the seed and config, so a world is fully reproducible: `seed + overrides → identical output`.

**Override file format:**

Overrides are specified as a JSON file that can be passed to the CLI or loaded in the GUI:

```json
{
  "version": "1.0",
  "target_star": "TOI-700",
  "target_planet": "d",
  "overrides": {
    "orbital_body": {
      "semi_major_axis": {
        "value": 0.163,
        "unit": "AU",
        "reference": "Gilbert+ 2023, ApJL, 944, L35",
        "instrument": "TESS + Spitzer",
        "uncertainty": 0.003
      },
      "radius": {
        "value": 1.073,
        "unit": "R_earth",
        "reference": "Gilbert+ 2023",
        "instrument": "TESS",
        "uncertainty": 0.065
      }
    },
    "atmosphere": {
      "composition": {
        "value": { "CO2": 0.95, "N2": 0.03, "H2O": 0.01, "Ar": 0.01 },
        "reference": "hypothetical JWST detection",
        "instrument": "JWST NIRSpec"
      }
    }
  }
}
```

**How it works in practice — the "new planet discovered" workflow:**

1. Day 1: Discovery paper announces a 1.4 R⊕ planet at 0.85 AU around HD 40307 via transit photometry. You create an override file with `radius: 1.4` and `semi_major_axis: 0.85` (Observed). Run `ymir generate`. Everything else is Derived from the star + these two measurements.

2. Month 3: Radial velocity follow-up measures mass at 3.2 M⊕. You add `mass: 3.2` to the override file. Run `ymir regenerate`. Stages 2–7 recompute. The atmosphere model changes (higher gravity → different retention), which changes climate, which changes biomes. The surface looks different because you have better data.

3. Year 2: JWST transmission spectrum detects CO₂, H₂O, and tentative O₃. You add `atmosphere.composition` to overrides. Run `ymir regenerate`. Only stages 3–7 recompute. The atmosphere model is now observationally anchored. If O₃ implies O₂ implies biology, the biome palette switches to Earth-like vegetation. The surface transforms.

4. Each version is stored with its manifest showing exactly which fields were observed vs. derived. You can diff versions to see how new data changed the model.

**Confidence visualization:**

The renderer can color-code tiles by confidence: regions whose appearance is constrained by observed data (e.g., biome driven by JWST-confirmed atmosphere) are rendered at full saturation. Regions driven entirely by procedural derivation are rendered slightly desaturated. This gives the user an immediate visual sense of "how much of this surface is constrained by real observations vs. simulation."

---

## 5. Stage Specifications

### 5.1 Stage 1 — Stellar Context (`stellar.rs`)

Ingests real star data and characterizes the stellar environment a planet would experience.

**Data sources:**

- **Gaia DR3** — 1.8 billion stars with parallax, luminosity, effective temperature, metallicity. For v1, pre-filter to a working catalog: solar-neighborhood stars within 100 parsecs with well-characterized properties (~500k stars, ~2GB as Parquet).
- **NASA Exoplanet Archive** — known exoplanets with orbital parameters, mass/radius estimates. Where real exoplanets exist around a star, use their actual data; fill gaps procedurally.
- **Hipparcos** — cross-reference for bright nearby stars with good proper motion data.

**Derived properties:**

```rust
struct StarContext {
    // Catalog data (observed)
    catalog_id: String,           // e.g., "Gaia DR3 4472832130942575872"
    name: Option<String>,         // common name if known, e.g., "Tau Ceti"
    spectral_type: SpectralType,  // e.g., G2V, K5V, M3V
    effective_temp: f64,          // K
    luminosity: f64,              // solar luminosities
    metallicity: f64,             // [Fe/H]
    mass: f64,                    // solar masses
    radius: f64,                  // solar radii
    age: f64,                     // Gyr (estimated from isochrones)
    distance: f64,                // parsecs from Sol

    // Derived (computed)
    hz_inner: f64,                // AU — conservative inner HZ (Kopparapu+ 2013)
    hz_outer: f64,                // AU — conservative outer HZ
    hz_inner_optimistic: f64,     // AU — optimistic (recent Venus) limit
    hz_outer_optimistic: f64,     // AU — optimistic (early Mars) limit
    uv_flux_hz: f64,              // UV flux at HZ midpoint (affects atmosphere)
    known_exoplanets: Vec<ExoplanetRecord>, // from NASA archive, if any
}
```

**Habitable zone calculation** uses the Kopparapu et al. (2013, 2014) parametric model:

```
S_eff = S_eff_sun + a*T_star + b*T_star^2 + c*T_star^3 + d*T_star^4
```

where `T_star = T_eff - 5780` and coefficients differ for each HZ boundary (moist greenhouse, maximum greenhouse, recent Venus, early Mars). Results are in stellar flux units, converted to AU via `d = sqrt(L / S_eff)`.

### 5.2 Stage 2 — Planetary System (`system.rs`)

Generates a plausible planetary system around the star. If real exoplanets are known, they anchor the system; remaining slots are filled procedurally.

**Planet placement algorithm:**

1. If the star has known exoplanets, place them at their observed orbital distances.
2. Generate additional planets using Kepler occurrence rate statistics: frequency of planets as a function of orbital period and radius, conditioned on stellar type and metallicity.
3. Space planets using mutual Hill sphere separation (Δ ≥ 8 R_Hill for stability, per Chambers+ 1996).
4. Assign masses via mass-radius relationships (Chen & Kipping 2017): `M = C * R^γ` with breakpoints at 1.23 R⊕ (Terran), 14.26 R⊕ (Neptunian).
5. Assign eccentricities from observed distribution (Rayleigh with σ ≈ 0.05 for compact systems, σ ≈ 0.25 for widely-spaced).
6. Assign axial tilt from uniform distribution (0–45°), with tidal lock check for close-in planets around low-mass stars.

**Output per planet:**

```rust
struct OrbitalBody {
    // Orbital parameters
    semi_major_axis: f64,     // AU
    eccentricity: f64,
    inclination: f64,         // degrees
    axial_tilt: f64,          // degrees

    // Bulk properties
    mass: f64,                // Earth masses
    radius: f64,              // Earth radii
    density: f64,             // g/cm³ (derived from mass + radius)
    surface_gravity: f64,     // m/s² (derived)

    // Derived from star + orbit
    solar_irradiance: f64,    // W/m² at semi-major axis
    equilibrium_temp: f64,    // K (before greenhouse)
    tidal_locked: bool,       // synchronous rotation check
    rotation_period: f64,     // hours (24h default, or locked)

    // Classification
    is_in_hz: bool,           // within conservative HZ?
    planet_type: PlanetType,  // Terran, SuperEarth, SubNeptune, ...
}
```

**Tidal locking criterion:** planets with orbital period < threshold (dependent on stellar mass and system age) are flagged as tidally locked. This fundamentally changes their climate — permanent day/night hemispheres with a terminator ring.

### 5.3 Stage 3 — Atmosphere (`atmosphere.rs`)

Models atmospheric composition, pressure, and greenhouse warming. This is the most physics-intensive derivation stage.

**Atmosphere retention check:**

1. Compute escape velocity: `v_esc = sqrt(2GM/R)`
2. Compute thermal velocity of atmospheric species at equilibrium temperature: `v_th = sqrt(3kT/m)`
3. If `v_esc / v_th < 6` for a given molecule, it escapes over geological time (Jeans escape).
4. Result: which molecules the planet can retain (H₂, He, H₂O, N₂, CO₂, O₂, Ar, CH₄).

**Composition model (simplified for v1):**

- Terran planets (0.5–2 R⊕): N₂/CO₂-dominated, with H₂O if in HZ. O₂ presence probabilistic (requires biology; tunable parameter).
- Super-Earths (2–4 R⊕): may retain H₂/He envelopes → higher surface pressure.
- Metallicity influence: high-metallicity stars produce planets with more volatiles.

**Greenhouse effect:**

```rust
struct AtmosphereModel {
    surface_pressure: f64,        // bar
    composition: HashMap<Gas, f64>, // mole fractions
    greenhouse_factor: f64,        // multiplicative temperature boost
    effective_surface_temp: f64,   // K (equilibrium_temp * greenhouse)
    scale_height: f64,            // km
    moisture_capacity: f64,        // relative (affects climate)
    uv_surface_flux: f64,         // fraction of stellar UV reaching surface
}
```

Greenhouse factor is computed from a parameterized model: `T_surface = T_eq * (1 + τ)^0.25` where τ is optical depth, derived from CO₂ and H₂O column densities. Earth-calibrated, then scaled by composition.

### 5.4 Stage 4 — Global Skeleton (`skeleton.rs`)

Generates the planetary surface at global scale. This is the geodesic grid + tectonics + macro elevation from the previous design, now parameterized by the planet's physical properties.

**Physics-driven parameters:**

- **Plate count:** scales with planet surface area and internal heat budget. Larger planets → more plates. Internal heat estimated from mass + age (radiogenic heating decay).
- **Mountain height cap:** `h_max ≈ (yield_strength) / (ρ * g)`. Earth: ~10km. Lower gravity → taller mountains. Mars (0.38g) can support Olympus Mons at 21km.
- **Continental fraction:** currently a free parameter (no established derivation from first principles). Could correlate with water inventory and tectonic regime. Parameterized with a physically motivated prior.
- **Ocean depth:** from estimated water inventory (correlates with mass and formation distance from star).

**Grid:** icosahedral geodesic, subdivision level scaled to planet radius. Earth-sized → level 5 (~10k tiles). Larger planets → level 6.

**Tectonics:** operates on the geodesic grid as previously designed (§4.5 of v0.4). Plate generation via Poisson-seeded flood fill, boundary classification (convergent/divergent/transform), elevation bias field.

**Heightmap:** tectonic bias + spherical fractal noise, with noise amplitude scaled by gravity (lower g → rougher terrain, higher relief).

```rust
struct SkeletonWorld {
    grid: GeodesicGrid,
    tiles: Vec<SkeletonTile>,
    tectonics: TectonicData,
    body: OrbitalBody,
    atmosphere: AtmosphereModel,
}
```

### 5.5 Stage 5 — Climate (`climate.rs`)

Global climate simulation driven by real stellar irradiance, atmosphere model, and rotation.

**Temperature field:**

```
T(lat, lon) = T_base(lat) - lapse_rate * elevation(lat, lon)
```

where `T_base(lat)` is derived from:
- Stellar irradiance at the planet's orbital distance
- Greenhouse-adjusted effective temperature
- Latitudinal distribution: `cos(lat)` scaling for irradiance angle
- Axial tilt: if >0°, introduces seasonal variation (v1: annual average; future: seasonal)

For **tidally locked planets**, the temperature model is fundamentally different: irradiance depends on angular distance from the substellar point, not latitude. The substellar point is permanently heated; the antistellar point is permanently dark. The terminator ring has moderate temperatures.

```rust
fn temperature_at(tile: &SkeletonTile, body: &OrbitalBody, atmo: &AtmosphereModel) -> f64 {
    if body.tidal_locked {
        // Irradiance drops as cos(angle_from_substellar)
        let angle = substellar_angle(tile, body);
        let local_irradiance = body.solar_irradiance * angle.cos().max(0.0);
        surface_temp_from_irradiance(local_irradiance, atmo)
    } else {
        // Standard latitudinal model
        let lat_factor = (tile.lat.to_radians().cos()).sqrt(); // diurnal average
        let local_irradiance = body.solar_irradiance * lat_factor;
        surface_temp_from_irradiance(local_irradiance, atmo) - lapse_rate(atmo) * elevation_m(tile)
    }
}
```

**Moisture field:**

Two-component model (unchanged from v0.4 design):
1. Latitude-banded atmospheric circulation (ITCZ, horse latitudes, polar front). For tidally locked planets: radial circulation from substellar point replaces latitude bands.
2. Wind-driven transport with orographic effects.

Moisture capacity scales with atmospheric pressure and temperature (Clausius-Clapeyron): hotter, thicker atmospheres carry more moisture.

**Wind model:**

For rotating planets: latitude-dependent prevailing winds (easterlies in tropics, westerlies in mid-latitudes, polar easterlies). Wind cell width scales with rotation rate (faster rotation → narrower cells → more climate bands).

For tidally locked planets: radial outflow from substellar point at surface, return flow aloft. Strong permanent winds at the terminator.

### 5.6 Stage 6 — Biome Classification (`biome.rs`)

Two-phase assignment unchanged from previous design:
1. Physical classification via Whittaker-style lookup (temperature × moisture → biome).
2. Markov smoothing with data-ingestible transition weights.

**Biome palette is context-dependent:**

The available biome set depends on the atmosphere model:
- O₂-bearing atmosphere (biological): full Earth-like biome palette (forests, grasslands, etc.)
- CO₂-dominated, no O₂: abiotic palette (rock desert, chemical sediment, volcanic terrain, ice, liquid water)
- Thin atmosphere (Mars-like): dust plains, exposed bedrock, polar CO₂ ice, impact regolith
- No atmosphere: bare regolith, impact craters, thermal fractures

```rust
fn biome_palette(atmo: &AtmosphereModel) -> BiomePalette {
    match atmo.classification() {
        AtmoClass::OxygenBearing => BiomePalette::earth_like(),
        AtmoClass::ThickCO2 => BiomePalette::venus_like(),
        AtmoClass::ThinCO2 => BiomePalette::mars_like(),
        AtmoClass::None => BiomePalette::airless(),
        AtmoClass::ThickN2H2O => BiomePalette::titan_like(),
    }
}
```

**Markov weight files** can be derived from real data for Earth (WWF ecoregions, MODIS) and Mars (USGS geologic map SIM 3292). Other palettes are hand-tuned with physical constraints. Weight file schema unchanged from v0.4.

### 5.7 Stage 7 — Regional Detail (`detail.rs`)

On-demand high-resolution generation for a region of the skeleton. Unchanged in architecture from v0.4:
- Hex grid subdividing a skeleton tile region
- Fractal noise perturbation of skeleton elevation
- Local wind-driven moisture with orographic effects
- River flow accumulation + lake formation
- Markov biome smoothing at fine resolution

Detail PRNG seeded by `hash(world_seed, region_index)` for determinism and independence.

---

## 6. Data Architecture

### 6.1 Star Catalog

**Working catalog:** Pre-filtered subset of Gaia DR3 stored as Parquet:
- Stars within 100 parsecs of Sol
- Well-characterized: parallax error < 20%, known T_eff, luminosity
- ~500k stars, ~2GB on disk
- Indexed by position (for spatial queries) and spectral type (for filtering)

**Exoplanet overlay:** NASA Exoplanet Archive data joined by host star ID. ~5,700 confirmed exoplanets as of 2025.

### 6.2 Generated World Storage

Each generated world is stored as a self-contained directory:

```
worlds/
└── tau_ceti_b_seed42/
    ├── manifest.json         # star context, body params, config, seed, overrides
    ├── overrides.json        # observational overrides (if any)
    ├── provenance.json       # per-field source tracking (Derived/Observed/Assumed)
    ├── skeleton.bin          # geodesic grid + tile data (compact binary)
    ├── climate.bin           # temperature + moisture fields
    ├── biomes.bin            # biome assignments
    ├── tectonics.bin         # plate data + boundaries
    └── detail/
        ├── region_0042.bin   # detail chunk (generated on demand)
        └── region_0108.bin
```

Binary format: custom, versioned, with a JSON manifest for human readability. SQLite considered but binary is faster for large grid data. Manifest includes full provenance: star catalog version, pipeline version, all config parameters, and override history with references. The `provenance.json` file records the `Source` tag for every field in the pipeline, enabling auditability and the confidence visualization.

### 6.3 Biome Weight Files

Unchanged from v0.4 design. JSON files conforming to `weight-schema.json`. Shipped presets:

| Weight File | Source | Biome Count |
|------------|--------|-------------|
| `earth_wwf.json` | WWF ecoregions (future: data-derived) | 16 |
| `earth_handtuned.json` | Hand-tuned Earth-like (v1 default) | 16 |
| `mars_usgs.json` | USGS SIM 3292 geologic units | 8 |
| `abiotic.json` | Hand-tuned for O₂-free worlds | 6 |
| `airless.json` | Hand-tuned for no-atmosphere bodies | 4 |

---

## 7. Architecture

### 7.1 Cargo Workspace and Crate Decomposition

The project is a Cargo workspace of independently publishable crates. Each crate has a focused responsibility, minimal dependencies, and a clean public API. Downstream consumers — researchers, game developers, worldbuilders — can `cargo add` only the crates they need.

**Workspace layout:**

```
ymir/
├── Cargo.toml                    # [workspace] definition
├── LICENSE-MIT                   # MIT license text
├── LICENSE-APACHE                # Apache 2.0 license text
├── README.md                    # Project overview + crate index
├── CONTRIBUTING.md              # Contribution guidelines
├── CITATION.cff                 # Citation metadata for academic use
│
├── crates/
│   ├── ymir-core/         # Foundation types, Sourced<T>, PRNG, override system
│   │   ├── Cargo.toml
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── sourced.rs       # Sourced<T> + Source provenance enum
│   │   │   ├── override_file.rs # Override JSON parsing + validation
│   │   │   ├── dependency_graph.rs
│   │   │   ├── provenance.rs    # Provenance report generation
│   │   │   └── prng.rs          # Seeded PRNG (PCG)
│   │   └── examples/
│   │       └── provenance_demo.rs
│   │
│   ├── ymir-catalog/      # Star catalog ingest + query
│   │   ├── Cargo.toml           # depends: core
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── gaia.rs          # Gaia DR3 Parquet reader
│   │   │   ├── exoplanets.rs    # NASA Exoplanet Archive reader
│   │   │   ├── star_context.rs  # StarContext + HZ calculation
│   │   │   └── catalog_index.rs # Spatial + spectral index
│   │   └── examples/
│   │       ├── query_nearby_stars.rs
│   │       └── hz_calculator.rs
│   │
│   ├── ymir-system/       # Planetary system generation
│   │   ├── Cargo.toml           # depends: core, catalog
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── placement.rs     # Orbital placement + Hill stability
│   │   │   ├── bulk_properties.rs
│   │   │   ├── tidal.rs         # Tidal locking, rotation
│   │   │   └── orbital_body.rs
│   │   └── examples/
│   │       └── generate_system.rs
│   │
│   ├── ymir-atmosphere/   # Atmosphere modeling
│   │   ├── Cargo.toml           # depends: core, system
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── retention.rs     # Jeans escape
│   │   │   ├── composition.rs
│   │   │   ├── greenhouse.rs
│   │   │   └── atmosphere_model.rs
│   │   └── examples/
│   │       └── earth_atmosphere_check.rs
│   │
│   ├── ymir-surface/      # Geodesic grid, tectonics, heightmap
│   │   ├── Cargo.toml           # depends: core, system, atmosphere
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── geodesic.rs
│   │   │   ├── hex.rs
│   │   │   ├── tectonics.rs
│   │   │   ├── heightmap.rs
│   │   │   ├── noise.rs
│   │   │   └── skeleton.rs
│   │   └── examples/
│   │       └── generate_skeleton.rs
│   │
│   ├── ymir-climate/      # Climate simulation
│   │   ├── Cargo.toml           # depends: core, surface, atmosphere
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── temperature.rs
│   │   │   ├── moisture.rs
│   │   │   ├── wind.rs
│   │   │   └── climate_field.rs
│   │   └── examples/
│   │       └── tidally_locked_climate.rs
│   │
│   ├── ymir-biome/        # Biome classification
│   │   ├── Cargo.toml           # depends: core, climate
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── whittaker.rs
│   │   │   ├── markov.rs
│   │   │   ├── palette.rs
│   │   │   └── weight_schema.rs
│   │   └── examples/
│   │       └── classify_biomes.rs
│   │
│   ├── ymir-detail/       # Regional detail generation
│   │   ├── Cargo.toml           # depends: core, surface, climate, biome
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── detail_grid.rs
│   │   │   ├── rivers.rs
│   │   │   ├── detail_pipeline.rs
│   │   │   └── detail_chunk.rs
│   │   └── examples/
│   │       └── generate_detail_region.rs
│   │
│   ├── ymir-render/       # Visualization (optional dependency)
│   │   ├── Cargo.toml           # depends: core, surface, climate, biome
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── globe_renderer.rs
│   │   │   ├── detail_renderer.rs
│   │   │   ├── projections.rs
│   │   │   ├── color_maps.rs
│   │   │   └── overlays.rs
│   │   └── examples/
│   │       └── render_mollweide.rs
│   │
│   └── ymir-storage/      # Persistence + world I/O
│       ├── Cargo.toml           # depends: core
│       ├── README.md
│       ├── src/
│       │   ├── lib.rs
│       │   ├── world_io.rs
│       │   └── manifest.rs
│       └── examples/
│           └── save_load_world.rs
│
├── src/                         # Binary crate: CLI application
│   ├── main.rs                  # Entry point + clap CLI
│   ├── pipeline.rs              # Top-level orchestration
│   ├── star_browser.rs          # Interactive star selection (GUI)
│   ├── system_view.rs
│   ├── globe_view.rs
│   ├── detail_view.rs
│   └── controls.rs
│
└── data/
    ├── catalogs/                # Pre-filtered star catalog Parquet files
    └── weights/                 # Shipped biome weight files
        ├── earth_handtuned.json
        ├── mars_usgs.json
        ├── abiotic.json
        └── airless.json
```

### 7.2 Crate Dependency Graph

```
ymir-core          (no dependencies)
       │
       ▼
ymir-catalog       → core
       │
       ▼
ymir-system        → core, catalog
       │
       ▼
ymir-atmosphere    → core, system
       │
       ▼
ymir-surface       → core, system, atmosphere
       │
       ▼
ymir-climate       → core, surface, atmosphere
       │
       ▼
ymir-biome         → core, climate
       │
       ▼
ymir-detail        → core, surface, climate, biome

ymir-render        → core, surface, climate, biome  (parallel to detail)
ymir-storage       → core  (used by binary, not required by lib crates)
ymir (binary)      → all crates
```

Each crate re-exports its primary types from `lib.rs`. No crate leaks types from downstream crates through its public API — if `ymir-climate` needs a `SkeletonTile`, it takes it as a generic parameter or uses a trait from `core`, not a concrete type from `surface`.

### 7.3 Public API Design Principles

Every crate follows these rules:

1. **Minimal public surface.** Only types and functions a downstream consumer actually needs are `pub`. Internal helpers are `pub(crate)`.
2. **No required runtime dependencies on downstream crates.** `cargo add ymir-atmosphere` does not pull in `ymir-surface`.
3. **Builder pattern for complex configs.** Stage configs use builders with sensible defaults so consumers aren't forced to specify every parameter.
4. **All public types implement `Serialize + Deserialize`.** Enables pipeline interop — a researcher can run `ymir-atmosphere` in Rust, serialize the output, and consume it in Python via JSON.
5. **All public functions are `#[doc]`-documented with examples.** A crate isn't published without docs passing `cargo doc --no-deps`.
6. **Every crate has at least one `examples/` program** demonstrating standalone usage without the full pipeline.
7. **Feature flags for optional heavy dependencies.** `ymir-catalog` has a `parquet` feature (default on) that can be disabled if someone wants to supply star data programmatically instead of from files. `ymir-render` has a `wgpu` feature that can be disabled for headless/server use.

### 7.4 Versioning and Compatibility

Crates are versioned independently following semver. The workspace `Cargo.toml` pins compatible ranges:

- Breaking changes to `ymir-core` types (especially `Sourced<T>`) require major version bumps across all dependent crates.
- Breaking changes to a leaf crate (e.g., `ymir-render`) only affect that crate's version.
- The binary crate (`ymir`) version tracks the "release" version of the full application and pins exact versions of all workspace crates.

A `CHANGELOG.md` per crate tracks changes. The workspace root `CHANGELOG.md` summarizes cross-crate changes per release.

### 7.5 CLI Interface (v1)

Before any GUI, the system is usable as a CLI tool:

```bash
# Generate a planet around Tau Ceti, seed 42, save to disk
ymir generate --star "Tau Ceti" --seed 42 --output worlds/tau_ceti_42/

# Generate with observational overrides
ymir generate --star "TOI-700" --planet d --seed 7 \
  --overrides overrides/toi700d.json --output worlds/toi700d/

# Regenerate after adding new observational data (only dirty stages recompute)
ymir regenerate worlds/toi700d/ --overrides overrides/toi700d_v2.json

# Show provenance report: which fields are observed vs. derived
ymir provenance worlds/toi700d/

# Diff two versions of the same world (before/after new override)
ymir diff worlds/toi700d_v1/ worlds/toi700d_v2/

# Generate around a star by Gaia ID
ymir generate --gaia-id 4472832130942575872 --seed 7

# List stars in catalog matching criteria
ymir catalog search --type K --distance-max 20 --hz-width-min 0.3

# Render a globe view of a generated world
ymir render globe worlds/tau_ceti_42/ --projection mollweide --output tau_ceti.png

# Render with confidence overlay (observed vs. derived coloring)
ymir render globe worlds/toi700d/ --overlay confidence --output toi700d_confidence.png

# Generate detail for a specific region
ymir detail worlds/tau_ceti_42/ --region 42 --radius 50

# Info about a generated world
ymir info worlds/tau_ceti_42/
```

---

## 8. Technology Stack

| Component | Choice | Justification |
|-----------|--------|---------------|
| Language | Rust | Performance for compute-heavy simulation, strong type system for agent-friendliness, no GC pauses |
| GPU compute | wgpu | Cross-platform (Vulkan/Metal/DX12), also compiles to WebGPU for future browser deployment |
| Star catalog storage | Apache Parquet via `parquet` crate | Columnar format, fast filtering, standard in data science |
| Noise generation | Custom or `noise` crate | Seeded 2D/3D simplex, spherical sampling |
| PRNG | PCG family via `rand_pcg` | Fast, well-distributed, seedable, reproducible across platforms |
| Rendering | wgpu + egui | egui for UI panels; wgpu for map/globe rendering |
| Serialization | `serde` + `bincode` for binary, `serde_json` for manifests | Standard Rust ecosystem |
| Testing | Built-in `#[test]` + `proptest` for property-based | No external test runner needed |
| CLI | `clap` | Standard Rust CLI framework |

**Future option:** Because wgpu compiles to WebGPU via wasm, the rendering and potentially the full pipeline could be ported to browser later without rewriting. This preserves optionality.

---

## 9. Licensing and Open Source

### 9.1 License

**Dual-licensed under MIT and Apache 2.0** — the standard Rust ecosystem convention. Downstream consumers may choose either license.

- **MIT** — maximally permissive, universally understood, no copyleft. Scientists can embed it in any research context without legal review. Commercial game studios and SaaS products can use it without warranty or attribution burden beyond preserving the license text.
- **Apache 2.0** — adds explicit patent grant, protecting both contributors and consumers from patent claims on contributed algorithms. Preferred by some corporate legal departments.

Every crate carries both license files and the standard Cargo.toml header:

```toml
[package]
license = "MIT OR Apache-2.0"
```

### 9.2 Academic Citation

A `CITATION.cff` file at the workspace root provides machine-readable citation metadata for researchers who use any Ymir crate in published work. This follows the Citation File Format standard supported by GitHub, Zenodo, and most reference managers.

```yaml
cff-version: 1.2.0
title: "Ymir: Causal star-to-surface planet simulation toolkit"
message: "If you use this software in research, please cite it as below."
type: software
license:
  - MIT
  - Apache-2.0
repository-code: "https://github.com/ymir-sim/ymir"
keywords:
  - exoplanets
  - procedural generation
  - habitable zones
  - planetary science
  - climate simulation
```

Individual crates that implement specific published algorithms (e.g., Kopparapu HZ calculations in `ymir-catalog`) should cite the original papers in their `README.md` and doc comments.

### 9.3 Contribution Model

- **DCO sign-off** (Developer Certificate of Origin) required for all contributions — lightweight, no CLA.
- All contributions are dual-licensed MIT/Apache-2.0 by default.
- `CONTRIBUTING.md` documents: code style, testing expectations (90%+ coverage on core crates), doc requirements (all public items documented), and the per-crate `examples/` requirement.
- Issues and PRs labeled by crate for discoverability.

### 9.4 Distribution

- **crates.io** — all library crates published individually. Binary crate (`ymir`) also published for `cargo install`.
- **GitHub Releases** — pre-built binaries for Linux, macOS, Windows.
- **Zenodo** — DOI-archived releases for academic citation stability.
- **Star catalog data** — the pre-filtered Gaia DR3 subset is too large for crates.io. Distributed separately via GitHub Releases or a dedicated data repository. `ymir-catalog` includes a `download-catalog` binary target that fetches and validates the data.

---

## 10. Testing Strategy

### 10.1 Unit Tests by Module

| Module | What to Test | Example |
|--------|-------------|---------|
| `star_context.rs` | HZ calculation matches published values | Kopparapu+ 2013 Table 3: Sun HZ inner = 0.99 AU ± 0.01 |
| `placement.rs` | Hill stability, Kepler rate consistency | No two planets within 8 mutual Hill radii; planet frequency matches Kepler occurrence within 2σ |
| `retention.rs` | Known atmosphere retention | Earth retains N₂, O₂, H₂O; loses H₂, He. Mars retains CO₂; loses H₂O over Gyr timescales |
| `greenhouse.rs` | Earth-calibrated | Earth inputs → ~288K surface temp (within 5K) |
| `geodesic.rs` | Grid completeness, neighbor consistency | All tiles have 5 or 6 neighbors; total tiles = 10×4^N+2; every neighbor relationship is symmetric |
| `tectonics.rs` | Plate coverage, boundary classification | Every tile in exactly one plate; convergent boundaries have positive elevation bias |
| `heightmap.rs` | Gravity-scaled relief | Lower-gravity planet has higher max elevation than Earth-gravity planet (same seed) |
| `temperature.rs` | Physically correct gradients | Equator warmer than poles; higher elevation colder; tidally locked substellar point hottest |
| `moisture.rs` | Circulation patterns | Equatorial band wetter than 30° band; rain shadow downwind of mountains |
| `rivers.rs` | Flow conservation, downhill invariant | Every tile flows to lower-elevation neighbor; enclosed basins become lakes |
| `biome.rs` | Physical constraints + palette consistency | No forest biomes on airless worlds; no tropical biome below 0°C |
| `pipeline.rs` | End-to-end determinism | Same star + seed + overrides → bit-identical output |
| `sourced.rs` | Provenance tracking | Observed fields survive pipeline reruns; derived fields recompute when upstream changes |
| `override_file.rs` | Override parsing + validation | Malformed override JSON → clear error; valid override → correct field injection |
| `dependency_graph.rs` | Dirty marking | Override at stage 3 marks stages 3–7 dirty; stages 1–2 untouched |

### 10.2 Integration Tests

- **Earth validation:** Generate a planet with Sol's parameters, Earth's mass/orbit. Resulting climate should produce recognizable Earth-like biome distribution: tropical forests near equator, deserts at ~30°, temperate forests at 40–60°, tundra/ice at poles.
- **Mars validation:** Generate with Sol + Mars orbit/mass. Thin atmosphere, no surface water, polar ice caps, volcanic terrain.
- **Tidal lock validation:** Generate around a late M-dwarf. Planet should show permanent day/night hemispheres with habitable terminator ring.
- **Override progression:** Generate TOI-700 d with radius-only override → add mass override → add atmosphere override. Each step should produce a different surface, with later versions more physically constrained. Provenance report should show increasing observed-field count at each step.

### 10.3 Property-Based Tests

Via `proptest`:
- For any star with T_eff ∈ [2600, 7200]K: HZ inner < HZ outer
- For any planet: surface_gravity > 0; escape_velocity > 0
- For any geodesic grid: neighbor relationships are symmetric
- For any skeleton: all tiles have elevation in [0, 1]
- For any override: observed fields in output match override values exactly (no silent recomputation)

### 10.4 Coverage

Target: 90%+ line coverage on all modules except `render/` and `ui/`.

---

## 11. Development Phases

### Phase 1 — Star-to-skeleton MVP
Pick one well-characterized star (Tau Ceti). Hardcode catalog lookup. Implement stages 1–4. CLI output: skeleton data + basic globe PNG. No GUI.

**Deliverable:** `ymir generate --star "Tau Ceti" --seed 42` produces a file on disk and a Mollweide projection PNG.

### Phase 2 — Climate, biomes, and override system
Implement stages 5–6. Implement `Sourced<T>`, override file parsing, dependency graph, and `regenerate` command. Earth validation test passes. Mars validation test passes. Tidally locked test passes. Override progression test passes.

**Deliverable:** Globe PNGs with biome coloring that look recognizably different for Earth-like vs. Mars-like vs. tidally locked worlds. `ymir regenerate` with new overrides recomputes only dirty stages.

### Phase 3 — Regional detail
Implement stage 7. Rivers, lakes, fine-grained biome boundaries.

**Deliverable:** `ymir detail` command produces high-res hex-grid regional maps.

### Phase 4 — Catalog, GUI, and confidence visualization
Implement Gaia DR3 Parquet ingest, star browser UI, system view, globe view, detail view. Implement confidence overlay rendering (observed vs. derived field coloring). Provenance report panel in GUI.

**Deliverable:** Interactive application: browse stars, generate planets, explore surfaces. Confidence overlay shows where the model is observationally constrained vs. procedurally generated.

### Phase 5 — Data-derived weights
Build offline pipeline to ingest WWF ecoregions → Markov transition matrices. Ingest USGS Mars geologic map. Ship as weight files.

### Phase 6 — 3D globe rendering
wgpu-based 3D sphere rendering with texture-mapped biome/elevation data. Camera controls for orbit and zoom.

### Phase 7 — crates.io publication and community release
Finalize public APIs, write per-crate READMEs with standalone examples, generate rustdoc, publish all library crates to crates.io, archive release to Zenodo for DOI, announce to relevant communities (r/rust, r/proceduralgeneration, r/worldbuilding, astrobiology mailing lists).

**Deliverable:** All library crates on crates.io with docs. `CITATION.cff` with Zenodo DOI. Binary installable via `cargo install ymir`.

---

## 12. Open Questions

1. **Atmosphere composition model fidelity.** The v1 simplified model (mass → retention → composition) skips atmospheric evolution (outgassing, photolysis, biological O₂ production). How much does this matter for surface biome realism? Probably acceptable for v1 since the biome palette switch (O₂-bearing vs. not) is the dominant effect.

2. **Tidally locked climate transport.** The substellar→antistellar heat redistribution depends heavily on atmosphere thickness. Thin atmospheres → extreme day/night contrast. Thick atmospheres → more uniform. The simple angular model in v1 doesn't capture this well. A future improvement is a 2D energy balance model with atmospheric heat transport.

3. **Continental fraction derivation.** No established physics for deriving land/ocean ratio from planetary properties. Water inventory correlates with formation distance and system age, but the relationship is stochastic. For v1: parameterized with a prior centered on 30% (Earth-like) with wide variance.

4. **Biological O₂ switch.** Whether a planet has biology (and thus O₂ and vegetation biomes) is the biggest single determinant of surface appearance, and it's fundamentally unknowable. For v1: user-configurable boolean with a default heuristic (planet in HZ for > 3 Gyr → probable biology, per Dole's estimate).

5. **Gaia DR3 data access.** The full catalog is ~1TB. Need to determine the best approach for building the working subset: pre-download and filter via Python script, or query the Gaia TAP service at build time? Recommend pre-download for reproducibility.

6. **Validation against SpaceEngine.** SpaceEngine's procedural planets for known stars could serve as a visual comparison target. Not for correctness (their model is different) but for "does this look reasonable to someone familiar with procedural planets."

7. **Performance budget.** Target: skeleton generation < 1 second for Earth-sized planet on modern hardware. Detail chunk generation < 100ms. Star catalog query < 10ms. These targets should be validated in Phase 1.

8. **Name.** ~~"Ymir" is a placeholder. Needs trademark search.~~ **Resolved.** Ymir — from Norse mythology, the primordial giant whose body was dismembered to create the world (flesh → earth, blood → seas, bones → mountains, skull → sky). Available on crates.io. No trademark conflicts in scientific/simulation software. Crate prefix: `ymir-core`, `ymir-catalog`, `ymir-climate`, etc.

9. **Override granularity.** Should overrides be per-field (current design) or per-stage (simpler)? Per-field is more powerful — you can override `atmosphere.composition` while leaving `atmosphere.surface_pressure` derived — but it complicates the dependency graph. If field A and field B of the same stage have different sources, the stage needs partial recomputation logic. Per-stage is simpler: either the whole stage output is observed or derived. Recommendation: start per-stage, add per-field in a later version if needed.

10. **Override conflict resolution.** What if an override is physically inconsistent with upstream data? E.g., user overrides mass to 0.1 M⊕ but also overrides atmosphere to include dense H₂ (which a 0.1 M⊕ planet can't retain). Options: warn and proceed (user's data takes precedence), reject with error, or flag inconsistency in provenance report. Recommendation: warn + flag in provenance, never silently override physics.

11. **Automated override ingest.** Could the system watch the NASA Exoplanet Archive for updates and automatically generate override files for newly announced planets? This is a Phase 5+ feature but the override file format should be designed to support machine generation from day one.

12. **Version diffing.** The `ymir diff` command needs to compare two world states meaningfully. Simple binary diff is useless. Need semantic diff: "atmosphere changed from CO₂-dominated to N₂/O₂; this caused biome palette switch from abiotic to Earth-like; 73% of tiles changed biome." This requires the provenance system to track causal chains.

13. **Cross-crate type sharing.** The strict "no downstream type leakage" rule may create friction. If `ymir-climate` needs `SkeletonTile` from `ymir-surface`, it either takes a trait object or duplicates the type. Recommendation: define shared traits in `ymir-core` (e.g., `trait GeoTile { fn lat(&self) -> f64; fn elevation(&self) -> f64; }`) so crates depend on abstractions, not concrete types from sibling crates.

14. **Python bindings.** The scientific community overwhelmingly uses Python. PyO3 bindings for key crates (`ymir-catalog`, `ymir-atmosphere`, `ymir-climate`) would dramatically increase adoption. Not v1 scope, but the public API should be designed with FFI-friendliness in mind: avoid complex generics in public signatures, prefer concrete types, provide JSON serialization for all outputs.

15. **WASM targets.** `ymir-core`, `ymir-catalog` (without Parquet, using JSON star data instead), `ymir-system`, and `ymir-atmosphere` should compile to `wasm32-unknown-unknown` for browser use. This means no filesystem access in their public APIs — all I/O goes through trait boundaries. Test this early: add `cargo build --target wasm32-unknown-unknown` to CI for applicable crates.
