# es-toolkit whole-crate transpile gate

Work toward es-toolkit's first whole-crate transpile (pinned ref `e008a2818cd8`,
tracked fixture `.github/compat/es-toolkit/Smelt.toml`). The scan named five
single-file blockers; each is fixed here as a general lowering rule (no per-file
special cases). Clearing them exposed several further blockers in the same files
(the scan reported only the first-failing test per file), which are also fixed.

## The five named blockers (all fixed)

1. **`unresolved class \`Request\`` — `src/predicate/isPlainObject.spec.ts`.**
   `Request` joins the marker-only host-object registry
   (`crates/smelt-stdlib/src/host_object.rs`) alongside `WeakMap`/`DataView`.
   es-toolkit only does `new Request('http://localhost')` and checks
   `isPlainObject(...) === false`; no structural surface is read, so construction
   stamps `__smelt_request` on an erased record and `instanceof`/presence-guard
   folding resolve through the marker (wired via `marker_only_builtin_marker`).

2. **`RegExp construction requires a string pattern and optional flags` —
   `src/compat/object/merge.spec.ts`.** Zero-argument `new RegExp()` (legal JS for
   the empty pattern `/(?:)/`) now lowers to an empty pattern string, exactly like
   `new RegExp('')` (`regexp_constructor_expression`).

3. **`for...in is only lowered for record-like objects` —
   `src/compat/object/defaultsDeep.spec.ts`.** `for (const key in fn)` over a
   function value. Smelt models a function as a **property-less closure** with no
   `SmeltUnknown`/record view, so casting it to a record would emit non-compiling
   Rust. The sound projection for Smelt's function model is the **empty** key
   list, and `Object.hasOwn(fn, key)` folds to a constant `false`. Both compile
   cleanly. (Attaching enumerable properties to a function is a dynamic-JS shape
   Smelt does not yet model, so the spec's own property assertions on `fn` would
   fail at runtime — a runtime-representation gap, not a transpile gap.)

4. **`TypeScript instanceof target \`Array\` is not a lowered class` —
   `src/compat/function/memoize.spec.ts`.** `x instanceof Array` now folds exactly
   like `Array.isArray(x)`: a list/tuple operand is `true`, an erased operand
   resolves through the runtime `UnknownIs { Array }` probe, any other concrete
   type is `false` (`instanceof_expression` in `lowering/guards.rs`).

5. **`function parameters must have explicit type annotations or default
   initializers` — `src/compat/predicate/matchesProperty.spec.ts`.** **FIXED, not
   excluded.** The shape is a constructor function
   `function Foo(object) { Object.assign(this, object); }` used with `new`. Smelt
   already synthesizes such functions into classes (the `object: any` form in
   `merge.spec.ts` works). A synthesized constructor's own fields are all typed
   `unknown`, so an *unannotated, non-defaulted* constructor parameter is the same
   genuinely dynamic boundary and defaults to `unknown`
   (`constructor_parameter_type` in `lowering/decls/constructor.rs`) — matching
   TypeScript's implicit-any inference for the identical idiom. The fallback is
   scoped to the constructor-function synthesis path; ordinary untyped function
   declarations still require explicit annotations.

## Further blockers exposed and fixed (same files)

Once a file's first-failing test lowered, later tests surfaced their own
blockers:

- **`String.raw` tagged template** (`isPlainObject.spec.ts`): `String.raw` is the
  one tagged template Smelt models — its raw quasi text with substitutions
  interpolated (`tagged_template_expression`). Other tags stay unsupported.
- **Cross-`it`-block class scope** (`matchesProperty.spec.ts`): a `function Foo`
  synthesized into a class in one `it` leaked into a sibling `it` declaring a
  differently-shaped `Foo`, blocking its re-synthesis. `self.locals` was already
  scoped per test case; the class registry now is too
  (`test_case_class_scope`/`restore_test_case_class_scope` in
  `lowering/testing/suites.rs`).
- **`Object.hasOwn` over a function value** (`defaultsDeep.spec.ts`): folds to
  `false`, consistent with the empty `for...in` projection above.
- **Constructor-presence-guard ternary** (`merge.spec.ts`):
  `Uint8Array ? new Uint8Array([1]) : { buffer: [1] }`. `Uint8Array` is a bare
  host constructor Smelt always models as present, so the probe is always true;
  the ternary folds to its consequent and keeps the concrete `List` shape.
  Reconciling the mismatched `List`/`Dict` arms through `SmeltUnknown` is exactly
  the avoidable erasure the ABI rules forbid
  (`identifier_is_always_present_global_constructor`).

## Status

- **All five named spec files now fully HIR-lower** (`smelt dump-hir`, which
  collects every error per file, is clean for each).
- **The whole crate now fully HIR-lowers.** `smelt build` previously aborted at
  the first spec file; it now clears HIR lowering for the entire crate and
  advances to **MIR lowering / HIR validation**.
- **The whole-crate transpile does NOT yet fully succeed.** MIR/validation
  reveals further, unrelated blockers beyond the five, in files the earlier abort
  never reached:
  - `await outside an async function` in async closures (e.g. `promise/**`,
    `array/**Async**` spec callbacks) — async-arrow *value* closures whose async
    state machine is not built on their body.
  - `only local, field, and index expressions can be assigned at
    src/array/pull.ts` — an assignment-target lowering blocker in a source module.
  These are the next gates; a full clean whole-crate transpile requires clearing
  this cascade. Because the transpile does not complete, `rust-test-report` was
  not run.

## Verification

- New regression tests (repo test style):
  `crates/smelt-frontend-ts/src/tests/estk_transpile_gate_tests.rs` (Request
  marker, zero-arg RegExp, `for...in`/`hasOwn` over a function value,
  `instanceof Array` list/unknown, `String.raw`, constructor-presence-guard
  fold) and additions to
  `crates/smelt-frontend-ts/src/tests/constructor_function_tests.rs`
  (unannotated constructor param, plain-function rejection preserved, sibling-`it`
  class scoping).
- End-to-end fixture (`smelt build` + generated `cargo test`): zero-arg RegExp,
  `instanceof Array`, `String.raw`, the presence-guard ternary, and the
  function-value `for...in`/`hasOwn` all **compile and run green**. The
  `Object.assign(this, object)` constructor populates no fields at runtime — a
  pre-existing synthesized-class limitation shared with the annotated `object:
  any` form, independent of the parameter-type fix.
- `cargo check --workspace`: clean.
- `cargo clippy --lib -- -W clippy::pedantic` on `smelt-stdlib` +
  `smelt-frontend-ts`: clean.
- `cargo test`: `smelt-frontend-ts` (841) + `smelt-stdlib` (21) + `smelt-hir`
  (511) + `smelt-codegen-rust` suite all green.
