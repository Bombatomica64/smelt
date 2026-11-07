if i need to tell you something multiple time put it here

## Project scope (north star)
the guiding question for every design decision is: **what if a team of engineers was rewriting this
TypeScript/Python codebase in Rust by hand?** That is the bar the output is judged against, not
"does it run".

A hand-writing Rust team would give a value the most precise type the source supports and carry that
type all the way down to runtime. They would reach for a concrete struct/enum first, then a generic
`T` with trait bounds when the code is genuinely polymorphic, then `dyn Trait` when they need
dynamic dispatch — and only at a real dynamic boundary would they reach for a tagged runtime value.
So: types correct all the way down to runtime, as few `SmeltUnknown`s as possible, and prefer
`T: Trait` over erasure whenever the shape is knowable. See "SmeltUnknown boundaries" below for how
that principle is enforced.

They would also not special-case: a rule that only fires for one library's spelling is a rule they
would refuse to merge. See "Type lowering".


## always run
Tight loop (`--lib` skips compiling inline tests):
cargo check --lib
cargo clippy --lib
When working ONLY on the TypeScript path (not Python), add `--no-default-features`
to drop the whole `ty` Python stack (ty_python_semantic/core/module_resolver +
ruff + smelt-frontend-py):
cargo check --lib --no-default-features
cargo clippy --lib --no-default-features
Full check before a commit only (compiles + type-checks all tests):
cargo clippy --all-targets
cargo test

