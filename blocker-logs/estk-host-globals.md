# es-toolkit host globals + Error `cause` options

Branch: `claude/estk-host-globals`. Source: `toss/es-toolkit` @ `e008a2818cd8`.

Five es-toolkit blocker families are addressed here. All fixes lower through
general rules (no per-source special cases): a host constructor/function used as
a *value* lowers to a first-class closure or shared marker record, and the
ES2022 Error options form lowers through one shared parts-based helper. Each
sub-task below lists the source usage found, the general rule implemented, and
how it was verified.

Verification tooling: the fixed binary's `dump-hir` (per-file, run against the
pinned checkout with the fixture `Smelt.toml`), focused end-to-end fixtures
(`smelt build` + generated `cargo test`), in-repo regression tests
(`crates/smelt-frontend-ts/src/tests/category_tests.rs`,
`crates/smelt-codegen-rust/src/tests/{part_2,part_7}_tests.rs`), and the
whole-crate `smelt build` abort point before/after.

## 1. `Proxy` as a first-class value

- Usage: `isFunction(Proxy)`, `new Proxy({}, {})` through a captured value,
  a `Proxy` entry in a value table (`src/compat/predicate/isFunction.spec.ts`,
  `src/predicate/isFunction.spec.ts`, `src/predicate/isPlainObject.spec.ts`).
- Rule: a bare `Proxy` reference lowers to the transparent identity closure
  `(target) => target` (`transparent_proxy_value_closure_expression` in
  `references.rs`), mirroring the existing `new Proxy(target, handler)`
  construction lowering that resolves to `target`. A dynamic `new` through the
  value (a closure call) reproduces the transparent result, and
  `typeof Proxy === 'function'` holds because the value is a real function.
- Verified: `isFunction.spec.ts` (both copies) now lower clean; e2e fixture
  asserts `typeof Proxy === 'function'` and `new P({a:1},{}).a === 1`;
  regression test `bare_proxy_value_lowers_to_transparent_constructor_closure`.

## 2. `Intl` namespace value + `new Intl.<Constructor>()`

- Usage: bare `Intl` value and `new Intl.Locale('en')`
  (`src/predicate/isPlainObject.spec.ts`).
- Rule: bare `Intl` joins `Math`/`JSON`/`Reflect`/... as a shared
  `__smelt_builtin_namespace` marker record; `new Intl.<Member>(...)` for a
  modeled ECMA-402 constructor lowers through the shared registry to a
  marker-only host-object record keyed on the full qualified path
  (`intl_namespace_constructor_expression` in `new_expr.rs`). An unmodeled
  `new Intl.<Member>()` falls through to the ordinary member-callee
  construction rather than being silently stamped with an Intl marker
  (preserves the qualified-path resolution rule from CLAUDE.md).
- Verified: `Proxy`/`Intl` blockers gone from `isPlainObject.spec.ts`; e2e
  fixture asserts `typeof Intl === 'object'`; regression tests
  `bare_intl_namespace_value_lowers_to_marker_record`,
  `new_intl_namespace_constructor_lowers_to_marker_record`,
  `new_unmodeled_intl_member_falls_through_without_marker`.

## 3. `encodeURI` as a value

- Usage: `encodeURI` referenced without an immediate call, e.g. a native-
  function table entry (`src/compat/object/cloneDeepWith.spec.ts`).
- Rule: the bare `encodeURI` reference lowers to a `(string) => string` closure
  running the same `ExprKind::UriEncode` op as the direct-call lowering
  (`references.rs`), so call and value forms share one IR op.
- Verified: `cloneDeepWith.spec.ts` lowers clean; e2e fixture asserts
  `encodeURI('a b') === 'a%20b'` for both call and value forms; regression test
  `encode_uri_call_and_value_forms_lower_to_uri_encode`.

## 4. `setTimeout` as a value

- Usage: `const original = globalThis.setTimeout;` before mocking
  (`src/compat/function/delay.spec.ts`); `globalThis.` normalizes to the bare
  name first.
- Rule: the bare `setTimeout` reference lowers to a
  `(callback, delayMs) => setTimeout(callback, delayMs)` closure running the
  same `AsyncOp::SetTimeout` as the direct-call timer lowering
  (`builtin_set_timeout_value_closure_expression` in `references.rs`), so the
  value form schedules on the shared virtual-time timer queue.
