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

## Emitted snippet templates
string templates are the emission mechanism; do NOT migrate emission to quote!/proc-macro2/AST printers (evaluated and rejected: template holes are rendered strings, and the compile-corpus tier already validates output more strongly)
when the SAME runtime snippet would be inlined at more than one emitter site, emit it ONCE as a doc-commented prelude function under its needs_* gate and have templates call it (e.g. `smelt_extract_callable`/`smelt_call_dynamic`) — never duplicate enormous format-string matches across emitter modules
keep per-site semantic differences (not-callable fallback, error vs panic vs default) at the call site; do not merge different behaviors into one helper
keep the generated runtime prelude as small as possible: no speculative helpers, everything gated on actual use — a big runtime defeats the optimization/compile passes
after any codegen-affecting change run the compile tier `cargo test -p smelt-codegen-rust --test compile_corpus -- --ignored` (plain cargo test does not compile generated Rust) and regenerate affected e2e goldens (`cargo build -p smelt-cli` first, then rebuild `examples/typescript/end-to-end/*/expected.rs`)

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

## git
After each feature, push a commit with the changes and a clear description of what was implemented
git status should as clean as possible

## NEVERS

NEVER reject a feature without asking me first, it doesn't matter how hard it is
