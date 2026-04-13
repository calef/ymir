//! Ymir: a causal star-to-surface planet simulator grounded in real astronomical data.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use ymir_atmosphere::AtmosphereModel;
use ymir_biome::{BiomeMap, BiomeMapConfig};
use ymir_catalog::exoplanets::ExoplanetRecord;
use ymir_catalog::sol::sol_context;
use ymir_catalog::star_context::StarContext;
use ymir_climate::{ClimateConfig, ClimateMap};
use ymir_core::{
    OverrideFile, PipelineDirtyState, ProvenanceReport, Source, Stage, StageOverrides, WorldRng,
    stable_derive_seed,
};
use ymir_detail::{RegionSpec, RegionalDetail, RegionalDetailConfig};
use ymir_render::biome_mollweide::{BiomeRenderConfig, render_biome_mollweide};
use ymir_render::globe_renderer::{GlobeRenderConfig, render_skeleton_mollweide_to_path};
use ymir_render::overlays::{BIOME_OFF_MAP_BG, render_confidence_from_report};
use ymir_render::regional::{RegionalRenderConfig, render_regional_detail};
use ymir_storage::manifest::{GenerationConfig, WorldManifest};
use ymir_storage::world_io::WorldDirectory;
use ymir_storage::{load_bin, load_provenance, save_bin, save_provenance};
use ymir_surface::skeleton::SkeletonWorld;
use ymir_system::{OrbitalBody, PlacementConfig, PlanetType, derive_body, place_planets};

#[derive(Parser)]
#[command(
    name = "ymir",
    version,
    about = "Causal star-to-surface planet simulator"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new world from a host star and seed.
    Generate {
        /// Host star name (only "Tau Ceti" is supported in Phase 1).
        #[arg(long)]
        star: String,
        /// Zero-based planet index within the system.
        #[arg(long, default_value_t = 0)]
        planet: usize,
        /// PRNG seed controlling all randomness.
        #[arg(long)]
        seed: u64,
        /// Output directory for the world (will be created).
        #[arg(long)]
        output: PathBuf,
        /// Geodesic grid subdivision level.
        #[arg(long, default_value_t = 5)]
        subdivision: u32,
        /// Disable biological processes (no free O2 production).
        #[arg(long)]
        no_biology: bool,
        /// Preview PNG width in pixels.
        #[arg(long, default_value_t = 1024)]
        preview_width: u32,
        /// Preview PNG height in pixels.
        #[arg(long, default_value_t = 512)]
        preview_height: u32,
        /// Halt generation after the skeleton stage (skip climate and biomes).
        /// Implies `--skip-biomes`.
        #[arg(long)]
        skip_climate: bool,
        /// Halt generation after the climate stage (skip biomes + biome preview).
        #[arg(long)]
        skip_biomes: bool,
    },
    /// Display information about a generated world.
    Info {
        /// Path to the world directory.
        path: PathBuf,
    },
    /// Generate regional detail for a single skeleton tile and persist to
    /// `<world>/detail/region_NNNN.bin`.
    ///
    /// Runs the DET-02..06 pipeline (hex grid, detail elevation, orographic
    /// moisture, flow routing, biome refinement) for the seed tile plus
    /// `radius` rings of neighbouring tiles. Updates the manifest's
    /// `regions_generated` list and optionally writes a region preview PNG.
    Detail {
        /// Path to the world directory (must contain manifest.json + the
        /// skeleton / climate / biome artifacts).
        #[arg(long)]
        world: PathBuf,
        /// Tile index (into `world.grid.tiles`) to use as the region's seed
        /// tile.
        #[arg(long)]
        region: u32,
        /// Number of ring-expansions of neighbouring tiles to include.
        #[arg(long, default_value_t = 1)]
        radius: u32,
        /// Optional PNG output path. If supplied, renders a regional PNG
        /// alongside the `.bin` save.
        #[arg(long)]
        output_image: Option<PathBuf>,
        /// Optional seed override for the detail stage's PRNG. Default is a
        /// stable derivation from `(world_seed, tile_index)` so repeated
        /// runs against the same world produce byte-identical regions.
        #[arg(long)]
        seed: Option<u64>,
    },
    /// List stars from the catalog matching optional spectral/distance/HZ filters.
    ///
    /// Loads the Gaia DR3 Parquet + NASA Exoplanet Archive CSV from
    /// `--catalog-dir` (default `data/catalog`). If the real catalog files
    /// are not on disk, falls back to the committed fixtures at
    /// `crates/ymir-catalog/fixtures/gaia_sample.parquet` +
    /// `crates/ymir-catalog/fixtures/exoplanet_sample.csv` so the command is
    /// always runnable from a fresh checkout.
    ListStars {
        /// Directory holding `gaia_dr3_100pc.parquet` and
        /// `exoplanet_archive.csv`. Falls back to the committed fixtures
        /// in `crates/ymir-catalog/fixtures/` if the real files are absent.
        #[arg(long, default_value = "data/catalog")]
        catalog_dir: PathBuf,
        /// Filter to a single Harvard spectral class letter (O/B/A/F/G/K/M).
        #[arg(long)]
        spectral_type: Option<String>,
        /// Maximum distance in parsecs (inclusive).
        #[arg(long)]
        within_pc: Option<f64>,
        /// Restrict to stars flagged as hosting at least one known HZ planet.
        #[arg(long)]
        has_planets: bool,
        /// Maximum number of rows to print (sorted by distance ascending).
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Print the full StarContext (catalog scalars, HZ edges, known planets)
    /// for a single star resolved by common name or Gaia DR3 source ID.
    ///
    /// Uses the same `--catalog-dir` fallback rule as `list-stars`. "Sol" /
    /// "Sun" resolve to a synthesized solar `StarContext` without touching
    /// the catalog.
    DescribeStar {
        /// Common name or Gaia source ID, e.g. "Epsilon Eridani", "Sol",
        /// or "5164707970261890560".
        query: String,
        /// Directory holding `gaia_dr3_100pc.parquet` and
        /// `exoplanet_archive.csv`. Falls back to the committed fixtures
        /// in `crates/ymir-catalog/fixtures/` if the real files are absent.
        #[arg(long, default_value = "data/catalog")]
        catalog_dir: PathBuf,
    },
    /// Print the provenance report for a generated world.
    ///
    /// Reads `<world>/provenance.json` (written by `ymir generate` and
    /// refreshed by `ymir regenerate`). With `--summary` (default) prints a
    /// per-stage histogram showing how many fields carry each source tag.
    /// With `--full` pretty-prints the complete JSON report.
    Provenance {
        /// Path to the world directory (must contain `provenance.json`).
        #[arg(long)]
        world: PathBuf,
        /// Print the full JSON dump instead of the per-stage histogram.
        #[arg(long, conflicts_with = "summary")]
        full: bool,
        /// Print the per-stage histogram (default behaviour).
        #[arg(long, conflicts_with = "full")]
        summary: bool,
    },
    /// Recompute dirty stages of an existing world after applying an override file.
    ///
    /// Given a world directory produced by `ymir generate` and a per-stage
    /// override JSON file, this command loads the manifest, determines which
    /// pipeline stages are downstream of the override (via CORE-05's
    /// dependency graph), recomputes only those dirty stages while re-using
    /// clean stage artifacts verbatim, and rewrites the manifest.
    Regenerate {
        /// Path to the existing world directory (must contain manifest.json).
        #[arg(long)]
        world: PathBuf,
        /// Path to the override JSON file (see design doc section 4.1).
        #[arg(long)]
        overrides: PathBuf,
        /// Preview PNG width in pixels (used when rebuilding previews).
        #[arg(long, default_value_t = 1024)]
        preview_width: u32,
        /// Preview PNG height in pixels (used when rebuilding previews).
        #[arg(long, default_value_t = 512)]
        preview_height: u32,
    },
    /// Authoring helpers for `overrides.json` — add, remove, list, and
    /// validate per-field overrides without editing JSON by hand.
    ///
    /// After editing overrides with these commands, run `ymir regenerate
    /// --world PATH --overrides <world>/overrides.json` to propagate them.
    Override {
        #[command(subcommand)]
        action: OverrideAction,
    },
    /// Re-render a generated world's preview images without re-running the
    /// pipeline.
    ///
    /// Reads the biome and skeleton artifacts from the world directory and
    /// writes one or more PNG previews. Useful for regenerating the confidence
    /// overlay after changing overrides, or for producing a higher-resolution
    /// PNG without re-simulating the world.
    ///
    /// Available modes: `biome`, `elevation`, `confidence`.
    Render {
        /// Path to the world directory (must contain `manifest.json`,
        /// `skeleton.bin`, `biomes.bin`, and `provenance.json`).
        #[arg(long)]
        world: PathBuf,
        /// Render mode: `biome` (default), `elevation`, or `confidence`.
        ///
        /// `confidence` desaturates each pixel proportionally to how much
        /// of the body's upstream data is observationally grounded.
        #[arg(long, default_value = "biome")]
        mode: String,
        /// Output PNG path. Defaults to `<world>/preview_<mode>.png`.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Output image width in pixels.
        #[arg(long, default_value_t = 2048)]
        width: u32,
        /// Output image height in pixels.
        #[arg(long, default_value_t = 1024)]
        height: u32,
    },
}

