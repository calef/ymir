# Contributing to Ymir

Thanks for your interest in Ymir. This document covers how to report issues, propose changes, set up a dev environment, claim a task, and ship a pull request. Keep it open while you work; the gates here match what CI enforces.

## Reporting Bugs

File a GitHub issue using the bug report template. A useful report includes:

- What you ran (exact CLI invocation, seed, star name, subdivision level).
- What you expected and what actually happened.
- A minimal reproduction if the bug is programmatic (a short Rust snippet or a test case is ideal).
- Output of `rustc --version` and `cargo --version`, plus your OS.
- Any panic backtrace (`RUST_BACKTRACE=1`) or relevant log excerpt.

If the bug touches determinism, reproducibility, or the physical plausibility of generated worlds, say so explicitly. Those are first-class concerns for this project.

## Proposing Features

All work is tracked in `TASKS.md` at the repo root. Before opening a feature request:

1. Skim `TASKS.md` to see whether the work is already captured (possibly as `pending` or `blocked`).
2. Skim `ARCHITECTURE.md` to see whether the feature contradicts or extends the current design.
3. Open a GitHub Discussion or an issue with the feature request template. Describe the motivation, sketch the API or CLI surface, and note any design-doc sections affected.

Accepted proposals become new entries in `TASKS.md` with a stable ID, dependencies, and a status. Implementation then follows the task lifecycle below.

## Minimum Supported Rust Version (MSRV)

Ymir pins its toolchain in `rust-toolchain.toml` at the repo root. The current MSRV is **Rust 1.88**. Edition 2024 sets the floor at 1.85, and the `image` crate (used by `ymir-render`) plus stabilized standard library APIs used in the workspace (e.g., `u32::is_multiple_of`) push the effective floor to 1.88.

If you have `rustup` installed, `cargo` will automatically pick up the pinned channel when you run any command inside the repo. CI uses `actions-rust-lang/setup-rust-toolchain@v1`, which honors `rust-toolchain.toml` directly. Raising the MSRV is a deliberate decision; open an issue or discussion before bumping the pinned channel.

## Dev Setup

Ymir is a Cargo workspace. You need a recent stable Rust toolchain (rustup is the easiest path).

Per-crate quick checks:

```bash
cargo check -p ymir-core
cargo test -p ymir-core
```

