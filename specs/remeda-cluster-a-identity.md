# Cluster A — Reference identity for typed arrays (+ function singletons)

Design plan (read-only investigation, 2026-06-26) for the 9 remeda tests that
fail on JS reference identity. **Key finding: these 9 do NOT share one
mechanism — there are 5 distinct ones.** Sequence the cheap, isolated wins first.

Target tests:
- `isShallowEqual_test::test_shallow_inequality_arrays_of_arrays`, `::test_shallow_inequality_objects_of_arrays`
- `tap_test::test_data_first_should_return_input_value`, `::test_data_last_should_return_input_value`
- `constant_test::test_returns_identity_doesn_t_clone`
- `mapWithFeedback_test::...references_to_the_accumulator`
- `reduce_test::test_data_first_indexed_1550` (callback array arg must `.toBe(data)`)
- `forEach_test::test_datalast_521` (dataLast forEach returns SAME array ref)
- `mergeDeep_test::test_runtime_datafirst_should_work_with_weird_object_types_functions` (function singleton)

## Verified root causes (5 mechanisms)

1. **Comparison at the typed `Vec` level with no id** (tap ×2, isShallowEqual ×2).
   `strict_identity_text` (`crates/smelt-codegen-rust/src/emitter/call_runtime.rs:1634-1637`)
   emits the literal `"false"` for two `Type::List` operands — contrast the Dict
   arm (`:1617-1626`) which emits `.id == .id`. Lists have no id, so codegen gives up.
2. **Library ABI strips the id: `SmeltUnknown::Array → Vec → SmeltUnknown::Array`**
   (forEach, reduce). `extract` drops `SmeltArray.id` at `coercion.rs:1507-1522`
   (`value.into_vec()`); the callback adapter re-wraps via `SmeltArray::new` → fresh id.
3. **Typed-record live-erasure freezes a snapshot** (`constant`, object half of mapWithFeedback).
   Erasing `SmeltRecord<…, Option<bool>>` with a value remap forces a fresh `HashMap`
   (`coercion.rs:806-816`); reuses `obj.id` (so `.toBe` passes) but detaches from the
   live `Rc<RefCell>`, so `toStrictEqual` after `obj.insert(...)` fails. **This is a
   record problem, not an array one.**
4. **Function singleton** (mergeDeep `doNothing`): `do_nothing()` builds a fresh `Rc`
   per call; `toStrictEqual` compares functions by `Rc::ptr_eq`. See §Function singleton.
5. **mapWithFeedback** also hits a frontend reducer-body drop (`acc[x]=x; return acc`
   lowered to bare `closure_arg_0.clone()`) + erased `toBe` lowered to structural `==`.
   Not fully fixable by array identity — **defer**.

