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
Tight loop (fast — `--lib` skips compiling the ~40k lines of inline tests in
smelt-frontend-ts/smelt-codegen-rust, so it doesn't rebuild them every edit):
cargo check --lib
cargo clippy --lib
When working ONLY on the TypeScript path (not Python), add `--no-default-features`
to drop the whole `ty` Python stack (ty_python_semantic/core/module_resolver +
ruff + smelt-frontend-py, ~86s of the cold build):
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

## Subagents
the orchestrating session may write feature code itself — the old "Fable must only orchestrate" rule is retired now that Opus 5 runs the main loop. Prefer doing small, well-understood changes inline over paying a dispatch round-trip for them; delegate when the work is large, parallelisable, or needs a context of its own
send code-writing subagents on Opus (`model: opus`), and review their diffs before merging rather than trusting the summary — verify the load-bearing claims independently

the real constraint is concurrent **cargo builds**, not agent count: parallel rustc makes this machine lag. So cap the builders, not the agents — at most two agents running cargo at a time, but any number of cargo-free agents (CI/YAML edits, report and blocker-log analysis, design plans, doc writing) may run alongside them
state explicitly in each dispatch prompt whether the agent is expected to compile. two agents sharing one worktree's `target/` serialize on the cargo lock rather than thrashing, so same-worktree builders are cheaper than agents given separate target dirs via `isolation: worktree`

## git
After each feature, push a commit with the changes and a clear description of what was implemented
git status should as clean as possible

## NEVERS

NEVER reject a feature without asking me first, it doesn't matter how hard it is
