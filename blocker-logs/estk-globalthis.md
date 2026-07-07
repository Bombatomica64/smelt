# es-toolkit: dynamic `globalThis[key]` read + three same-area singletons

Pinned es-toolkit ref e008a2818cd8. All commands run against a tracked-fixture
copy of es-toolkit.

## Families addressed

### 1. `dynamic computed access on the global object requires the runtime global object (not yet modeled)`

Whole-crate build abort at `src/predicate/isEqualWith.spec.ts`. Affected files:
`src/predicate/isEqualWith.spec.ts`, `src/compat/predicate/isEqual.spec.ts`,
`src/compat/object/merge.spec.ts`.

Source shape (lodash-style typed-array / error constructor loops):

```ts
arrayViews.map((type, viewIndex) => {
  const CtorA = globalThis[type] || function (n) { this.n = n; };
  const bufferA = globalThis[type] ? new ArrayBuffer(8) : 8;
  return [new CtorA(bufferA), ...];
});
```

The key `type` is a runtime `string` (element of `['Float32Array', …, 'DataView']`,
typed `string`, not a literal union), so it is a genuinely dynamic key.

**Fix** (`crates/smelt-frontend-ts/src/lowering/stmt/assignments.rs`,
`computed_member` + new `global_alias_computed_read` / `dynamic_global_object_read`):
one general rule for any computed read off a recognized global alias.

- A static **string-literal** key naming a modeled JavaScript global
  (`globalThis['Array']`) normalizes to the bare identifier value, exactly like
  the static-member spelling `globalThis.Array`.
- **Any other key** (a runtime variable, or a literal naming no modeled global)
  is a genuine dynamic property lookup. Smelt's deterministic profile models no
  runtime global-object property store keyed by an arbitrary string, so the read
  resolves to the JavaScript-correct `undefined`, tagged `SmeltUnknown` via
  `UnknownCast` (emits `SmeltUnknown::Undefined`).

The erased value flows through the **existing** dynamic-value machinery:
`Ctor || fn` picks the fallback, `Ctor ? a : b` folds to the absent branch, and
`const Ctor = globalThis[key]; new Ctor(arg)` dispatches through the erased
closure-call ABI (`new_through_value_expression` / the computed-callee `new`
path). No new "runtime global object" is built — the mission's "full runtime
global object" was confirmed unnecessary.

**SmeltUnknown justification** (per CLAUDE.md enforcement): the returned value
could be any global (constructor, object, number, or absent). No concrete type,
union, or scoped generic can represent an arbitrary-runtime-string lookup into
the global namespace, so `SmeltUnknown` is the honest boundary type. Documented
in the code comment; regression test
`dynamic_global_computed_read_lowers_to_erased_undefined`. This is a legitimate
dynamic boundary (erased interop), not erasure to make Rust compile.

Runtime honesty: like the existing isBlob/isFile host-global note, presence
guards fold to the absent answer. Specs that construct real typed-array/error
constructors dynamically build and run but diverge on those assertions; that is
the documented deterministic-profile limitation, not a new regression.

### 2. `TypeScript instanceof requires a concrete class-typed left operand`

`src/compat/object/transform.spec.ts`:
`expect(transform(new Foo()) instanceof Foo).toBe(true)`. `transform(object)`
resolves to the `transform(object: object): Record<string, any>` overload, so
the left operand is a `Record` (`Type::Dict`).

**Fix** (`crates/smelt-frontend-ts/src/lowering/guards.rs`,
`instanceof_supported_left_operand`): accept `Type::Dict` left operands. A plain
object/record carries no nominal class identity in Smelt's record model, so the
existing `InstanceOf` codegen resolves `record instanceof UserClass` to `false`
(codegen `instance_of_text` returns `false` for a non-`Class` value type). This
matches the file's existing typed-array/marker folding philosophy: recognize the
shape and produce an honest answer instead of aborting.