/// Subcommands for `ymir override`.
#[derive(Subcommand)]
enum OverrideAction {
    /// Set a per-field override in `<world>/overrides.json`.
    ///
    /// Creates the file if it does not exist. Field paths use dot notation
    /// to address a specific `Sourced<T>` field within a pipeline stage,
    /// e.g. `orbital_body.radius` or `atmosphere.surface_pressure`.
    ///
    /// Aggregate stages (skeleton, climate, biome) do not carry per-field
    /// provenance and therefore cannot be targeted with this command; use
    /// a JSON override file directly with `ymir regenerate --overrides` for
    /// those.
    Add {
        /// Path to the world directory (must contain `manifest.json`).
        #[arg(long)]
        world: PathBuf,
        /// Dot-separated field path, e.g. `orbital_body.radius`.
        #[arg(long)]
        field: String,
        /// New value for the field (parsed as JSON; strings need quoting).
        #[arg(long)]
        value: String,
        /// Optional unit annotation stored in the provenance comment
        /// (e.g. `R_earth`). Stored as metadata only — the pipeline uses
        /// the numeric value directly.
        #[arg(long)]
        unit: Option<String>,
        /// Bibliographic reference for the observational value
        /// (e.g. `"Gilbert+ 2023"`).
        #[arg(long, default_value = "user override")]
        reference: String,
        /// Instrument or method used to obtain the value
        /// (e.g. `"TESS"`).
        #[arg(long, default_value = "ymir override add")]
        instrument: String,
    },
    /// Remove a per-field override from `<world>/overrides.json`.
    ///
    /// If the stage section becomes empty after removing the field, the
    /// entire stage key is dropped. Removing the last field from every
    /// stage produces a valid but empty overrides file (which is a no-op
    /// for `ymir regenerate`).
    Remove {
        /// Path to the world directory.
        #[arg(long)]
        world: PathBuf,
        /// Dot-separated field path to remove, e.g. `orbital_body.radius`.
        #[arg(long)]
        field: String,
    },
    /// Print all overrides currently stored in `<world>/overrides.json`.
    ///
    /// Displays each override as `<stage>.<field> = <value>` with its
    /// provenance reference and instrument. If the file does not exist,
    /// reports that no overrides are set.
    List {
        /// Path to the world directory.
        #[arg(long)]
        world: PathBuf,
    },
    /// Validate that a field path resolves to a real `Sourced<T>` field.
    ///
    /// Reads `<world>/provenance.json` and checks that the given path
    /// exists as a `{ value, source }` leaf in the per-field stages
    /// (stellar, orbital_body, atmosphere). Aggregate stages (skeleton,
    /// climate, biome) are always rejected because they carry no per-field
    /// tree.
    ///
    /// Exits 0 if the path is valid, non-zero otherwise.
    Validate {
        /// Path to the world directory (must contain `provenance.json`).
        #[arg(long)]
        world: PathBuf,
        /// Dot-separated field path to validate, e.g. `orbital_body.radius`.
        #[arg(long)]
        field: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Generate {
            star,
            planet,
            seed,
            output,
            subdivision,
            no_biology,
            preview_width,
            preview_height,
            skip_climate,
            skip_biomes,
        } => {
            // --skip-climate implies --skip-biomes; you can't compute biomes
            // without a climate field.
            let effective_skip_biomes = skip_biomes || skip_climate;
            run_generate(&GenerateArgs {
                star,
                planet,
                seed,
                output,
                subdivision,
                enable_biology: !no_biology,
                preview_width,
                preview_height,
                skip_climate,
                skip_biomes: effective_skip_biomes,
            })
        }
        Commands::Info { path } => run_info(&path),
        Commands::Detail {
            world,
            region,
            radius,
            output_image,
            seed,
        } => run_detail(&DetailArgs {
            world,
            region,
            radius,
            output_image,
            seed,
        }),
        Commands::Regenerate {
            world,
            overrides,
            preview_width,
            preview_height,
        } => run_regenerate(&RegenerateArgs {
            world,
            overrides,
            preview_width,
            preview_height,
        }),
        Commands::ListStars {
            catalog_dir,
            spectral_type,
            within_pc,
            has_planets,
            limit,
        } => run_list_stars(&ListStarsArgs {
            catalog_dir,
            spectral_type,
            within_pc,
            has_planets,
            limit,
        }),
        Commands::DescribeStar { query, catalog_dir } => {
            run_describe_star(&DescribeStarArgs { query, catalog_dir })
        }
        Commands::Provenance {
            world,
            full,
            summary: _,
        } => run_provenance(&world, full),
        Commands::Override { action } => match action {
            OverrideAction::Add {
                world,
                field,
                value,
                unit,
                reference,
                instrument,
            } => run_override_add(&OverrideAddArgs {
                world,
                field,
                value,
                unit,
                reference,
                instrument,
            }),
            OverrideAction::Remove { world, field } => run_override_remove(&world, &field),
            OverrideAction::List { world } => run_override_list(&world),
            OverrideAction::Validate { world, field } => run_override_validate(&world, &field),
        },
        Commands::Render {
            world,
            mode,
            output,
            width,
            height,
        } => run_render(&RenderArgs {
            world,
            mode,
            output,
            width,
            height,
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // `describe-star` uses a `__nomatch__` sentinel prefix so the
            // user-facing "No match for '...'" message lands on stderr
            // without being prefixed by the generic `error: ` banner.
            if let Some(msg) = err.strip_prefix("__nomatch__") {
                eprintln!("{msg}");
            } else {
                eprintln!("error: {err}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Resolved arguments for the `generate` subcommand.
struct GenerateArgs {
    star: String,
    planet: usize,
    seed: u64,
    output: PathBuf,
    subdivision: u32,
    enable_biology: bool,
    preview_width: u32,
    preview_height: u32,
    /// Skip the climate stage (and therefore biomes).
    skip_climate: bool,
    /// Skip the biomes stage (and biome preview render).
    skip_biomes: bool,
}

/// Resolved star/body selection. `Derived` goes through placement and bulk
/// property derivation; `Fixed` bypasses those stages and uses hand-filled
/// observational values for Solar-system references (Earth, Mars).
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum StarSelection {
    /// Tau Ceti path: derive the chosen body via placement + `derive_body`.
    Derived {
        star: StarContext,
        known: Vec<ExoplanetRecord>,
    },
    /// Solar-system reference path: use a hand-filled [`OrbitalBody`] directly
    /// so validation tests compare against known-good values rather than
    /// a re-derivation from the placement sampler.
    Fixed {
        star: StarContext,
        body: OrbitalBody,
    },
}

/// Hand-filled [`OrbitalBody`] for Earth using published observational values.
/// Used when `--star Earth` (or `Sol`/`Sun`) is passed. We bypass
/// `place_planets` + `derive_body` here because we want ground-truth values,
/// not a statistical re-derivation from the placement sampler.
fn earth_body() -> OrbitalBody {
    let obs = |v: f64| ymir_core::Sourced::observed(v, "IAU / NASA planetary fact sheet", "Earth");
    OrbitalBody {
        semi_major_axis: obs(1.0),
        eccentricity: obs(0.0167),
        inclination: obs(0.0),
        axial_tilt: obs(23.4),
        mass: obs(1.0),
        radius: obs(1.0),
        density: obs(5.51),
        surface_gravity: obs(9.81),
        solar_irradiance: obs(1361.0),
        equilibrium_temp: obs(254.0),
        tidal_locked: ymir_core::Sourced::observed(
            false,
            "IAU / NASA planetary fact sheet",
            "Earth",
        ),
        rotation_period: obs(24.0),
        is_in_hz: true,
        planet_type: PlanetType::Terran,
        name: Some("Earth".to_string()),
        is_known_exoplanet: false,
        continental_fraction: Some(ymir_core::Sourced::observed(
            0.29,
            "Earth hypsometric curve (ETOPO1, NOAA NGDC 2009)",
            "global bathymetry + topography",
        )),
    }
}

/// Hand-filled [`OrbitalBody`] for Mars using published observational values.
/// Used when `--star Mars` is passed. Like [`earth_body`], bypasses the
/// placement + derivation stages to preserve ground-truth values.
fn mars_body() -> OrbitalBody {
    let obs = |v: f64| ymir_core::Sourced::observed(v, "IAU / NASA planetary fact sheet", "Mars");
    OrbitalBody {
        semi_major_axis: obs(1.524),
        eccentricity: obs(0.0934),
        inclination: obs(0.0),
        axial_tilt: obs(25.19),
        mass: obs(0.107),
        radius: obs(0.532),
        density: obs(3.93),
        surface_gravity: obs(3.72),
        solar_irradiance: obs(588.0),
        equilibrium_temp: obs(210.0),
        tidal_locked: ymir_core::Sourced::observed(
            false,
            "IAU / NASA planetary fact sheet",
            "Mars",
        ),
        rotation_period: obs(24.6),
        is_in_hz: false,
        planet_type: PlanetType::Terran,
        name: Some("Mars".to_string()),
        is_known_exoplanet: false,
        continental_fraction: Some(ymir_core::Sourced::observed(
            1.0,
            "Mars topography (MOLA, NASA MGS 2001)",
            "Mars Orbiter Laser Altimeter",
        )),
    }
}

/// Look up a star selection by case-insensitive, trimmed name.
///
/// Supported:
/// - "Tau Ceti" -> derived pipeline (placement + bulk-property derivation).
/// - "Earth" / "Sol" / "Sun" -> Sol star with hand-filled Earth body.
/// - "Mars" -> Sol star with hand-filled Mars body.
fn lookup_star(name: &str) -> Result<StarSelection, String> {
    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "tau ceti" => Ok(StarSelection::Derived {
            star: StarContext::tau_ceti(),
            known: ExoplanetRecord::tau_ceti_system(),
        }),
        "earth" | "sol" | "sun" => Ok(StarSelection::Fixed {
            star: sol_context(),
            body: earth_body(),
        }),
        "mars" => Ok(StarSelection::Fixed {
            star: sol_context(),
            body: mars_body(),
        }),
        _ => Err(format!(
            "unknown star \"{name}\" (supported: \"Tau Ceti\", \"Earth\", \"Sol\", \"Sun\", \"Mars\")"
        )),
    }
}

/// Compute stages 1-3 (stellar context + planetary system + atmosphere),
/// applying any per-stage overrides from `overrides`.
///
/// Returns the resolved `(star_ctx, body, atmosphere)` tuple. This is the
/// shared upstream path used by both `run_generate` and `run_regenerate`.
fn compute_upstream(
    star_name: &str,
    planet_index: usize,
    seed: u64,
    enable_biology: bool,
    overrides: &StageOverrides,
) -> Result<(StarContext, OrbitalBody, AtmosphereModel), String> {
    let selection = lookup_star(star_name)?;

    let (star_ctx, mut body) = match selection {
        StarSelection::Derived { star, known } => {
            let mut placement_rng = WorldRng::new(seed).child("placement");
            let placed = place_planets(
                &star,
                &known,
                &mut placement_rng,
                &PlacementConfig::default(),
            );
            if placed.is_empty() {
                return Err("placement produced no planets".to_string());
            }
            if planet_index >= placed.len() {
                return Err(format!(
                    "planet index {} out of range (system has {} planets)",
                    planet_index,
                    placed.len()
                ));
            }
            let chosen = &placed[planet_index];
            let mut body_rng = WorldRng::new(seed).child("body");
            let derived = derive_body(chosen, &star, &mut body_rng, planet_index as u64);
            (star, derived)
        }
        StarSelection::Fixed { star, body } => {
            if planet_index != 0 {
                return Err(format!(
                    "--planet must be 0 for hand-filled Sol-system bodies (got {planet_index}); \
                     Earth and Mars are returned as single fixed bodies"
                ));
            }
            (star, body)
        }
    };

    if let Some(ov) = &overrides.orbital_body {
        body = apply_json_override(&body, ov)
            .map_err(|e| format!("failed to apply orbital_body override: {e}"))?;
    }

    let mut atmosphere = AtmosphereModel::derive(&body, &star_ctx, enable_biology);
    if let Some(ov) = &overrides.atmosphere {
        atmosphere = apply_json_override(&atmosphere, ov)
            .map_err(|e| format!("failed to apply atmosphere override: {e}"))?;
    }

    Ok((star_ctx, body, atmosphere))
}

/// Compute the skeleton stage (stage 4), applying any skeleton override.
fn compute_skeleton(
    body: OrbitalBody,
    atmosphere: AtmosphereModel,
    subdivision: u32,
    seed: u64,
    overrides: &StageOverrides,
) -> Result<SkeletonWorld, String> {
    let mut skeleton = SkeletonWorld::build(body, atmosphere, subdivision, seed);
    if let Some(ov) = &overrides.skeleton {
        skeleton = apply_json_override(&skeleton, ov)
            .map_err(|e| format!("failed to apply skeleton override: {e}"))?;
    }
    Ok(skeleton)
}

/// Compute the climate stage (stage 5), applying any climate override.
fn compute_climate(
    skeleton: &SkeletonWorld,
    overrides: &StageOverrides,
) -> Result<ClimateMap, String> {
    let mut climate = ClimateMap::build(skeleton, &ClimateConfig::default());
    if let Some(ov) = &overrides.climate {
        climate = apply_json_override(&climate, ov)
            .map_err(|e| format!("failed to apply climate override: {e}"))?;
    }
    Ok(climate)
}

/// Compute the biome stage (stage 6), applying any biome override.
fn compute_biomes(
    skeleton: &SkeletonWorld,
    climate: &ClimateMap,
    overrides: &StageOverrides,
) -> Result<BiomeMap, String> {
    let mut biomes = BiomeMap::build(skeleton, climate, &BiomeMapConfig::default());
    if let Some(ov) = &overrides.biome {
        biomes = apply_json_override(&biomes, ov)
            .map_err(|e| format!("failed to apply biome override: {e}"))?;
    }
    Ok(biomes)
}

/// Apply a partial JSON override to a serde-serializable value by merging
/// the override's object fields onto a JSON view of `base`, then
/// deserializing back.
///
/// This is the per-stage override merge strategy for Phase 2: compute the
/// stage normally, then overlay any user-supplied fields. The override JSON
/// is expected to be an object; scalar, array, or null overrides replace
/// wholesale only at the outermost level.
fn apply_json_override<T>(base: &T, override_value: &serde_json::Value) -> Result<T, String>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let mut base_json = serde_json::to_value(base).map_err(|e| format!("serialize base: {e}"))?;
    merge_json(&mut base_json, override_value);
    serde_json::from_value(base_json).map_err(|e| format!("deserialize merged: {e}"))
}

/// Recursively merge `patch` into `target`. Object members are merged key by
/// key; everything else is replaced wholesale. This mirrors the common
/// "deep-merge JSON" pattern and keeps override files terse (callers supply
/// only the fields they want to change).
///
/// `Sourced<T>` shim: if the target is a two-key object with `value` +
/// `source` (the serialized shape of `Sourced<T>`) and the patch is a bare
/// scalar/array/null, the patch is re-shaped into
/// `{value: <patch>, source: Observed{reference: "user override", ...}}` so
/// the override automatically flips the field's provenance tag to Observed.
fn merge_json(target: &mut serde_json::Value, patch: &serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(t), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                merge_json(t.entry(k.clone()).or_insert(serde_json::Value::Null), v);
            }
        }
        (t, p) => {
            if is_sourced_shape(t) && !matches!(p, serde_json::Value::Object(_)) {
                // Wrap the scalar patch into the Sourced envelope and tag
                // the source as user-supplied Observed.
                let wrapped = make_user_observed_sourced_json(p.clone());
                *t = wrapped;
            } else {
                *t = p.clone();
            }
        }
    }
}

/// Returns true if `v` is a JSON object matching the serialized `Sourced<T>`
/// shape: exactly the keys `value` and `source`.
fn is_sourced_shape(v: &serde_json::Value) -> bool {
    let serde_json::Value::Object(map) = v else {
        return false;
    };
    map.len() == 2 && map.contains_key("value") && map.contains_key("source")
}

/// Build a JSON value representing `Sourced { value, source: Observed { .. } }`
/// with provenance tagged as a user override.
fn make_user_observed_sourced_json(value: serde_json::Value) -> serde_json::Value {
    let source = serde_json::json!({
        "Observed": {
            "reference": "user override",
            "instrument": "ymir CLI --override",
            "date": "",
            "uncertainty": null,
        }
    });
    serde_json::json!({
        "value": value,
        "source": source,
    })
}

/// Render the elevation preview PNG.
fn render_elevation_preview(
    wd: &WorldDirectory,
    skeleton: &SkeletonWorld,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let render_cfg = GlobeRenderConfig {
        width,
        height,
        background: [0, 0, 0],
    };
    let preview_path = wd.root.join("preview.png");
    render_skeleton_mollweide_to_path(skeleton, &render_cfg, &preview_path)
        .map_err(|e| format!("failed to write preview {}: {}", preview_path.display(), e))
}

/// Render the biome preview PNG.
fn render_biome_preview(
    wd: &WorldDirectory,
    skeleton: &SkeletonWorld,
    biomes: &BiomeMap,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let biome_cfg = BiomeRenderConfig { width, height };
    let biome_preview_path = wd.root.join("preview_biome.png");
    let img = render_biome_mollweide(skeleton, biomes, &biome_cfg);
    img.save(&biome_preview_path).map_err(|e| {
        format!(
            "failed to write biome preview {}: {}",
            biome_preview_path.display(),
            e
        )
    })
}

/// Render the confidence overlay preview PNG.
///
/// Composites the confidence desaturation wash onto a freshly-rendered biome
/// base image and writes `preview_confidence.png` to the world directory.
///
/// The confidence level is derived from `report` (body-level provenance from
/// the stellar, orbital_body, and atmosphere stages). In Phase 1 this is a
/// uniform body-level wash; per-tile gradients are deferred to Phase 2.
fn render_confidence_preview(
    wd: &WorldDirectory,
    skeleton: &SkeletonWorld,
    biomes: &BiomeMap,
    report: &ProvenanceReport,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let biome_cfg = BiomeRenderConfig { width, height };
    let base = render_biome_mollweide(skeleton, biomes, &biome_cfg);
    let confidence_img = render_confidence_from_report(&base, report, BIOME_OFF_MAP_BG);
    let out_path = wd.root.join("preview_confidence.png");
    confidence_img.save(&out_path).map_err(|e| {
        format!(
            "failed to write confidence preview {}: {}",
            out_path.display(),
            e
        )
    })
}

/// Resolved arguments for the `render` subcommand.
struct RenderArgs {
    world: PathBuf,
    mode: String,
    output: Option<PathBuf>,
    width: u32,
    height: u32,
}

/// Re-render a world's preview PNG without re-running the pipeline.
///
/// Reads existing artifacts from the world directory and writes a PNG.
/// Mode `biome` → `preview_biome.png`, `elevation` → `preview.png`,
/// `confidence` → `preview_confidence.png` (or the path given by `--output`).
fn run_render(args: &RenderArgs) -> Result<(), String> {
    let wd = WorldDirectory {
        root: args.world.clone(),
    };
    if !wd.manifest_path().exists() {
        return Err(format!(
            "manifest.json not found in {} (is this a ymir world directory?)",
            args.world.display()
        ));
    }

    let mode = args.mode.as_str();
    match mode {
        "elevation" => {
            if !wd.skeleton_path().exists() {
                return Err(format!(
                    "skeleton.bin missing from {} (run `ymir generate` first)",
                    args.world.display()
                ));
            }
            let skeleton = load_skeleton(&wd)?;
            let out = args
                .output
                .clone()
                .unwrap_or_else(|| wd.root.join("preview.png"));
            let cfg = GlobeRenderConfig {
                width: args.width,
                height: args.height,
                background: [0, 0, 0],
            };
            render_skeleton_mollweide_to_path(&skeleton, &cfg, &out)
                .map_err(|e| format!("failed to write {}: {}", out.display(), e))?;
            println!("Wrote elevation preview: {}", out.display());
        }
        "biome" => {
            if !wd.skeleton_path().exists() {
                return Err(format!(
                    "skeleton.bin missing from {} (run `ymir generate` first)",
                    args.world.display()
                ));
            }
            if !wd.biomes_path().exists() {
                return Err(format!(
                    "biomes.bin missing from {} (world was generated with --skip-biomes)",
                    args.world.display()
                ));
            }
            let skeleton = load_skeleton(&wd)?;
            let biomes = load_biomes(&wd)?;
            let out = args
                .output
                .clone()
                .unwrap_or_else(|| wd.root.join("preview_biome.png"));
            let cfg = BiomeRenderConfig {
                width: args.width,
                height: args.height,
            };
            let img = render_biome_mollweide(&skeleton, &biomes, &cfg);
            img.save(&out)
                .map_err(|e| format!("failed to write {}: {}", out.display(), e))?;
            println!("Wrote biome preview: {}", out.display());
        }
        "confidence" => {
            if !wd.skeleton_path().exists() {
                return Err(format!(
                    "skeleton.bin missing from {} (run `ymir generate` first)",
                    args.world.display()
                ));
            }
            if !wd.biomes_path().exists() {
                return Err(format!(
                    "biomes.bin missing from {} (world was generated with --skip-biomes)",
                    args.world.display()
                ));
            }
            if !wd.provenance_path().exists() {
                return Err(format!(
                    "provenance.json missing from {} (run `ymir generate` first)",
                    args.world.display()
                ));
            }
            let skeleton = load_skeleton(&wd)?;
            let biomes = load_biomes(&wd)?;
            let report = load_provenance(&wd.provenance_path()).map_err(|e| {
                format!(
                    "failed to load provenance.json from {}: {}",
                    args.world.display(),
                    e
                )
            })?;
            let out = args
                .output
                .clone()
                .unwrap_or_else(|| wd.root.join("preview_confidence.png"));
            let cfg = BiomeRenderConfig {
                width: args.width,
                height: args.height,
            };
            let base = render_biome_mollweide(&skeleton, &biomes, &cfg);
            let confidence_img = render_confidence_from_report(&base, &report, BIOME_OFF_MAP_BG);
            confidence_img
                .save(&out)
                .map_err(|e| format!("failed to write {}: {}", out.display(), e))?;
            println!("Wrote confidence preview: {}", out.display());
        }
        other => {
            return Err(format!(
                "unknown render mode '{other}': expected 'biome', 'elevation', or 'confidence'"
            ));
        }
    }

    Ok(())
}

/// Run the full Phase 1 pipeline and persist the resulting world.
fn run_generate(args: &GenerateArgs) -> Result<(), String> {
    let empty_overrides = StageOverrides::default();

    println!("[1/7] star context: {}", args.star);
    println!(
        "[2/7] system placement / body selection (seed={})...",
        args.seed
    );
    println!("[3/7] atmosphere (biology = {})...", args.enable_biology);
    let (star_ctx, body, atmosphere) = compute_upstream(
        &args.star,
        args.planet,
        args.seed,
        args.enable_biology,
        &empty_overrides,
    )?;

    println!("[4/7] skeleton (subdivision = {})...", args.subdivision);
    let skeleton = compute_skeleton(
        body.clone(),
        atmosphere.clone(),
        args.subdivision,
        args.seed,
        &empty_overrides,
    )?;

    println!("[5/7] persisting world to {}", args.output.display());
    let wd = WorldDirectory::create(&args.output).map_err(|e| {
        format!(
            "failed to create world dir {}: {}",
            args.output.display(),
            e
        )
    })?;
    save_skeleton(&wd, &skeleton)?;

    let mut stages_computed = vec![
        "stellar".to_string(),
        "system".to_string(),
        "atmosphere".to_string(),
        "skeleton".to_string(),
    ];

    println!(
        "[6/7] rendering elevation preview ({}x{})...",
        args.preview_width, args.preview_height
    );
    render_elevation_preview(&wd, &skeleton, args.preview_width, args.preview_height)?;

    // Phase 2 stages: climate + biomes. Run in order unless user opted out.
    let mut climate: Option<ClimateMap> = None;
    let mut biomes: Option<BiomeMap> = None;

    if !args.skip_climate {
        println!("[climate] building ClimateMap...");
        let c = compute_climate(&skeleton, &empty_overrides)?;
        save_climate(&wd, &c)?;
        stages_computed.push("climate".to_string());
        climate = Some(c);

        if !args.skip_biomes {
            println!("[biomes] building BiomeMap...");
            let climate_ref = climate.as_ref().expect("climate just computed");
            let b = compute_biomes(&skeleton, climate_ref, &empty_overrides)?;
            save_biomes(&wd, &b)?;
            stages_computed.push("biomes".to_string());

            println!(
                "[7/7] rendering biome preview ({}x{})...",
                args.preview_width, args.preview_height
            );
            render_biome_preview(&wd, &skeleton, &b, args.preview_width, args.preview_height)?;
            biomes = Some(b);
        }
    }

    let manifest = WorldManifest {
        version: "1.0".to_string(),
        pipeline_version: env!("CARGO_PKG_VERSION").to_string(),
        star_name: star_ctx.name.clone().unwrap_or_else(|| args.star.clone()),
        star_catalog_id: Some(star_ctx.catalog_id.clone()),
        planet_index: args.planet,
        planet_name: body.name.clone(),
        seed: args.seed,
        created_at: chrono::Utc::now().to_rfc3339(),
        overrides_file: None,
        stages_computed,
        config: GenerationConfig {
            grid_subdivision_level: args.subdivision,
            enable_biology: args.enable_biology,
            continental_fraction: body.continental_fraction.as_ref().map(|s| *s.inner()),
        },
        regions_generated: Vec::new(),
    };
    wd.save_manifest(&manifest)
        .map_err(|e| format!("failed to save manifest: {e}"))?;

    let provenance = build_provenance(
        &star_ctx,
        &skeleton.body,
        &skeleton.atmosphere,
        &skeleton,
        climate.as_ref(),
        biomes.as_ref(),
    )?;
    save_world_provenance(&wd, &provenance)?;

    // Render confidence overlay alongside the biome preview if biomes were
    // computed. Uses body-level provenance (Phase 1 limitation: no per-tile
    // confidence; a uniform wash is applied to every pixel).
    if let Some(b) = biomes.as_ref() {
        println!(
            "[confidence] rendering confidence overlay ({}x{})...",
            args.preview_width, args.preview_height
        );
        render_confidence_preview(
            &wd,
            &skeleton,
            b,
            &provenance,
            args.preview_width,
            args.preview_height,
        )?;
    }

    println!();
    print_summary(
        &star_ctx,
        args.planet,
        &skeleton,
        climate.as_ref(),
        biomes.as_ref(),
        &args.output,
    );
    Ok(())
}

/// Resolved arguments for the `regenerate` subcommand.
struct RegenerateArgs {
    world: PathBuf,
    overrides: PathBuf,
    preview_width: u32,
    preview_height: u32,
}

/// Compute the set of dirty stages from the override file via CORE-05's
/// dependency graph. Each override in `StageOverrides` targets a specific
/// pipeline stage; applying an override at stage N marks N and all downstream
/// stages dirty.
fn dirty_stages_from_overrides(overrides: &StageOverrides) -> PipelineDirtyState {
    let mut state = PipelineDirtyState::clean();
    if overrides.orbital_body.is_some() {
        state.mark_override_at(Stage::PlanetarySystem);
    }
    if overrides.atmosphere.is_some() {
        state.mark_override_at(Stage::Atmosphere);
    }
    if overrides.skeleton.is_some() {
        state.mark_override_at(Stage::Skeleton);
    }
    if overrides.climate.is_some() {
        state.mark_override_at(Stage::Climate);
    }
    if overrides.biome.is_some() {
        state.mark_override_at(Stage::Biome);
    }
    state
}

/// Recompute only the stages marked dirty by an override file, re-using
/// clean stage artifacts verbatim.
///
/// The flow:
/// 1. Load the existing manifest.
/// 2. Parse the override file.
/// 3. Compute the dirty-stage set via CORE-05's dependency graph.
/// 4. For each stage in order: recompute if dirty, otherwise load from disk.
/// 5. Persist newly-computed artifacts in place (clean files remain
///    byte-identical on disk since we don't touch them).
/// 6. Rewrite the manifest with the new `overrides_file` pointer.
fn run_regenerate(args: &RegenerateArgs) -> Result<(), String> {
    let wd = WorldDirectory {
        root: args.world.clone(),
    };
    if !wd.manifest_path().exists() {
        return Err(format!(
            "manifest.json not found in {} (is this a ymir world directory?)",
            args.world.display()
        ));
    }

    let mut manifest = wd.load_manifest().map_err(|e| {
        format!(
            "failed to load manifest at {}: {}",
            wd.manifest_path().display(),
            e
        )
    })?;

    let override_file = OverrideFile::load(&args.overrides).map_err(|e| {
        format!(
            "failed to load overrides {}: {}",
            args.overrides.display(),
            e
        )
    })?;

    let overrides = &override_file.overrides;
    let dirty = dirty_stages_from_overrides(overrides);
    let dirty_names: Vec<&str> = dirty.dirty_stages().iter().map(|s| s.name()).collect();
    println!(
        "regenerate: override target_star={}, dirty stages: [{}]",
        override_file.target_star,
        dirty_names.join(", ")
    );

    let star_name = &manifest.star_name;
    let seed = manifest.seed;
    let planet_index = manifest.planet_index;
    let subdivision = manifest.config.grid_subdivision_level;
    let enable_biology = manifest.config.enable_biology;

    // --- Upstream stages (1-3): stellar, system, atmosphere. --------------
    //
    // These all live inside skeleton.bin (there is no separate stellar.bin /
    // system.bin / atmosphere.bin in Phase 2). If none of stages 1-3 are
    // dirty AND skeleton itself is clean, we can skip recomputing them
    // entirely by loading skeleton.bin and extracting body + atmosphere.
    let need_upstream = dirty.is_dirty(Stage::PlanetarySystem)
        || dirty.is_dirty(Stage::Atmosphere)
        || dirty.is_dirty(Stage::Skeleton);

    let loaded_skeleton: Option<SkeletonWorld> = if !need_upstream
        || (!dirty.is_dirty(Stage::Skeleton)
            && !dirty.is_dirty(Stage::PlanetarySystem)
            && !dirty.is_dirty(Stage::Atmosphere))
    {
        // When skeleton itself is clean, just load it.
        Some(load_skeleton(&wd)?)
    } else {
        None
    };

    let skeleton = if let Some(s) = loaded_skeleton {
        println!("[skeleton] clean: loaded from disk");
        s
    } else {
        // Recompute upstream. lookup_star uses the manifest's recorded star
        // name so we recover the same StarContext. When only atmosphere is
        // dirty (stages 1-2 clean), we still rebuild body/atmosphere; that's
        // acceptable because the upstream types don't live on disk and their
        // outputs are deterministic given the manifest's seed+star+planet.
        let (_star_ctx, body, atmosphere) =
            compute_upstream(star_name, planet_index, seed, enable_biology, overrides)?;
        println!(
            "[skeleton] rebuilding (atmosphere class = {:?})...",
            atmosphere.class
        );
        let skel = compute_skeleton(body, atmosphere, subdivision, seed, overrides)?;
        save_skeleton(&wd, &skel)?;
        render_elevation_preview(&wd, &skel, args.preview_width, args.preview_height)?;
        skel
    };

    // --- Climate (stage 5). ------------------------------------------------
    let climate: Option<ClimateMap> = if manifest.stages_computed.iter().any(|s| s == "climate") {
        if dirty.is_dirty(Stage::Climate) {
            println!("[climate] dirty: recomputing...");
            let c = compute_climate(&skeleton, overrides)?;
            save_climate(&wd, &c)?;
            Some(c)
        } else {
            println!("[climate] clean: loading from disk");
            Some(load_climate(&wd)?)
        }
    } else {
        None
    };

    // --- Biomes (stage 6). -------------------------------------------------
    let biomes: Option<BiomeMap> = if manifest.stages_computed.iter().any(|s| s == "biomes") {
        if let Some(climate_ref) = climate.as_ref() {
            if dirty.is_dirty(Stage::Biome) {
                println!("[biomes] dirty: recomputing...");
                let b = compute_biomes(&skeleton, climate_ref, overrides)?;
                save_biomes(&wd, &b)?;
                render_biome_preview(&wd, &skeleton, &b, args.preview_width, args.preview_height)?;
                Some(b)
            } else {
                println!("[biomes] clean: loading from disk");
                Some(load_biomes(&wd)?)
            }
        } else {
            // Biomes recorded but no climate; shouldn't happen in practice.
            None
        }
    } else {
        None
    };

    // --- Manifest update. --------------------------------------------------
    //
    // `stages_computed` stays the same (we regenerate the same set of
    // stages the original `generate` produced). The new `overrides_file`
    // pointer records which override file drove this regeneration.
    manifest.overrides_file = Some(args.overrides.display().to_string());
    wd.save_manifest(&manifest)
        .map_err(|e| format!("failed to save manifest: {e}"))?;

    // Refresh provenance.json to reflect any source-tag changes from overrides.
    let provenance = build_provenance(
        // We don't have the StarContext readily available in regenerate (it's
        // not persisted standalone). Re-derive it from the star name recorded
        // in the manifest so provenance stays accurate.
        &lookup_star(star_name)
            .map(|sel| match sel {
                StarSelection::Derived { star, .. } => star,
                StarSelection::Fixed { star, .. } => star,
            })
            .unwrap_or_else(|_| sol_context()),
        &skeleton.body,
        &skeleton.atmosphere,
        &skeleton,
        climate.as_ref(),
        biomes.as_ref(),
    )?;
    save_world_provenance(&wd, &provenance)?;

    // Re-render confidence overlay so it reflects updated provenance after
    // overrides are applied.
    if let Some(b) = biomes.as_ref() {
        render_confidence_preview(
            &wd,
            &skeleton,
            b,
            &provenance,
            args.preview_width,
            args.preview_height,
        )?;
    }

    println!();
    println!(
        "Regenerate complete. Overrides applied: [{}]",
        dirty_names.join(", ")
    );
    println!("Manifest updated at {}", wd.manifest_path().display());
    println!(
        "Skeleton: {} tiles, atmosphere class {:?}",
        skeleton.elevation.elevations_m.len(),
        skeleton.atmosphere.class
    );
    if let Some(c) = climate.as_ref() {
        println!(
            "Climate: mean T {:.1} K, min {:.1}, max {:.1}",
            c.temperature.mean(),
            c.temperature.min(),
            c.temperature.max()
        );
    }
    if let Some(b) = biomes.as_ref() {
        println!("Biomes: {} tiles", b.len());
    }

    Ok(())
}

/// Resolved arguments for the `detail` subcommand.
struct DetailArgs {
    world: PathBuf,
    region: u32,
    radius: u32,
    output_image: Option<PathBuf>,
    seed: Option<u64>,
}

/// Run the regional detail pipeline for a single tile and persist the
/// resulting [`RegionalDetail`] to `<world>/detail/region_NNNN.bin`.
///
/// The seed for the detail PRNG defaults to
/// `stable_derive_seed(world_seed, tile_index)` so re-running the command
/// against the same world reproduces byte-identical region files. Callers
/// can override with `--seed` for experimentation.
fn run_detail(args: &DetailArgs) -> Result<(), String> {
    let wd = WorldDirectory {
        root: args.world.clone(),
    };
    if !wd.manifest_path().exists() {
        return Err(format!(
            "manifest.json not found in {} (is this a ymir world directory?)",
            args.world.display()
        ));
    }

    let mut manifest = wd.load_manifest().map_err(|e| {
        format!(
            "failed to load manifest at {}: {}",
            wd.manifest_path().display(),
            e
        )
    })?;

    if !wd.skeleton_path().exists() {
        return Err(format!(
            "skeleton.bin missing from {} (run `ymir generate` first)",
            args.world.display()
        ));
    }
    if !wd.climate_path().exists() {
        return Err(format!(
            "climate.bin missing from {} (world was generated with --skip-climate)",
            args.world.display()
        ));
    }
    if !wd.biomes_path().exists() {
        return Err(format!(
            "biomes.bin missing from {} (world was generated with --skip-biomes)",
            args.world.display()
        ));
    }

    let skeleton = load_skeleton(&wd)?;
    let climate = load_climate(&wd)?;
    let biomes = load_biomes(&wd)?;

    let tile_count = skeleton.grid.tiles.len();
    if (args.region as usize) >= tile_count {
        return Err(format!(
            "--region {} out of range (world has {} tiles)",
            args.region, tile_count
        ));
    }

    let resolved_seed = args
        .seed
        .unwrap_or_else(|| stable_derive_seed(manifest.seed, args.region as u64));

    let spec = RegionSpec::new(args.region, args.radius);
    let detail_cfg = RegionalDetailConfig {
        seed: resolved_seed,
        ..RegionalDetailConfig::default()
    };

    println!(
        "[detail] tile {} radius {} (seed = {:#x}, world seed = {})",
        args.region, args.radius, resolved_seed, manifest.seed
    );
    let region = RegionalDetail::build(&skeleton, &climate, &biomes, spec, detail_cfg);

    let path = save_region(&wd, &mut manifest, &region)?;
    wd.save_manifest(&manifest)
        .map_err(|e| format!("failed to save manifest: {e}"))?;
    println!("[detail] wrote {}", path.display());

    print_region_summary(&region);

    if let Some(img_path) = &args.output_image {
        println!("[detail] rendering preview to {}", img_path.display());
        let render_cfg = RegionalRenderConfig::default();
        let img = render_regional_detail(&region, &skeleton, &render_cfg);
        img.save(img_path).map_err(|e| {
            format!(
                "failed to write region preview {}: {}",
                img_path.display(),
                e
            )
        })?;
    }

    Ok(())
}

/// Print the human-readable summary for a freshly-built [`RegionalDetail`].
///
/// Histogram is sorted by hex count descending, ties broken by the
/// debug-formatted biome variant name for determinism.
fn print_region_summary(region: &RegionalDetail) {
    let n = region.hex_grid.cells.len();
    let elevs = &region.elevation.per_hex_m;

    let (mut min_e, mut max_e, mut sum_e) = (f64::INFINITY, f64::NEG_INFINITY, 0.0_f64);
    for &e in elevs {
        if e < min_e {
            min_e = e;
        }
        if e > max_e {
            max_e = e;
        }
        sum_e += e;
    }
    let mean_e = if n == 0 { 0.0 } else { sum_e / n as f64 };

    let river_threshold = ymir_detail::DetailBiomeConfig::default().river_flow_threshold;
    let river_count = region
        .flow
        .flow_accumulation
        .iter()
        .zip(region.flow.is_lake.iter())
        .filter(|&(&acc, &lake)| !lake && acc >= river_threshold)
        .count();
    let lake_count = region.flow.is_lake.iter().filter(|&&l| l).count();

    println!(
        "Region tile {} (radius {}): {} hexes",
        region.spec.tile_index, region.spec.radius_tiles, n
    );
    if n > 0 {
        println!("Elevation: min={min_e:.1} m, max={max_e:.1} m, mean={mean_e:.1} m");
    }
    println!(
        "Rivers: {river_count} hexes (accumulation >= {river_threshold:.1}); \
         Lakes: {lake_count} hexes"
    );

    if n == 0 {
        return;
    }

    let mut counts: std::collections::HashMap<ymir_biome::Biome, usize> =
        std::collections::HashMap::new();
    for &b in &region.biomes.per_hex {
        *counts.entry(b).or_insert(0) += 1;
    }
    let mut hist: Vec<(ymir_biome::Biome, usize)> = counts.into_iter().collect();
    hist.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)))
    });

    println!("Biome histogram:");
    for (biome, count) in hist.iter() {
        let pct = (*count as f64) / (n as f64) * 100.0;
        println!(
            "  {:<24} {:>6}  ({:.1}%)",
            format!("{:?}", biome),
            count,
            pct
        );
    }
}

