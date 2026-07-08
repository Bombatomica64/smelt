# Remeda regression investigation

Investigating a suspected regression in remeda transpilation caused by the
~17 es-toolkit-focused merges (`#127` .. `#143`) landed on top of `53c5b4d0`.

- Target: remeda pinned ref `3c80f28bb394edbf89f1fc9978571dec8ed20edc`
- Fixture: `.github/compat/remeda/Smelt.toml` (roots `packages/remeda/src`,
  test-prefix `**/*.test.ts`, entry `packages/remeda/src/index.ts`, strict
  TS+Py, aggressive clone strategy). Copied into the remeda checkout root,
  mimicking `scripts/probe_libraries.py`.
- Method: build a release `smelt` at the current main tip (`f3c30ca0`) and at
  the pre-es-toolkit baseline (`53c5b4d0`, the main tip immediately before the
  first es-toolkit merge `#127` = `bc37fb87`), then run the identical
  `smelt build` + `cargo check` + `cargo test` on the same remeda checkout and
  diff the two.

## Results

| Metric | Baseline `53c5b4d0` | Current `f3c30ca0` |
| --- | --- | --- |
| `smelt build` | exit 0, emits `dist-smelt` | exit 0, emits `dist-smelt` |
| generated `.rs` files | 391 | 391 (identical file set) |
| `cargo check` | passed | passed |
| generated-crate errors | 0 | 0 |
| generated-crate warnings | 239 | 238 |
| `#[test]` annotations | 1663 | 1663 |
| `cargo test` (generated) | 1789 passed; 0 failed | 1789 passed; 0 failed |
| total generated src lines | 79351 | 79362 (+11) |

The emitted file set is byte-for-byte identical in membership (`diff` of the
sorted file lists reports no differences), the `#[test]` annotation count is
identical, and the full generated `cargo test` suite passes completely on both
binaries.

## Source-level delta (benign)

136 of 391 generated files differ textually, netting +11 lines. Every hunk is
one of the two coercion-seam refinements the es-toolkit work introduced:

1. Iterable-to-list extraction now normalizes its source through the
   `IntoSmeltUnknown` boundary adapter
   (`({text}).clone().into_smelt_unknown()` instead of `{text}.clone()`) before
   matching `SmeltUnknown::` arms — `emitter/coercion.rs`. `IntoSmeltUnknown` is
   identity on an existing `SmeltUnknown` (and is also implemented for `String`,
   `Option<T>`, `SmeltList<T>`), so already-erased callers are unaffected. This
   is what accounts for remeda's `purry`-wrapped predicate callbacks (e.g.
   `allPass`, `anyPass`) reformatting.
2. Array predicate callbacks (`filter`/`find`/`some`/`every`) now route a
   non-`bool` callback result through `value_truthy_text`, and zero-parameter
   callbacks are called with no arguments — `emitter/list_query.rs`,
   `emitter/types.rs`.

Both refinements leave the generated remeda crate compiling cleanly and passing
every test, so they are behaviorally inert for remeda (if anything a marginal
improvement: one fewer warning).

## Verdict: NO REGRESSION

Remeda did **not** regress across `#127`..`#143`. Both the baseline
(`53c5b4d0`) and current-main (`f3c30ca0`) binaries produce a remeda
`dist-smelt` that:

- transpiles fully (`smelt build` exit 0, no abort),
- compiles with zero Rust errors, and
- passes all 1789 generated `cargo test` cases with zero failures.

There is no abort/error class present with the current binary that is absent
with the baseline binary — the defining shape of a regression. Per the mission
brief, no fixes were invented: there is nothing to fix. The coordinator-flagged
coercion / callback-shape / mutable-reference paths were reviewed
(`emitter/coercion.rs`, `emitter/core.rs`, `emitter/list_query.rs`,
`emitter/types.rs`, `emitter/call.rs`, `emitter/mod.rs`) and confirmed to be the
source of the benign textual delta only, not of any behavioral regression.

Because no transpiler code was changed, es-toolkit is unaffected by this
investigation (identical current-main binary), so no es-toolkit backslide is
possible from this branch.

## Reproduction

```
# baseline binary
git worktree add <wt> 53c5b4d0 && (cd <wt> && cargo build --release --bin smelt)
# current binary: cargo build --release --bin smelt on f3c30ca0

# remeda checkout
git clone https://github.com/remeda/remeda && cd remeda \
  && git checkout 3c80f28bb394edbf89f1fc9978571dec8ed20edc
cp <repo>/.github/compat/remeda/Smelt.toml ./Smelt.toml

# per binary
<smelt> build --manifest-path ./Smelt.toml
(cd dist-smelt && cargo check && RUSTFLAGS=-Awarnings cargo test --no-fail-fast)
```
