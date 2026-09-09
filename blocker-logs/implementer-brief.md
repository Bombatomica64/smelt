# Implementer brief (shared by the Hono and standards-tier streams)

You are implementing general transpiler features in Smelt (TypeScript/Python -> Rust). Read
`CLAUDE.md` first and obey it: no function-name / library special cases; concrete types before
generics before erasure; never add `SmeltUnknown` to make Rust compile (document the boundary and
add a regression test when a genuine dynamic boundary needs it); docstrings on modules and
functions; keep emitter helpers in focused, documented modules; prefer well-known crates over
custom machinery. Never reject a feature on your own: if a plan item is impossible as designed,
say so and propose the alternative.

You are in your OWN git worktree. You ARE expected to compile; you are one of exactly two agents
allowed to run cargo, and the other one runs concurrently, so never touch a target dir that is
not yours. Disk is tight: `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, delete
generated-crate targets between gate runs, and never delete another worktree's `target-priv`.

## Setup (do this first, verbatim)
```bash
cd <your worktree root>
git merge --no-edit claude/estoolkit-test-failures-4fuf9e   # the worktree may be cut from a stale base
export CARGO_TARGET_DIR="$PWD/target-priv" CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
```
Generated crates you build (Hono, corpora, examples) use `CARGO_TARGET_DIR="$PWD/target-gen-priv"`.
**When you are completely done, `rm -rf target-priv target-gen-priv`** and say so in your report.

## Tight loop
```bash
cargo check --lib --no-default-features && cargo clippy --lib --no-default-features
```

## Verification (required, in this order)
1. Focused unit/snapshot tests for the code you changed (`smelt-transpiler` has no lib target:
   use `--bin smelt`).
2. Regression tests: extend an existing runtime tier in `crates/smelt-codegen-rust/tests/*_runtime.rs`
   (`#[ignore]`d; run with `cargo test -p smelt-codegen-rust --test <name> -- --ignored`) or create
   a new tier file AND register it in the `.github/workflows/runtime-tiers.yml` matrix. Add the
   cheaper frontend/emitter snapshot test where one applies. End-to-end fixtures live in
   `examples/typescript/end-to-end/<NN_name>/` (input.ts, expected.hir/.mir/.rs/.stdout) and are
   listed in `crates/smelt-transpiler/tests/hir_cli_cross_language_tests.rs`; take the next free
   number and diff `expected.stdout` against Node 22 before committing.
3. Gates that must stay green (run once at the end, not per edit):
   - `cargo test -p smelt-frontend-ts --no-default-features`, `cargo test -p smelt-codegen-rust`,
     `cargo test --bin smelt`, and the end-to-end test in `hir_cli_cross_language_tests.rs`.
   - examples invariant: `target-priv/debug/smelt smelt-unknown-report examples --baseline blocker-logs/smelt-unknown-baseline.json --fail-on-regression`
     (avoidable must stay 0; re-snapshot with `--format json --output` only for prelude/legitimate growth).
   - es-toolkit ratchet: clone `toss/es-toolkit` at `e008a2818cd8d07469a5cc12ee0c02405d523e07`, copy
     `.github/compat/es-toolkit/.` over it, `smelt build`, then
     `smelt smelt-unknown-report <checkout>/dist-smelt/src --baseline blocker-logs/smelt-unknown-baseline-es-toolkit.json --fail-on-regression`.
     A fall re-snapshots in the same commit; a rise blocks.
   - remeda: `remeda/remeda` at `3c80f28bb394edbf89f1fc9978571dec8ed20edc` + `.github/compat/remeda/.`,
     build, `cargo test` the generated crate: 1789 passed / 0 failed.
   - radash: `sodiray/radash` at `4cab1900d08e0997abc4f17aec3cbfe18958d766` + `.github/compat/radash/.`,
     `sed -i "/import { assert } from 'chai'/a import { describe, test } from 'vitest'" src/tests/typed.test.ts`,
     build, 84 / 84.
   - Before attributing any gate failure to another stream: regenerate the corpus from a clean
     `dist-smelt` with a freshly built `smelt`, or report it as unverified.
4. `cargo clippy --all-targets` must introduce no NEW findings in files you touched (pre-existing
   `smelt-specialize` findings are known).

## Reporting
Commit in your worktree with clear messages and **push your worktree branch to origin**
(`git push -u origin <worktree-branch>`) so nothing is lost if the machine restarts. Do NOT push to
`claude/estoolkit-test-failures-4fuf9e` and do NOT open PRs; the orchestrator merges.
New notes under `blocker-logs/` are gitignored by default: add `!blocker-logs/<name>.md` to
`.gitignore` and check `git status --ignored blocker-logs` before committing.
Final report: per item landed / not landed and why, every gate's numbers, SmeltUnknown deltas,
files touched, deviations, and anything you found but did not fix (with a note file).