/// Bincode-serialize a [`SkeletonWorld`] to `<world>/skeleton.bin`.
fn save_skeleton(wd: &WorldDirectory, skeleton: &SkeletonWorld) -> Result<(), String> {
    let path = wd.skeleton_path();
    let file =
        File::create(&path).map_err(|e| format!("failed to create {}: {}", path.display(), e))?;
    let writer = BufWriter::new(file);
    bincode::serialize_into(writer, skeleton)
        .map_err(|e| format!("failed to serialize skeleton to {}: {}", path.display(), e))
}

/// Load a previously saved [`SkeletonWorld`] from `<world>/skeleton.bin`.
fn load_skeleton(wd: &WorldDirectory) -> Result<SkeletonWorld, String> {
    let path = wd.skeleton_path();
    let file =
        File::open(&path).map_err(|e| format!("failed to open {}: {}", path.display(), e))?;
    let reader = BufReader::new(file);
    bincode::deserialize_from(reader).map_err(|e| {
        format!(
            "failed to deserialize skeleton at {}: {}",
            path.display(),
            e
        )
    })
}

/// Bincode-serialize a [`ClimateMap`] to `<world>/climate.bin`.
///
/// `ymir-storage` cannot depend on `ymir-climate` (layering rule), so this
/// concrete wrapper lives in the binary crate and delegates to the generic
/// [`save_bin`] helper.
fn save_climate(wd: &WorldDirectory, climate: &ClimateMap) -> Result<(), String> {
    let path = wd.climate_path();
    save_bin(&path, climate)
        .map_err(|e| format!("failed to serialize climate to {}: {}", path.display(), e))
}

