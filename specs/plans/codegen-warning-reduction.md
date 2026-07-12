# Warning-Reduction Plan for the Rust Emitter (es-toolkit: ~950 warnings)

*(Fable planning pass, 2026-07-12. Root causes verified in emitter code + generated output.)*

Baseline mix: unused_mut 482, unused_assignments 157, unused_parens 43, unreachable_code 16, plus stragglers.

## 1. `unused_mut` (~482) — highest priority
Representative: `filterAsync.rs` `let mut results = results.clone();` in closure-capture preludes with read-only use; `every.rs:115` `let mut _smelt_adapted_callback` never mutated.
Root causes (verified):
1. **Capture preludes force `mut` for all collection types** — `emitter/list_query.rs:836-853` and `:1396-1406`: `... || matches!(ty, Type::List(_) | Type::Set(_) | Type::Dict(_,_))` makes EVERY list/set/dict capture mut regardless of writes. Dominant source.
2. **Unconditional mut on adapted callbacks** — `emitter/core.rs:3811` hardcodes `let mut _smelt_adapted_callback` (contrast line 3802's non-mut `.call` path).
3. **`local_binding_needs_mut` over-approximation** — `emitter/core.rs:402-491`: `.unwrap_or(true)` defaults (lines 410-417); repeating-region rule fires even when the `let` itself re-executes per iteration; closure params (list_query.rs:1131) inherit it.
Fixes: drop the type-based disjunct (rely on/extend `closure_capture_body_writes`); make core.rs:3811 conditional; change `.unwrap_or(true)` to computed; repeating-region rule only when the binding's `let` is outside the region. Expected −350..450. Risk medium — under-approximation becomes E0596/E0384, loudly caught by generated `cargo check`.

## 2. `unused_assignments` (~157)
Representative: `bind.rs:54-61` — counter update dead because both branches diverge into `panic!("recursive closure control flow is not structured yet")` (~60 in the bind/bindKey family); predeclared `let x;` temps with dead first stores.
Root cause: predeclared-local machinery (`core.rs:4948`, `core.rs:318`) + `control_flow.rs:214-220` emits every MIR assign including stores post-dominated by diverging terminators.
Fix: skip emitting Assign when `block_eventually_terminates` (control_flow.rs:1266) holds for the continuation and no read precedes termination; cheap first step: only emit stores whose local has any subsequent use. Expected −100..140. Risk medium-high — eliding a live store corrupts RUNTIME behavior; gate with the full generated test suite, not just check. Do after unused_mut.

## 3. `unused_parens` (~43)
Root cause: defensive parens at coercion sites — `coercion.rs:215` `format!("({} as f64)")`, `:221`, `call_runtime.rs:2423`. Fix: route through the `rendered_value.rs` precedence machinery (`parenthesized_if_needed`), or strip outer parens at call-argument emission when the arg is a single balanced parenthesized expr. Expected −35..43, risk low.

## 4. `unreachable_code` (~16)
Root cause: fallthrough-return emission doesn't consult `block_eventually_terminates` before appending a trailing return after a fully-diverging match (`every.rs:116-122`). Fix: skip when all arms diverge. Expected −16, risk low.

## 5. Long tail
Unused closure params → emit `_closure_arg_N` when the body (already rendered before the param list, list_query.rs:1125+) never references them; non_camel_case_types via sanitize_ident or targeted allow.

## Analysis vs `#![allow]` verdict
- unused_mut: **analyze** (3 concrete fix sites; spurious mut misleads reviewers/LLMs).
- unused_assignments: **hybrid** — diverging-terminator suppression now; residual dataflow-hard cases get *function-scoped* `#[allow]` with an emitter comment, never crate-level.
- unused_parens / unreachable_code: **fix** (mechanical; unreachable-code warnings are future-bug signal).
- Never crate-level `#![allow]`: it blinds `rust-diagnostics` (sorted-by-count reports are the LLM debug entrypoint) and masks future emitter regressions.

## 3-round campaign (2 Opus agents/round)
- **R1**: A = unused_mut (list_query preludes + core.rs:3811 + needs_mut tightening); B = unused_parens + unreachable_code (owns control_flow.rs exclusively). Gate: workspace check/clippy; regen es-toolkit, zero new errors, counts drop as predicted; mtime-preservation spot-check.
- **R2**: A = unused_assignments dead-store suppression; B = unused params/imports/non_camel_case. Gate adds `smelt rust-test-report` (semantic risk) + smelt-unknown-report vs baseline.
- **R3**: A = residual top class + scoped allows for irreducible cases; B = regression gate (fixture crate + checked-in warning-class budget in CI). Full cargo test before final commit.

Target: ~950 → under ~60 warnings, no crate-level allows, write-if-changed path untouched.
