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

## Phase 1 — `SmeltUnknown::Undefined` variant + all match arms (NET-NEUTRAL) ✅ shippable

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
crate's `#![allow(dead_code)]` covers the never-constructed variant. **Gate:
generated crate compiles, full report unchanged (21), `cargo test` + `clippy`
green** (update the codegen unit goldens that assert `typeof`/truthiness/to-string).

## Phase 2 — the producer (`Constant::Undefined`) + reconciliation (ATOMIC)

Land all of these together; report-gate the whole batch (never ship a partial
producer — it regresses, see table above):

Producers (each must yield `Undefined`, not `Null`):
1. `undefined` literal/identifier (`builder_part16.rs`) → `Literal::Undefined`;
   all-types provider `undefined` shorthand (`builder_part01.rs`),
   callback-body (`builder_part13.rs`), const-expr (`builder_part16.rs`).
2. missing property access (`smelt_get_object_field` in `lib.rs`; `place.rs`
   field-access fallbacks).
3. out-of-bounds index; optional-absent erasure; optional-chaining `?.`
   short-circuit; `void`; function with no/`undefined` return; destructuring miss.

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

## Gating discipline

Every step lands green on `cargo run … rust-test-report --full` +
`cargo test` + `cargo clippy`, regenerating `third_party/remeda` first. Never
commit a net-negative step. Phase 2 is one atomic change (or a sequence where
each commit is ≥ net-neutral).

## State / pointers

- Full attempt-2 working tree preserved on branch `wip/distinct-undefined-grind`.
- Investigation: `blocker-logs/plan-nullish-promise-2026-06-23.md` (producer/site map).
- Baseline: 21 failing (`blocker-logs/remeda-after-jsstricteq-2026-06-23.md`).
