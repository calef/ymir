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
use ymir_core::WorldRng;
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

/// Run the full Phase 1 pipeline and persist the resulting world.
fn run_generate(args: &GenerateArgs) -> Result<(), String> {
    println!("[1/7] star context: {}", args.star);
    let selection = lookup_star(&args.star)?;

    let (star_ctx, body) = match selection {
        StarSelection::Derived { star, known } => {
            println!("[2/7] system placement (seed={})...", args.seed);
            let mut placement_rng = WorldRng::new(args.seed).child("placement");
            let placed = place_planets(
                &star,
                &known,
                &mut placement_rng,
                &PlacementConfig::default(),
            );
            if placed.is_empty() {
                return Err("placement produced no planets".to_string());
            }
            if args.planet >= placed.len() {
                return Err(format!(
                    "planet index {} out of range (system has {} planets)",
                    args.planet,
                    placed.len()
                ));
            }
            let chosen = &placed[args.planet];
            let chosen_label = chosen
                .name
                .clone()
                .unwrap_or_else(|| format!("planet #{}", args.planet));
            println!(
                "       chose {} at {:.3} AU (R = {:.2} R_earth, known = {})",
                chosen_label, chosen.semi_major_axis, chosen.radius, chosen.is_known
            );

            println!("[3/7] bulk properties...");
            let mut body_rng = WorldRng::new(args.seed).child("body");
            let derived = derive_body(chosen, &star, &mut body_rng, args.planet as u64);
            (star, derived)
        }
        StarSelection::Fixed { star, body } => {
            // Hand-filled Solar-system bodies bypass placement and derivation,
            // so --planet must be 0 (the single fixed body). Any other value
            // is rejected loudly to avoid silently ignoring user input.
            if args.planet != 0 {
                return Err(format!(
                    "--planet must be 0 for hand-filled Sol-system bodies (got {}); \
                     Earth and Mars are returned as single fixed bodies",
                    args.planet
                ));
            }
            let body_label = body.name.clone().unwrap_or_else(|| "planet #0".to_string());
            println!(
                "[2/7] fixed Sol-system body: {} at {:.3} AU (bypassing placement)",
                body_label, body.semi_major_axis
            );
            println!(
                "[3/7] hand-filled bulk properties (M = {:.3} M_earth, R = {:.3} R_earth)",
                body.mass, body.radius
            );
            (star, body)
        }
    };

    println!("[4/7] atmosphere (biology = {})...", args.enable_biology);
    let atmosphere = AtmosphereModel::derive(&body, &star_ctx, args.enable_biology);

    println!("[5/7] skeleton (subdivision = {})...", args.subdivision);
    let skeleton = SkeletonWorld::build(
        body.clone(),
        atmosphere.clone(),
        args.subdivision,
        args.seed,
    );

    println!("[6/7] persisting world to {}", args.output.display());
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
        "[7/7] rendering elevation preview ({}x{})...",
        args.preview_width, args.preview_height
    );
    let render_cfg = GlobeRenderConfig {
        width: args.preview_width,
        height: args.preview_height,
        background: [0, 0, 0],
    };
    let preview_path = wd.root.join("preview.png");
    render_skeleton_mollweide_to_path(&skeleton, &render_cfg, &preview_path)
        .map_err(|e| format!("failed to write preview {}: {}", preview_path.display(), e))?;

    // Phase 2 stages: climate + biomes. Run in order unless user opted out.
    let mut climate: Option<ClimateMap> = None;
    let mut biomes: Option<BiomeMap> = None;

    if !args.skip_climate {
        println!("[climate] building ClimateMap...");
        let c = ClimateMap::build(&skeleton, &ClimateConfig::default());
        save_climate(&wd, &c)?;
        stages_computed.push("climate".to_string());
        climate = Some(c);

        if !args.skip_biomes {
            println!("[biomes] building BiomeMap...");
            let climate_ref = climate.as_ref().expect("climate just computed");
            let b = BiomeMap::build(&skeleton, climate_ref, &BiomeMapConfig::default());
            save_biomes(&wd, &b)?;
            stages_computed.push("biomes".to_string());

            println!(
                "[biomes] rendering biome preview ({}x{})...",
                args.preview_width, args.preview_height
            );
            let biome_cfg = BiomeRenderConfig {
                width: args.preview_width,
                height: args.preview_height,
            };
            let biome_preview_path = wd.root.join("preview_biome.png");
            let img = render_biome_mollweide(&skeleton, &b, &biome_cfg);
            img.save(&biome_preview_path).map_err(|e| {
                format!(
                    "failed to write biome preview {}: {}",
                    biome_preview_path.display(),
                    e
                )
            })?;
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
}
