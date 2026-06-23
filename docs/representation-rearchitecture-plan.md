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

   **STATUS 2026-06-23 — memoization DONE but insufficient (branch
   `worktree-agent-a427cd9dd2b15c2a8`, commit `b33cdd06`, green on `cargo test` +
   clippy, NOT merged).** Implemented as a thread-local
   `SMELT_ERASED_FUNCTION_IDENTITIES: HashMap<usize, SmeltUnknown>` keyed on
   `Rc::as_ptr` of the source callback, wrapping the `erase_value` `Type::Function`
   arm via a `smelt_erase_function_identity(source_key, build)` helper (lib.rs +
   coercion.rs). The cache value transitively owns the source `Rc`, closing the
   ABA pointer-reuse hazard.

   This proves the mechanism but **resolves 0 of the target tests**, because the
   real blocker is *upstream of the erase site*: a **named function used as a
   value is materialized as a fresh wrapper per use** (e.g. `isDeepEqual(func1,
   func1)` builds two distinct `Rc::new(|| func1())`; `doNothing()` builds a fresh
   `SmeltErasedFunction` per call). The two source `Rc`s already differ before
   erasure, so memoization can't unify them. Also note `isStrictEqual.test.ts` has
   **no** function-comparison cases (that target was a mis-attribution; its
   failures are all array/object/set/uint/promise = B1 step 2). The genuine fix is
   **function-reference lowering**: resolve a reference to a named function (or a
   module-level singleton like `doesNothing`) to a single stable instance instead
   of re-wrapping per use — a frontend/MIR change. Combine that with the
   memoization branch to flip isDeepEqual-functions and mergeDeep-functions.
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
3. Sets and typed arrays (uint8) ride on the same list-identity mechanism.

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

---

## Sequencing recommendation
Do **B1 step 1 (function identity)** first — smallest, unblocks ~3-4 tests, proves
the identity-memoization mechanism. Then **B2** (distinct undefined) as a focused
pass. Then **B1 step 2 (list identity)** last, as the largest change. Each is its
own branch + review, gated on the full generated suite and `cargo test`.