/// Load a previously saved [`ClimateMap`] from `<world>/climate.bin`.
fn load_climate(wd: &WorldDirectory) -> Result<ClimateMap, String> {
    let path = wd.climate_path();
    load_bin(&path).map_err(|e| format!("failed to load climate at {}: {}", path.display(), e))
}

/// Bincode-serialize a [`BiomeMap`] to `<world>/biomes.bin`.
///
/// Mirror of [`save_climate`]: `ymir-storage` cannot depend on `ymir-biome`,
/// so this lives in the binary and delegates to [`save_bin`].
fn save_biomes(wd: &WorldDirectory, biomes: &BiomeMap) -> Result<(), String> {
    let path = wd.biomes_path();
    save_bin(&path, biomes)
        .map_err(|e| format!("failed to serialize biomes to {}: {}", path.display(), e))
}

/// Load a previously saved [`BiomeMap`] from `<world>/biomes.bin`.
fn load_biomes(wd: &WorldDirectory) -> Result<BiomeMap, String> {
    let path = wd.biomes_path();
    load_bin(&path).map_err(|e| format!("failed to load biomes at {}: {}", path.display(), e))
}

/// Bincode-serialize a [`RegionalDetail`] to
/// `<world>/detail/region_NNNN.bin`, creating the `detail/` subdirectory if
/// it does not already exist and recording the parent skeleton tile index
/// in `manifest`.
///
/// `ymir-storage` cannot depend on `ymir-detail` (see the crate graph in
/// `CLAUDE.md`), so the concrete region writer lives in the binary crate
/// and delegates to the generic [`save_bin`] helper. The manifest is left
/// dirty (not persisted) so callers can batch multiple region saves with a
/// single [`WorldDirectory::save_manifest`] call at the end.
///
/// Used by the `ymir detail` subcommand.
fn save_region(
    wd: &WorldDirectory,
    manifest: &mut WorldManifest,
    region: &RegionalDetail,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(wd.detail_dir())
        .map_err(|e| format!("failed to create {}: {}", wd.detail_dir().display(), e))?;
    let tile_index = region.spec.tile_index;
    let path = wd.region_path(tile_index);
    save_bin(&path, region)
        .map_err(|e| format!("failed to serialize region to {}: {}", path.display(), e))?;
    manifest.mark_region_generated(tile_index);
    Ok(path)
}

/// Load a previously saved [`RegionalDetail`] from
/// `<world>/detail/region_NNNN.bin`.
///
/// Currently only exercised through the binary's test suite; kept for
/// parity with [`save_region`] and for future read-only commands (e.g.
/// `ymir detail-info`).
#[allow(dead_code)]
fn load_region(wd: &WorldDirectory, tile_index: u32) -> Result<RegionalDetail, String> {
    let path = wd.region_path(tile_index);
    load_bin(&path).map_err(|e| format!("failed to load region at {}: {}", path.display(), e))
}

// ---------------------------------------------------------------------------
// Provenance helpers
// ---------------------------------------------------------------------------

/// Build a [`ProvenanceReport`] from the fully-computed pipeline outputs.
///
/// Per-field stages (stellar, orbital_body, atmosphere) are serialized in
/// full so every `Sourced<T>` leaf appears as a `{ value, source }` node.
/// Aggregate stages (skeleton, climate, biome) emit a compact summary node
/// because their per-tile arrays would balloon the file to many megabytes.
fn build_provenance(
    star: &StarContext,
    body: &OrbitalBody,
    atmosphere: &AtmosphereModel,
    _skeleton: &SkeletonWorld,
    climate: Option<&ClimateMap>,
    biomes: Option<&BiomeMap>,
) -> Result<ProvenanceReport, String> {
    let mut report = ProvenanceReport::new();

    // Stage 1: stellar context (per-field Sourced<T>).
    report.add_perfield_stage("stellar", star)?;

    // Stage 2: orbital body (per-field Sourced<T>).
    report.add_perfield_stage("orbital_body", body)?;

    // Stage 3: atmosphere (per-field Sourced<T>).
    report.add_perfield_stage("atmosphere", atmosphere)?;

    // Stage 4: skeleton — aggregate tag (no per-tile serialisation).
    report.add_aggregate_stage(
        "skeleton",
        &Source::Derived {
            from_stage: "skeleton".to_string(),
        },
    );

    // Stage 5: climate — aggregate tag, only if computed.
    if climate.is_some() {
        report.add_aggregate_stage(
            "climate",
            &Source::Derived {
                from_stage: "climate".to_string(),
            },
        );
    }

    // Stage 6: biome — aggregate tag, only if computed.
    if biomes.is_some() {
        report.add_aggregate_stage(
            "biome",
            &Source::Derived {
                from_stage: "biome".to_string(),
            },
        );
    }

    Ok(report)
}

/// Write `provenance.json` to the world directory.
fn save_world_provenance(wd: &WorldDirectory, report: &ProvenanceReport) -> Result<(), String> {
    let path = wd.provenance_path();
    save_provenance(&path, report).map_err(|e| {
        format!(
            "failed to write provenance.json at {}: {}",
            path.display(),
            e
        )
    })
}

/// Run `ymir provenance [--world PATH] [--summary | --full]`.
///
/// Loads `<world>/provenance.json` and either prints the per-stage histogram
/// (default / `--summary`) or pretty-prints the full JSON (`--full`).
fn run_provenance(world: &std::path::Path, full: bool) -> Result<(), String> {
    let wd = WorldDirectory {
        root: world.to_path_buf(),
    };
    let path = wd.provenance_path();
    if !path.exists() {
        return Err(format!(
            "provenance.json not found in {} \
             (run `ymir generate` to create it)",
            world.display()
        ));
    }

    let report =
        load_provenance(&path).map_err(|e| format!("failed to load {}: {}", path.display(), e))?;

    if full {
        let json = report
            .to_pretty_json()
            .map_err(|e| format!("failed to format provenance JSON: {e}"))?;
        println!("{json}");
    } else {
        println!("Provenance summary for {}", world.display());
        report.print_summary();
    }

    Ok(())
}

/// Compute (min, mean, max) elevation from a [`SkeletonWorld`].
fn elevation_stats(skeleton: &SkeletonWorld) -> (f64, f64, f64) {
    let elevs = &skeleton.elevation.elevations_m;
    if elevs.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0;
    for &e in elevs {
        if e < min {
            min = e;
        }
        if e > max {
            max = e;
        }
        sum += e;
    }
    (min, sum / elevs.len() as f64, max)
}

/// Print the post-generation human-readable summary block.
fn print_summary(
    star: &StarContext,
    planet_index: usize,
    skeleton: &SkeletonWorld,
    climate: Option<&ClimateMap>,
    biomes: Option<&BiomeMap>,
    out: &Path,
) {
    let body = &skeleton.body;
    let atmo = &skeleton.atmosphere;
    let (e_min, e_mean, e_max) = elevation_stats(skeleton);
    let star_name = star.name.as_deref().unwrap_or(&star.catalog_id);
    let planet_label = body
        .name
        .clone()
        .unwrap_or_else(|| format!("planet #{planet_index}"));

    println!("=== Generation summary ===");
    println!("Star:            {star_name} ({})", star.catalog_id);
    println!("Planet:          {planet_label} (index {planet_index})");
    println!("Semi-major axis: {:.4} AU", body.semi_major_axis.inner());
    println!(
        "Mass / Radius:   {:.3} M_earth / {:.3} R_earth ({:?})",
        body.mass.inner(),
        body.radius.inner(),
        body.planet_type
    );
    println!("Surface gravity: {:.3} m/s^2", body.surface_gravity.inner());
    println!(
        "T_eq / T_surf:   {:.1} K / {:.1} K (in HZ: {})",
        body.equilibrium_temp.inner(),
        atmo.effective_surface_temp.inner(),
        body.is_in_hz
    );
    println!(
        "Atmosphere:      {:?}, P = {:.4} bar, retained gases = {}",
        atmo.class,
        atmo.surface_pressure.inner(),
        atmo.retained.len()
    );
    println!(
        "Elevation:       min {:.1} m, mean {:.1} m, max {:.1} m ({} tiles)",
        e_min,
        e_mean,
        e_max,
        skeleton.elevation.elevations_m.len()
    );
    if let Some(c) = climate {
        println!(
            "Climate:         Mean T: {:.1} K  (min {:.1}, max {:.1}) | \
             Mean moisture: {:.2} | Wind cells: {}",
            c.temperature.mean(),
            c.temperature.min(),
            c.temperature.max(),
            c.moisture.mean(),
            c.wind.cell_count
        );
    }
    if let Some(b) = biomes {
        let total = b.len();
        if total > 0 {
            let mut hist = b.histogram();
            // Sort by count descending, then by debug-name for deterministic
            // tie-breaking (histogram() already sorts by name alphabetically).
            hist.sort_by(|a, b| {
                b.1.cmp(&a.1)
                    .then_with(|| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)))
            });
            println!("Biomes:");
            for (biome, count) in hist.iter().take(5) {
                let pct = (*count as f64) / (total as f64) * 100.0;
                println!(
                    "  {:<24} {:>6} tiles ({:.1}%)",
                    format!("{:?}", biome),
                    count,
                    pct
                );
            }
        }
    }
    println!("Output:          {}", out.display());
}