Full-workspace quality gates (what CI runs):

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --no-deps --workspace
```

Auto-format before committing:

```bash
cargo fmt
```

Integration tests can be slow in debug mode. If a test suite crawls, try `cargo test --workspace --release`.

## Catalog Setup

Ymir ingests real stellar data from Gaia DR3. The working catalog is a
pre-filtered Parquet subset (stars within 100 parsecs of Sol, parallax error
under 20%, known T_eff and luminosity) and lives at
`data/catalog/gaia_dr3_100pc.parquet`. It is ~10 MB on disk and is **not**
committed; `data/catalog/*.parquet` is gitignored.

To fetch it:

```bash
./scripts/fetch_gaia_catalog.py
```

The script submits an async ADQL job to the ESA Gaia archive TAP service,
downloads the CSV result, and writes a Zstd-compressed Parquet file. The
run takes a few minutes end-to-end (mostly waiting on the TAP job). It is
idempotent: re-running skips the download if the output already exists and
is at least 20 MB. Delete the file to force a refresh.

Dependencies: `uv` (the script uses `uv run` with `requests`, `pyarrow`,
and `pandas` pinned in its shebang). Install `uv` via Homebrew
(`brew install uv`) or `pipx install uv`.

Tests and CI do not need the full catalog. A 50-row fixture is committed
at `crates/ymir-catalog/fixtures/gaia_sample.parquet`; see the fixture's
`README.md` for how it was derived. Rebuild it with
`scripts/build_gaia_fixture.py` after a catalog refresh.

### Citation

Gaia Collaboration, Vallenari A., Brown A.G.A., Prusti T., et al. 2023,
*A&A* 674, A1. Cite this paper (and the ESA/Gaia mission paper, Gaia
Collaboration 2016, *A&A* 595, A1) in any published work built on the
catalog.

### NASA Exoplanet Archive

The real confirmed-planet overlay comes from the NASA Exoplanet
Archive's `pscomppars` composite-parameters table. One row per planet,
~6,200 rows as of 2026. The working catalog lives at
`data/catalog/exoplanet_archive.csv`, is under 1 MB, and is **not**
committed (`data/catalog/*.csv` is gitignored alongside the Parquet
files).

To fetch it:

```bash
./scripts/fetch_exoplanet_catalog.py
```

The script runs a sync ADQL query against the Exoplanet Archive TAP
service at `https://exoplanetarchive.ipac.caltech.edu/TAP/sync` and
writes the CSV directly. The run takes a few seconds. It is
idempotent: re-running skips the download if the output already exists
and is at least 500 KB. Delete the file to force a refresh.

Dependencies: `uv` (the script uses `uv run` with `requests` pinned in
its shebang).

Tests and CI do not need the full CSV. A ~46-row fixture covering ten
well-known systems (Proxima Centauri, Tau Ceti, TRAPPIST-1,
Kepler-186, KOI-351 / Kepler-90, TOI-700, HD 10180, 55 Cancri,
HR 8799, PDS 70) is committed at
`crates/ymir-catalog/fixtures/exoplanet_sample.csv`. Rebuild it with
`scripts/build_exoplanet_fixture.py` after a catalog refresh.

#### Citation

NASA Exoplanet Archive, operated by the California Institute of
Technology under contract with the National Aeronautics and Space
Administration. DOI: `10.26133/NEA12` (`pscomppars` table).

## Claiming a Task

`TASKS.md` is the source of truth for what's ready to pick up. The lifecycle:

1. Find a task with `Status: ready`. Read its body, its dependencies, and any referenced design-doc section.
2. Edit the task header: set `Status: in-progress` and put your name or handle in `Assignee`. Commit that change as part of your first PR or as a small prep commit.
3. Implement. Write tests. Document public items.
4. Run the quality gates listed above. All four must pass on the affected crates (and ideally the full workspace).
5. Open a PR. On merge, set the task to `Status: done` and add a short `NOTE:` line summarizing what landed and any surprises.
6. Cascade: scan `pending` tasks whose `Depends on` list is now fully `done`. Flip each of those to `ready`.

If a task cannot proceed (needs a human decision, external input, or an unresolved design question), set `Status: blocked` and fill in `Blocked:` with the reason. Don't silently skip it.

## Pull Requests

- Branch from `main`. Keep PRs focused; one task per PR is the norm.
- Conventional-commit style subjects are welcomed but not required: `feat(surface): add Voronoi plate partition`, `fix(climate): clamp albedo at pole`, `docs: expand CONTRIBUTING`.
- Reference the task ID in the PR description (e.g. `Implements SURF-04.`). If the PR closes a task, say so explicitly.
- Fill out the PR template. It prompts for a summary, test plan, and any design-doc sections touched.
- CI must be green before review. If CI fails on something unrelated to your change, flag it in the PR rather than disabling the check.

## Commit Sign-Off (DCO)

Ymir uses the Developer Certificate of Origin. Every commit must carry a `Signed-off-by:` trailer asserting that you have the right to contribute the code under the project's license. The easiest way is the `--signoff` flag:

```bash
git commit --signoff -m "feat(core): add Sourced<T>"
```

That appends a line like:

```
Signed-off-by: Jane Doe <jane@example.com>
```

Use your real name and a reachable email. See <https://developercertificate.org/> for the full text of the DCO. PRs without sign-off will be asked to amend.

## Hard Rules

These are non-negotiable. CI enforces them; a PR that breaks one won't merge.

- **Public items are documented.** Every `pub` type, function, trait, and module has a `#[doc]` comment. `cargo doc --no-deps` runs in CI with rustdoc warnings treated as errors.
- **No sibling-crate type leaks.** The workspace has a strict layered dependency graph (see `ARCHITECTURE.md` §7.2). A crate must not re-export or expose concrete types from a sibling through its public API. Cross-crate sharing goes through traits defined in `ymir-core`.
- **Floating-point comparisons use `approx`.** Don't use `==` or `!=` on `f32`/`f64` in tests. Use `approx::assert_relative_eq!` or `approx::assert_abs_diff_eq!` with an explicit tolerance.
- **All randomness flows from the world seed.** Use `rand_pcg` (the PCG family). Seed every RNG deterministically from the top-level world seed. No `thread_rng`, no clock-based seeding, no nondeterministic ordering in pipeline outputs.
- **New dependencies must pass `cargo deny check`.** CI runs `cargo deny check advisories licenses sources bans` against `deny.toml`. A new crate is accepted only if its license is in the allow list (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Unicode-DFS-2016, Unicode-3.0, CC0-1.0, Zlib, MPL-2.0), it comes from crates.io, and it carries no open RustSec advisories. Install `cargo-deny` locally (`cargo install cargo-deny`) and run `cargo deny check` before adding a dependency.

## Publishing to crates.io

Workspace-level `cargo publish` is currently **gated** on pushing the repo to a real GitHub remote. Every crate inherits `repository`, `homepage`, and the README from `[workspace.package]` in the root `Cargo.toml`, and those fields point at the placeholder `https://github.com/TODO/ymir`. Before the first publish:

1. Push the repo to its real GitHub location.
2. Update `repository` and `homepage` in `[workspace.package]` (root `Cargo.toml`) to the real URL.
3. Replace the TODO markers in `README.md` (CI badge, crates.io badge, docs.rs links) with the real paths.
4. Publish in dependency-layer order: `ymir-core` first, then `ymir-catalog`, `ymir-system`, `ymir-atmosphere`, `ymir-surface`, `ymir-climate`, `ymir-biome`, `ymir-detail`, `ymir-render`, `ymir-storage`. Each publish waits on the index to refresh before the next layer's crates can resolve it.

The `ymir` binary crate at the repo root sets `publish = false`; it is an end-user CLI, not a reusable library. Revisit that choice if/when a CLI distribution strategy (Homebrew, cargo-binstall, etc.) is picked.

## Getting Help

- **Security vulnerabilities:** follow the policy in `SECURITY.md`. Do not open a public issue for a suspected vulnerability.
- **Code of conduct concerns:** see `CODE_OF_CONDUCT.md` for reporting channels.
- **Everything else:** open a GitHub issue for bugs and concrete work items, or start a Discussion for open-ended questions, design conversations, and help requests.
