# Ymir Project Instructions

## Build Commands

```bash
cargo check                     # Type-check the full workspace
cargo build                     # Build all crates
cargo test                      # Run all tests
cargo clippy -- -D warnings     # Lint (treat warnings as errors)
cargo fmt --check               # Check formatting
cargo fmt                       # Auto-format
cargo doc --no-deps             # Build docs (all public items must be documented)
```

To work on a single crate:

```bash
cargo check -p ymir-core
cargo test -p ymir-core
```

## Task Tracking

All work is tracked in `TASKS.md` at the repo root. Read it before starting work to understand what's ready, what's blocked, and what depends on what.

### Task lifecycle

1. Check `TASKS.md` for tasks with **Status: ready**.
2. Set the task to `in-progress` before starting work.
3. Implement, test, ensure `cargo check`, `cargo test`, and `cargo clippy -- -D warnings` pass for the affected crate.
4. Set the task to `done` when complete.
5. Check if any `pending` tasks now have all dependencies `done`; if so, update them to `ready`.

### Blocking a task

If a task cannot proceed (needs a human decision, external dependency, unresolved design question), set **Status** to `blocked` and write the reason in the **Blocked** field. Do not silently skip blocked work.

### Task format

Tasks use a fixed-format header for grepability. See the "How to Use This File" section in TASKS.md for the template and status definitions.

## Workspace Structure

Cargo workspace with 10 library crates under `crates/` and one binary crate at the repo root. Crate dependency graph is strictly layered (see design doc section 7.2). No crate should leak types from sibling crates through its public API.

```
ymir-core       → (no deps)
ymir-catalog    → core
ymir-system     → core, catalog
ymir-atmosphere → core, system
ymir-surface    → core, system, atmosphere
ymir-climate    → core, surface, atmosphere
ymir-biome      → core, climate
ymir-detail     → core, surface, climate, biome
ymir-render     → core, surface, climate, biome
ymir-storage    → core
```

## Code Conventions

- All public types implement `Serialize + Deserialize` (serde).
- All public types and functions have `#[doc]` comments.
- Internal helpers are `pub(crate)`, not `pub`.
- Use `Sourced<T>` (from ymir-core) for any pipeline value that could be overridden by observational data. Phase 1 uses per-stage dirty tracking only; per-field recomputation is deferred.
- Cross-crate type sharing goes through traits defined in ymir-core (e.g., `GeoTile`, `PipelineStage`), not concrete types from sibling crates.
- Floating-point comparisons in tests use the `approx` crate, not exact equality.
- Property-based tests via `proptest` for invariants (e.g., HZ inner < HZ outer for all valid stars).
- Builder pattern for complex stage configs with sensible defaults.
- PRNG: PCG family via `rand_pcg`. All randomness flows from the world seed. Deterministic and reproducible.

## Design Reference

The authoritative design document is `ymir-design.md` at the repo root. Refer to it for stage specifications, struct definitions, algorithms, and open questions.

## Phase 1 Scope

Phase 1 implements stages 1-4 (star context through skeleton/heightmap) plus basic rendering and CLI. The target deliverable: `ymir generate --star "Tau Ceti" --seed 42` produces a Mollweide projection PNG.

- Star catalog: hardcoded Tau Ceti data only. No Parquet ingest.
- Rendering: software rasterizer via the `image` crate. No wgpu.
- Overrides: per-stage only. Per-field recomputation is Phase 2+.