fn run_info(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!(
            "world directory does not exist: {}",
            path.display()
        ));
    }
    if !path.is_dir() {
        return Err(format!("not a directory: {}", path.display()));
    }

    let world = WorldDirectory {
        root: path.to_path_buf(),
    };

    let manifest_path = world.manifest_path();
    if !manifest_path.exists() {
        return Err(format!(
            "manifest.json not found in {} (is this a ymir world directory?)",
            path.display()
        ));
    }

    let manifest = world.load_manifest().map_err(|e| {
        format!(
            "failed to load manifest at {}: {}",
            manifest_path.display(),
            e
        )
    })?;

    print_manifest(&manifest);

    // If a skeleton.bin is present, load it and report body / atmosphere /
    // elevation stats. If not, skip gracefully.
    let skeleton_path = world.skeleton_path();
    if skeleton_path.exists() {
        match load_skeleton(&world) {
            Ok(skeleton) => {
                println!();
                print_skeleton_section(&skeleton);
            }
            Err(e) => {
                eprintln!("warning: failed to load skeleton.bin: {e}");
            }
        }
    } else {
        println!();
        println!("(skeleton.bin not present; skipping body/atmosphere/elevation section)");
    }

    // Climate summary (if climate.bin exists).
    if world.climate_path().exists() {
        match load_climate(&world) {
            Ok(climate) => {
                println!();
                print_climate_section(&climate);
            }
            Err(e) => {
                eprintln!("warning: failed to load climate.bin: {e}");
            }
        }
    }

    // Biome histogram (if biomes.bin exists).
    if world.biomes_path().exists() {
        match load_biomes(&world) {
            Ok(biomes) => {
                println!();
                print_biome_section(&biomes);
            }
            Err(e) => {
                eprintln!("warning: failed to load biomes.bin: {e}");
            }
        }
    }

    Ok(())
}

fn print_manifest(m: &WorldManifest) {
    let planet_label = match &m.planet_name {
        Some(name) => format!("{} ({}, planet #{})", name, m.star_name, m.planet_index),
        None => format!("{} planet #{}", m.star_name, m.planet_index),
    };

    println!("World: {} (seed: {})", planet_label, m.seed);
    if let Some(catalog_id) = &m.star_catalog_id {
        println!("Star catalog ID: {catalog_id}");
    }
    println!("Created: {}", m.created_at);
    println!("Manifest version: {}", m.version);
    println!("Pipeline version: {}", m.pipeline_version);
    if let Some(overrides) = &m.overrides_file {
        println!("Overrides file: {overrides}");
    }
    println!();

    print_config(&m.config);
    println!();

    if m.stages_computed.is_empty() {
        println!("Stages computed: (none)");
    } else {
        println!("Stages computed: {}", m.stages_computed.join(", "));
    }
}

fn print_config(cfg: &GenerationConfig) {
    println!("Generation Config:");
    println!("  Grid subdivision: {}", cfg.grid_subdivision_level);
    println!("  Biology enabled: {}", cfg.enable_biology);
    match cfg.continental_fraction {
        Some(v) => println!("  Continental fraction: {v:.3}"),
        None => println!("  Continental fraction: default (physics-derived)"),
    }
}

fn print_skeleton_section(skeleton: &SkeletonWorld) {
    let body = &skeleton.body;
    let atmo = &skeleton.atmosphere;
    let (e_min, e_mean, e_max) = elevation_stats(skeleton);

    println!("Body:");
    println!("  Mass:            {:.3} M_earth", body.mass.inner());
    println!("  Radius:          {:.3} R_earth", body.radius.inner());
    println!(
        "  Surface gravity: {:.3} m/s^2",
        body.surface_gravity.inner()
    );
    println!("  Equilibrium T:   {:.1} K", body.equilibrium_temp.inner());
    println!("  In HZ:           {}", body.is_in_hz);

    println!("Atmosphere:");
    println!("  Class:           {:?}", atmo.class);
    println!(
        "  Surface pressure:{:.4} bar",
        atmo.surface_pressure.inner()
    );
    println!(
        "  Surface T (eff): {:.1} K",
        atmo.effective_surface_temp.inner()
    );
    println!("  Retained gases:  {}", atmo.retained.len());

    let total = skeleton.elevation.elevations_m.len();
    let ocean = skeleton
        .elevation
        .elevations_m
        .iter()
        .filter(|&&e| e < 0.0)
        .count();
    let ocean_frac = if total == 0 {
        0.0
    } else {
        ocean as f64 / total as f64
    };
    println!("Elevation ({total} tiles):");
    println!("  Min:  {e_min:.1} m");
    println!("  Mean: {e_mean:.1} m");
    println!("  Max:  {e_max:.1} m");
    println!("  Ocean tiles (elev < 0): {ocean} / {total} ({ocean_frac:.3})");
}

fn print_climate_section(climate: &ClimateMap) {
    let temps = &climate.temperature.per_tile_k;
    let n = temps.len();
    println!("Climate ({n} tiles):");
    if n == 0 {
        println!("  (empty temperature field)");
        return;
    }
    let t_min = climate.temperature.min();
    let t_max = climate.temperature.max();
    let t_mean = climate.temperature.mean();
    let m_mean = climate.moisture.mean();
    let m_coverage = climate.moisture.coverage_above(0.5);

    println!("  Temperature:     mean {t_mean:.1} K (min {t_min:.1} K, max {t_max:.1} K)");
    println!(
        "  Moisture:        mean {:.3}, coverage >0.5 = {:.1}%",
        m_mean,
        m_coverage * 100.0
    );
    println!("  Wind cell count: {}", climate.wind.cell_count);
}

fn print_biome_section(biomes: &BiomeMap) {
    let total = biomes.len();
    println!("Biomes ({total} tiles, palette: {:?}):", biomes.palette);
    if total == 0 {
        println!("  (empty biome map)");
        return;
    }
    let mut hist = biomes.histogram();
    // Sort by count descending for the "top N" display, breaking ties on the
    // already-deterministic Debug-name order produced by BiomeMap::histogram.
    hist.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)))
    });
    let top = hist.iter().take(10);
    for (biome, count) in top {
        let pct = (*count as f64) / (total as f64) * 100.0;
        println!(
            "  {:<24} {:>6} tiles ({:.1}%)",
            format!("{:?}", biome),
            count,
            pct
        );
    }
    if hist.len() > 10 {
        println!("  ... and {} more biome(s)", hist.len() - 10);
    }
}

/// Resolved arguments for the `list-stars` subcommand.
struct ListStarsArgs {
    catalog_dir: PathBuf,
    spectral_type: Option<String>,
    within_pc: Option<f64>,
    has_planets: bool,
    limit: usize,
}

/// Resolved arguments for the `describe-star` subcommand.
struct DescribeStarArgs {
    query: String,
    catalog_dir: PathBuf,
}

/// Resolve the Gaia Parquet + Exoplanet CSV paths from `--catalog-dir`,
/// falling back to the committed fixtures if either real file is absent.
///
/// Keeps the command runnable from a fresh checkout without requiring the
/// 100+ MB Gaia download; documented in each subcommand's `--help`.
fn resolve_catalog_paths(catalog_dir: &Path) -> (PathBuf, PathBuf) {
    let gaia_real = catalog_dir.join("gaia_dr3_100pc.parquet");
    let exo_real = catalog_dir.join("exoplanet_archive.csv");
    if gaia_real.is_file() && exo_real.is_file() {
        return (gaia_real, exo_real);
    }
    // Fixtures live at the workspace root, relative to the current working
    // directory. The binary is always invoked from the workspace root in
    // CI and during integration tests (cargo sets CWD there).
    let fallback_dir = PathBuf::from("crates/ymir-catalog/fixtures");
    (
        fallback_dir.join("gaia_sample.parquet"),
        fallback_dir.join("exoplanet_sample.csv"),
    )
}

/// Parse a one-letter spectral class argument. Accepts upper- or lowercase.
fn parse_spectral_class(s: &str) -> Result<ymir_catalog::star_context::SpectralClass, String> {
    use ymir_catalog::star_context::SpectralClass;
    match s.trim().to_ascii_uppercase().as_str() {
        "O" => Ok(SpectralClass::O),
        "B" => Ok(SpectralClass::B),
        "A" => Ok(SpectralClass::A),
        "F" => Ok(SpectralClass::F),
        "G" => Ok(SpectralClass::G),
        "K" => Ok(SpectralClass::K),
        "M" => Ok(SpectralClass::M),
        other => Err(format!(
            "invalid spectral class '{other}' (expected one of O, B, A, F, G, K, M)"
        )),
    }
}

/// Run `ymir list-stars`. Prints a plain-ASCII table of matching stars
/// sorted by distance ascending, truncated to `--limit` rows.
fn run_list_stars(args: &ListStarsArgs) -> Result<(), String> {
    use ymir_catalog::{Catalog, CatalogQuery};

    let (gaia_path, exo_path) = resolve_catalog_paths(&args.catalog_dir);
    let catalog =
        Catalog::open(&gaia_path, &exo_path).map_err(|e| format!("failed to open catalog: {e}"))?;

    let spectral = match &args.spectral_type {
        Some(s) => Some(parse_spectral_class(s)?),
        None => None,
    };
    let query = CatalogQuery {
        spectral,
        min_distance_pc: None,
        max_distance_pc: args.within_pc,
        hz_hosts_only: args.has_planets,
    };

    let mut results = catalog.list(&query);
    results.sort_by(|a, b| {
        a.distance_pc
            .partial_cmp(&b.distance_pc)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(args.limit);

    // Header. Column widths chosen so real-catalog rows fit in ~100 chars:
    // gaia_id up to 20 digits, name up to 24 chars, spectral like "M4", etc.
    println!(
        "{:<20} {:<24} {:<8} {:>11} {:>8} {:>8} {:<14}",
        "gaia_id", "name", "spectral", "distance_pc", "teff_K", "L_sun", "has_hz_planet"
    );
    println!(
        "{:-<20} {:-<24} {:-<8} {:->11} {:->8} {:->8} {:-<14}",
        "", "", "", "", "", "", ""
    );
    for s in &results {
        let name = s.common_name.as_deref().unwrap_or("-");
        let name_display = if name.len() > 24 {
            // Truncate gracefully rather than smearing the column layout.
            let mut t = name[..23].to_string();
            t.push('…');
            t
        } else {
            name.to_string()
        };
        let spectral = format!("{}{}", s.spectral.class, s.spectral.subtype);
        let has_hz = if s.has_hz_planet { "yes" } else { "no" };
        println!(
            "{:<20} {:<24} {:<8} {:>11.2} {:>8.0} {:>8.3} {:<14}",
            s.gaia_id, name_display, spectral, s.distance_pc, s.teff_k, s.luminosity_sun, has_hz
        );
    }
    Ok(())
}

/// Run `ymir describe-star`. Prints a structured, human-readable dump of
/// the resolved `StarContext`, including catalog provenance, HZ edges, and
/// any known exoplanets.
fn run_describe_star(args: &DescribeStarArgs) -> Result<(), String> {
    use ymir_catalog::Catalog;

    let (gaia_path, exo_path) = resolve_catalog_paths(&args.catalog_dir);
    let catalog =
        Catalog::open(&gaia_path, &exo_path).map_err(|e| format!("failed to open catalog: {e}"))?;

    let Some(ctx) = catalog.resolve(&args.query) else {
        // Print the canonical "No match" message to stderr and bail with
        // a non-zero exit code. Returning an `Err` here would make `main`
        // print a second `error: ...` line; we use a sentinel that main
        // recognizes and swallows.
        return Err(format!("__nomatch__No match for '{}'", args.query));
    };

    // Also pull the raw exoplanet rows for the detailed planet table; the
    // `StarContext` only carries name strings in `known_exoplanets`.
    let planets: Vec<_> = extract_gaia_id(&ctx.catalog_id)
        .map(|gid| catalog.exoplanets_for(gid).to_vec())
        .unwrap_or_default();

    let display_name = ctx.name.clone().unwrap_or_else(|| args.query.clone());
    println!("Star: {} ({})", display_name, ctx.catalog_id);
    println!();

    println!("Catalog");
    // For stars with no Gaia row (Sol), RA/Dec is not meaningful; fall back
    // to "-" there. `from_catalog` does not populate RA/Dec on StarContext
    // itself, so we pull those from the summary if available.
    let summary = extract_gaia_id(&ctx.catalog_id).and_then(|gid| catalog.summary(gid));
    if let Some(s) = &summary {
        println!("  RA/Dec:      {:.4}° / {:.4}°", s.ra_deg, s.dec_deg);
    } else {
        println!("  RA/Dec:      - / -");
    }

    let dist_src = format_source(ctx.distance.source());
    let teff_src = format_source(ctx.effective_temp.source());
    let lum_src = format_source(ctx.luminosity.source());
    println!(
        "  Distance:    {:.2} pc            {}",
        ctx.distance.inner(),
        dist_src
    );
    println!(
        "  Teff:        {:.0} K             {}",
        ctx.effective_temp.inner(),
        teff_src
    );
    println!(
        "  Luminosity:  {:.3} L_sun         {}",
        ctx.luminosity.inner(),
        lum_src
    );
    println!("  Spectral:    {}", ctx.spectral_type);
    println!();

    println!("Habitable Zone");
    println!("  Inner: {:.3} AU", ctx.hz_inner.inner());
    println!("  Outer: {:.3} AU", ctx.hz_outer.inner());
    println!();

    if planets.is_empty() {
        println!("Known exoplanets (0)");
        println!("  (none in catalog)");
    } else {
        println!("Known exoplanets ({})", planets.len());
        for p in &planets {
            let label = if p.planet_letter.is_empty() {
                p.host_name.clone()
            } else {
                format!("{} {}", p.host_name, p.planet_letter)
            };
            let period = p
                .orbital_period_days
                .map(|v| format!("P={v:.0} d"))
                .unwrap_or_else(|| "P=?".to_string());
            let sma = p
                .semi_major_axis_au
                .map(|v| format!("a={v:.2} AU"))
                .unwrap_or_else(|| "a=?".to_string());
            let mass = p
                .planet_mass_earth
                .map(|v| format!("m={v:.2} M_earth"))
                .unwrap_or_else(|| "m=?".to_string());
            let radius = p
                .planet_radius_earth
                .map(|v| format!("r={v:.2} R_earth"))
                .unwrap_or_else(|| "r=?".to_string());
            println!(
                "  {:<16} {:<6} {:<10} {:<12} {:<14} {}",
                label,
                p.discovery_method.as_archive_str(),
                period,
                sma,
                mass,
                radius
            );
        }
    }

    Ok(())
}

/// Pretty-print a `Source` as a short bracketed label for the describe
/// output. Keeps `[Observed - {reference} ({date})]` compact so the table
/// stays readable.
fn format_source(source: &ymir_core::Source) -> String {
    use ymir_core::Source;
    match source {
        Source::Observed {
            reference, date, ..
        } => {
            if date.is_empty() {
                format!("[Observed - {reference}]")
            } else {
                format!("[Observed - {reference} ({date})]")
            }
        }
        Source::Derived { from_stage } => format!("[Derived from {from_stage}]"),
        Source::Assumed { reason } => format!("[Assumed - {reason}]"),
    }
}

/// Parse `"Gaia DR3 {id}"` into the numeric source ID. Returns `None` for
/// synthesized catalog IDs like "Sol".
fn extract_gaia_id(catalog_id: &str) -> Option<u64> {
    catalog_id
        .strip_prefix("Gaia DR3 ")
        .and_then(|s| s.trim().parse::<u64>().ok())
}

// ---------------------------------------------------------------------------
// Override authoring helpers (CORE-08)
// ---------------------------------------------------------------------------

/// Pipeline stages that carry per-field `Sourced<T>` provenance and have a
/// corresponding slot in [`OverrideFile`], making them valid targets for
/// `ymir override add`.
///
/// Note: `stellar` has per-field provenance in `provenance.json` but no
/// dedicated override slot in [`OverrideFile`] (Phase 1 does not support
/// re-running catalog lookup from an override). It is excluded here.
/// Aggregate stages (skeleton, climate, biome) store only a single summary
/// source node and do not expose individual fields.
const PER_FIELD_STAGES: &[&str] = &["orbital_body", "atmosphere"];

/// Aggregate stages that reject per-field overrides. Also includes `stellar`
/// because [`OverrideFile`] has no stellar slot in Phase 1.
const AGGREGATE_STAGES: &[&str] = &["stellar", "skeleton", "climate", "biome"];

/// Resolved arguments for `ymir override add`.
struct OverrideAddArgs {
    world: PathBuf,
    field: String,
    value: String,
    unit: Option<String>,
    reference: String,
    instrument: String,
}

/// Load the world's `overrides.json`, or return a default [`OverrideFile`]
/// seeded from the world manifest if the file does not yet exist.
///
/// If `overrides.json` does not exist we need a valid `target_star` to seed the
/// new file; we pull it from the manifest.
fn load_or_create_override_file(wd: &WorldDirectory) -> Result<OverrideFile, String> {
    let path = wd.overrides_path();
    if path.exists() {
        OverrideFile::load(&path).map_err(|e| format!("failed to load {}: {}", path.display(), e))
    } else {
        // Bootstrap from the manifest's star name.
        let manifest = wd.load_manifest().map_err(|e| {
            format!("failed to load manifest (needed to create overrides.json): {e}")
        })?;
        Ok(OverrideFile {
            version: "1.0".to_string(),
            target_star: manifest.star_name,
            target_planet: manifest.planet_name,
            overrides: StageOverrides::default(),
        })
    }
}

/// Save an [`OverrideFile`] to `<world>/overrides.json`.
fn save_override_file(wd: &WorldDirectory, file: &OverrideFile) -> Result<(), String> {
    let path = wd.overrides_path();
    let json = serde_json::to_string_pretty(file)
        .map_err(|e| format!("failed to serialize overrides: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("failed to write {}: {}", path.display(), e))
}

/// Split a dot-separated field path into `(stage, field_tail)`.
///
/// The first segment is the stage name; the remainder (joined with `.`) is the
/// field path within that stage's JSON object. Returns an error if the path has
/// fewer than two segments or if the stage is unsupported.
fn parse_field_path(field: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = field.splitn(2, '.').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        return Err(format!(
            "invalid field path '{field}': expected '<stage>.<field>' \
             e.g. 'orbital_body.radius'"
        ));
    }
    let stage = parts[0].to_string();
    let tail = parts[1].to_string();

    // Reject aggregate stages immediately — they have no per-field override slot.
    if AGGREGATE_STAGES.contains(&stage.as_str()) {
        let reason = if stage == "stellar" {
            "the stellar stage has no override slot in Phase 1 \
             (star catalog data cannot be replaced via overrides.json)"
        } else {
            "skeleton/climate/biome are aggregate stages with no per-field \
             provenance tree; use a full stage-level JSON override with \
             `ymir regenerate --overrides` instead"
        };
        return Err(format!(
            "stage '{stage}' does not support per-field overrides: {reason}"
        ));
    }

    if !PER_FIELD_STAGES.contains(&stage.as_str()) {
        let known: Vec<&str> = PER_FIELD_STAGES
            .iter()
            .chain(AGGREGATE_STAGES.iter())
            .copied()
            .collect();
        return Err(format!(
            "unknown stage '{stage}'; known stages: {}",
            known.join(", ")
        ));
    }

    Ok((stage, tail))
}

/// Like [`parse_field_path`] but also accepts `stellar` for read-only
/// validation against `provenance.json`.
///
/// Used by `ymir override validate`, which checks field existence in the
/// provenance tree rather than writing to `overrides.json`. The stellar stage
/// has no override slot but does carry per-field provenance, so its fields
/// are valid targets for validation even though they cannot be overridden.
fn parse_field_path_lenient(field: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = field.splitn(2, '.').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        return Err(format!(
            "invalid field path '{field}': expected '<stage>.<field>' \
             e.g. 'orbital_body.radius'"
        ));
    }
    let stage = parts[0].to_string();
    let tail = parts[1].to_string();

    // All per-field stages (including stellar) are valid for validation.
    let all_perfield = &["stellar", "orbital_body", "atmosphere"];
    if AGGREGATE_STAGES.contains(&stage.as_str()) && stage != "stellar" {
        return Err(format!(
            "stage '{stage}' is an aggregate stage and does not have a \
             per-field provenance tree; no field paths are valid for it"
        ));
    }
    if !all_perfield.contains(&stage.as_str()) {
        let known: Vec<&str> = all_perfield
            .iter()
            .chain(AGGREGATE_STAGES.iter().filter(|&&s| s != "stellar"))
            .copied()
            .collect();
        return Err(format!(
            "unknown stage '{stage}'; known stages: {}",
            known.join(", ")
        ));
    }

    Ok((stage, tail))
}

/// Get a mutable reference to the stage's JSON value in the override file,
/// creating an empty object if it is not yet set.
fn stage_value_mut<'a>(
    overrides: &'a mut StageOverrides,
    stage: &str,
) -> &'a mut serde_json::Value {
    let slot = match stage {
        "orbital_body" => &mut overrides.orbital_body,
        "atmosphere" => &mut overrides.atmosphere,
        _ => unreachable!("caller checked stage list via PER_FIELD_STAGES"),
    };
    slot.get_or_insert_with(|| serde_json::Value::Object(Default::default()))
}

