# Representation rearchitecture plan (Remeda "cluster B")

Two core-value-model gaps block ~25 of the remaining Remeda generated-test
failures. Neither is a localized patch — each touches a fundamental piece of the
erased-value model and must be rolled out incrementally with the full generated
suite (`smelt rust-test-report --full`) plus `cargo test` gating every step. This
document is the plan to tackle them as dedicated efforts.

Baseline when written: 42 generated tests failing (down from 75 this session).

---

## B1 — Reference identity for erased arrays / lists / functions (~16 tests)

### Problem
JavaScript `===` on objects/arrays/functions is *reference* identity. In Smelt:
- `SmeltObject`/`SmeltRecord` carry an explicit `id` (`smelt_next_object_id()`),
  threaded on erasure via `SmeltObject::with_id(record.id, …)`, and
  `same_js_key`/`smelt_unknown_structural_eq` compare objects by `id`. So objects
  have stable identity.
- **Typed lists are plain `Vec<T>`** with no id. Each erasure to
  `SmeltUnknown::Array` runs `SmeltArray::new` → a **fresh** `smelt_next_object_id()`.
  So the *same* source list erased twice has two different ids → `x === x` is false.
- **Functions**: each erasure builds a new `Rc::new(move |…| …)`; `same_js_key`
  uses `Rc::ptr_eq`, so two erasures of the same source fn compare unequal.

### Affected tests
- `isStrictEqual`: arrays_1266, uint_arrays_1268, sets_1270, promises_1273
- `isShallowEqual`: shallow_inequality_arrays_of_arrays, shallow_inequality_objects_of_arrays (inner array identity)
- `isDeepEqual`: functions_same_function_is_equal
- `tap`: data_first/last_should_return_input_value
- `forEach`: datalast_521
- `mapWithFeedback`: same-accumulator-identity
- `reduce`: data_first_indexed_1550 (`expect(data).toBe(array)`)
- `isIncludedIn`: 3 reference-equality tests
- `mergeDeep`: should_work_with_weird_object_types_functions (`doNothing()` singleton identity)

### Approach (incremental)
1. **Functions first (smaller, lower risk).** Memoize the erased `SmeltErasedFunction`
   wrapper per source `Rc`, so erasing the same function twice yields the same
   `Rc` (reuse / extend `smelt_register_function_origin` to also dedupe identity,
   or key a thread-local identity map on `Rc::as_ptr` of the typed callback).

   **STATUS 2026-06-23 — DONE via transpile-time per-item accessors (commit
   `d1f2c7e0`, on `main`, green on full `cargo test` + clippy). Resolved
   `isDeepEqual::functions` (34 → 33 generated failures, 0 regressions).**

   Earlier runtime-cache attempts failed: erase-site memoization keyed on the
   source `Rc::as_ptr` couldn't help because a named function used as a value is
   materialized as a *fresh wrapper per reference* (the source `Rc`s already
   differ); and a generic `smelt_function_item_value::<T>` cache couldn't unify
   `T` (each `|| func1()` literal is a distinct closure type → `Box<dyn Any>`
   downcast miss) without the fragile `Rc<dyn Fn..>` type-text.

   The working design moves identity resolution to **transpile time**: tag the
   bare function-item wrapper at its frontend origin
   (`ClosureExpr::function_item` → `MirClosure::function_item_key`, the ItemId
   index; only `item_function_closure_expression` / `callback_function_item_closure`
   set it, so user arrows keep fresh identity). When such a wrapper is **erased to
   `SmeltUnknown`** (`coercion.rs` `erase`), emit a call to a per-item accessor
   `__smelt_fn_value_<key>()` and record its body in an `EmitContext` collector;
   `lib.rs` flushes one accessor per item after the function loop:
   `fn __smelt_fn_value_K() -> SmeltUnknown { thread_local OnceCell; cell.get_or_init(|| SmeltUnknown::Function(/* forwards to fn item by name */)).clone() }`.
   All references to one item call the same accessor → one shared
   `SmeltUnknown::Function` → equal under `Rc::ptr_eq`. Monomorphic (returns
   concrete `SmeltUnknown`), so no generic downcast and no `Rc<dyn Fn..>`
   type-text. `erased_rest_forwarding_closure_text` was extracted from
   `rest_vector_unknown_adapter_text` and shared.

   **REMAINING: `mergeDeep::functions` (the `doNothing()` singleton).** Not fixed
   by the above because `doNothing()` (`do_nothing()`) returns its value in a
   **typed** context — `SmeltErasedFunction`, not erased `SmeltUnknown` — so it
   never reaches the erase hook, and each call builds a fresh `SmeltErasedFunction`
   (generated `doNothing.rs` shows a fresh build, plus an unnecessary
   `SmeltErasedFunction`→`SmeltErasedFunction` re-wrap on the return coercion). To
   flip it: route a function-item value in the erased-rest/`SmeltErasedFunction`
   context to a per-item accessor returning `SmeltErasedFunction` (a concrete type,
   so a `OnceCell<SmeltErasedFunction>` accessor works — same shape as the
   `SmeltUnknown` one), at the `Rvalue::Closure` / `closure_text_for_type`
   `SmeltErasedFunction` branch; and make the `typeof doesNothing` return coercion
   a pass-through instead of re-wrapping. Then erasing the shared
   `SmeltErasedFunction` for comparison preserves identity.

   Note: `isStrictEqual.test.ts` has **no** function-comparison cases (that target
   was a mis-attribution; its failures are all array/object/set/uint/promise = B1
   step 2).
