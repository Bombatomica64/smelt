# TS7 parity, phase 2, round 7: Hono mount

Branch `feat/hono-round7`. Hono v5 `0f5ad7b6`: **448 passed / 6 failed** of
454, up from 446. No test that passed before fails.

Three mount failures were traced at run time, with `eprintln!` probes in the
generated crate (investigation only). Each has a general fix.

## A function field's contextual type (`replaceRequest`)

`mount(app, { replaceRequest: (req) => req })` typed the arrow `(Context) =>
Context`, so every mounted request became `about:blank`.

- The record arm of `MountOptions` lowers to a `Dict`. Its value type joins
  every field's type, so both `MountOptionHandler` and `MountReplaceRequest`
  were offered as the property's hint, and the first function arm won.
- A property hint that names more than one function signature is now
  dropped (`unambiguous_property_hint`), and the arrow types itself through
  the TS7 checker.
- Pinned by `record_function_field_takes_its_own_signature` (CLI `dump-hir`).
  The test fails when the rule is disabled.
- There is no end-to-end example: a `Dict` record with two different function
  fields has no compilable Rust form yet (its values become a union the
  literal does not wrap), and the union-arm spelling erases the options, which
  the examples corpus forbids. Lowering inline object types to structs, as
  object aliases already are, would remove both limits.

## Self-construction argument types (`basePath`)

`#clone()` builds `new Hono<E, S, ..>({ router, getPath })` inside `class Hono`.

- The class item does not exist yet while its own body lowers, so the
  arguments were lowered with no hint. The options object became an erased
  record, and the result type lost its type arguments.
- A prepass now records each class's constructor parameter types
  (`collect_class_constructor_param_types`, in the class's type-parameter
  scope).
- A pending or scoped-name construction hints its arguments from them, with
  explicit type arguments (else defaults) substituted, and keeps the explicit
  class arguments.
- Example 183 covers it. `tail_r8_tests::generic_class_without_args_*` now
  sees `Cache<T>` instead of the `Cache<_>` placeholder: the same requirement
  (generic arguments are not dropped), with a precise type.
- The prepass interns types earlier, which renumbers interned types in 22
  examples' HIR/MIR dumps. That churn is ordering only.

## Function-typed fields survive erasure (`basePath`, continued)

`Object.assign(this, rest)` copies `HonoOptions` through its erased view. The
`IntoSmeltUnknown` impl of an interface struct wrote every function field as
`SmeltUnknown::Null`, so the `Router` read back had a `match` that returned
`null`.

- The impl now wraps such a field in a callable adapter
  (`erased_function_field_text`):
  - arguments are converted with `SmeltFromUnknown`, the target type
    inferred from the callable;
  - a borrowed `&T` parameter gets a reference;
  - the result is erased with `IntoSmeltUnknown`;
  - the typed callable is registered as its origin, so converting back
    returns that same callable.
- Signatures the adapter cannot express are declined, keeping the old
  spelling: rest, async, `&mut` and `impl` parameters, and parameters that
  cannot come from `SmeltUnknown`.
- Pinned by `record_function_fields_erase_to_callable_adapters`, which fails
  when the adapter is disabled.

## Still failing (6)

- **Timeout (3).** Unchanged from round 6: priming and the virtual clock.
- **Mount (3).** These need a decision. Each passes a value that violates
  its declared type:
  - `return undefined as unknown as Response`, then `if (res)` in `mount`
    (2 tests);
  - a `// @ts-ignore` mock `{ waitUntil: 'waitUntil', .. }` as an
    `ExecutionContext`, whose fields are methods.

## Ratchets

- es-toolkit: 1052 passed / 7 failed (the same 7); avoidable 22157
  (unchanged); no program less typed.
- Examples: avoidable 0; no program less typed; example 183 added to the
  typedness baseline.