/// Get an immutable reference to the stage's JSON value if present.
fn stage_value<'a>(overrides: &'a StageOverrides, stage: &str) -> Option<&'a serde_json::Value> {
    match stage {
        "orbital_body" => overrides.orbital_body.as_ref(),
        "atmosphere" => overrides.atmosphere.as_ref(),
        _ => None,
    }
}

/// Set a nested dot-path within a JSON object to `new_value`, creating
/// intermediate objects as needed.
fn json_set_path(root: &mut serde_json::Value, path: &str, new_value: serde_json::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut cur = root;
    for &key in &parts[..parts.len() - 1] {
        cur = cur
            .as_object_mut()
            .expect("intermediate JSON node is not an object")
            .entry(key)
            .or_insert_with(|| serde_json::Value::Object(Default::default()));
    }
    let last_key = *parts.last().expect("parts non-empty");
    cur.as_object_mut()
        .expect("leaf parent is not an object")
        .insert(last_key.to_string(), new_value);
}

/// Remove a nested dot-path from a JSON object. Returns `true` if the key was
/// present and removed.
fn json_remove_path(root: &mut serde_json::Value, path: &str) -> bool {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() == 1 {
        return root
            .as_object_mut()
            .map(|m| m.remove(parts[0]).is_some())
            .unwrap_or(false);
    }
    // Navigate to the parent object.
    let mut cur = root;
    for &key in &parts[..parts.len() - 1] {
        match cur.as_object_mut() {
            Some(m) => match m.get_mut(key) {
                Some(child) => cur = child,
                None => return false,
            },
            None => return false,
        }
    }
    let last_key = *parts.last().expect("parts non-empty");
    cur.as_object_mut()
        .map(|m| m.remove(last_key).is_some())
        .unwrap_or(false)
}

/// Build the JSON value to store for an override field.
///
/// For per-field `Sourced<T>` stages the value is stored as a bare scalar or
/// JSON value. The `merge_json` function in the pipeline knows how to wrap
/// it in the `{value, source: Observed{...}}` envelope on read-back.
/// We additionally store `_reference` and `_instrument` as sibling keys so
/// `ymir override list` can surface them without re-parsing the full pipeline.
fn build_override_value(
    value_json: serde_json::Value,
    reference: &str,
    instrument: &str,
    unit: Option<&str>,
) -> serde_json::Value {
    // If the caller gave us a bare scalar (the common case), store as:
    //   { "__value": <scalar>, "__reference": "...", "__instrument": "...", "__unit": "..." }
    // When regenerate applies the override, `merge_json` sees a JSON Object at
    // the Sourced leaf and merges its keys, so we must wrap the actual value
    // under a recognized key. However, `merge_json` currently only looks at
    // two-key {value,source} objects; to avoid confusion we use the pipeline's
    // existing envelope format directly.
    //
    // Preferred shape: `{"value": <v>, "source": {"Observed": {...}}}`.
    // This is exactly what `make_user_observed_sourced_json` produces, extended
    // with optional unit metadata in the source annotation.
    let mut source_inner = serde_json::json!({
        "reference": reference,
        "instrument": instrument,
        "date": "",
        "uncertainty": null,
    });
    if let Some(u) = unit {
        source_inner["unit"] = serde_json::Value::String(u.to_string());
    }
    serde_json::json!({
        "value": value_json,
        "source": { "Observed": source_inner },
    })
}

/// Run `ymir override add`.
fn run_override_add(args: &OverrideAddArgs) -> Result<(), String> {
    let wd = WorldDirectory {
        root: args.world.clone(),
    };
    ensure_world_dir(&wd)?;

    let (stage, field_tail) = parse_field_path(&args.field)?;

    // Parse the supplied value as JSON.
    let value_json: serde_json::Value = serde_json::from_str(&args.value).map_err(|_| {
        // Try treating as a bare string.
        format!(
            "could not parse --value '{}' as JSON; \
             wrap strings in quotes, e.g. --value '\"text\"'",
            args.value
        )
    })?;

    // Load or create the overrides file.
    let mut ov_file = load_or_create_override_file(&wd)?;

    // Build the `{value, source: Observed{...}}` envelope.
    let envelope = build_override_value(
        value_json,
        &args.reference,
        &args.instrument,
        args.unit.as_deref(),
    );

    // Inject into the stage slot.
    let stage_val = stage_value_mut(&mut ov_file.overrides, &stage);
    json_set_path(stage_val, &field_tail, envelope);

    save_override_file(&wd, &ov_file)?;

    let unit_display = args
        .unit
        .as_deref()
        .map(|u| format!(" {u}"))
        .unwrap_or_default();
    println!(
        "override add: {}.{} = <value>{unit_display}  [{}]",
        stage, field_tail, args.reference
    );
    println!("  saved to {}", wd.overrides_path().display());
    println!(
        "  run `ymir regenerate --world {} --overrides {}` to propagate",
        wd.root.display(),
        wd.overrides_path().display()
    );
    Ok(())
}

/// Run `ymir override remove`.
fn run_override_remove(world: &Path, field: &str) -> Result<(), String> {
    let wd = WorldDirectory {
        root: world.to_path_buf(),
    };
    ensure_world_dir(&wd)?;

    let path = wd.overrides_path();
    if !path.exists() {
        return Err(format!(
            "overrides.json not found in {} (no overrides set)",
            world.display()
        ));
    }

    let (stage, field_tail) = parse_field_path(field)?;
    let mut ov_file = OverrideFile::load(&path)
        .map_err(|e| format!("failed to load {}: {}", path.display(), e))?;

    let stage_val = match stage.as_str() {
        "orbital_body" => ov_file.overrides.orbital_body.as_mut(),
        "atmosphere" => ov_file.overrides.atmosphere.as_mut(),
        _ => unreachable!("parse_field_path validates stage"),
    };

    let removed = match stage_val {
        Some(v) => json_remove_path(v, &field_tail),
        None => false,
    };

    if !removed {
        return Err(format!(
            "field '{field}' not found in overrides.json (nothing to remove)"
        ));
    }

    // If the stage object is now empty, clear the slot entirely.
    let clear_orbital = stage == "orbital_body"
        && ov_file
            .overrides
            .orbital_body
            .as_ref()
            .and_then(|v| v.as_object())
            .map(|m| m.is_empty())
            .unwrap_or(false);
    let clear_atmosphere = stage == "atmosphere"
        && ov_file
            .overrides
            .atmosphere
            .as_ref()
            .and_then(|v| v.as_object())
            .map(|m| m.is_empty())
            .unwrap_or(false);

    if clear_orbital {
        ov_file.overrides.orbital_body = None;
    }
    if clear_atmosphere {
        ov_file.overrides.atmosphere = None;
    }

    save_override_file(&wd, &ov_file)?;
    println!("override remove: removed '{field}' from overrides.json");
    Ok(())
}

/// Run `ymir override list`.
fn run_override_list(world: &Path) -> Result<(), String> {
    let wd = WorldDirectory {
        root: world.to_path_buf(),
    };
    let path = wd.overrides_path();
    if !path.exists() {
        println!(
            "No overrides set (overrides.json not found in {}).",
            world.display()
        );
        return Ok(());
    }

    let ov_file = OverrideFile::load(&path)
        .map_err(|e| format!("failed to load {}: {}", path.display(), e))?;

    println!("Overrides for {} ({})", ov_file.target_star, path.display());

    let mut found_any = false;
    for stage in PER_FIELD_STAGES {
        if let Some(stage_val) = stage_value(&ov_file.overrides, stage) {
            if let Some(obj) = stage_val.as_object() {
                for (key, val) in obj {
                    print_override_field(stage, key, val);
                    found_any = true;
                }
            }
        }
    }
    if !found_any {
        println!("  (no per-field overrides set)");
    }
    Ok(())
}

/// Print a single override field entry for `ymir override list`.
fn print_override_field(stage: &str, field: &str, val: &serde_json::Value) {
    // The value is either a plain scalar or a `{value, source}` envelope.
    if let Some(obj) = val.as_object() {
        if let (Some(v), Some(src)) = (obj.get("value"), obj.get("source")) {
            let reference = src
                .get("Observed")
                .and_then(|o| o.get("reference"))
                .and_then(|r| r.as_str())
                .unwrap_or("?");
            let instrument = src
                .get("Observed")
                .and_then(|o| o.get("instrument"))
                .and_then(|i| i.as_str())
                .unwrap_or("?");
            let unit = src
                .get("Observed")
                .and_then(|o| o.get("unit"))
                .and_then(|u| u.as_str())
                .map(|u| format!(" {u}"))
                .unwrap_or_default();
            println!("  {stage}.{field} = {v}{unit}  [{reference} / {instrument}]");
            return;
        }
    }
    // Fallback: raw JSON.
    println!("  {stage}.{field} = {val}");
}

