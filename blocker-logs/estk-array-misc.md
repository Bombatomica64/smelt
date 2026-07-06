# es-toolkit array/callback misc lowering gaps (`claude/estk-array-misc`)

Investigation and fixes for six small array/callback lowering sub-families
observed on es-toolkit (pinned `e008a2818cd8`), nine spec files. Each family
was attributed with `smelt dump-hir`, reproduced in a minimal TS fixture
project, fixed with a general rule (no per-library special cases), and
verified end to end (`smelt build` + `cargo test` on the generated crate).

Baseline whole-project first abort (unchanged by this work, different family):
`src/predicate/isEqualWith.spec.ts` — "const item expression shape is not
supported for inlining yet".

Stale-binary correction: an earlier draft of this report attributed the root
abort to "dynamic computed access on the global object requires the runtime
global object (not yet modeled)". That message came from a stale debug binary
left in the scratchpad (the on-disk `baseline-abort.txt` / `eqw.err` captures
reflect it). Re-verified with three freshly compared binaries — the pre-change
baseline, the prior WIP build, and this finalized `target/debug/smelt` — all
abort at the identical point on `src/predicate/isEqualWith.spec.ts` with "const
item expression shape is not supported for inlining yet". The whole-project root
abort is therefore genuinely IDENTICAL before and after this work, and unrelated
to the six families fixed here.

## 1. Default `sort()` on union/erased element arrays — FIXED

- Files: `compat/array/shuffle.spec.ts`, `compat/array/sampleSize.spec.ts`,
  `compat/object/values.spec.ts`, `compat/object/valuesIn.spec.ts`.
- Diagnostic: `array sort supports boolean, number, and string arrays for now`.
- Root cause: the mission's guess (comparator support) was already implemented;
  the actual failures are comparator-**less** `.sort()` calls whose receiver
  element type is a union (`values(object)` → `string | number`) or an erased
  indexed-access surface (`shuffle(object)` → `Array<T[keyof T]>` → `unknown`).
  Both the frontend (`stdlib.rs::list_sort_call`) and the emitter
  (`list_mutation.rs::list_sort_text`) restricted default sort to scalar
  elements.
- Rule implemented: JavaScript's default sort compares the `ToString` coercion
  of each element. The frontend now accepts `Unknown` and `Union` element
  types for comparator-less sorts; the emitter sorts them with a stable
  `sort_by` over the shared JS string-coercion match (factored into
  `js_string_coercion_match_text`, reused by `string_like_operand_text`).
  Concrete unions project through `into_smelt_unknown` first. Structured
  concrete shapes (nested lists/records) stay rejected because their JS
  `ToString` (`"1,2"` for arrays) is not modeled yet.
- Known divergence (documented in code): JS hoists `undefined` elements to the
  end without comparing; the coercion sorts them as the string `"undefined"`.
- Verification: `sortgen` fixture (mixed `string | number` values sorted as
  `[2, 'a', 'b']`, plus scalar regression) passes end to end; frontend tests
  `lowers_default_sort_on_{union,erased}_element_arrays`; codegen test
  `emits_string_coercion_default_sort_for_union_elements`.

## 2. `concat` widening for non-matching arguments — FIXED

- File: `compat/array/reverse.spec.ts` (`range(n).concat([null as any])`).
- Diagnostic: `array concat requires an array or element argument matching the
  receiver`.
- Root cause: `null as any` lowers to `None`, so the argument is `List<None>`;
  `list_ops.rs::list_concat_argument` only accepted arguments whose element
  type matched (or erased into) the receiver's `Float`.
- Rule implemented: JavaScript `A[].concat(B[])` yields `Array<A | B>`. The
  concat lowering now widens the result element type using the same
  unification array literals use (`T`/`null` mixes widen to `Optional<T>`,
  exactly how `[0, 1, 2, null]` infers), re-typing the accumulated left list
  so the emitter injects elements at use. Pairs that would only unify by
  erasing to `unknown` still keep the explicit diagnostic instead of silently
  introducing a tagged dynamic value (per the SmeltUnknown enforcement rules).
- Verification: `concatnull` fixture (null appended after `range(3)`, plus
  plain scalar/array concat regressions) passes end to end; frontend test
  `concat_widens_element_type_for_null_list_argument` asserts the
  `List<Optional<Float>>` result type.

## 3. Lodash `[path, srcValue]` array iteratee — FIXED

- File: `compat/function/memoize.spec.ts`.
- Diagnostic: `array callback methods currently require arrow function
  callbacks`.
- Root cause: the mission's guess (function expressions/references) was
  already supported; the real span is
  `lodashStable.find(this.__data__, ['key', key])` — the lodash
  `matchesProperty` array iteratee shorthand, an `ArrayExpression` in callback
  position that the classifier rejected.
- Rule implemented: callback classification now lowers a two-element
  `[stringLiteral, expr]` array argument as the matchesProperty predicate
  `element[path] === srcValue` (`classify.rs::matches_property_iteratee_callback`).
  A callable is required in that position for native array methods, so an
  array literal reaching callback classification can only be this iteratee
  form; non-literal paths keep the existing diagnostic.
