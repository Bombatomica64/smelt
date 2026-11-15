# Architecture map

[`index.html`](index.html) is a single self-contained page (inline SVG, CSS and a little JS, no network) that draws Smelt's pipeline, crate graph, module maps, type erasure points, MIR passes, CI gates, a "where does X live" table and a churn heat map. Open it from a checkout: every box links to the file it names.

It reflects commit `df75a5d57` (2026-11-14). Every name, edge, count and ordering was read from the code at that commit; nothing was copied from `specs/`. To refresh a number, rerun the command that produced it:

- Lines of Rust: `git ls-files 'crates/*.rs' | xargs wc -l | sort -rn` (per crate or module: `git ls-files crates/<crate>/src | grep '\.rs$' | xargs wc -l`).
- Crate edges and features: `cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | .name as $p | .dependencies[] | select(.name | startswith("smelt-")) | "\($p) -> \(.name) \(.kind // "normal") optional=\(.optional)"'`, plus `[features]` in each `Cargo.toml`.
- MIR order: `sed -n '/fn compile_observed/,/^}/p' crates/smelt-mir/src/compile.rs`, `sed -n '/fn run_finalization_passes/,/^}/p' crates/smelt-mir/src/lower/mod.rs`, `sed -n '/fn fixpoint_passes/,/^}/p;/fn try_optimize/,/^}/p' crates/smelt-mir/src/opt/mod.rs`.
- Churn: `git log --since='60 days ago' --no-merges --name-only --format=@@%h` (commits per module) and the same with `--numstat` (lines changed).
- Erasure and CI numbers: `jq '.erasure.by_reason' blocker-logs/smelt-unknown-baseline-{es-toolkit,remeda}.json`, the other `blocker-logs/*baseline*.json` files, and the pinned `test result` lines in `.github/workflows/ci.yml`.

To regenerate the whole page: `python3 docs/architecture/gen/churn.py && python3 docs/architecture/gen/gen.py && python3 docs/architecture/gen/validate.py docs/architecture/index.html`. Counts, edges, churn and dates are read from the checkout; the pipeline layout and the module groupings are written by hand in `gen/s*.py`, and the generator fails when a module is in no group, which is the signal that the map needs a human look.
