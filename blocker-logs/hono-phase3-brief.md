# Hono phase 3 — the framework's own tests against the generated crate (orchestrator brief)

Precondition: the non-test Hono crate reaches 0 `cargo check` errors (round 33). Phase 3 turns
the tests on and establishes the pass/fail baseline that phases 4 (erasure baseline) and 5 (CI
promotion) hang off. `hono-campaign-plan.md` §5 items 3–5 are the contract; this brief is the
operational plan.

## 0. Why the crate has zero tests today

`.github/compat/hono/Smelt.toml` says `test-prefix = ["src/**/*.test.ts"]`, but
`discover_test_paths` (`smelt-transpiler/src/lowering.rs`) matches globs against paths RELATIVE
TO EACH SOURCE ROOT, and the root is `src`, so the pattern demands a second `src/` segment and
matches nothing. remeda's overlay spells `**/*.test.ts` and emits 1663 `#[test]`s. First commit:
fix the glob, and add a manifest-load warning (or error) when a test glob matches zero files under
every root — a silent empty test set is exactly the failure mode that hid this for 30 rounds.

## 1. Bring the test closure in

With the glob fixed, `smelt build` from the repo root (`--manifest-path <abs>`) with a fresh
full-feature binary. 101 `*.test.ts` files exist under `src/`; 44 are outside the excluded
globs. Expect new source-lowering blockers from test-only shapes (`vi.fn`, `vi.spyOn`,
`vi.stubGlobal`, `expect.extend`, `describe.each`, `it.each`, `beforeEach` with async, `await
expect(..).rejects`, `toMatchInlineSnapshot`). Rules:
- vitest/jest surface is a HOST MODULE (`vitest`) in `smelt-stdlib`, modeled the way remeda's
  and radash's suites already exercise it; extend the model for members Hono uses, never
  special-case a test's spelling.
- `vi.stubGlobal` / `vi.spyOn` on a global are a monkey-patching model Smelt does not have
  (`hono-scope.md` §3): a test FILE that needs them is added to the overlay's `exclude` with a
  one-line reason, as the two client tests already are. Count them in the report.
- A blocker in a test file that is a general language feature (not a vitest member) is a numbered
  family like any other and gets fixed in the transpiler, not excluded.
Metric for this step: `smelt probe` files-with-blockers on the closure INCLUDING tests → 0, and
the number of `#[test]` functions in `dist-smelt` (the honest in-scope ceiling; the plan's 2081 is
the vitest count for all 101 files).

## 2. Compile the test build

`cargo check --tests` on `dist-smelt`. Test bodies reach code paths the non-test build does not
(assertion coercions, `expect(x).toEqual(y)` over concrete unions and records, `Response`
inspection). Same discipline as rounds 30–33: per-code table, families, general rules, fixtures
in `examples/typescript/end-to-end/`, push after every commit.

## 3. Run, baseline, family

`smelt rust-test-report --full --cargo-manifest <checkout>/dist-smelt/Cargo.toml --output
blocker-logs/hono-tests.md` (the tool groups failures by panic message and source file). Record
`passed / failed / total` in `hono-current.md` as the phase-3 baseline. Then family the failures
the es-toolkit way (root cause per family, not per test) and write each family into
`hono-campaign-plan.md`'s table with a proposed general rule. Do NOT start fixing families in
this phase's first round: the deliverable is the baseline and the family table. Fixes are
dispatched from that table in later rounds, two builders at a time.

## 4. Erasure baseline (phase 4, same round)

`smelt smelt-unknown-report <checkout>/dist-smelt/src --format json --output
blocker-logs/smelt-unknown-baseline-hono.json` as an advisory baseline (remeda-style; never
blocks), plus a paragraph in `hono-tests.md` naming the top five avoidable shapes with counts.

## 5. CI (phase 5)

Promote `hono-advisory` in `.github/workflows/ci.yml`: keep probe/check, add `smelt build` and
`cargo check` (hard, once 0 errors is stable for two rounds) and `cargo test --no-fail-fast`
(advisory `continue-on-error` until the phase-3 number is stable). Mirror the radash job's
structure and the `run-regressions` label gating. YAML-only; a cargo-free agent may do it in
parallel.

Gates for every commit: the implementer-brief set (examples invariant 0, es-toolkit ratchet,
remeda 1789/0, radash 84/0, suites, clippy). Notes: `blocker-logs/hono-tests.md` and
`blocker-logs/hono-phase3-round1.md`, allowlisted.