- Verification: `matchprop` fixture (memoize-style `CustomCache` class using
  `utils.find(this.__data__, ['key', key])`) passes end to end; frontend test
  `lowers_matches_property_array_iteratee_callback`.

## 4. `Object` as a callback — FIXED

- File: `compat/math/parseInt.spec.ts` (`['6', '08', '10'].map(Object)`).
- Diagnostic: `array callback local callback `Object` is not in scope`.
- Root cause: the builtin-function-as-callback table (`Number`, `Boolean`,
  `parseInt`, ...) did not model the global `Object` function.
- Rule implemented: `Object(value)` boxes a primitive into its wrapper object;
  Smelt does not model wrapper objects separately from their primitive values
  (a boxed string coerces back to the same string everywhere), so `Object` in
  callback position lowers as the typed identity closure on the receiver's
  element type (`dispatch.rs::callback_argument`). This keeps the mapped
  list's concrete element type (`string[]` stays `string[]`) instead of
  erasing it, so the follow-up `strings.map(parseInt)` keeps its real parse.
  Shadowed local/imported `Object` bindings take precedence.
- Verification: `objcb` fixture (`map(Object)` then a compat-parseInt-shaped
  helper mapping to `[6, 8, 10]`) passes end to end; frontend test
  `lowers_object_builtin_as_identity_callback` asserts the mapped list keeps
  `List<String>`.

## 5. `in` operator inside callback array literals — FIXED

- File: `compat/object/unset.spec.ts`
  (`props.map(key => { ...; return [unset(object, key), toString(key) in object]; })`).
- Diagnostic: `callback binary operator is not supported yet`.
- Root cause: the callback array-literal lowering in `dispatch.rs` had its own
  restricted per-element `BinaryExpression` arm that called
  `callback_binary_op` directly, bypassing the full callback expression
  dispatcher's dedicated `in`/`typeof`/`instanceof`/nullish handling.
- Rule implemented: the duplicated arm was removed; binary elements now fall
  through to the general `as_expression → callback_expression` path, so every
  operator form works inside array literals exactly as in any other callback
  position. (`callback_binary_op`'s residual diagnostic now names the
  offending operator.)
- Verification: `incb` fixture (`keys.map(key => [key !== '', key in object])`
  with expected truth table, plus a direct `key in object` predicate) passes
  end to end; frontend test `lowers_in_operator_inside_callback_array_literal`.

## 6. Compound member assignment in callbacks — FIXED

- File: `compat/object/pickBy.spec.ts` (`args => { args[1] += ''; return args; }`).
- Diagnostic: `callback assignment targets must be captured locals`.
- Root cause: member-target stores inside the side-effect-free callback
  expression IR already retried through full closure-body lowering, but only
  for plain `=`; compound operators (`+=`, ...) fell through to the hard
  error.
- Rule implemented: any member-target assignment operator now raises the
  `callback member assignment needs closure-body lowering` retry signal, and
  the closure-body path lowers the compound store as a real indexed mutation.
- Verification: `memberassign` fixture (`row[1] += ''` and a suffix-appending
  helper asserting the mutated rows) passes end to end; frontend test
  `compound_member_assignment_in_callback_retries_closure_body`; codegen test
  `emits_member_store_for_compound_callback_assignment`.

## Remaining diagnostics on the nine files (different families, not in scope)

- `compat/array/shuffle.spec.ts` — clean (lowers fully).
- `compat/array/reverse.spec.ts` — clean.
- `compat/math/parseInt.spec.ts` — clean.
- `compat/object/unset.spec.ts` — clean.
- `compat/object/pickBy.spec.ts` — clean.
- `compat/array/sampleSize.spec.ts`, `compat/object/values.spec.ts`,
  `compat/object/valuesIn.spec.ts` — next blocker is
  `const item expression shape is not supported for inlining yet` (const-item
  inlining family, unrelated to the six above).
- `compat/function/memoize.spec.ts` — next blocker is
  `this class type is not resolvable yet` (`new ImmutableCache() as this`,
  polymorphic-this family).

## Whole-project abort point

- Before: aborts at `src/predicate/isEqualWith.spec.ts` ("const item expression
  shape is not supported for inlining yet").
- After: identical — same file, same "const item expression shape is not
  supported for inlining yet" message (that file sorts before the nine fixed
  files in the build order and belongs to a different family). The nine target
  files no longer raise any of the six diagnostics above.
- See the stale-binary correction near the top of this report: the abort is
  provably unchanged; the previously reported "dynamic computed access on the
  global object" message was a stale-binary artifact, not a real difference.

## Deferrals

- None of the six sub-families were deferred. Two adjacent gaps observed while
  fixturing were left out of scope and are noted for follow-up:
  `js_strict_eq` is not implemented on concrete `SmeltUnion*` enums (field
  compare on a mixed-type record), and `String.length` comparisons against
  float literals inside compact callbacks emit an `i64`/`f64` mismatch.