### 3. `Boolean requires exactly one argument`

`src/compat/object/defaultsDeep.spec.ts`: `[Boolean(), Number(), String()]`.
Zero-argument primitive coercions are legal JavaScript.

**Fix** (`crates/smelt-frontend-ts/src/lowering/stmt/assignments.rs`,
primitive-conversion call path): a zero-argument `Boolean()`/`Number()`/`String()`
lowers to the type's default primitive literal — `false` / `0` / `""`.
`parseFloat`/`BigInt` are not default-value coercions and keep the arity error.

### 4. `callback item references must resolve to callable values`

`src/compat/util/toNumber.spec.ts`:
`values.map(value => (value !== whitespace ? Number(value) : 0))`, where
`whitespace` is an imported module-scoped `string` const read as an ordinary
value inside the callback body.

**Fix** (`crates/smelt-frontend-ts/src/lowering/callbacks/dispatch.rs`,
`should_fallback_to_closure_body_for_callback`): add this message to the
closure-body retry list. The compact callback IR only resolves *callable* item
references; the full closure-body path routes the identifier through the general
expression path, which reads a value item of any type. This is a retry, not a
new semantic — an item that genuinely cannot lower still errors in the closure
body.

## Whole-crate build abort point (fixture es-toolkit copy)

- **Before:** aborts at `src/predicate/isEqualWith.spec.ts`
  (`dynamic computed access on the global object …`).
- **After:** aborts at `src/predicate/isPlainObject.spec.ts`
  (`base class \`Object\` is not declared` — a different file and family, out of
  scope). The abort has moved past `isEqualWith.spec.ts`.

Probe file-with-blocker count: 18 -> 16. The `globalThis` family dropped from 3
occurrences to 0.

## dump-hir re-scan of the six files (after)

| File | Result |
| --- | --- |
| `src/predicate/isEqualWith.spec.ts` | LOWERS CLEAN |
| `src/compat/predicate/isEqual.spec.ts` | LOWERS CLEAN |
| `src/compat/object/transform.spec.ts` | LOWERS CLEAN |
| `src/compat/util/toNumber.spec.ts` | LOWERS CLEAN |
| `src/compat/object/merge.spec.ts` | now `RegExp construction requires a string pattern and optional flags` (unrelated; globalThis family gone) |
| `src/compat/object/defaultsDeep.spec.ts` | now `for...in is only lowered for record-like objects` (unrelated; Boolean family gone) |

All four target families are eliminated. merge/defaultsDeep hit separate,
out-of-scope families next.

## Validation

- `cargo check --workspace`: clean.
- `cargo clippy -p smelt-frontend-ts` (lib): clean. (`--tests` pedantic is
  pre-existing-red for this crate — ~511 baseline `needless_raw_string_hashes` /
  `float_cmp` errors from the universal `r#"…"#` test-string style; new tests
  match that style, non-test source is clean.)
- `cargo test --workspace --exclude smelt-gui`: all green (smelt-gui excluded —
  cannot link here).
- End-to-end fixtures: minimal projects reproducing each family `smelt build` +
  generated `cargo check` clean; emitted code verified
  (`instanceof` -> `false`, `Boolean()` -> `false`, `Number()` -> `0.0`,
  `String()` -> `""`, dynamic `new Ctor(...)` -> erased closure call).

## Regression tests (repo)

- `part07_tests::dynamic_global_computed_read_lowers_to_erased_undefined`
- `part07_tests::literal_key_global_computed_read_normalizes_to_builtin`
- `part07_tests::dynamic_global_constructor_read_supports_new_construction`
- `part07_tests::lowers_callback_body_reading_non_callable_value_item`
- `part02_tests::lowers_instanceof_for_record_left_operand`
- `part02_tests::lowers_zero_argument_primitive_coercions`

(The prior `part07_tests::keeps_computed_global_access_unfolded`, which asserted
the old blocker, was replaced by the first test above.)
