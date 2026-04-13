# Changelog

All notable changes to Ymir will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Dates are [ISO-8601](https://en.wikipedia.org/wiki/ISO_8601) (YYYY-MM-DD).

## [Unreleased]

### Added

### Changed

### Deprecated

### Removed

### Fixed

### Security

## [0.1.0] - 2026-04-12

First tagged development baseline. Covers Phase 1 (stellar context through
skeleton/heightmap) and Phase 2 (climate, biomes, overrides, validation).

### Added

- 10-crate Cargo workspace: `ymir-core`, `ymir-catalog`, `ymir-system`,
  `ymir-atmosphere`, `ymir-surface`, `ymir-climate`, `ymir-biome`,
  `ymir-detail`, `ymir-render`, `ymir-storage`, plus the `ymir` binary.
- Stage 1: stellar context and habitable zone computation.
- Stage 2: orbital placement and bulk planetary properties.
- Stage 3: atmosphere retention, composition, and greenhouse forcing.
- Stage 4: geodesic grid, tectonic plates, heightmap, and skeleton assembly.
- Stage 5: temperature, moisture, and wind fields producing a unified
  `ClimateMap`.
- Stage 6: Whittaker classification with Markov smoothing, `BiomeMap`
  construction, water-biome overlay, palette, and biome transitions.
- Hardcoded catalog entries for Earth, Mars, and a tidally-locked reference
  world; Earth `continental_fraction` observational override.
- `Sourced<T>` override plumbing, override file parsing (CORE-04), and a
  dependency graph with per-stage dirty-tracking (CORE-05).
- CLI commands: `ymir generate`, `ymir info`, `ymir regenerate`.
- Rendering: Mollweide projection PNG output with elevation hillshade and
  biome colors.
- Persistence: per-stage binary artifacts (`skeleton.bin`, `climate.bin`,
  `biomes.bin`) alongside a JSON manifest.
- Validation integration tests for Earth, Mars, tidally-locked, and
  override-progression scenarios.
- CI gates: `cargo fmt --check`, `cargo clippy`, `cargo test --release`, and
  `cargo doc`, all run with `-D warnings`.

### Note

This is a pre-release development baseline. APIs across all crates may change
without deprecation cycles until the 1.0 release.

[Unreleased]: https://github.com/calef/ymir/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/calef/ymir/releases/tag/v0.1.0
