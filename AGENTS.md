if i need to tell you something multiple time put it here


## always run
cargo test (only run full tests before a commit)
cargo check
cargo clippy

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

## git
After each feature, push a commit with the changes and a clear description of what was implemented
git status should as clean as possible

## NEVERS

NEVER reject a feature without asking me first, it doesn't matter how hard it is
