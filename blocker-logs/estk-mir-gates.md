# es-toolkit MIR-lowering gates

Status of the first whole-crate `smelt build` of the pinned es-toolkit checkout
(`e008a2818cd8`). Earlier work took the crate through complete HIR lowering; this
pass clears the lowering/validation families that stood between HIR and an emitted
crate, iterating one first-abort at a time (`smelt build` → fix the family
generally → repeat).

## How far `smelt build` gets now

Before: aborted at the first HIR/MIR family (`unresolved class Request`, then the
`await outside an async function` and `arr.length` assignment families named in
the task brief).

After: the build advances well past those, through the whole `array/`, `promise/`,
`function/` surface and deep into `compat/`, now aborting at
`src/compat/util/uniqueId.ts` on the **module-level mutable global** family
(documented below as a deliberate deferral). Every family listed under "Fixed"
was verified by reducing it to a minimal TS fixture that builds end to end.

## Fixed families

Each fix lowers through a general rule (no per-file special-casing); regression
tests are noted per family.

1. **`Request` host object** (HIR `unresolved class`).
   `new Request(...)` is only ever probed for host identity
   (`isPlainObject(new Request(...)) === false`), so it is now a marker-only host
   object like `WeakMap`/`DataView`: added to `smelt_stdlib::host_object::HOST_OBJECTS`
   (`__smelt_request`) and to the marker-only constructor dispatch in
   `crates/smelt-frontend-ts/src/lowering/new_expr.rs`. `instanceof Request` and the
   runtime for-in/`toStringTag` prelude pick it up automatically from the registry.
   Test: `new_request_lowers_to_marker_record`.