/// Run `ymir override validate`.
///
/// Reads `provenance.json` and checks that `field` exists as a `{value, source}`
/// leaf in the per-field stages. Prints a success or failure message and
/// returns `Ok(())` / `Err(...)` accordingly.
fn run_override_validate(world: &Path, field: &str) -> Result<(), String> {
    let wd = WorldDirectory {
        root: world.to_path_buf(),
    };
    ensure_world_dir(&wd)?;

    // Use the lenient parser so `stellar.*` fields are also checkable even
    // though they cannot be overridden via `ymir override add`.
    let (stage, field_tail) = parse_field_path_lenient(field)?;

    let prov_path = wd.provenance_path();
    if !prov_path.exists() {
        return Err(format!(
            "provenance.json not found in {} \
             (run `ymir generate` first to create it)",
            world.display()
        ));
    }

    let report =
        load_provenance(&prov_path).map_err(|e| format!("failed to load provenance.json: {e}"))?;

    let stage_prov = report.stages.get(&stage).ok_or_else(|| {
        format!(
            "stage '{stage}' not found in provenance.json \
             (available: {})",
            report.stages.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    })?;

    // Walk the field tree to find the leaf.
    if find_field_in_json(&stage_prov.fields, &field_tail) {
        println!("valid: '{field}' resolves to a Sourced field in stage '{stage}'");
        Ok(())
    } else {
        Err(format!(
            "unknown field path '{field}': '{field_tail}' not found as a \
             `Sourced<T>` leaf in the '{stage}' provenance tree"
        ))
    }
}

/// Walk a JSON value tree looking for a `{value, source}` leaf at the given
/// dot-separated path.
fn find_field_in_json(root: &serde_json::Value, path: &str) -> bool {
    let parts: Vec<&str> = path.split('.').collect();
    let mut cur = root;
    for &key in &parts {
        match cur.as_object() {
            Some(map) => {
                // If the current node is already a Sourced leaf, stop — we
                // can't descend further into its `value` or `source` children.
                if map.len() == 2 && map.contains_key("value") && map.contains_key("source") {
                    return false;
                }
                match map.get(key) {
                    Some(child) => cur = child,
                    None => return false,
                }
            }
            None => return false,
        }
    }
    // At the target node: confirm it's a Sourced leaf.
    if let Some(map) = cur.as_object() {
        map.len() == 2 && map.contains_key("value") && map.contains_key("source")
    } else {
        false
    }
}

/// Verify that `world` is a directory containing `manifest.json`.
fn ensure_world_dir(wd: &WorldDirectory) -> Result<(), String> {
    if !wd.root.is_dir() {
        return Err(format!(
            "world directory '{}' does not exist or is not a directory",
            wd.root.display()
        ));
    }
    if !wd.manifest_path().exists() {
        return Err(format!(
            "manifest.json not found in '{}' (is this a ymir world directory?)",
            wd.root.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(seed: u64, output: PathBuf, planet: usize) -> GenerateArgs {
        GenerateArgs {
            star: "Tau Ceti".to_string(),
            planet,
            seed,
            output,
            subdivision: 2,
            enable_biology: true,
            preview_width: 64,
            preview_height: 32,
            skip_climate: false,
            skip_biomes: false,
        }
    }

    #[test]
    fn lookup_star_tau_ceti_case_insensitive() {
        assert!(matches!(
            lookup_star("Tau Ceti").unwrap(),
            StarSelection::Derived { .. }
        ));
        assert!(matches!(
            lookup_star("tau ceti").unwrap(),
            StarSelection::Derived { .. }
        ));
        assert!(matches!(
            lookup_star("  TAU CETI  ").unwrap(),
            StarSelection::Derived { .. }
        ));
    }

    #[test]
    fn lookup_star_unknown_errors() {
        let err = lookup_star("Sirius").unwrap_err();
        assert!(err.contains("unknown star"));
        assert!(err.contains("Sirius"));
    }

    #[test]
    fn lookup_star_earth_variants() {
        for alias in ["Earth", "earth", "  EARTH  ", "Sol", "sun", "SUN"] {
            match lookup_star(alias).unwrap() {
                StarSelection::Fixed { star, body } => {
                    assert_eq!(star.name.as_deref(), Some("Sol"));
                    assert_eq!(body.name.as_deref(), Some("Earth"));
                    assert!((body.semi_major_axis - 1.0).abs() < 1e-9);
                }
                StarSelection::Derived { .. } => panic!("{alias} should map to Fixed"),
            }
        }
    }

    #[test]
    fn lookup_star_mars() {
        match lookup_star("Mars").unwrap() {
            StarSelection::Fixed { star, body } => {
                assert_eq!(star.name.as_deref(), Some("Sol"));
                assert_eq!(body.name.as_deref(), Some("Mars"));
                assert!((body.semi_major_axis - 1.524).abs() < 1e-9);
                assert!(!body.is_in_hz);
            }
            StarSelection::Derived { .. } => panic!("Mars should map to Fixed"),
        }
    }

    #[test]
    fn generate_earth_creates_expected_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("earth");
        let mut a = args(1, out.clone(), 0);
        a.star = "Earth".to_string();
        run_generate(&a).expect("generate earth ok");
        assert!(out.join("manifest.json").is_file());
        assert!(out.join("skeleton.bin").is_file());
        assert!(out.join("preview.png").is_file());
        let wd = WorldDirectory { root: out };
        let manifest = wd.load_manifest().expect("load manifest");
        assert_eq!(manifest.star_name, "Sol");
        assert_eq!(manifest.planet_name.as_deref(), Some("Earth"));
    }

    #[test]
    fn generate_mars_creates_expected_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("mars");
        let mut a = args(1, out.clone(), 0);
        a.star = "Mars".to_string();
        run_generate(&a).expect("generate mars ok");
        let wd = WorldDirectory { root: out };
        let manifest = wd.load_manifest().expect("load manifest");
        assert_eq!(manifest.star_name, "Sol");
        assert_eq!(manifest.planet_name.as_deref(), Some("Mars"));
    }

    #[test]
    fn generate_earth_rejects_nonzero_planet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut a = args(1, tmp.path().join("w"), 3);
        a.star = "Earth".to_string();
        let err = run_generate(&a).unwrap_err();
        assert!(
            err.contains("--planet must be 0"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn generate_creates_expected_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("world");
        run_generate(&args(42, out.clone(), 0)).expect("generate ok");
        assert!(out.join("manifest.json").is_file());
        assert!(out.join("skeleton.bin").is_file());
        assert!(out.join("preview.png").is_file());
        assert!(out.join("climate.bin").is_file());
        assert!(out.join("biomes.bin").is_file());
        assert!(out.join("preview_biome.png").is_file());

        // Manifest round-trips and includes all six stages.
        let wd = WorldDirectory { root: out.clone() };
        let manifest = wd.load_manifest().expect("load manifest");
        assert_eq!(manifest.star_name, "Tau Ceti");
        assert_eq!(manifest.seed, 42);
        assert_eq!(manifest.planet_index, 0);
        assert!(manifest.config.enable_biology);
        assert_eq!(
            manifest.stages_computed,
            vec![
                "stellar".to_string(),
                "system".to_string(),
                "atmosphere".to_string(),
                "skeleton".to_string(),
                "climate".to_string(),
                "biomes".to_string(),
            ]
        );
    }

    #[test]
    fn generate_skip_climate_stops_after_skeleton() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("world");
        let mut a = args(42, out.clone(), 0);
        a.skip_climate = true;
        a.skip_biomes = true; // mirrors the CLI's implication
        run_generate(&a).expect("generate ok");
        assert!(out.join("skeleton.bin").is_file());
        assert!(out.join("preview.png").is_file());
        assert!(!out.join("climate.bin").exists());
        assert!(!out.join("biomes.bin").exists());
        assert!(!out.join("preview_biome.png").exists());

        let wd = WorldDirectory { root: out };
        let manifest = wd.load_manifest().expect("load manifest");
        assert_eq!(
            manifest.stages_computed,
            vec![
                "stellar".to_string(),
                "system".to_string(),
                "atmosphere".to_string(),
                "skeleton".to_string(),
            ]
        );
    }

    #[test]
    fn generate_skip_biomes_stops_after_climate() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("world");
        let mut a = args(42, out.clone(), 0);
        a.skip_biomes = true;
        run_generate(&a).expect("generate ok");
        assert!(out.join("climate.bin").is_file());
        assert!(!out.join("biomes.bin").exists());
        assert!(!out.join("preview_biome.png").exists());

        let wd = WorldDirectory { root: out };
        let manifest = wd.load_manifest().expect("load manifest");
        assert_eq!(
            manifest.stages_computed,
            vec![
                "stellar".to_string(),
                "system".to_string(),
                "atmosphere".to_string(),
                "skeleton".to_string(),
                "climate".to_string(),
            ]
        );
    }

    #[test]
    fn generate_is_deterministic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out_a = tmp.path().join("a");
        let out_b = tmp.path().join("b");
        run_generate(&args(7, out_a.clone(), 0)).expect("generate a");
        run_generate(&args(7, out_b.clone(), 0)).expect("generate b");
        let bytes_a = std::fs::read(out_a.join("skeleton.bin")).expect("read a");
        let bytes_b = std::fs::read(out_b.join("skeleton.bin")).expect("read b");
        assert_eq!(bytes_a, bytes_b, "skeleton.bin not deterministic");
    }

    #[test]
    fn generate_unknown_star_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut a = args(1, tmp.path().join("w"), 0);
        a.star = "Unknown Star".to_string();
        let err = run_generate(&a).unwrap_err();
        assert!(err.contains("unknown star"));
    }

    #[test]
    fn generate_planet_out_of_range_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let a = args(1, tmp.path().join("w"), 999);
        let err = run_generate(&a).unwrap_err();
        assert!(err.contains("out of range"), "unexpected error: {err}");
    }

    // --- Provenance tests ---------------------------------------------------

    /// Generate an Earth world and verify that `provenance.json` is written
    /// with the expected stage tree. Earth uses `sol_context()` which tags
    /// catalog scalars as `Assumed` (from_params path) and HZ bounds as
    /// `Derived`. The Earth body is hand-filled with `Observed` tags.
    #[test]
    fn generate_earth_writes_provenance_json() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("earth_prov");
        let mut a = args(1, out.clone(), 0);
        a.star = "Earth".to_string();
        run_generate(&a).expect("generate earth ok");

        let wd = WorldDirectory { root: out };
        let prov_path = wd.provenance_path();
        assert!(prov_path.exists(), "provenance.json should exist");

        let report = load_provenance(&prov_path).expect("parse provenance.json");

        // All expected stages must be present (climate + biome too: Earth runs full pipeline).
        for stage in [
            "stellar",
            "orbital_body",
            "atmosphere",
            "skeleton",
            "climate",
            "biome",
        ] {
            assert!(report.stages.contains_key(stage), "missing stage: {stage}");
        }

        // Sol stellar context uses from_params → catalog scalars are Assumed,
        // HZ bounds are Derived. No Observed tags expected here.
        let stellar = &report.stages["stellar"];
        assert!(
            stellar.counts.assumed > 0,
            "Sol stellar stage should have some Assumed fields (from_params path)"
        );
        assert!(
            stellar.counts.derived > 0,
            "Sol stellar stage should have some Derived fields (HZ bounds)"
        );
        // Total fields must be non-zero.
        assert!(stellar.counts.total() > 0);

        // For Earth (fixed body with `obs()` constructor), all numeric fields
        // are Observed (e.g., mass, radius, semi_major_axis ...).
        let body = &report.stages["orbital_body"];
        assert!(
            body.counts.observed > 0,
            "orbital_body stage should have Observed fields for Earth"
        );
        assert_eq!(
            body.counts.assumed, 0,
            "Earth orbital_body should have zero Assumed fields"
        );

        // The skeleton aggregate node must have exactly 1 derived count.
        let skeleton = &report.stages["skeleton"];
        assert_eq!(skeleton.counts.derived, 1);
        assert_eq!(skeleton.counts.observed, 0);
        assert_eq!(skeleton.counts.assumed, 0);
    }

    /// For Tau Ceti the AtmosphereModel is fully derived from simulation;
    /// assert that the atmosphere stage has zero Observed fields.
    #[test]
    fn generate_tau_ceti_atmosphere_is_all_derived() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("tau_ceti_prov");
        run_generate(&args(42, out.clone(), 0)).expect("generate tau ceti ok");

        let wd = WorldDirectory { root: out };
        let report = load_provenance(&wd.provenance_path()).expect("parse provenance.json");

        let atmo = &report.stages["atmosphere"];
        assert_eq!(
            atmo.counts.observed, 0,
            "Tau Ceti atmosphere should have zero Observed fields"
        );
        // Must be all Derived (none Assumed either, since AtmosphereModel::derive always uses Derived).
        assert!(
            atmo.counts.derived > 0,
            "Tau Ceti atmosphere should have some Derived fields"
        );
        assert_eq!(atmo.counts.assumed, 0);

        // Climate and biome stages should be present for a full generate.
        assert!(report.stages.contains_key("climate"));
        assert!(report.stages.contains_key("biome"));
    }

    /// The `run_provenance` helper should succeed for a world with a report
    /// and return the correct counts in summary mode.
    #[test]
    fn run_provenance_reads_existing_report() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("prov_world");
        let mut a = args(1, out.clone(), 0);
        a.star = "Earth".to_string();
        run_generate(&a).expect("generate earth");

        // Summary mode should succeed without error.
        run_provenance(&out, false).expect("provenance summary");
        // Full mode should also succeed.
        run_provenance(&out, true).expect("provenance full");
    }

    // --- Regenerate helpers -------------------------------------------------

    #[test]
    fn dirty_stages_empty_overrides_all_clean() {
        let dirty = dirty_stages_from_overrides(&StageOverrides::default());
        assert!(dirty.dirty_stages().is_empty());
    }

    #[test]
    fn dirty_stages_atmosphere_override_marks_3_through_7() {
        let overrides = StageOverrides {
            atmosphere: Some(serde_json::json!({})),
            ..Default::default()
        };
        let dirty = dirty_stages_from_overrides(&overrides);
        assert!(!dirty.is_dirty(Stage::StellarContext));
        assert!(!dirty.is_dirty(Stage::PlanetarySystem));
        assert!(dirty.is_dirty(Stage::Atmosphere));
        assert!(dirty.is_dirty(Stage::Skeleton));
        assert!(dirty.is_dirty(Stage::Climate));
        assert!(dirty.is_dirty(Stage::Biome));
        assert!(dirty.is_dirty(Stage::RegionalDetail));
    }

    #[test]
    fn dirty_stages_biome_override_marks_only_6_and_7() {
        let overrides = StageOverrides {
            biome: Some(serde_json::json!({})),
            ..Default::default()
        };
        let dirty = dirty_stages_from_overrides(&overrides);
        for stage in [
            Stage::StellarContext,
            Stage::PlanetarySystem,
            Stage::Atmosphere,
            Stage::Skeleton,
            Stage::Climate,
        ] {
            assert!(!dirty.is_dirty(stage), "{stage:?} should be clean");
        }
        assert!(dirty.is_dirty(Stage::Biome));
        assert!(dirty.is_dirty(Stage::RegionalDetail));
    }

    #[test]
    fn dirty_stages_orbital_body_override_marks_2_through_7() {
        let overrides = StageOverrides {
            orbital_body: Some(serde_json::json!({})),
            ..Default::default()
        };
        let dirty = dirty_stages_from_overrides(&overrides);
        assert!(!dirty.is_dirty(Stage::StellarContext));
        for stage in [
            Stage::PlanetarySystem,
            Stage::Atmosphere,
            Stage::Skeleton,
            Stage::Climate,
            Stage::Biome,
            Stage::RegionalDetail,
        ] {
            assert!(dirty.is_dirty(stage), "{stage:?} should be dirty");
        }
    }

    #[test]
    fn merge_json_replaces_scalar_fields() {
        let mut target = serde_json::json!({"a": 1, "b": 2});
        let patch = serde_json::json!({"b": 5});
        merge_json(&mut target, &patch);
        assert_eq!(target, serde_json::json!({"a": 1, "b": 5}));
    }

    #[test]
    fn merge_json_deep_merges_nested_objects() {
        let mut target = serde_json::json!({
            "outer": {"x": 1, "y": 2},
            "leave_alone": 99
        });
        let patch = serde_json::json!({"outer": {"y": 42, "z": 7}});
        merge_json(&mut target, &patch);
        assert_eq!(
            target,
            serde_json::json!({
                "outer": {"x": 1, "y": 42, "z": 7},
                "leave_alone": 99
            })
        );
    }

    #[test]
    fn apply_json_override_changes_named_field() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Foo {
            a: i32,
            b: String,
        }
        let base = Foo {
            a: 1,
            b: "hi".to_string(),
        };
        let patch = serde_json::json!({"a": 42});
        let out = apply_json_override(&base, &patch).expect("apply ok");
        assert_eq!(
            out,
            Foo {
                a: 42,
                b: "hi".to_string()
            }
        );
    }

    /// Build a minimal skeleton/climate/biomes bundle for Earth at
    /// subdivision 2 and produce a [`RegionalDetail`] for tile 0.
    fn sample_regional_detail(seed: u64) -> RegionalDetail {
        use ymir_biome::BiomeMapConfig;
        use ymir_climate::ClimateConfig;
        use ymir_detail::{RegionSpec, RegionalDetailConfig};

        let body = earth_body();
        let star_ctx = sol_context();
        let atmosphere = AtmosphereModel::derive(&body, &star_ctx, true);
        let world = SkeletonWorld::build(body, atmosphere, 2, seed);
        let climate = ClimateMap::build(&world, &ClimateConfig::default());
        let biomes = BiomeMap::build(&world, &climate, &BiomeMapConfig::default());
        let spec = RegionSpec::new(0, 1);
        RegionalDetail::build(
            &world,
            &climate,
            &biomes,
            spec,
            RegionalDetailConfig {
                seed,
                ..RegionalDetailConfig::default()
            },
        )
    }

    #[test]
    fn save_then_load_round_trips_region() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let wd = WorldDirectory::create(tmp.path().join("w").as_path()).expect("create");
        let mut manifest = WorldManifest {
            version: "1.0".to_string(),
            pipeline_version: "0.1.0".to_string(),
            star_name: "Sol".to_string(),
            star_catalog_id: None,
            planet_index: 0,
            planet_name: Some("Earth".to_string()),
            seed: 1,
            created_at: "2026-04-12T00:00:00Z".to_string(),
            overrides_file: None,
            stages_computed: vec!["stellar".to_string()],
            config: GenerationConfig::default(),
            regions_generated: Vec::new(),
        };

        let region = sample_regional_detail(1);
        let path = save_region(&wd, &mut manifest, &region).expect("save region");
        assert!(path.is_file(), "region file should exist at {path:?}");
        assert_eq!(manifest.regions_generated, vec![region.spec.tile_index]);

        let loaded = load_region(&wd, region.spec.tile_index).expect("load region");

        // Byte-identical after bincode round-trip.
        let original_bytes = bincode::serialize(&region).expect("serialize original");
        let loaded_bytes = bincode::serialize(&loaded).expect("serialize loaded");
        assert_eq!(
            original_bytes, loaded_bytes,
            "RegionalDetail round-trip should be byte-identical"
        );
    }

    #[test]
    fn save_region_marks_manifest_idempotently() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let wd = WorldDirectory::create(tmp.path().join("w").as_path()).expect("create");
        let mut manifest = WorldManifest {
            version: "1.0".to_string(),
            pipeline_version: "0.1.0".to_string(),
            star_name: "Sol".to_string(),
            star_catalog_id: None,
            planet_index: 0,
            planet_name: Some("Earth".to_string()),
            seed: 1,
            created_at: "2026-04-12T00:00:00Z".to_string(),
            overrides_file: None,
            stages_computed: vec!["stellar".to_string()],
            config: GenerationConfig::default(),
            regions_generated: Vec::new(),
        };

        let region = sample_regional_detail(1);
        save_region(&wd, &mut manifest, &region).expect("save 1");
        save_region(&wd, &mut manifest, &region).expect("save 2");
        assert_eq!(
            manifest.regions_generated,
            vec![region.spec.tile_index],
            "repeated saves must not duplicate the tracked tile index"
        );
        assert!(manifest.has_region(region.spec.tile_index));
    }

    // --- Detail subcommand --------------------------------------------------

    /// Build a minimal Earth world at subdivision 2 on disk, return its
    /// directory. Shared across detail subcommand tests.
    fn earth_world_for_detail(tmp: &std::path::Path, seed: u64) -> PathBuf {
        let out = tmp.join("earth");
        let mut a = args(seed, out.clone(), 0);
        a.star = "Earth".to_string();
        run_generate(&a).expect("generate earth");
        out
    }

    #[test]
    fn detail_subcommand_succeeds_on_earth_world() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_detail(tmp.path(), 1);

        run_detail(&DetailArgs {
            world: world.clone(),
            region: 0,
            radius: 1,
            output_image: None,
            seed: None,
        })
        .expect("run_detail ok");

        let region_file = world.join("detail").join("region_0000.bin");
        assert!(
            region_file.is_file(),
            "expected region .bin at {region_file:?}"
        );

        let wd = WorldDirectory { root: world };
        let manifest = wd.load_manifest().expect("load manifest");
        assert_eq!(manifest.regions_generated, vec![0]);
    }

    #[test]
    fn detail_subcommand_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_detail(tmp.path(), 1);

        let detail_args = DetailArgs {
            world: world.clone(),
            region: 0,
            radius: 1,
            output_image: None,
            seed: None,
        };

        run_detail(&detail_args).expect("first run");
        let region_file = world.join("detail").join("region_0000.bin");
        let bytes_a = std::fs::read(&region_file).expect("read a");

        run_detail(&detail_args).expect("second run");
        let bytes_b = std::fs::read(&region_file).expect("read b");

        assert_eq!(
            bytes_a, bytes_b,
            "back-to-back `ymir detail` calls must write byte-identical region files"
        );

        let wd = WorldDirectory { root: world };
        let manifest = wd.load_manifest().expect("load manifest");
        assert_eq!(
            manifest.regions_generated,
            vec![0],
            "manifest must record the region exactly once"
        );
    }

    #[test]
    fn detail_with_output_image_writes_png() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_detail(tmp.path(), 1);
        let png_path = tmp.path().join("region0.png");

        run_detail(&DetailArgs {
            world: world.clone(),
            region: 0,
            radius: 1,
            output_image: Some(png_path.clone()),
            seed: None,
        })
        .expect("run_detail ok");

        assert!(png_path.is_file(), "PNG should exist at {png_path:?}");
        let decoded = image::open(&png_path).expect("decodes as a valid image");
        assert!(decoded.width() > 0);
        assert!(decoded.height() > 0);
    }

    // -------------------------------------------------------------------------
    // CORE-08: override authoring helpers
    // -------------------------------------------------------------------------

    /// Generate a minimal Earth world for override tests.
    fn earth_world_for_override(tmp: &std::path::Path) -> PathBuf {
        let out = tmp.join("world");
        let mut a = args(1, out.clone(), 0);
        a.star = "Earth".to_string();
        run_generate(&a).expect("generate earth for override test");
        out
    }

    // --- parse_field_path -------------------------------------------------------

    #[test]
    fn parse_field_path_valid() {
        let (stage, tail) = parse_field_path("orbital_body.radius").unwrap();
        assert_eq!(stage, "orbital_body");
        assert_eq!(tail, "radius");
    }

    #[test]
    fn parse_field_path_nested() {
        let (stage, tail) = parse_field_path("atmosphere.surface_pressure").unwrap();
        assert_eq!(stage, "atmosphere");
        assert_eq!(tail, "surface_pressure");
    }

    #[test]
    fn parse_field_path_rejects_missing_dot() {
        let err = parse_field_path("orbital_body").unwrap_err();
        assert!(
            err.contains("expected '<stage>.<field>'"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn parse_field_path_rejects_aggregate_skeleton() {
        let err = parse_field_path("skeleton.anything").unwrap_err();
        assert!(err.contains("aggregate"), "unexpected: {err}");
    }

    #[test]
    fn parse_field_path_rejects_aggregate_climate() {
        let err = parse_field_path("climate.temperature").unwrap_err();
        assert!(err.contains("aggregate"), "unexpected: {err}");
    }

    #[test]
    fn parse_field_path_rejects_stellar() {
        let err = parse_field_path("stellar.effective_temp").unwrap_err();
        assert!(
            err.contains("stellar"),
            "should mention stellar stage: {err}"
        );
    }

    #[test]
    fn parse_field_path_rejects_unknown_stage() {
        let err = parse_field_path("bogus.field").unwrap_err();
        assert!(err.contains("unknown stage"), "unexpected: {err}");
    }

    // --- parse_field_path_lenient -----------------------------------------------

    #[test]
    fn parse_field_path_lenient_accepts_stellar() {
        let (stage, tail) = parse_field_path_lenient("stellar.effective_temp").unwrap();
        assert_eq!(stage, "stellar");
        assert_eq!(tail, "effective_temp");
    }

    #[test]
    fn parse_field_path_lenient_rejects_aggregate() {
        let err = parse_field_path_lenient("skeleton.anything").unwrap_err();
        assert!(err.contains("aggregate"), "unexpected: {err}");
    }

    // --- add / list / remove round-trip -----------------------------------------

    #[test]
    fn override_add_creates_overrides_json() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());

        run_override_add(&OverrideAddArgs {
            world: world.clone(),
            field: "orbital_body.radius".to_string(),
            value: "1.073".to_string(),
            unit: Some("R_earth".to_string()),
            reference: "Gilbert+ 2023".to_string(),
            instrument: "TESS".to_string(),
        })
        .expect("override add should succeed");

        let ov_path = world.join("overrides.json");
        assert!(ov_path.is_file(), "overrides.json should be created");
        let ov_file = OverrideFile::load(&ov_path).expect("should parse");
        assert!(
            ov_file.overrides.orbital_body.is_some(),
            "orbital_body slot should be set"
        );
        let ob = ov_file.overrides.orbital_body.unwrap();
        let radius = ob.get("radius").expect("radius key missing");
        let val = radius.get("value").expect("value key missing");
        assert!(
            (val.as_f64().unwrap() - 1.073).abs() < 1e-9,
            "value mismatch: {val}"
        );
    }

    #[test]
    fn override_list_shows_added_field() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());

        run_override_add(&OverrideAddArgs {
            world: world.clone(),
            field: "orbital_body.mass".to_string(),
            value: "1.07".to_string(),
            unit: None,
            reference: "Test ref".to_string(),
            instrument: "TESS".to_string(),
        })
        .expect("add ok");

        // list should return Ok and not error.
        run_override_list(&world).expect("list ok");
    }

    #[test]
    fn override_remove_removes_field() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());

        run_override_add(&OverrideAddArgs {
            world: world.clone(),
            field: "orbital_body.radius".to_string(),
            value: "1.073".to_string(),
            unit: None,
            reference: "ref".to_string(),
            instrument: "inst".to_string(),
        })
        .expect("add ok");

        run_override_remove(&world, "orbital_body.radius").expect("remove ok");

        let ov_path = world.join("overrides.json");
        let ov_file = OverrideFile::load(&ov_path).expect("load ok");
        // After removing the only field, the slot should be cleared.
        assert!(
            ov_file.overrides.orbital_body.is_none(),
            "orbital_body slot should be None after removing last field"
        );
    }

    #[test]
    fn override_add_remove_multiple_fields_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());

        run_override_add(&OverrideAddArgs {
            world: world.clone(),
            field: "orbital_body.radius".to_string(),
            value: "1.073".to_string(),
            unit: Some("R_earth".to_string()),
            reference: "ref1".to_string(),
            instrument: "inst1".to_string(),
        })
        .expect("add radius ok");

        run_override_add(&OverrideAddArgs {
            world: world.clone(),
            field: "orbital_body.mass".to_string(),
            value: "1.07".to_string(),
            unit: Some("M_earth".to_string()),
            reference: "ref2".to_string(),
            instrument: "inst2".to_string(),
        })
        .expect("add mass ok");

        // Both fields present.
        let ov_path = world.join("overrides.json");
        let ov_file = OverrideFile::load(&ov_path).expect("load");
        let ob = ov_file.overrides.orbital_body.as_ref().unwrap();
        assert!(ob.get("radius").is_some());
        assert!(ob.get("mass").is_some());

        // Remove just radius.
        run_override_remove(&world, "orbital_body.radius").expect("remove radius ok");
        let ov_file2 = OverrideFile::load(&ov_path).expect("load2");
        let ob2 = ov_file2.overrides.orbital_body.as_ref().unwrap();
        assert!(ob2.get("radius").is_none(), "radius should be gone");
        assert!(ob2.get("mass").is_some(), "mass should remain");
    }

    #[test]
    fn override_remove_missing_field_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());

        run_override_add(&OverrideAddArgs {
            world: world.clone(),
            field: "orbital_body.radius".to_string(),
            value: "1.0".to_string(),
            unit: None,
            reference: "ref".to_string(),
            instrument: "inst".to_string(),
        })
        .expect("add ok");

        let err = run_override_remove(&world, "orbital_body.mass").unwrap_err();
        assert!(
            err.contains("not found"),
            "expected 'not found' error: {err}"
        );
    }

    // --- validate ----------------------------------------------------------------

    #[test]
    fn override_validate_accepts_known_field() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());
        // radius is a Sourced<f64> field on OrbitalBody.
        run_override_validate(&world, "orbital_body.radius").expect("validate ok");
    }

    #[test]
    fn override_validate_accepts_stellar_field() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());
        // effective_temp is a Sourced<f64> field on StarContext.
        run_override_validate(&world, "stellar.effective_temp").expect("validate stellar ok");
    }

    #[test]
    fn override_validate_accepts_atmosphere_field() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());
        run_override_validate(&world, "atmosphere.surface_pressure").expect("validate atmo ok");
    }

    #[test]
    fn override_validate_rejects_unknown_field() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());
        let err = run_override_validate(&world, "orbital_body.nonexistent_field").unwrap_err();
        assert!(
            err.contains("not found") || err.contains("unknown"),
            "expected path-not-found error: {err}"
        );
    }

    #[test]
    fn override_validate_rejects_aggregate_stage() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());
        let err = run_override_validate(&world, "skeleton.anything").unwrap_err();
        assert!(
            err.contains("aggregate"),
            "expected aggregate-stage rejection: {err}"
        );
    }

    #[test]
    fn override_validate_rejects_unknown_stage() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());
        let err = run_override_validate(&world, "bogus.field").unwrap_err();
        assert!(
            err.contains("unknown"),
            "expected unknown-stage error: {err}"
        );
    }

    #[test]
    fn override_add_rejects_aggregate_stage() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());
        let err = run_override_add(&OverrideAddArgs {
            world,
            field: "skeleton.anything".to_string(),
            value: "1.0".to_string(),
            unit: None,
            reference: "ref".to_string(),
            instrument: "inst".to_string(),
        })
        .unwrap_err();
        assert!(
            err.contains("aggregate") || err.contains("skeleton"),
            "expected aggregate-stage rejection: {err}"
        );
    }

    #[test]
    fn override_list_empty_when_no_overrides_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let world = earth_world_for_override(tmp.path());
        // No overrides.json written yet — list should succeed and say "none".
        run_override_list(&world).expect("list on missing file should succeed");
    }
}
