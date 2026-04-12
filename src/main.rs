//! Ymir: a causal star-to-surface planet simulator grounded in real astronomical data.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use ymir_storage::manifest::{GenerationConfig, WorldManifest};
use ymir_storage::world_io::WorldDirectory;

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
    /// Display information about a generated world.
    Info {
        /// Path to the world directory.
        path: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Info { path } => match run_info(&path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        },
    }
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
