if i need to tell you something multiple time put it here

## Project scope (north star)
The guiding question for every design decision: **what if a team of engineers was rewriting this
TypeScript/Python codebase in Rust by hand?** That is the bar the output is judged against, not "does it run".
They give each value the most precise type the source supports: concrete struct/enum first, then generic
`T: Trait` when genuinely polymorphic, then `dyn Trait` for dynamic dispatch, a tagged runtime value only at
a real dynamic boundary. So: types correct all the way down to runtime, as few `SmeltUnknown`s as possible.
They would also refuse to merge a rule that only fires for one library's spelling.

Visual map of the architecture (where each concern lives): `docs/architecture/index.html`.

## Build commands (corgi required)
Use [corgi](https://corgi.build/) (`cargo install corgi-build`, one shared cache in `~/.local/share/corgi`)
instead of `cargo` for every build, check, clippy and run, from the workspace root. Tight loop (`--lib` skips inline tests):
corgi check --workspace --lib
corgi clippy --workspace --lib
ONLY on the TypeScript path, drop the `ty` Python stack with `--no-default-features` but keep the TypeScript checker
(`ts7`): plain `--no-default-features` also drops `ts7`, which skips the checker code (`#[cfg(feature = "ts7")]` in
`ts_facts.rs`) and builds a `smelt` that refuses every TypeScript build by default (`type_gate.rs`: unverified sources):
corgi check --workspace --lib --no-default-features --features smelt-transpiler/ts7
corgi clippy --workspace --lib --no-default-features --features smelt-transpiler/ts7
Also `corgi build --workspace`, `corgi run -p smelt-transpiler --bin smelt -- <args>`. Full check before a commit only:
cargo clippy --workspace --all-targets
cargo test

- Plain `cargo` only for `cargo test`, `cargo clippy --all-targets`, `cargo install`, third-party subcommands
  (`corgi test` does not work: integration tests spawn `cargo` on generated crates and read repo-wide files).
- CI clippy fails on deny-level lints and on any rise in `blocker-logs/clippy-baseline.json` (per-crate,
  per-lint counts; may only fall). Locally: `cargo clippy --workspace --all-targets --keep-going --message-format=json > clippy.json`,
  then `python3 scripts/check_clippy_ratchet.py --baseline blocker-logs/clippy-baseline.json --messages clippy.json --cargo-status $?`
  (`--update` when a count fell).
- Artifacts: note `du -sh ~/.local/share/corgi` before a heavy build and report it if it grew > ~2 GB. No
  `--target-dir` exports or plain `cargo build` in worktrees or `/tmp`; scratch and generated-crate builds go
  under `~/scratch/<task>/`, deleted at the end. When done: `git worktree remove` your worktrees,
  `corgi clean --older-than 3d` (`corgi clean --cache` only with the user's OK), no builds or runners left running.

## Generated Rust investigation
- Warnings/blockers: `corgi run -p smelt-transpiler --bin smelt -- rust-diagnostics --cargo-manifest <generated-crate>/Cargo.toml --output blocker-logs/<name>.md`
  (sorted by count: start with the biggest class)
- Runtime-compatibility failures: `skills/smelt-debug-workflow/SKILL.md` and `smelt rust-test-report`, not
  repeated hand-run test/diagnostics commands. Report to `blocker-logs/<name>.md`; load raw context only for the selected failure family.
- Reports in `blocker-logs/` are temporary: delete yours in the PR that closes the investigation, durable conclusions go to `docs/` (`blocker-logs/README.md`).

## Code rules
- Keep Rust emission helpers in separate modules and document them carefully. A codegen feature grown too
  large uses a well-known Rust library over custom machinery.
- Finish the active feature phase before broad division refactors (unless a small split is low-risk and
  directly curbs current-file growth); put new code in existing focused modules, then do a deliberate
  architecture pass once the phase stabilizes.
- Frontend validation boundaries: where `tsc`/Python checks reject invalid source first, HIR/MIR may share
  representations (Map and Record sharing Dict); do not block useful mappings because source spelling is
  erased internally. Keep frontend checks/tests for shapes Smelt can cheaply validate itself.
-  Run [`deslop-please`](https://github.com/deterministic-machines/deslop-please) to check for sloppy code structure.
-  Errors flagged by `deslop-please` must be fixed from first principles, don't do mechanical moves to hide them.
    - `module-cycle` & `tangle`: what is the indented layer of abstraction? Which module should be at which layer?
    - `field-leak` & `shallow-module`: Why we have these fields/functions at all? What are the patterns of use? Can they be hidden behind a narrow interface? Don't fix each leak one at a time, fix them holistically.
    - Draw a diagram to help user understand the before and proposed after structure.

## Type lowering
- WE DO NOT DO SPECIAL CASES FOR CODE, everything must lower through general rules, except test functions.
- Qualified type references preserve or resolve the full alias path, never `Namespace.Member` → `Class(Member)`.

## SmeltUnknown boundaries
- `SmeltUnknown` is never the default internal ABI for a value with useful static shape. Order: concrete types, scoped
  generics/type parameters, downstream specialization; tagged `SmeltUnknown` only at real dynamic
  boundaries (source `unknown`, erased interop, JSON/plugin values, runtime-narrowed values).
- A TS `unknown` that is only type-level plumbing: preserve or recover the concrete/generic shape.
- New conversions are explicit boundary adapters (`IntoSmeltUnknown`, checked casts, guards). Never use
  `SmeltUnknown` merely to make generated Rust compile, reconcile union arms, bypass missing flow narrowing,
  or avoid writing typed adapters.

## SmeltUnknown enforcement
- Before introducing or expanding `SmeltUnknown`: document the genuine dynamic boundary in a code comment and add a
  regression test proving concrete types, unions, or scoped generics cannot represent it.
- When touching existing `SmeltUnknown` code, try a concrete type, generated union, or generic; report any net increase.
- Measure with `smelt smelt-unknown-report <generated-crate>/src --baseline <baseline>` (method:
  `blocker-logs/smelt-unknown-report.md`). A PR that regenerates a corpus includes the report delta; an avoidable
  fall re-snapshots in the same commit.

## Typedness ratchet
`smelt typedness-report` counts each program's values, property reads and calls as concrete, generic or erased
(`SmeltUnknown`/`SmeltErasedFunction` anywhere in the type); `smelt build` writes `<output>/smelt-typedness.json`.
Method: `blocker-logs/typedness-report.md`.
- More typed: re-snapshot with `--update-baseline` in the same commit.

## Verification contracts and audit evidence
Read `docs/verification-and-audit.md` before correcting an expectation, adding a general lowering rule, auditing,
changing allocation/copying/recursion/size arithmetic/runtime adapters, or optimizing.
- Source behavior and type preservation are requirements independent of the implementation. Never make a fix
  pass by weakening assertions, deleting coverage, or broadening erasure classifications (a relaxed baseline is
  refused by the ratchets, see the table below); a wrong expectation is corrected by documenting old and new
  requirement, source evidence, and why the guarantee holds.
- Fix lowering, emission, or runtime helpers at the source; editing generated Rust is investigation only.
  Regenerate affected committed artifacts and verify they match; validate the final committed tree and rerun
  affected checks if it changes.
- A general lowering rule states its behavioral and type-preservation requirements, checked on valid source
  (runtime behavior and emitted types); compilation proves neither. High-risk rule: plant a defect, see the check fail, restore, record it.
- Audits judge behavior, type preservation and weakened checks/stale output separately, shared helpers included,
  with concrete evidence; resource changes are checked on boundary inputs; benchmarks are defined before optimizing.
- Report exactly what the evidence establishes and what remains unchecked.

## Rules a check enforces
Each row is a rule that used to be prose here. The check's error names the fix; do not restate the rule elsewhere.

| Rule | Enforced by |
| --- | --- |
| Every module and function has a docstring | `missing_docs` and `clippy::missing_docs_in_private_items` in `Cargo.toml` `[workspace.lints]`, counted by `blocker-logs/clippy-baseline.json` |
| No clippy lint count rises | `scripts/check_clippy_ratchet.py` against `blocker-logs/clippy-baseline.json`, CI `build` job |
| Every non-`.rs` file read with `include_str!` is declared in `corgi.toml` `[extra-inputs]` | `scripts/check_source_rules.py` (`corgi-inputs`), CI `ci-gates` job |
| A frontend never interns `Type::Unknown` bare: it calls `erase(reason)` (`docs/erasure-reasons.md`), or `unknown_type(NotNewErasure::..)` for a comparison or an already-erased value | `scripts/check_source_rules.py` (`bare-erasure`); the `NotNewErasure` enum in `crates/smelt-frontend-ts/src/lowering/erasure.rs` is the allowlist |
| The emitter writes a generated file only when its bytes change | `identical_bytes_leave_the_file_untouched_and_changed_bytes_rewrite_it` in `crates/smelt-codegen-rust/src/crate_writer.rs` |
| `CLAUDE.md` is `AGENTS.md`, and AGENTS.md names no missing path | `scripts/check_source_rules.py` (`agent-docs`) |
| Examples corpus: avoidable erasure stays 0 | `blocker-logs/smelt-unknown-baseline.json`, `--fail-on-regression`, blocking |
| es-toolkit corpus: avoidable erasure only stays equal or falls | `blocker-logs/smelt-unknown-baseline-es-toolkit.json`, `--fail-on-regression`, blocking |
| remeda, hono corpora: a diff reference | `blocker-logs/smelt-unknown-baseline-remeda.json`, `blocker-logs/smelt-unknown-baseline-hono.json`, never block |
| A scan that found no files, or lost >10% of the baseline's, judges nothing and never rewrites it | `coverage_loss` in `crates/smelt-transpiler/src/unknown_report/baseline.rs` |
| No program's erased count rises (per kind, and in total); a regression cannot be snapshotted | `smelt typedness-report --fail-on-regression`: `blocker-logs/typedness-baseline.json` and `blocker-logs/typedness-baseline-es-toolkit.json` block, `blocker-logs/typedness-baseline-remeda.json` is advisory |
| A baseline program that is no longer measured fails the gate; one removed on purpose is named with `--retire-program` | `vanished` in `crates/smelt-transpiler/src/typedness_report.rs` |

## Subagents
- The main session may implement feature code directly; prefer small, well-understood changes inline and
  delegate large, parallelisable or context-heavy work.
- Honor the user's model choice, else pick a suitable available model (Opus 5.5 and Codex currently); never
  hard-code a provider alias or require Opus for every coder. Review delegated diffs; independently verify
  the claims that decide correctness.
- At most two concurrent Cargo builds across the session and subagents (separate target dirs count); Cargo-free
  work may run alongside. State in each dispatch whether the agent compiles.

## git
After each feature, push a commit with the changes and a clear description of what was implemented.
git status should be as clean as possible.

## NEVERS
NEVER reject a feature without asking me first, it doesn't matter how hard it is
