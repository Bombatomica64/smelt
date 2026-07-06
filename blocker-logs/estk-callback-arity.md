# es-toolkit blocker family: `array callback callback item parameter count is not supported`

## Failure family

Largest es-toolkit lowering blocker family (30 spec files at pinned ref
`e008a2818cd8`): passing a *named* function as an array callback aborted
lowering whenever the function's declared parameter count did not fit the
receiver's supplied `(value, index, array)` triple.

Two concrete shapes:

1. **Zero-parameter named callbacks** (the dominant shape): lodash-style stubs
   passed to `map`/`filter`/`forEach` — `values.map(stubTrue)`,
   `falsey.map(stubArray)`, `values.map(noop)`, `expected = values.map(stubZero)`.
   Every one of the 30 affected files uses this shape.
2. **Over-arity named callbacks**: a function declaring *more* parameters than
   the receiver supplies, with an optional tail —
   `[[2,1,3],[3,2,1]].map(orderBy)` where compat `orderBy` declares
   `(collection, criteria?, orders?, guard?)` (4 > 3).

## Root cause

`ModuleBuilder::callback_argument` (crates/smelt-frontend-ts/src/lowering/callbacks/dispatch.rs)
rejected any item callback with `params.is_empty() || params.len() > expected_param_tys.len()`.
The compact predicate path (`named_callback_reference` in
crates/smelt-frontend-ts/src/lowering/callbacks/classify.rs) had the equivalent
`function_params_len < expected_param_tys.len().min(1)` guard, and the local
callback branch rejected `callback.params.is_empty()` too.

On the codegen side, `list_callback_iteration_parts`
(crates/smelt-codegen-rust/src/emitter/list_query.rs) *required* an item
parameter, and four `function_ty.params.is_empty()` guards short-circuited to a
silently wrong `Default::default()` placeholder, so the frontend restriction was
also masking a codegen gap.

## General rule implemented

JavaScript adapts callback arity at the call site; the lowering now mirrors
that as one general rule (no per-function special cases):

- **Fewer declared parameters than supplied (including zero)**: the callback
  ignores the extra arguments. The item wrapper closure
  (`item_function_closure_expression`) is built at the item's own arity and the
  list-callback emitter now passes exactly the arguments the closure declares —
  down to a zero-argument `(smelt_callback)()` call. The compact predicate path
  builds the zero-argument `Call` naturally once the guard is removed. The same
  rule applies to zero-parameter local callback bindings.
- **More declared parameters than supplied**: the unsupplied tail is
  `undefined` in JavaScript (and must be optional for the source to typecheck).
  A new `item_function_closure_expression_with_max_params` wraps the item
  capped at the receiver's supplied arity; the wrapper calls the item with the
  supplied prefix and the existing call-lowering pads the optional tail with
  its default (`None`), which is exactly the JS `undefined` tail
  (`with_tail(a, b, c, None::<f64>)` in generated Rust). Truncated wrappers are
  not tagged with `function_item` so they never alias the full-arity per-item
  wrapper cache used for `===` reference identity.

Codegen: the four `Default::default()` placeholder guards in
`list_query.rs` were removed and `list_callback_iteration_parts` now treats the
item parameter as optional, emitting real iteration for zero-parameter
callbacks.

No `SmeltUnknown` was introduced or expanded.

## Arity shapes fixed vs deferred

Fixed:
- zero-parameter named item callbacks in every array-callback position
  (`map`, `flatMap`, `forEach`, `filter`, `find*`, `some`, `every`);
- zero-parameter local callback bindings (`const stub = () => 42; xs.map(stub)`);
- named item callbacks declaring more parameters than supplied with an
  optional tail (`xs.map(orderBy)`), padded with defaults per parameter type.

Deferred:
- *local* callback literals declaring more parameters than the receiver
  supplies still error (`local callback parameter count is not supported`):
  the compact callback IR would need `undefined` bindings for the dropped
  parameter names. Not observed in the es-toolkit corpus.
- over-arity padding uses each parameter type's default value; for a
  non-optional concrete tail parameter (invalid TS at such a call site,
  rejected by `tsc` upstream) this would diverge from `undefined`.

## Verification

- Fixture project (source + spec + Smelt.toml modeled on the es-toolkit
  probe) covering all fixed shapes: `smelt build` succeeds and the generated
  crate's `cargo test` passes 7/7 end to end, including
  `values.map(sumWithTail)` proving the `None` tail padding at runtime.
- Compiler regression tests added:
  `crates/smelt-frontend-ts/src/tests/part07_tests.rs` (4 tests) and
  `crates/smelt-codegen-rust/src/tests/part_7_tests.rs` (3 tests).
- `cargo check --workspace` clean; `cargo clippy -p smelt-frontend-ts
  -p smelt-codegen-rust` clean; `cargo test --workspace --exclude smelt-gui`
  green.
- Per-file `smelt dump-hir` re-scan of all 30 affected es-toolkit files: the
  family diagnostic is gone from **all 30**. 27 files now lower with no
  diagnostics at all; 3 files surface pre-existing unrelated diagnostics:
  - `src/compat/predicate/isMatch.spec.ts`,
    `src/compat/predicate/matches.spec.ts`:
    `const item expression shape is not supported for inlining yet`
  - `src/compat/predicate/matchesProperty.spec.ts`:
    `function parameters must have explicit type annotations or default initializers`
- Whole-project `smelt build` abort point: unchanged before/after at
  `src/predicate/isEqualWith.spec.ts`
  (`const item expression shape is not supported for inlining yet`, a separate
  family owned by other work), as expected — this change's metric is the
  per-file scan above.