2. **Lists/arrays (the large change).** Give typed lists a stable JS id. Two
   viable shapes:
   - **(2a) Id-bearing list backing**: introduce a list wrapper carrying `id`
     (analogous to `SmeltObject`), thread it through every list literal,
     operation, and the `Type::List` erase site in `coercion.rs`. Most correct,
     most invasive (hundreds of emission sites + the runtime prelude).
   - **(2b) Erasure memoization**: keep `Vec<T>` but, when a list *local* crosses
     to `SmeltUnknown`, reuse a stable `SmeltArray` id keyed on the source binding
     (so re-erasing the same binding is identity-stable). Less invasive but needs
     binding-level identity tracking in codegen and doesn't cover list *values*
     that flow through transforms.
   Recommendation: prototype (2b) for the common `x === x` / `toBe(input)` cases;
   fall back to (2a) if value-flow cases (isIncludedIn, mapWithFeedback) need it.

   **STATUS 2026-06-23 — (2b) binding-level DONE for the `x === x` cases (commit
   `ade26b59`, on `main`, full `cargo test` + clippy green). 33 → 30 generated
   failures.** When a source *binding* (param / user `let`/`const`, NOT a temp) of
   `Type::List` is erased, a stable id is keyed on the live `Vec`'s storage
   address via a `smelt_list_identity` thread-local + `SmeltArray::with_id`
   (`coercion.rs` `erase` Type::List arms + `lib.rs` prelude). Temps / list
   literals keep `SmeltArray::new` (fresh id), matching JS. Key uses the in-scope
   `operand_text` reference (minus `.clone()`) via `(<local>).as_ptr()` (auto-refs
   `Vec` and `&Vec`). **Resolved:** `isStrictEqual::{arrays_1266, uint_arrays_1268}`,
   `isIncludedIn::datafirst_arrays`. **Known limitation:** empty-`Vec` `as_ptr`
   sentinel can collide distinct empty bindings.

   **Still failing (need value-flow identity / 2a, deferred):** `tap` data-first/last
   (input returned *through* the tap function — identity must survive the call);
   `isStrictEqual::sets_1270` (sets erase via a sorted-collect *temp*, so no binding
   to key on); `isShallowEqual::{arrays_of_arrays, objects_of_arrays}` (inner/nested
   arrays); `isIncludedIn` data-last (identity through extraction); `reduce`/`mapWithFeedback`
   (accumulator identity through callbacks). These need a true id-bearing list value
   (option 2a) or per-path identity preservation, not binding-level keying.
3. Sets and typed arrays (uint8) ride on the same list-identity mechanism — but note
   sets erase through a sorted-collect temp (see above), so the binding-level fix
   does not reach them; they need the value-level id.

### Risk / gating
Very high blast radius (list backing touches nearly all list codegen). Roll out
behind the full suite at each step; expect multiple rounds. Land functions (step 1)
independently first.

---

