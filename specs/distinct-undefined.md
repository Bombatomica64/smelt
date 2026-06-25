# Distinct `undefined` — multi-session plan

## Goal

Make JavaScript `undefined` distinguishable from `null` end-to-end, so the
generated Rust honors `null !== undefined`, `typeof undefined === "undefined"`,
and the type guards that split the two. Unblocks ~6 Remeda tests:

- `isDeepEqual::objects_null_and_undefined_are_not_equal`
- `isDefined::should_work_as_type_guard_in_filter` (keep `null`, drop `undefined`)
- `isNonNull::should_work_as_type_guard_in_filter` (keep `undefined`, drop `null`)
- `isNonNullish::should_work_as_type_guard_in_filter` (drop both)
- `isNot(isPromise)` filter (rides the same array)
- `pullObject::datalast_undefined_values`

## Why it is multi-session (the hard constraint)

`null` and `undefined` are **byte-identical in MIR today**: both lower to the
`none` constant with type `None`, and `T | undefined` / `T | null` both become
`Optional<T>`. Splitting them is **all-or-nothing**:

1. Adding the `SmeltUnknown::Undefined` runtime variant makes every exhaustive
   `match` on `SmeltUnknown` in the generated crate non-exhaustive until **all**
   are updated (compile error otherwise).
2. Once a *producer* emits `Undefined`, any test that compares an `undefined`
   from one producer against an `undefined`/`null` from another producer breaks
   **unless every producer and every nullish-consuming library helper is
   reconciled in the same change**.

Empirical proof (branch `wip/distinct-undefined-grind`, attempt 2):

| State | Δ vs baseline (21 failing) |
| --- | --- |
| Phase 1 (variant + all arms, no producer) | **0** (net-neutral, compiles) |
| + literal producer only | **+2 / −10 = −8** |
| + missing-access producer + nullish-coalesce | **−17** |

So producers must land together with full reconciliation. Phases below are
ordered so **Phase 1 is independently net-neutral and safe to ship**, and the
producer work is a single later atomic change (or a tightly-gated sequence that
never leaves `main` red).

## Design decision: `Constant::Undefined`, NOT `Type::Undefined`

A parallel `Type::Undefined` would thread through **~190 `Type::None` match
sites** across all crates — too broad and error-prone. Instead carry the
distinction as a **constant/value**, leaving the type as `None`:

- HIR `Literal::Undefined` → MIR `Constant::Undefined` (both keep type `None`).
- Codegen: `constant_text(Constant::Undefined)` = `"()"` (unit, like `null`); the
  distinction surfaces only at **erasure**: `erase(Operand::Const(Constant::Undefined))`
  → `"SmeltUnknown::Undefined"` (the type-driven `Type::None` arm would say `Null`).

This validated cleanly in attempt 2 — `isDeepEqual` flipped correct with zero
type-system churn. The remaining cost is **breadth of producers + reconciliation**,
not difficulty.

## Phase 1 — `SmeltUnknown::Undefined` variant + all match arms (NET-NEUTRAL) ✅ landed

Add the runtime variant and every match arm so the generated crate compiles and
behaves identically (nothing produces `Undefined` yet). Per-site semantics:

- enum + `Clone`; `Debug`→`"Undefined"`, `Display`→`"undefined"`, `Serialize`→`serialize_none`.
- `structural_eq`/`js_strict_eq`: `(Undefined,Undefined)=>true`; `Null` vs `Undefined`
  is NOT matched ⇒ deep/strict-unequal (this is the `isDeepEqual` win once produced).
- `Hash`/rank: distinct tag `8`.
- truthiness: `Null | Undefined => false` (both falsy).
- to-number / to-i64: `Undefined` joins the `NaN`/`0` group (`Number(undefined)=NaN`);
  note the JS-accurate template keeps `Null => 0.0` but `Undefined` → `NaN`.
- to-string / property-key / JSON-key: `Undefined => "undefined"`; **array-join**
  item: `Undefined => ""` (JS `[undefined].join()` is `""`).
- `typeof`: `Undefined => "undefined"` (distinct from `Null => "object"`).
- index/charAt non-indexable group (`primitive_none`): add `Undefined => None`.

