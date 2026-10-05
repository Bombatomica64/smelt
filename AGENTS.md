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

Three committed baselines: `blocker-logs/smelt-unknown-baseline.json` (examples corpus) is a hard invariant — avoidable stays 0, and CI enforces it with `--fail-on-regression`; `blocker-logs/smelt-unknown-baseline-es-toolkit.json` is a ratchet — avoidable may only stay equal or fall, also blocking; `blocker-logs/smelt-unknown-baseline-remeda.json` is advisory — it exists so remeda's report has something meaningful to diff against, and it never blocks. Any PR that regenerates a corpus must include the report delta. `avoidable(current) > avoidable(baseline)` blocks merge (CI runs the es-toolkit report with `--fail-on-regression`) unless the PR (1) documents the genuine dynamic boundary in a code comment at the emit site and (2) adds a regression test proving concrete types/unions/scoped generics cannot represent it — then reclassify via `classify_line` in `crates/smelt-transpiler/src/unknown_report.rs` and re-snapshot the baseline in the same commit rather than accepting the increase. legitimate-boundary increases never block; avoidable decreases re-snapshot in the same commit.

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