2. **`String.raw` tagged template** (HIR — tagged templates were unsupported).
   Added a general `tagged_template_expression` lowering. `String.raw` is a stdlib
   builtin with fully-defined semantics for any template — concatenate the **raw**
   quasis interleaved with the string-coerced substitutions — implemented exactly
   (`new_expr.rs`), and dispatched from both the expression and call-argument paths.
   User-defined tags (which need a `TemplateStringsArray` with a `.raw` sibling that
   Smelt's homogeneous array model cannot represent) remain an explicit, descriptive
   error. Test: `string_raw_tagged_template_lowers_to_raw_string`.

3. **`await outside an async function` for async matchers** (MIR gate #1, named).
   `expect(promise).rejects.toThrow(...)` desugars to an inlined awaited try/catch
   (`lowering/stdlib.rs::vitest_rejects_to_throw_call`). A non-`async`
   `() => expect(...).rejects.toThrow()` test callback therefore contains a real
   `await`, but was lowered as a non-async test function with no async state machine,
   which HIR validation (surfaced by MIR `lower_hir`) rejects. Fix: a test callback
   whose lowered body directly contains an `await` is marked async and given a state
   machine, whether the source spelled `async` or the matcher inlined the await
   (matching JS, where the callback returns the pending promise for the framework to
   await). `body_contains_await` helper + both test-function paths in
   `lowering/testing/suites.rs`; async tests already emit `#[tokio::test] async fn`.
   Test: `rejects_matcher_marks_test_function_async`.

4. **`arr.length = n` assignment** (MIR gate #2, named — `src/array/pull.ts`).
   Assigning an array's `length` resizes it. The shrink case (`n <= arr.length`,
   which is what real code overwhelmingly does) lowers to an in-place truncating
   splice (`ListSplice { start: n, delete_count: None, mutate: true }`), reusing the
   existing splice machinery rather than inventing an op. Intercepted at the
   assignment-statement level in `lowering/stmt/assignments.rs::
   try_lower_list_length_assignment_statement`. *Deferred subset:* growing a list past
   its current length (JS pads with `undefined` holes) has no representation in a
   homogeneous `Vec<T>`; that lowers to a no-op growth, documented in the helper.
   Test: `array_length_assignment_lowers_to_splice`.

5. **Index write on an optional array after default-init** (`src/compat/_internal/copyArray.ts`).
   `if (array == null) { array = ...; } … array[i] = source[i];` on `array?: T[]`.
   Post-`if` null narrowing was missing for the reassigning branch, so `array`
   stayed `Optional<List<T>>` and the write became a non-assignable `OptionalIndex`.
   Fix: after a no-`else` `if` whose guard is a nullish test and whose consequent
   unconditionally reassigns the guarded variable to a non-nullish value, narrow it
   to its non-null type (the same merge the existing must-exit path performs).
   `branch_reassigns_to_nonnull` in `lowering/testing/matchers.rs` + the if-statement
   handler in `lowering/decls/types_iface.rs`.
   Test: `index_write_after_default_init_uses_index_target`.

6. **`let` arrow reassignment** (`src/compat/array/xorWith.ts`).
   `let comparator = (a, b) => …; … comparator = other;` lifted the arrow to an
   immutable function item, so the reassignment target was a non-assignable
   `ExprKind::Item`. Fix: only lift `const` arrow declarations to the callback/item
   form (sound — a `const` can never be reassigned); `let`/`var` arrows fall through
   to the general initializer path, binding a mutable closure-valued local so the
   reassignment is a plain local write. `lowering/testing/matchers.rs::
   variable_declaration`. Test: `let_arrow_reassignment_binds_closure_local`.

7. **Postfix update inside a nested function-expression initializer**
   (`src/compat/function/bind.ts`).
   `const bound = function (...) { … a[k++] … };` — a variable-declaration
   initializer sets a pending "deferred postfix updates" list so `const y = x++`
   observes the old value, but that deferral leaked across the nested function
   boundary: the `k++` inside the function body deferred into the *outer*
   declaration's list, which then flushed an assignment statement referencing the
   nested body's locals into the enclosing body — a cross-body dangling expr id that
   MIR lowering caught as "HIR expr index should be valid". Fix: reset (save/restore)
   `deferred_postfix_updates` at nested-body boundaries in
   `lowering/expr/operators.rs::function_expression_value` and
   `lowering/callbacks/body_lowering.rs::closure_body_expr_from_parts`.
   Test: corpus case `postfix_update_in_nested_function_initializer`.

## Deferred family (current build frontier)

**Module-level mutable global state** — `src/compat/util/uniqueId.ts`:

```ts
let idCounter = 0;
export function uniqueId(prefix = ''): string {
  const id = ++idCounter;         // <- aborts here
  return `${prefix}${id}`;
}
```

`++idCounter` mutates a module-level `let` shared across calls. Smelt currently
const-folds a numeric module-level `let` to its initializer, so `idCounter` reads
as the literal `0` and the increment has no assignable place ("only local, field,
and index expressions can be assigned"). Modeling this correctly is a distinct,
larger feature: module-level mutable variables that are written from function
bodies need a real shared-mutable-state representation in generated Rust
(`static`/`thread_local`/atomic) plus flow that stops const-folding a mutated
`let`. This is an architectural change rather than a lowering-rule gap, so it is
deferred here rather than hacked. It is the next family to clear before the crate
emits.

## Validation

- `cargo test --workspace --exclude smelt-gui` — green (updated the
  `27_optional_chains` end-to-end snapshot for the new `__smelt_request` marker in
  the emitted runtime prelude).
- `cargo clippy -p smelt-frontend-ts -p smelt-stdlib --lib -- -W clippy::pedantic` — clean.
- New regression tests: 6 HIR-level tests in
  `crates/smelt-frontend-ts/src/tests/estk_mir_gate_tests.rs` and 5 end-to-end cases
  in `crates/smelt-codegen-rust/tests/compile_corpus.rs`.

Because the crate does not yet fully emit `dist-smelt` (the module-mutable-global
family above still aborts the whole-crate build), the generated-Rust diagnostics
and test-report "prize" artifacts were not produced this pass.
