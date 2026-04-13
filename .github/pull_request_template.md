## Summary

<!-- One-line description of this PR. -->

## Linked tasks

<!-- One line per task closed. Example: -->
<!-- Closes TASKS.md#CORE-01 -->
<!-- Closes TASKS.md#RENDER-03 -->

Closes TASKS.md#CRATE-NN

## Changes

<!-- Bulleted list of what changed in this PR. -->

-
-

## Test plan

Commands run locally (check each that passed):

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace --release`
- [ ] `cargo doc --workspace --no-deps`

Manual verification (list commands and observed output, e.g. `ymir generate --star Earth --seed 1`):

-

## Screenshots / artifacts

<!-- Required for render-affecting changes. Embed PNGs inline or link to artifact paths. -->
<!-- ![Mollweide projection](docs/artifacts/example.png) -->

## Checklist

- [ ] Tests added or updated
- [ ] Public items documented (cargo doc clean with -D warnings)
- [ ] CHANGELOG.md entry added under `[Unreleased]`
- [ ] TASKS.md task(s) updated (status, NOTE if appropriate, dependency cascade)
- [ ] Commit(s) signed off per CONTRIBUTING.md (DCO)
- [ ] No new sibling-crate type leaks (per design doc §7.2 layering)