## B2 — A distinct `undefined` (~4-5 tests)

### Problem
TS `undefined` and `null` both lower to HIR `Literal::None` and runtime
`SmeltUnknown::Null` (confirmed: `const a = null` and `const b = undefined` both
dump-mir to `none`). JS distinguishes them: `null !== undefined`, and the failed
promise / missing value cases collapse together.

### Affected tests
- `isDefined` / `isNonNull` / `isNonNullish`: should_work_as_type_guard_in_filter
  (the array has `null`, `undefined`, and a promise all as `SmeltUnknown::Null`;
  no predicate over the collapsed value can give the JS-correct count of 18).
- `isDeepEqual`: objects_null_and_undefined_are_not_equal.
- Also the prerequisite for the `isDefined`/`isNonNull` coercion fix already
  identified (mapping erased `SmeltUnknown::Null → None` in `value_at_type`'s
  operand form — see `coercion.rs` ~line 112; correct but inert until `undefined`
  is distinct).

### Approach
1. Add a `SmeltUnknown::Undefined` variant distinct from `Null` (and a HIR
   `Literal::Undefined`, or a flag on the none literal).
2. Lower TS `undefined`/optional-absent → Undefined; `null` → Null.
3. Update every `SmeltUnknown::Null` match site to handle Undefined per JS
   semantics: truthiness (both falsy), `==` (null == undefined loosely, but `===`
   distinguishes), `typeof` (`"undefined"` vs `"object"`), JSON (undefined omitted),
   structural eq, coercions. This is the whole erased model — exhaustive matches
   on `SmeltUnknown` will force coverage, but the behavioral semantics need care.
4. Then apply the inert `value_at_type` `Null → None` mapping for optional-typed
   predicates.

### Risk / gating
High — touches every Null handling path. The compiler's exhaustive matches help
(adding a variant flags all sites), but semantics (loose vs strict equality,
typeof, JSON) must be reasoned per site. Full-suite gating each step.

### STATUS 2026-06-23 — ATTEMPTED, REVERTED (net-negative when partial)
Implemented the variant + all match arms (HIR `Literal::Undefined` → MIR
`Constant::Undefined` → `SmeltUnknown::Undefined`, the full prelude impls, typeof,
and ~25 emitter coercion templates — the generated remeda crate compiled). But it
**regressed 30 → 39** (+2 resolved, **11 newly failing**). Reverted.

**Root cause / lesson:** distinct `undefined` is not "lower the literal + add match
arms" — it requires **consistent undefined-PRODUCTION at every source**, or the
nullish space splits (literal→`Undefined`, everything else→`Null`) and any test
comparing two undefineds from different sources breaks. The producers that must
ALSO yield `Undefined` (not `Null`): missing property access (`obj.missing`),
out-of-bounds index, optional-absent (`Option::None` erasure / `map_or`), the
`void` operator, a function with no/`undefined` return, destructuring misses, and
optional chaining `?.`. Plus the library nullish helpers must treat both
(`defaultTo`, `isEmptyish`, `isNullish`, `??`, `prop` missing-path → `undefined`).
Newly-failing in the partial attempt confirmed this: `prop` (×4, missing-path
returns `Null` but test expects the `undefined` literal), `defaultTo` undefined
fallback, `isEmptyish` nullish, `clone` undefined, `conditional` default,
`countBy`, `identity`, `pullObject` (×2). And the type-guard cluster
(isDefined/isNonNull/isNonNullish/isNot) needs distinct `undefined` AND the
**promise-distinct** fix together (the all-types array's promise also erases to
`Null`; `isNonNull(promise)` must be true). 

So B2 is a dedicated **multi-round** effort: (a) add the variant + arms (done,
on the stalled-agent worktree branch for reference), THEN (b) sweep every
undefined-PRODUCING site to emit `Undefined`, THEN (c) reconcile the library
nullish helpers, gating on the full suite each step. Not a single focused pass.

---

## Sequencing recommendation
Do **B1 step 1 (function identity)** first — smallest, unblocks ~3-4 tests, proves
the identity-memoization mechanism. Then **B2** (distinct undefined) as a focused
pass. Then **B1 step 2 (list identity)** last, as the largest change. Each is its
own branch + review, gated on the full generated suite and `cargo test`.
