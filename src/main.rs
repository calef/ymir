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
use ymir_core::{OverrideFile, PipelineDirtyState, Stage, StageOverrides, WorldRng};
use ymir_render::biome_mollweide::{BiomeRenderConfig, render_biome_mollweide};
use ymir_render::globe_renderer::{GlobeRenderConfig, render_skeleton_mollweide_to_path};
use ymir_storage::manifest::{GenerationConfig, WorldManifest};
use ymir_storage::world_io::WorldDirectory;
use ymir_storage::{load_bin, save_bin};
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
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
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
    OrbitalBody {
        semi_major_axis: 1.0,
        eccentricity: 0.0167,
        inclination: 0.0,
        axial_tilt: 23.4,
        mass: 1.0,
        radius: 1.0,
        density: 5.51,
        surface_gravity: 9.81,
        solar_irradiance: 1361.0,
        equilibrium_temp: 254.0,
        tidal_locked: false,
        rotation_period: 24.0,
        is_in_hz: true,
        planet_type: PlanetType::Terran,
        name: Some("Earth".to_string()),
        is_known_exoplanet: false,
    }
}

/// Hand-filled [`OrbitalBody`] for Mars using published observational values.
/// Used when `--star Mars` is passed. Like [`earth_body`], bypasses the
/// placement + derivation stages to preserve ground-truth values.
fn mars_body() -> OrbitalBody {
    OrbitalBody {
        semi_major_axis: 1.524,
        eccentricity: 0.0934,
        inclination: 0.0,
        axial_tilt: 25.19,
        mass: 0.107,
        radius: 0.532,
        density: 3.93,
        surface_gravity: 3.72,
        solar_irradiance: 588.0,
        equilibrium_temp: 210.0,
        tidal_locked: false,
        rotation_period: 24.6,
        is_in_hz: false,
        planet_type: PlanetType::Terran,
        name: Some("Mars".to_string()),
        is_known_exoplanet: false,
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
                    "--planet must be 0 for hand-filled Sol-system bodies (got {}); \
                     Earth and Mars are returned as single fixed bodies",
                    planet_index
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
fn merge_json(target: &mut serde_json::Value, patch: &serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(t), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                merge_json(t.entry(k.clone()).or_insert(serde_json::Value::Null), v);
            }
        }
        (t, p) => {
            *t = p.clone();
        }
    }
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
            continental_fraction: None,
        },
    };
    wd.save_manifest(&manifest)
        .map_err(|e| format!("failed to save manifest: {e}"))?;

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
    println!("Semi-major axis: {:.4} AU", body.semi_major_axis);
    println!(
        "Mass / Radius:   {:.3} M_earth / {:.3} R_earth ({:?})",
        body.mass, body.radius, body.planet_type
    );
    println!("Surface gravity: {:.3} m/s^2", body.surface_gravity);
    println!(
        "T_eq / T_surf:   {:.1} K / {:.1} K (in HZ: {})",
        body.equilibrium_temp, atmo.effective_surface_temp, body.is_in_hz
    );
    println!(
        "Atmosphere:      {:?}, P = {:.4} bar, retained gases = {}",
        atmo.class,
        atmo.surface_pressure,
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
        println!("Star catalog ID: {}", catalog_id);
    }
    println!("Created: {}", m.created_at);
    println!("Manifest version: {}", m.version);
    println!("Pipeline version: {}", m.pipeline_version);
    if let Some(overrides) = &m.overrides_file {
        println!("Overrides file: {}", overrides);
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
        Some(v) => println!("  Continental fraction: {:.3}", v),
        None => println!("  Continental fraction: default (physics-derived)"),
    }
}

fn print_skeleton_section(skeleton: &SkeletonWorld) {
    let body = &skeleton.body;
    let atmo = &skeleton.atmosphere;
    let (e_min, e_mean, e_max) = elevation_stats(skeleton);

    println!("Body:");
    println!("  Mass:            {:.3} M_earth", body.mass);
    println!("  Radius:          {:.3} R_earth", body.radius);
    println!("  Surface gravity: {:.3} m/s^2", body.surface_gravity);
    println!("  Equilibrium T:   {:.1} K", body.equilibrium_temp);
    println!("  In HZ:           {}", body.is_in_hz);

    println!("Atmosphere:");
    println!("  Class:           {:?}", atmo.class);
    println!("  Surface pressure:{:.4} bar", atmo.surface_pressure);
    println!("  Surface T (eff): {:.1} K", atmo.effective_surface_temp);
    println!("  Retained gases:  {}", atmo.retained.len());

    println!(
        "Elevation ({} tiles):",
        skeleton.elevation.elevations_m.len()
    );
    println!("  Min:  {:.1} m", e_min);
    println!("  Mean: {:.1} m", e_mean);
    println!("  Max:  {:.1} m", e_max);
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

    println!(
        "  Temperature:     mean {:.1} K (min {:.1} K, max {:.1} K)",
        t_mean, t_min, t_max
    );
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
}