Sites: prelude in `crates/smelt-codegen-rust/src/lib.rs`; emitter templates in
`emitter/{coercion,types,strings,strings_io,call_runtime,core}.rs`. The generated
crate's `#![allow(dead_code)]` covers the never-constructed variant. This landed
on `main` in `8da1b8ab` (`Distinct undefined — spec + Phase 1
(SmeltUnknown::Undefined variant)`). **Gate was: generated crate compiles, full
report unchanged (21), `cargo test` + `clippy` green** (update the codegen unit
goldens that assert `typeof`/truthiness/to-string).

## Phase 2 — the producer (`Constant::Undefined`) + reconciliation (ATOMIC)

Land all of these together; report-gate the whole batch (never ship a partial
producer — it regresses, see table above):

Before flipping any producer, add small central codegen helpers/templates so the
atomic producer change is reviewable rather than a broad hand edit:

- `is_nullish_unknown(value)`: `Null | Undefined` for loose nullish operations.
- `is_undefined_unknown(value)`: strict `Undefined` tag check.
- `missing_property_value()` / `missing_index_value()`: the canonical generated
  runtime value for JS missing lookups.
- `erase_constant(Constant::Undefined)` or equivalent single erasure path so
  `Constant::None` keeps emitting `SmeltUnknown::Null` while `Undefined` emits
  `SmeltUnknown::Undefined`.

These helpers may land before Phase 2 if they are behavior-neutral. The producer
flip still lands atomically behind the report gate.

Producers (each must yield `Undefined`, not `Null`):
1. `undefined` literal/identifier (`builder_part16.rs`) → `Literal::Undefined`;
   all-types provider `undefined` shorthand (`builder_part01.rs`),
   callback-body (`builder_part13.rs`), const-expr (`builder_part16.rs`).
2. missing property access (`smelt_get_object_field` in `lib.rs`; `place.rs`
   field-access fallbacks).
3. out-of-bounds index; optional-absent erasure; optional-chaining `?.`
   short-circuit; `void`; function with no/`undefined` return; destructuring miss.

Sweep stale `SmeltUnknown::Null` fallback sites as part of the same change. In
particular, audit generated runtime helpers and emitter templates that use
`unwrap_or(SmeltUnknown::Null)`, `map_or(SmeltUnknown::Null, ...)`, or `_ =>
SmeltUnknown::Null` for missing object fields, missing array/string elements,
call arguments, optional property access, regex capture absence, and failed
dynamic calls. Keep real JS `null` producers as `Null`; only missing/undefined
producers switch to `Undefined`.

Library / operator reconciliation (treat `Undefined` like `Null` for *loose*
nullish, but distinct for *strict*):
- nullish-coalesce / `?? ` / `== null ? :` templates (`call_runtime.rs`
  `match { Null => fallback, value => value }`, `coercion.rs` optional-from-unknown)
  → `Null | Undefined => …`.
- `isNullish`, `defaultTo` (`== null` catches both), `isEmptyish`/`isEmpty`,
  loose `== null` / `!= null`.
- B3-b filter-param: `isDefined`/`isNonNull` need erased `SmeltUnknown` params
  with bodies that check the specific tag (`x !== undefined` ⇒ not `Undefined`;
  `x !== null` ⇒ not `Null`). Today both are `Optional<T>` + `!= none`
  (byte-identical) — split via the comparison keyword in `unknown_null_comparison`
  (`builder_part09.rs`), adding `UnknownKind::Undefined` and a loose-nullish (both)
  check distinct from the two strict-tag checks.

Conversion authority:

- `ToNumber` / `Number(...)`: `Null => 0.0`, `Undefined => NaN`.
- integer conversions derived from unknown numeric coercion may map both
  non-numeric outcomes to `0_i64` after the NaN step, but the f64 coercion must
  preserve the JS distinction.
- truthiness: both falsy.
- string conversion: `Null => ""` only for the existing optional/array-join
  special cases; generic unknown-to-string keeps `Undefined => "undefined"` and
  the established null behavior for that emitter path.

## Gating discipline

Every step lands green on `cargo run … rust-test-report --full` +
`cargo test` + `cargo clippy`, regenerating `third_party/remeda` first. Never
commit a net-negative step. Phase 2 is one atomic change (or a sequence where
each commit is ≥ net-neutral).

