//! Ymir: a causal star-to-surface planet simulator grounded in real astronomical data.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use ymir_atmosphere::AtmosphereModel;
use ymir_catalog::exoplanets::ExoplanetRecord;
use ymir_catalog::star_context::StarContext;
use ymir_core::WorldRng;
use ymir_render::globe_renderer::{GlobeRenderConfig, render_skeleton_mollweide_to_path};
use ymir_storage::manifest::{GenerationConfig, WorldManifest};
use ymir_storage::world_io::WorldDirectory;
use ymir_surface::skeleton::SkeletonWorld;
use ymir_system::{PlacementConfig, derive_body, place_planets};

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
        } => run_generate(&GenerateArgs {
            star,
            planet,
            seed,
            output,
            subdivision,
            enable_biology: !no_biology,
            preview_width,
            preview_height,
        }),
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
}

/// Look up a star context by case-insensitive, trimmed name. Phase 1 only
/// supports Tau Ceti.
fn lookup_star(name: &str) -> Result<(StarContext, Vec<ExoplanetRecord>), String> {
    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "tau ceti" => Ok((StarContext::tau_ceti(), ExoplanetRecord::tau_ceti_system())),
        _ => Err(format!(
            "unknown star \"{name}\" (Phase 1 supports only \"Tau Ceti\")"
        )),
    }
}

/// Run the full Phase 1 pipeline and persist the resulting world.
fn run_generate(args: &GenerateArgs) -> Result<(), String> {
    println!("[1/7] star context: {}", args.star);
    let (star_ctx, known) = lookup_star(&args.star)?;

    println!("[2/7] system placement (seed={})...", args.seed);
    let mut placement_rng = WorldRng::new(args.seed).child("placement");
    let placed = place_planets(
        &star_ctx,
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
    let body = derive_body(chosen, &star_ctx, &mut body_rng, args.planet as u64);

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
    let manifest = WorldManifest {
        version: "1.0".to_string(),
        pipeline_version: env!("CARGO_PKG_VERSION").to_string(),
        star_name: star_ctx.name.clone().unwrap_or_else(|| args.star.clone()),
        star_catalog_id: Some(star_ctx.catalog_id.clone()),
        planet_index: args.planet,
        planet_name: chosen.name.clone(),
        seed: args.seed,
        created_at: chrono::Utc::now().to_rfc3339(),
        overrides_file: None,
        stages_computed: vec![
            "stellar".to_string(),
            "system".to_string(),
            "atmosphere".to_string(),
            "skeleton".to_string(),
        ],
        config: GenerationConfig {
            grid_subdivision_level: args.subdivision,
            enable_biology: args.enable_biology,
            continental_fraction: None,
        },
    };
    wd.save_manifest(&manifest)
        .map_err(|e| format!("failed to save manifest: {e}"))?;

    println!(
        "[7/7] rendering preview ({}x{})...",
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

    println!();
    print_summary(&star_ctx, args.planet, &skeleton, &args.output);
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
fn print_summary(star: &StarContext, planet_index: usize, skeleton: &SkeletonWorld, out: &Path) {
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
        }
    }

    #[test]
    fn lookup_star_tau_ceti_case_insensitive() {
        assert!(lookup_star("Tau Ceti").is_ok());
        assert!(lookup_star("tau ceti").is_ok());
        assert!(lookup_star("  TAU CETI  ").is_ok());
    }

    #[test]
    fn lookup_star_unknown_errors() {
        let err = lookup_star("Sirius").unwrap_err();
        assert!(err.contains("unknown star"));
        assert!(err.contains("Sirius"));
    }

    #[test]
    fn generate_creates_expected_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("world");
        run_generate(&args(42, out.clone(), 0)).expect("generate ok");
        assert!(out.join("manifest.json").is_file());
        assert!(out.join("skeleton.bin").is_file());
        assert!(out.join("preview.png").is_file());

        // Manifest round-trips.
        let wd = WorldDirectory { root: out.clone() };
        let manifest = wd.load_manifest().expect("load manifest");
        assert_eq!(manifest.star_name, "Tau Ceti");
        assert_eq!(manifest.seed, 42);
        assert_eq!(manifest.planet_index, 0);
        assert!(manifest.config.enable_biology);
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
