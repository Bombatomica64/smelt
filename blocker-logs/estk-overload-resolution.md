# es-toolkit: function-overload resolution + `toHaveProperty` blockers

Investigation of the `no overload of \`X\` matches this call` family (11
es-toolkit compat spec files) and the
`expect(...).toHaveProperty(...) requires an object or map actual value` gap
(2 files), against es-toolkit pinned at `e008a2818cd8`.

## Root cause of the `args: []` family

Every one of the 11 overload failures — including the plausible-looking ones
(`clamp` with 1 arg, `inRange(0)`, `fill(array)`, `isMatchWith(a, b)`) — sits
directly under a `// @ts-expect-error` or `// @ts-ignore` pragma in the spec
source. The lodash-compat suites deliberately call the helpers with too few or
mistyped arguments to probe runtime coercion behavior:

```ts
// @ts-expect-error
expect(split()).toEqual(['']);          // args: []
// @ts-expect-error - testing runtime behavior when only one argument is provided
expect(clamp(5)).toBe(5);               // args: [Float] vs [2,3]-ary overloads
```

The `args: []` reports were not an argument-collection bug: the calls really
have zero arguments. `tsc` accepts these files only because the author
suppressed the checker on that line; Smelt's overload resolver had no notion of
that suppression (it only recognized `@ts-expect-error` comments carrying the
literal code `ts2353`) and aborted the whole file — and any one unbuildable
spec aborts the entire crate build.

## Fix 1: suppressed calls fall back to the implementation signature

`crates/smelt-frontend-ts/src/lowering/stmt/assignments.rs` gains
`has_ts_error_suppression_before(start)`: a line-precise scan that reports a
`@ts-expect-error` / `@ts-ignore` pragma on the contiguous comment lines
directly above the line containing `start` (matching `tsc`'s
next-line-suppression semantics; the pragma cannot leak past the statement it
annotates — regression-tested).

`selected_overload_signature`
(`crates/smelt-frontend-ts/src/lowering/stdlib/call_dispatch.rs`) now returns
`Ok(None)` instead of erroring when no overload matches **and** the call site
is suppressed. The caller already handles `None` generally: the call lowers
against the *implementation* signature (the same path used when a sibling in
the library calls the implementation directly), and the Rust emitter pads
missing trailing parameters with each parameter's default (`undefined` for
erased slots, `None` for `Optional<T>`). No per-function logic anywhere.

Semantics note: a missing argument pads faithfully (as JS `undefined`) whenever
the implementation parameter is erased (`any`/`unknown`) or optional — which
covers `split()`, `replace()`, `range()`, `rangeRight()`, `fromPairs()`,
`flatMapDeep(1)`, `invokeMap(1)`, `inRange(0)`, `fill(...)`, `isMatchWith(...)`.
A missing argument for a *required concrete* slot (es-toolkit compat `clamp`'s
`bound1: number`) pads with the type default (`0.0`), which can deviate from JS
`undefined` semantics at runtime; representing `undefined` in a concrete `f64`
ABI slot would need call-site-driven parameter widening — deferred.

## Fix 2: `None`-collapse tie-break in overload selection (cloneWith)

`cloneWith(args, noop)` mis-selected its first overload: Smelt interns
TypeScript `void`, `undefined`, and `null` as one `None` type, so matching the
`noop` callback bound `R := None` and passed the
`R extends object | string | number | boolean | null` constraint that `tsc`
fails for `void` (and `object` lowers to `Unknown`, which accepts everything).
The call's static type collapsed to `None` and the runtime value was erased to
`()`.

Overload selection now scores candidates by how free their inferences are of
`None` collapse (`overload_substitution_none_collapse_score`), applied after
the literal and specificity scores and before declaration order. A candidate
that absorbs the callback's undefined return through an explicit
`R | undefined` slot (leaving `R` unbound — `infer_overload_type` deliberately
binds nothing for `Optional(_) ⊇ None`) mirrors the checker's real selection
and wins. `cloneWith(args, noop)` now selects the `R | T` overload and keeps
the argument value. General typing rule; no per-function logic. The full
frontend suite (796 tests) passes unchanged apart from the new tests.

## Fix 3: `toHaveProperty` over erased class-shaped actuals

`clone(args)` / `cloneWith(args, noop)` produce actuals typed by the ambient
`IArguments` interface — a `Type::Class` with no local declaration, which is
represented as a live `SmeltUnknown` at runtime (frontend predicate
`class_type_erases_to_unknown`, codegen predicate `is_erased_class_type`).

