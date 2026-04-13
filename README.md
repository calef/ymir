# Ymir

Ymir, a causal star-to-surface procedural planet generator.

[![CI](https://github.com/calef/ymir/actions/workflows/ci.yml/badge.svg)](https://github.com/calef/ymir/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/rust-1.88%2B-blue.svg)](rust-toolchain.toml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
<!-- TODO: crates.io badges once published. Publishing is gated on pushing the repo to a real GitHub -->
<!-- remote and updating the placeholder `https://github.com/TODO/ymir` URLs in the workspace root -->
<!-- Cargo.toml (`[workspace.package]`). See CONTRIBUTING.md §Publishing to crates.io. -->
<!-- [![Crates.io](https://img.shields.io/crates/v/ymir-core.svg)](https://crates.io/crates/ymir-core) -->
<!-- [![Docs.rs](https://docs.rs/ymir-core/badge.svg)](https://docs.rs/ymir-core) -->

## What Ymir is

Ymir is a deterministic simulation pipeline. It starts from a real star (or synthetic star parameters) and derives every downstream property (orbital placement, atmosphere composition, geology, climate, biomes, regional detail) through a causal chain, so any upstream change cascades naturally through the rest of the model.

Inputs are small:

- a host star (catalog entry or explicit parameters),
- a `u64` seed that threads every PRNG draw in the pipeline,
- optional observational overrides wrapped in `Sourced<T>` for any stage.

Outputs are a world directory: manifest JSON, stage artifacts, a Mollweide preview PNG, and a biome-colored PNG.

## What Ymir is not

- Not a game engine. It produces data, not a runtime.
- Not a real-time renderer. Phase 1 and 2 render via the `image` crate on CPU; wgpu integration is a future phase.
- Not a scientific tool for publication-grade simulation. Ymir trades fidelity for coherence so that a star change still produces a self-consistent planet.

## Why a causal pipeline

Every stage reads only from upstream stages, never sideways. That property makes per-stage overrides well-defined, dirty tracking trivial (mark a stage dirty, mark everything downstream dirty), and reproducibility exact. Two runs with the same star, seed, and overrides produce byte-identical artifacts.

## Quickstart

```bash
# Clone and install
git clone https://github.com/calef/ymir ymir
cd ymir
cargo install --path .

# Or run in-place without installing
cargo run --release -- generate --star Earth --seed 1 --output worlds/earth-1/

# Generate an Earth-like world
ymir generate --star Earth --seed 1 --output worlds/earth-1/

# Inspect what was generated
ymir info worlds/earth-1/

# Apply an override and recompute only dirty stages
ymir regenerate --world worlds/earth-1/ --overrides my-overrides.json
```

Supported stars in Phase 1: `Tau Ceti` (derived pipeline), `Earth` / `Sol` / `Sun` (hand-filled Earth body), `Mars` (hand-filled Mars body).

Queries against arbitrary cataloged stars require the Gaia DR3 working
catalog at `data/catalog/gaia_dr3_100pc.parquet`. That file is not committed;
run `./scripts/fetch_gaia_catalog.py` once to download it (~10 MB, a few
minutes). See `CONTRIBUTING.md` "Catalog Setup" for details. The hardcoded
Earth / Mars / Tau Ceti paths work without the catalog file.

After running the quickstart, two preview images appear in the output directory:

```
worlds/earth-1/preview.png        # Mollweide elevation/shading
worlds/earth-1/preview_biome.png  # Mollweide biome classification
```

<!-- Sample previews will be embedded here once committed to the repo. -->
<!-- ![Mollweide elevation preview](worlds/earth-1/preview.png) -->
<!-- ![Biome classification preview](worlds/earth-1/preview_biome.png) -->

## Workspace layout

Ten library crates plus one binary, strictly layered. A crate may only depend on crates listed below it.

```
ymir-core        no deps
ymir-catalog     core
ymir-system      core, catalog
ymir-atmosphere  core, system
ymir-surface     core, system, atmosphere
ymir-climate     core, surface, atmosphere
ymir-biome       core, climate
ymir-detail      core, surface, climate, biome
ymir-render      core, surface, climate, biome
ymir-storage     core
```

Public APIs share types through traits defined in `ymir-core` (for example `GeoTile`, `PipelineStage`). No crate leaks concrete types from a sibling crate across its public boundary.

## Build and test

```bash
cargo check                     # type-check the workspace
cargo build                     # build all crates
cargo test                      # run all tests
cargo clippy -- -D warnings     # lint (warnings are errors)
cargo fmt --check               # formatting check
cargo doc --no-deps             # docs (all public items must carry #[doc])
```

MSRV is Rust 1.88 (pinned in `rust-toolchain.toml`).

## Current status

- **Phase 1 complete.** Star context, orbital placement, atmosphere, skeleton/heightmap, rendering, CLI.
- **Phase 2 complete.** Climate, biomes, per-stage override plumbing, validation suite.
- **Phase 3 in progress.** Regional detail (terrain refinement, hydrology, biome micro-structure).

See [TASKS.md](TASKS.md) for active work, blockers, and the ready queue. The file drives both human and agent contributors.

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md), the authoritative design document (stage specs, algorithms, data model, open questions).
- [CONTRIBUTING.md](CONTRIBUTING.md), how to set up a dev environment, claim a task, and open a PR.
- [CHANGELOG.md](CHANGELOG.md), release notes.
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) and [SECURITY.md](SECURITY.md).
- <!-- TODO: per-crate API docs at https://docs.rs/ymir-core, https://docs.rs/ymir-system, etc. once published to crates.io. See CONTRIBUTING.md §Publishing to crates.io. -->

## License

Dual-licensed under either of:

- MIT license, see [LICENSE-MIT](LICENSE-MIT),
- Apache License 2.0, see [LICENSE-APACHE](LICENSE-APACHE),

at your option. Contributions are accepted under the same dual license per the [DCO sign-off](CONTRIBUTING.md#commit-sign-off-dco) workflow.
