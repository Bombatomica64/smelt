# es-toolkit — recognized builtins used as bare VALUES (2026-06-29)

Scope: a recognized JS/TS builtin referenced as a bare value (passed as an
argument, assigned to a const) used to error `unresolved identifier X`
(missing-stdlib). Builtin FUNCTIONS in value position are now lowered to
concrete `Rc<dyn Fn(...)>` closures routed through the existing stdlib IR ops
(`PrimitiveCast`, `NumericPredicate`), never an erased `SmeltUnknown` tag.

## Modeled concretely (DONE)

Bare value form (`take(parseInt)`, `const f = Number`, …) and array-callback
form (`xs.map(Number)`, `xs.filter(isFinite)`) for:

| Builtin     | Closure body op                          |
| ----------- | ---------------------------------------- |
| `Number`    | `PrimitiveCast::ToJsNumber`              |
| `String`    | `PrimitiveCast::ToString` (pre-existing) |
| `Boolean`   | `PrimitiveCast::ToBool`                  |
| `parseInt`  | `PrimitiveCast::ToInt` (string param)    |
| `parseFloat`| `PrimitiveCast::ToFloat` (string param)  |
| `isNaN`     | `NumericPredicate::IsNaN`                |
| `isFinite`  | `NumericPredicate::IsFinite`             |

`parseInt`/`parseFloat` take a concrete `string` parameter so the cast emits the
real numeric parse instead of the erased-operand `0.0` fallback. The previous
array-callback path emitted a `Literal::None` placeholder for `Boolean`/`String`
and did not handle `Number`/`parseInt`/predicates at all; it now reuses the same
concrete closures and only the genuinely-imported predicates
(`isEmpty`/`isArray`/`isString`/`isObject`/`trim`) keep the opaque shape.

Implementation: `crates/smelt-frontend-ts/src/lowering/builder_part16.rs`
(`builtin_function_value_expression` + `builtin_*_closure_expression` helpers,
`closure_value_return_ty`) and the array-callback dispatch in
`builder_part13.rs` (`callback_argument`).

Verified end-to-end: a generated test crate compiles and runs, asserting all
seven builtins produce correct results in value form and callback form.

## Counts (whole-crate probe)

- `missing-stdlib`: 63 → 55 (−8)
- `unresolved identifier X` (missing-stdlib): 47 occ / 41 files → 39 occ / 33 files
- files with blockers: 331 → 327
- `parseInt` value-form blocker: 3 → 0
- `unsupported-lowering` 228 → 232: files that aborted at the builtin value now
  advance to their NEXT downstream blocker (e.g. `callback method apply`).

## Left as DOCUMENTED blockers (not erased)

The remaining recognized-builtin `unresolved identifier` cases are NOT bare
value-form functions — they are member access on namespace objects / constructor
prototype chains, which need member/call handlers (or a concrete namespace
model), not value-form lowering. Erasing the bare object to `SmeltUnknown` was
explicitly rejected.

- `Array.prototype.slice` (`predicate/isFunction.spec.ts`) — prototype member chain.
- `Math.PI` (`array/chunk.spec.ts`) — static numeric constant member; `Math.ceil`
  value-form is already handled but `Math.PI` is not a recognized member.
- `Reflect.ownKeys` (`object/pick.spec.ts`, `predicate/isJSONValue.ts`) — needs a
  `Reflect` namespace model + `ownKeys` member handler.
- `Map`/`WeakMap`/`Function`/`Buffer`/`Blob`/`ArrayBuffer`/`Promise`/`File` bare
  references — appear feeding `instanceof`/`new`/member access (struct-backed
  builtins / class lane), not as plain callable values.

## Deferred (per instructions — NOT touched)

Ambient globals `globalThis` (4), `global` (1), `self`, `window`, `document` (1):
left as blockers by design.