## REVISED representation decision (2026-06-26) — simpler than Rc<RefCell>
The 6 target array tests (tap×2, isShallowEqual×2, forEach, reduce) need identity via a
**shared id**, NOT aliased shared mutation (that's mapWithFeedback / Cluster F, deferred).
So model `SmeltList<T>` directly on the existing `SmeltArray` (lib.rs:646-670): a plain
`{ id: usize, values: Vec<T> }` where `Clone` copies the id and deep-clones the values, with
**`Deref<Target=[T]>` + `DerefMut`**. This keeps the ~140 read sites (`.iter()`, `.len()`,
`[i]`, slicing) compiling UNTOUCHED — only construction (`vec![..] -> SmeltList::from`),
`ListCopy`/spread (mint fresh id), erase/extract (carry id), and `===`/`toBe` comparison
(compare id) need edits. Value semantics are identical to today's `Vec::clone` (deep), so no
immutability-test regressions; the only new behavior is id-based reference equality (strictly
more correct than the current hardcoded `"false"`). The full `Rc<RefCell<Vec>>` version below
is deferred to whenever Cluster F (aliased mutation: mapWithFeedback) is tackled.

## Chosen representation (for mechanisms 1 & 2)

`SmeltList<T> = { id: usize, values: Rc<RefCell<Vec<T>>> }`, mirroring
`SmeltObject`/`SmeltRecord` (`crates/smelt-codegen-rust/src/lib.rs:602-622, 461-520`),
replacing `Type::List(item) → Vec<{item}>` at `emitter/types.rs:588`.

- **Clone = shared-ref** (clone the `Rc` + copy `id`), NOT deep — this is what makes
  `let b = a.clone()` share identity (isShallowEqual) and aliased mutation visible.
  Biggest behavioral shift (see R1).
- Rejected: id-sidecar keyed on `Vec::as_ptr()` (brittle — `Vec::clone` reallocates,
  can't survive the ABI rebuild). Only works for "same binding erased twice w/o clone".
- Hybrid sequencing: introduce `SmeltList<T>` as a newtype with `From<Vec>` so the
  ~140 read sites keep compiling; explicit methods (NOT `Deref<[T]>` — unsound through
  `RefCell`) for `len/get/iter/push/...`. The explicit-method churn is the main cost (R2).

### Seams to change
- Type lowering: `emitter/types.rs:588`.
- Prelude: add `SmeltList<T>` (gated like `needs_erased_function`) + `SmeltJsKeyEq`/`SmeltJsStrictEq` (id compare) + structural `PartialEq`/`Hash` (contents, for HashSet membership — keep id OUT of Hash, R6).
- Literal/construct: `emitter/list.rs:407,225`, `core.rs:3971`, `mod.rs:419` → `SmeltList::from(vec![...])` (fresh id).
- Index/iter/len/slice: `place.rs`, `core.rs`, `list_query.rs`, `list.rs` → new methods.
- Mutation: `list_mutation.rs`, `place.rs` AssignPlace{Index} → `borrow_mut()`. `Rvalue::ListCopy` (`opt.rs:516`) must produce a **fresh-id snapshot** (`to_vec()`), not a shared clone — honors Remeda's no-mutation contract.
- erase: `coercion.rs:761-797, 1088-1101` → id lives on the value; the `smelt_list_identity` thread-local (`lib.rs:432-440`) + `list_local_identity_key` (`coercion.rs:923-944`) can be **deleted** for the list path (simplification).
- extract (id-drop fix): `coercion.rs:1507-1522` → `SmeltList::from_unknown_list(value)` preserving `value.id`. This is what lets forEach/reduce forward the original id.
- Comparison: `call_runtime.rs:1634-1637` → `{lhs}.id() == {rhs}.id()` (frontend already routes list `toBe` to strict identity at `builder_part06.rs:175-231`); also `function_bearing_equality_text` `:1569-1575`.

### MIR ownership interactions
- CopyPropagation (`crates/smelt-mir/src/opt.rs`, the only pass present today — memory's "Pass 2 borrow `&T`" is NOT in tree): shared-ref clone keeps alias-collapse sound; `ListCopy` is a distinct Rvalue so not alias-collapsed. No pass change.
- `parameter_needs_mutable_reference` (`core.rs:736-815`): interior mutability makes `&mut SmeltList` redundant but not incorrect; leave it.

## Function singleton (mechanism 4) — CHEAPEST WIN, do first
Per `blocker-logs/plan-function-constant-identity-2026-06-23.md`:
- **1b:** `core.rs::erased_rest_function_value_text` (~L2425) — if source is already a
  `SmeltErasedFunction` (`is_erased_unknown_rest_function && !may_throw`), return `None`
  so `value_at_type` emits `{text}.clone()` (shares inner `Rc`) instead of re-wrapping.
- **1a:** memoize nullary function-item constants — `__smelt_fn_erased_<key>()` returning
  a `thread_local OnceCell<SmeltErasedFunction>`, routed from the `SmeltErasedFunction`
  branch in `list_query.rs` (~L720) when `function_item_key.is_some()`.
- **R3 caveat:** `into_smelt_unknown` (`lib.rs:719-723`) mints a fresh OUTER `Rc` per erase;
  `toStrictEqual` compares the outer `Rc`. Caching the `SmeltErasedFunction` alone may be
  insufficient — likely also memoize the derived `SmeltUnknown::Function` `Rc` per source
  callback. Confirm against regenerated `mergeDeep_test.rs`.

## Records (mechanism 3, `constant`)
Record analogue of the array work: erasing a `SmeltRecord` whose values are mutated-after-
erasure must share the `Rc<RefCell>` (live view). Gap is only when the value type needs a
remap (`coercion.rs:806-816`). Options: (a) frontend types such records as
`SmeltRecord<String, SmeltUnknown>` from the start; (b) shared-backing on-read-remap adapter.
Deferred per `plan-function-constant-identity` Part 2 + `docs/representation-rearchitecture-plan.md`.

## Phased plan (gate each on regen → cargo test → clippy → compile-corpus tier → rust-test-report --full)
- **Phase 0 (low-risk):** function singletons (§mechanism 4). Flips `mergeDeep` (1). No array change.
- **Phase 1 (behavior-neutral):** add `SmeltList<T>` newtype, gated/unused.
- **Phase 2 (the switch):** `types.rs:588 → SmeltList<T>` + fix all read/construct/mutate/erase/extract/compare per module group, gating each group. Shared-ref Clone from the start. Flips tap×2, forEach, reduce, isShallowEqual×2 (6). HIGH RISK — own branch.
- **Phase 3 (records):** shared-backing record erasure. Flips `constant` (1).
- **Defer:** mapWithFeedback (frontend reducer-body + erased-`toBe`).

## Risks
- **R1** shared vs deep `Vec::clone` — code relying on independent copies now aliases. Audit ListCopy/slice/spread/`splitAt`/`chunk`/`swapIndices` + immutability property tests. Where regressions concentrate.
- **R2** explicit methods at ~140 sites (no `Deref<[T]>`); missed site = compile fail (caught only by compile-corpus tier, not plain `cargo test`).
- **R3** function-singleton outer-`Rc` identity may need follow-on.
- **R4** `Rc<RefCell>` `BorrowMutError` on self-referential index assign `list[i]=list[j]` — audit AssignPlace{Index}.
- **R5** removing the `as_ptr`-keyed thread-local must not regress current `list_local_identity_key` passers.
- **R6** keep id OUT of `Hash`/structural eq (HashSet membership uses structural; `===` uses id — four distinct equalities, `lib.rs:582-600`).