- Verified: `delay.spec.ts` lowers clean; e2e fixture asserts
  `typeof setTimeout === 'function'` through a captured value; regression test
  `bare_set_timeout_value_lowers_to_timer_closure`.

## 5. ES2022 Error / AggregateError options constructor

- Usage: `new Error(msg, { cause })` and
  `new AggregateError(errors, msg, { cause })` (`src/object/clone.spec.ts`).
- Rule: the Error constructor lowering was refactored to a single parts-based
  helper `error_constructor_parts` (`new_expr.rs`) producing the message plus
  the retained `cause` option and `AggregateError` leading `errors` list. The
  throw path keeps only the message; the record-building value path
  (`error_object_constructor_expression`) retains `cause`/`errors` alongside the
  `__smelt_error` marker and `message`. These mirror JavaScript's
  non-enumerable own error properties: the runtime for-in / `Object.keys`
  filter (`smelt_is_for_in_object_key` / `_record_key`) hides
  `__smelt_error | message | cause | errors`. A non-literal `options` argument
  stays an honest `UnsupportedLowering` blocker, because whether a `cause` is
  attached depends on `"cause" in options`, which a general static rule can only
  answer for a literal spelling.
- Verified: `clone.spec.ts` lowers clean; focused e2e fixture builds and runs
  green (construct + read `.message`/`.cause`, `AggregateError` `.errors`, and
  the default `"Error"` message); regression tests
  `error_options_constructor_retains_cause_and_aggregate_errors` and
  `error_options_non_literal_argument_stays_a_blocker`, plus codegen
  `emits_error_constructor_values_with_runtime_error_identity` (updated to
  assert the extended for-in filter).

## Whole-crate build abort (es-toolkit)

- Before and after these fixes: `smelt build` aborts at
  `src/predicate/isEqualWith.spec.ts` ("const item expression shape is not
  supported for inlining yet") — unchanged, no regression. The host-global
  fixes clear files that sort *after* that abort, so they do not move the abort
  point.
- `src/object/clone.spec.ts` was re-included in
  `.github/compat/es-toolkit/Smelt.toml` (its only exclusion reason was the
  Error-options constructor, now resolved). Re-inclusion was confirmed not to
  move the abort point earlier.

## Remaining per-file diagnostics (after fixes)

| File | Status |
| --- | --- |
| `src/compat/predicate/isFunction.spec.ts` | lowers clean |
| `src/predicate/isFunction.spec.ts` | lowers clean |
| `src/predicate/isPlainObject.spec.ts` | Proxy/Intl fixed; remaining `base class Object is not declared` (`class extends Object`) and `Request` host global — separate, out of scope |
| `src/compat/object/cloneDeepWith.spec.ts` | lowers clean |
| `src/compat/function/delay.spec.ts` | lowers clean |
| `src/object/clone.spec.ts` | lowers clean; the spec's own `clone` round-trip still exercises dynamic `Object.getPrototypeOf` / `prototype.constructor` / `new Constructor(...)` reconstruction, so some of its runtime assertions remain gated on that separate dynamic-prototype work |

## Deferrals

- `class extends Object` and the `Request` host global in `isPlainObject.spec.ts`
  are distinct blockers not in this branch's scope; left for follow-up.
- `clone.spec.ts` runtime round-trip assertions depend on dynamic-prototype
  reconstruction (`Object.getPrototypeOf`/`prototype.constructor`/dynamic
  `new`), which is separate work; the Error-cause constructor + read-back path
  itself is verified end to end.

## Validation summary

- `cargo build --bin smelt`, `cargo check --workspace`: clean.
- `cargo clippy` (lib/bins, pedantic=deny) on the touched crates: clean. Note:
  `clippy --all-targets`/`--tests` surfaces ~457 pre-existing
  `needless_raw_string_hashes` (and other pedantic) violations across 25 test
  files unrelated to this change; plain `cargo clippy` does not compile
  `#[cfg(test)]` modules, so that is not the enforced gate and was not churned.
- `cargo test --workspace --exclude smelt-gui` (gui cannot link in this
  container): 1684 passed, 0 failed. Updated one stale snapshot
  (`examples/typescript/end-to-end/27_optional_chains/expected.rs`) whose only
  drift was the shared runtime-prelude helpers this change extends (Intl host
  markers, the `cause`/`errors` for-in filter, and the new
  `smelt_object_to_string_tag` helper).