## corgi (required for builds)
Agents MUST use [corgi](https://corgi.build/) instead of `cargo` for every build, check, clippy and run. It keeps one shared build cache per machine (`~/.local/share/corgi`) instead of a `target/` per worktree, so parallel worktrees and subagents stop duplicating multi-GB artifacts. Install once with `cargo install corgi-build`, then run from the workspace root:
corgi check --workspace --lib
corgi clippy --workspace --lib
corgi build --workspace
corgi run -p smelt-transpiler --bin smelt -- <args>
Use plain `cargo` only for what corgi does not support: `cargo test`, `cargo clippy --all-targets` (full check before a commit), `cargo install` and third-party subcommands. `corgi test` does not work yet: the integration tests spawn `cargo` on generated crates, bake `env!("CARGO_MANIFEST_DIR")` into binaries and read repo-wide files.

corgi sandboxes rustc, so any non-`.rs` file read at compile time (`include_str!`) must be declared in `corgi.toml` under `[extra-inputs]` (key = package name, globs relative to that crate).

### Keep track of artifacts (do not pollute the system)
- Before a heavy build, note the store size: `du -sh ~/.local/share/corgi`. Report it again in your final message if it grew by more than ~2 GB.
- The shared store is the only real build output; corgi only exports the final binaries into the worktree's `target/`. Do not add `--target-dir` exports or run plain `cargo build` (which creates a full private `target/`) in worktrees or `/tmp`. Generated-crate builds and scratch output go under `~/scratch/<task>/` and are deleted when the task ends.
- When you finish, remove worktrees you created (`git worktree remove`) and trim the store: `corgi clean --older-than 3d` (`corgi clean --cache` wipes it entirely; only with the user's OK).
- Never leave background builds or runners running past the end of the task.

## Generated Rust diagnostics
when working on generated Rust warnings or blockers, use:
`cargo run --bin smelt -- rust-diagnostics --cargo-manifest <generated-crate>/Cargo.toml --output blocker-logs/<name>.md`
This produces a grouped Markdown report sorted by diagnostic count so LLMs can start with the biggest warning/error classes.

## Generated Rust incremental builds
the Rust emitter intentionally preserves generated file mtimes by writing files only when their bytes change. This lets Cargo reuse incremental artifacts for large generated crates. Do not replace this with unconditional `fs::write`, and avoid touching/regenerating generated Rust files unless their contents actually changed.

## Generated test investigation workflow
when fixing generated Rust runtime compatibility in any source project, use `skills/smelt-debug-workflow/SKILL.md` and `smelt rust-test-report` instead of manually issuing repeated generated-crate test and diagnostics commands
write each investigation report to `blocker-logs/<name>.md`; agents should consult that readable report and load additional raw/generated context only for the selected failure family

## Style
put docstrings in modules and functions

## Rust codegen
keep Rust source emission helpers in separate modules so codegen can be refactored incrementally
document Rust codegen helper functions carefully because unclear helpers make LLM-generated changes worse
if a codegen feature becomes too large, prefer well-known Rust libraries that are likely familiar to LLMs over custom machinery

## Refactoring timing
finish active feature phases before broad codebase division refactors unless a small split is clearly low-risk and directly reduces current-file growth
put new feature code into existing focused modules where practical, then do a deliberate architecture pass after the feature phase stabilizes

## Frontend validation boundaries
when `tsc` or Python compile/type checks would reject invalid source before Smelt runs, it is ok for HIR/MIR to use interchangeable internal representations such as Map and Record sharing Dict
do not block useful mappings only because source spelling is erased internally; keep frontend checks/tests for shapes Smelt can cheaply validate itself

## Type lowering
WE DO NOT DO SPECIAL CASES FOR CODE, everything must lower through general rules, except test functions
qualified type references must preserve or resolve the full alias path instead of blindly turning `Namespace.Member` into `Class(Member)`

## SmeltUnknown boundaries
do not use `SmeltUnknown` as the default internal ABI for values that still have useful static shape
prefer concrete Rust types first, then scoped Rust generics/type parameters, then downstream specialization; use tagged `SmeltUnknown` only for real dynamic boundaries such as source `unknown`, erased interop, JSON/plugin values, or values that are inspected through runtime narrowing
when a TypeScript `unknown` spelling is only type-level helper plumbing, preserve or recover the concrete/generic shape instead of routing normal data flow through runtime tags
new `SmeltUnknown` conversions should be explicit boundary adapters (`IntoSmeltUnknown`, checked casts, guards), not a way to make ordinary generated Rust type-check

## SmeltUnknown enforcement
Before introducing or expanding `SmeltUnknown`, document the genuine dynamic boundary in a code comment and add a regression test proving concrete types, unions, or scoped generics cannot represent it.

Never use `SmeltUnknown` merely to make generated Rust compile, reconcile concrete union arms, bypass missing flow narrowing, or avoid implementing typed adapters.

When touching existing `SmeltUnknown` code, check whether the value can now use a concrete type, generated union, or generic. Report any net increase in `SmeltUnknown` usage.

Measure it: `smelt smelt-unknown-report <generated-crate>/src --baseline blocker-logs/smelt-unknown-baseline.json` classifies generated `SmeltUnknown` into runtime-prelude, legitimate-boundary, and avoidable-erasure. A rise in avoidable-erasure is a regression to justify; see `blocker-logs/smelt-unknown-report.md` for methodology.

Every erasure site names its reason (`ErasureReason`, see `docs/erasure-reasons.md`): use `erase(reason)` in the frontends, never a bare `intern(Type::Unknown)`; `unknown_type()` is only for comparisons and already-erased propagation. `smelt build` writes `<output>/smelt-erasure.json` and the unknown report explains avoidable erasure by reason and source line from it.

Three committed baselines: `blocker-logs/smelt-unknown-baseline.json` (examples corpus) is a hard invariant — avoidable stays 0, and CI enforces it with `--fail-on-regression`; `blocker-logs/smelt-unknown-baseline-es-toolkit.json` is a ratchet — avoidable may only stay equal or fall, also blocking; `blocker-logs/smelt-unknown-baseline-remeda.json` is advisory — it exists so remeda's report has something meaningful to diff against, and it never blocks. Any PR that regenerates a corpus must include the report delta. `avoidable(current) > avoidable(baseline)` blocks merge (CI runs the es-toolkit report with `--fail-on-regression`) unless the PR (1) documents the genuine dynamic boundary in a code comment at the emit site and (2) adds a regression test proving concrete types/unions/scoped generics cannot represent it — then reclassify via `classify_line` in `crates/smelt-transpiler/src/unknown_report/classify.rs` and re-snapshot the baseline in the same commit rather than accepting the increase. legitimate-boundary increases never block; avoidable decreases re-snapshot in the same commit. The comparison also fails on a scan of zero files, or one that lost more than 10% of the baseline's `files_scanned`, and such a scan never rewrites the baseline. The comparison also fails on a scan of zero files, or one that lost more than 10% of the baseline's `files_scanned`, and such a scan never rewrites the baseline.

## Typedness ratchet
The positive counterpart of the SmeltUnknown report (design adapted from Bun's `--aot` typedness tests, oven-sh/bun#45631): `smelt typedness-report` counts, on optimized MIR and through the Rust emitter's own type rendering, how many values (locals), property reads and calls of each program are concrete, generic (a type parameter carried as a real Rust generic) or erased (`SmeltUnknown`/`SmeltErasedFunction` anywhere in the Rust type). `smelt build` writes `<output>/smelt-typedness.json`; the examples are measured by lowering each `input.ts` in-process.

A program may never become less typed: for every kind (values, reads, calls) its erased count may not rise; the corpus total is judged the same way. The typed share `(concrete+generic)/total` is reported but does not judge. Old requirement (Bun's `isNoLessTyped`): erased may not rise *and* typed share may not fall. Why it changed: counts are per MIR entry, so removing redundant concrete temporaries (let-temp elision) lowers the share with types unchanged; the erased count is the quantity the north star asks to minimise, and a value that loses its type moves into it. Three baselines mirror the SmeltUnknown ones: `blocker-logs/typedness-baseline.json` (examples) and `blocker-logs/typedness-baseline-es-toolkit.json` block in CI with `--fail-on-regression`; `blocker-logs/typedness-baseline-remeda.json` is advisory. When a program gets more typed, re-snapshot with `--update-baseline` in the same commit; that rewrite only takes records whose erased counts did not rise, so a regression can never be snapshotted. A baseline program that is no longer measured fails the gate and survives `--update-baseline`; one that is gone on purpose is named with `--retire-program` in the commit that removes it. A baseline program that is no longer measured fails the gate and survives `--update-baseline`; one that is gone on purpose is named with `--retire-program` in the commit that removes it. A real regression is fixed at its source, never by editing the baseline. Methodology: `blocker-logs/typedness-report.md`.

## Verification contracts and generated output
Treat expected source behavior and type preservation as requirements independent of the implementation. Never make a fix pass by weakening assertions, deleting coverage, broadening erasure classifications, or relaxing a baseline. If an expectation is wrong, document the old and new requirement, the source-language evidence, and why the correction preserves the intended guarantee. Follow the specific baseline-update rules above.

Fix lowering, emission, or runtime helpers at their source. Edits to generated Rust may be used to investigate a failure, but are not a durable fix. When a change affects committed generated artifacts, regenerate the affected artifacts and verify that they match the committed output. Preserve unchanged file mtimes; use temporary output for comparisons where appropriate. Validate the final tree being committed, and rerun affected checks if it changes afterward.

For a new or corrected general lowering rule, state both its behavioral and type-preservation requirements. Use valid source examples and compare source-runtime behavior with generated Rust where practical; also check the emitted concrete or generic types. Successful compilation alone does not establish either requirement.

For high-risk lowering rules or suspected coverage gaps, temporarily introduce a plausible defect and verify that the relevant check fails. Examples include dropping a namespace qualifier, erasing a generic parameter, or breaking a union-narrowing branch. Restore the implementation before committing. Record which defect the check detects; a passing suite alone does not demonstrate that it would catch that defect.

## Audit evidence
Review behavioral equivalence, type preservation, and weakened checks or stale generated output as separate questions. Include shared lowering and runtime helpers, not only the changed call sites. When running repeated independent audits, give each auditor a fresh context with the requirements and relevant artifacts, and require concrete evidence for findings.

Prioritize findings reproducible through accepted TypeScript or Python source or a supported public API. For an internal-state finding, establish how a caller can reach that state; record unreachable states separately rather than treating them as demonstrated source-level failures. This does not replace frontend validation or rejection tests.

When changing allocation, copying, recursion, size arithmetic, or runtime adapters, check resource behavior with representative and boundary inputs as well as returned values. For optimization work, define the benchmark and acceptance criteria before changing the implementation; accept improvements only while behavioral and type-preservation checks still pass.

Report exactly what the evidence establishes and what remains unchecked. Tests cover their exercised cases; proofs cover their stated properties under their assumptions. Neither a clean audit round nor successful compilation establishes complete correctness. Any future formal verification must review the specification and proof statements themselves, including assumptions about compilation and runtime behavior.

## Subagents
The main session may implement feature code directly. Prefer doing small, well-understood changes inline; delegate when the work is large, parallelisable, or needs a context of its own.

Current work uses Opus 5.5 and Codex models. Do not require every code-writing subagent to use Opus or hard-code a provider-specific model alias. Honor the user's model choice; otherwise use a suitable available model, discovering the current provider/model IDs through the orchestration tools when needed. Review delegated diffs and independently verify the claims that determine correctness.

Limit concurrent Cargo builds to two across the session and its subagents; parallel rustc makes this machine lag. Cargo-free work may run alongside them within the runtime's agent limits. State in each dispatch whether the agent is expected to compile. Builds sharing a worktree's `target/` serialize on Cargo's lock; separate target directories can compile concurrently, so count them toward the same limit.

## git
After each feature, push a commit with the changes and a clear description of what was implemented
git status should as clean as possible

## NEVERS

NEVER reject a feature without asking me first, it doesn't matter how hard it is