- `crates/smelt-frontend-ts/src/lowering/testing/matchers.rs`
  (`dict_contains_key_expr`): accepts an actual whose class-shaped type erases
  to `SmeltUnknown`, alongside the existing `Unknown`/`Union`/`TypeParam`
  arms; the rejection message now includes the offending actual type.
- `crates/smelt-codegen-rust/src/emitter/map.rs`
  (`dict_contains_key_uses_erased_object`): such class types emit the same
  live-object inspection as `unknown` actuals instead of folding the check to
  a constant `false`.

Statically primitive actuals are still rejected (regression-tested).

## Verification

- `cargo check --workspace`, `cargo clippy -p smelt-frontend-ts -p
  smelt-codegen-rust` (pedantic set), `cargo test --workspace --exclude
  smelt-gui`: green.
- New regression tests: `part04_tests.rs`
  (`lowers_suppressed_zero_argument_overload_call_against_implementation`,
  `lowers_ts_ignore_suppressed_under_applied_overload_call`,
  `rejects_unsuppressed_overload_call_with_missing_arguments`,
  `suppression_pragma_does_not_leak_past_intervening_code`,
  `prefers_undefined_absorbing_overload_for_void_callback`) and
  `part06_tests.rs` (`lowers_to_have_property_over_erased_class_actual`,
  `rejects_to_have_property_over_primitive_actual`).
- Minimal fixture projects (suppressed zero-arg / under-applied / mistyped
  overload calls; `toHaveProperty` over `IArguments` actuals through generic
  `clone`/`cloneWith`): `smelt build` succeeds and all generated-crate
  `cargo test` cases pass, including runtime values (`joinText()` → `'-'`,
  `scale(5)` → `5`).
- No new `SmeltUnknown` conversions were introduced: the fallback lowers
  through concrete implementation types, and the matcher/codegen changes reuse
  the value's existing erased representation (ambient interop is already a
  documented dynamic boundary).

### dump-hir on the 13 files (before → after)

| file | before | after |
| --- | --- | --- |
| compat/array/fill.spec.ts | no overload of `fill` | `array callback callback item parameter count` (pre-existing, separate family) |
| compat/array/flatMapDeep.spec.ts | no overload of `flatMapDeep` | clean |
| compat/array/invokeMap.spec.ts | no overload of `invokeMap` | clean |
| compat/math/clamp.spec.ts | no overload of `clamp` | clean |
| compat/math/inRange.spec.ts | no overload of `inRange` | `array callback callback item parameter count` (pre-existing, separate family) |
| compat/math/range.spec.ts | no overload of `range` | clean |
| compat/math/rangeRight.spec.ts | no overload of `rangeRight` | clean |
| compat/object/fromPairs.spec.ts | no overload of `fromPairs` | clean |
| compat/predicate/isMatchWith.spec.ts | no overload of `isMatchWith` | `array callback callback item parameter count` (pre-existing, separate family) |
| compat/string/replace.spec.ts | no overload of `replace` | clean |
| compat/string/split.spec.ts | no overload of `split` | clean |
| compat/object/clone.spec.ts | toHaveProperty requires object/map | `unresolved identifier \`document\`` (DOM, already excluded in the fixture Smelt.toml) |
| compat/object/cloneWith.spec.ts | toHaveProperty requires object/map | `unresolved identifier \`document\`` (DOM, already excluded in the fixture Smelt.toml) |

### Whole-crate `smelt build` abort point

Unchanged before vs after: `src/predicate/isEqualWith.spec.ts`
(`const item expression shape is not supported for inlining yet`) — the
pre-existing blocker already documented in the fixture `Smelt.toml`; the
compat specs fixed here are not on the crate's current entry/test graph, so
the whole-build abort point is governed by that separate family.

## Deferred

- Runtime `undefined` semantics for suppressed calls that omit a *required
  concrete-typed* parameter (compat `clamp(5)`): needs undefined-capable ABI
  slots or call-site-driven widening.
- The `array callback callback item parameter count` family now surfaced in
  fill/inRange/isMatchWith (was masked by the earlier abort) — separate
  investigation.
- The `arguments`-object model carries only `length`; `toHaveProperty('0')`
  over a real `toArgs([1,2,3])` value lowers and runs but reports the indexed
  key as absent at runtime.