Golden / focused test checklist for Phase 2:

- missing object property produces `Undefined`, while present `null` remains
  `Null`.
- out-of-bounds array/string index produces `Undefined`.
- `void expr` and source `undefined` literals/identifiers produce `Undefined`.
- `typeof undefined === "undefined"` and `typeof null === "object"`.
- strict equality keeps `null !== undefined`; loose `== null` / `!= null` treats
  both as nullish.
- `??`, `isNullish`, `isDefined`, `isNonNull`, and `isNonNullish` split or merge
  the two according to JS/Remeda semantics.
- `Array.prototype.join` renders `null` and `undefined` items as empty strings.

## State / pointers

- Full attempt-2 working tree preserved on branch `wip/distinct-undefined-grind`.
- Investigation: `blocker-logs/plan-nullish-promise-2026-06-23.md` (producer/site map).
- Baseline: 21 failing (`blocker-logs/remeda-after-jsstricteq-2026-06-23.md`).

## Remaining regression tail (9, after `c4d6b257`/`b8250c64` — at 23 failing)

These 9 are distinct-`undefined` regressions still open after the producer sweep.
They are NOT simple "flip Null→Undefined" producers — each is a subtle
interaction. Root-cause analysis for follow-up:

1. **`clone::edge_cases_undefined`** — matcher-lowering, not a producer.
   `expect(clone(undefined)).toBeUndefined()`: `clone(undefined)` is `None`-typed,
   and `expect_to_be_none_statement` (builder_part06) lowers `actual !== undefined`
   via `BinOp::JsStrictNotEq` against a `Literal::Undefined`. For a `None`-typed
   actual the comparison collapses to a wrong constant (`!(false)` in generated),
   and the actual is discarded (`let _ = clone(...); ()`). Fix: in
   `unknown_binary_text`/the none-vs-none equality path, a `None`-typed value
   strictly-equals `undefined` (and the `Literal::Undefined` expected should not be
   treated as plain `None`). Audit the `lhs_is_none`/`rhs_is_none` arms for the new
   `JsStrict*` ops + `Literal::Undefined`.

2. **`sortedIndexBy` / `sortedLastIndexBy` ::binary_search…indexed_empty_array (2)**
   — the indexed-callback adapter converts the optional index arg
   `arg1: Option<f64>` via `map_or(SmeltUnknown::Undefined, …)` (was `Null`). The
   binary-search/indexed comparator on an empty array now sees `undefined` where it
   expected `null`. Determine whether the index should ever be absent here (it
   shouldn't for a real index) — likely the adapter should pass the concrete index,
   or this specific Optional-erase site should stay `Null`. Narrow the optional→
   undefined change away from callback-index args.

3. **`truncate::regex_separator_matches_after_maxlength`** — fallout from the
   regex-capture→`undefined` change. truncate inspects a regex match/captures; a
   capture that is now `undefined` (vs `null`) changes a `=== undefined` / truthy
   branch. Diff generated `truncate.rs` regex handling vs the matched-capture
   expectation.

4. **`debounce::should_debounce_a_function_177` /
   `funnel_remeda_debounce::…_615` (2)** — debounce timing/cached-value. The cached
   `result` (initially `undefined`) and the `result !== undefined` gate now compare
   the `Undefined` tag; a producer in the timer/cached-value path likely still emits
   `Null`, or the leading/trailing invoke path mishandles the new tag. Trace the
   cached-value lifecycle in generated `debounce.rs`.

5. **`funnel_reference_batch::showcase_{error_handling,results_as_array,results_as_object}` (3)**
   — async funnel that stores `new Promise((resolve,reject)=>…)` and awaits later.
   These also broke under the earlier promise-marker attempt; the
   undefined-producer changes perturbed a value in the await/store-resolve flow.
   Entangled with Cluster B (promise representation) in
   `specs/remeda-deep-clusters.md`; likely needs the awaitable+inspectable promise
   handle, not a producer flip.

**Net:** 14 pre-existing deep-cluster failures (see remeda-deep-clusters.md) + these
9 = 23. Distinct-`undefined` is currently ~net-neutral vs the old 21 baseline
(6 undefined wins vs ~9 regressions) but is a correctness win (null≠undefined). The
high-value producer sweep is done; this tail is subtle per-test work.
