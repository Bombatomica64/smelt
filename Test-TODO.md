# Test TODO

## Goal

Smelt should compile source-language tests from supported TypeScript and Python projects into Rust `#[test]` functions that run under `cargo test`. Generated tests should use `smelt-test` for public test API behavior.

For v1, Smelt should not try to transpile Vitest or pytest internals. The v1 goal is public API compatibility: tests written against common source-language testing APIs should lower into native Rust tests that assert against transpiled Rust code.

## Target Repos

| Priority | Repo | Why |
|---|---|---|
| 1 | `date-fns/date-fns` | Large typed TS utility library with many simple Vitest tests. |
| 2 | `Textualize/rich` | Real Python library with many plain pytest asserts. |
| 3 | `remeda/remeda` | Large typed TS utility library with Vitest tests and less runtime-framework noise than Effect. |
| 4 | `encode/httpx` | Typed Python library with real pytest tests and useful stdlib/protocol coverage. |

Deferred TS stress target:

- `Effect-TS/effect`: keep as a later stress target for large runtime/schema/typeclass patterns
  after utility-library and test-framework parity is stronger.

## Current Baseline

- Workspace health from the latest required check:
  - `cargo test`: passed.
  - `cargo check`: passed.
  - `cargo clippy`: passed with warnings only.
- External repo checks can be used as signal again.

### External Probe: 2026-05-12 Alternative TS Targets

Fresh clone root: `/tmp/smelt-alt-ts-probe-20260512_165432/repos`

Checked the first three proposed Effect alternatives:

| Repo | Approx TS files | Approx runtime test files | Framework | Probe | `smelt check` | Current first blocker |
|---|---:|---:|---|---|---:|---|
| `remeda/remeda` | `585` total, `517` package runtime `.ts` files | `174` | Vitest | full `packages/remeda/src/**/*.ts`, excluding `.d.ts` | fail | Now reaches `packages/remeda/src/pipe.ts`: callback expression kind, destructured `for...of` binding, and field access on a non-record/class/interface type. |
| `remeda/remeda` | same | same | Vitest | focused `toUpperCase` slice | fail | Rerun focused slice after type-test `.test-d.ts` handling advances. |
| `sindresorhus/ky` | `52` | AVA tests in `test/*.ts` | AVA | full `source` + `test` manifest | fail | `source/core/Ky.ts`: object spread properties, optional class fields, `Symbol`, `Object`, richer callbacks, and callback binary operators. |
| `sindresorhus/ky` | same | same | AVA | focused `source/index.ts` slice | fail | `source/core/constants.ts`: exported non-primitive constants/calls, array callback arity, unresolved callback locals, and unannotated exported arrow constant. |
| `supermacro/neverthrow` | `8` | `2` | Vitest | full `src` + `tests` manifest | fail | Superseded by the cycle fix: now reaches `src/result.ts` and fails on forward class references to `Ok`/`Err`, parenthesized function types returning `Generator`, generic interface methods, generic classes, tuple `never`, tuple rest types, and conditional tuple element types. |
| `supermacro/neverthrow` | same | same | Vitest | focused `src/result.ts` slice | fail | Superseded by the cycle fix: same `src/result.ts` lowering surface as the full manifest. |

Recommendation:

- Promote `remeda/remeda` over `Effect-TS/effect` as the third primary target for now. Its
  first blocker is narrow and directly useful for typed utility libraries.
- Keep `neverthrow` as a small focused follow-up for generic class and advanced type-surface
  support; it is relevant but too small to replace a large target.
- Keep `ky` as a later async/HTTP/browser-runtime target. It adds AVA compatibility plus Fetch,
  server, stream, object spread, class-field, and browser API pressure all at once.

### External Probe: 2026-05-12 Effect Rerun

Re-ran Effect after the latest TypeScript/date work.

- Focused numeric slice manifest:
  `/tmp/smelt-reclone-rerun-20260511-145915/runs/effect/Smelt.toml`
- Focused numeric slice log root: `/tmp/effect_probe_20260512_164905`
- Full package manifest/log root: `/tmp/effect_full_probe_20260512_164926`
- Full package manifest size: `1758` `.ts`/`.tsx` files under `packages`.

Results:

| Slice | `smelt check` | Generated `cargo test` | Current first blocker |
|---|---:|---:|---|
| `Effect-TS/effect` numeric slice | fail | n/a | `packages/effect/src/Number.ts`: exported `dual(...)` constants and other non-Math helper calls still block lowering before the test file can run. |
| `Effect-TS/effect` full packages | fail | n/a | `packages/ai/ai/src/AiError.ts`: `typeof TypeId` type query, namespace member import resolution for `Predicate.hasProperty`, `Schema.Struct(...).annotations(...)` exported const calls, `Schema.TaggedError(...)` class inheritance, and block-bodied callbacks with more than a single return. |

Effect test-framework compatibility is not the current front wall:

- `@effect/vitest` imports are recognized.
- `describe.concurrent` is accepted.
- `U.deepStrictEqual(...)` is already represented as an Effect-style assertion helper target.

The remaining Effect blockers are mostly TypeScript library semantics and Effect runtime/schema
patterns, not public test API discovery.

### External Probe: 2026-05-11 Fresh Reclones

Re-cloned the eight external target repos and re-ran focused source/test slices.

- Fresh clone root: `/tmp/smelt-reclone-rerun-20260511-145915/repos`
- Manifest/log root: `/tmp/smelt-reclone-rerun-20260511-145915/runs`

Results:

| Slice | `smelt check` | `smelt build` | Generated `cargo test` | Current first blocker |
|---|---:|---:|---:|---|
| `date-fns/date-fns` `quartersToMonths` | pass | pass | pass, 4/4 tests | Green. Only warning is generated non-snake-case module stub. |
| `Effect-TS/effect` numeric slice | fail | fail | n/a | Superseded by the 2026-05-12 rerun: `packages/effect/src/Number.ts` still blocks on exported `dual(...)` constants, non-Math helper calls, `Iterable`, `reduce`, unannotated `for...of` bindings, unresolved helpers such as `multiply`, `sum`, `subtract`, and `Order`, and one exported arrow-function constant that still needs an explicit return type. |
| `Textualize/rich` `NullFile` | fail | fail | n/a | `_null_file.py`: member/method call rejected with “only calls to top-level functions, class constructors, and print() are supported”. |
| `encode/httpx` status codes | fail | fail | n/a | `_status_codes.py`: primitive conversion over unsupported value and class/member call rejected. |
| `pallets/click` `_utils` | fail | fail | n/a | `Sentinel` uses complex generic base-class expression; follow-on unresolved `Sentinel` and type variable `t`. |
| `TanStack/query` infinite options | fail | fail | n/a | TS function declaration missing explicit return type. |
| `remeda/remeda` `toUpperCase` | fail | fail | n/a | `purry.ts`: `TSFunctionType` annotation with rest args, `any`, and `LazyEvaluator` unsupported. |
| `psf/requests` hooks | fail | fail | n/a | dict comprehension, `*args`/`**kwargs`, and unresolved `TYPE_CHECKING`. |

Generated `date-fns` crate test command needed AppImage environment cleanup when run outside the
workspace:

```bash
env -u APPIMAGE -u APPDIR -u REDIRECT_APPIMAGE -u TARGET_APPIMAGE -u ELECTRON_RUN_AS_NODE -u LD_LIBRARY_PATH -u GSETTINGS_SCHEMA_DIR \
  PATH=/home/lollo/.cargo/bin:/usr/local/bin:/usr/bin:/bin cargo test
```

### External Probe: 2026-05-10

Re-ran the four narrow target slices from temporary clones under `/tmp/smelt-big-probe`.

`date-fns/date-fns`:

- Manifest slice:
  - `src/constants/index.ts`
  - `src/quartersToMonths/index.ts`
  - `src/quartersToMonths/test.ts`
- `vitest` globals are no longer the first failure.
- `quartersToMonths/test.ts` alone now fails only because `quartersToMonths` is unavailable without
  its library module.
- Full slice can now fold exported primitive constant expressions such as `Math.pow(...) * ...`
  and `-maxTime`.
- Next blocker is any remaining non-foldable exported constant expression shape in the target slice,
  not Vitest public API lowering.
- Superseded by the 2026-05-11 rerun: this target is now green, including generated Rust
  `cargo test` passing all four `quartersToMonths` cases.

`Textualize/rich`:

- Manifest slice:
  - `rich/_null_file.py`
  - `tests/test_null_file.py`
- Pytest unannotated test discovery is no longer the first failure.
- Library file now accepts generic base metadata such as `class NullFile(IO[str])`; `NULL_FILE =
  NullFile()` still needs module-level constructed constant support.
- Test file alone fails on local variable `file = NullFile()` being unresolved because imported module symbols are not available in that isolated run.
- Next blockers are Python class/protocol support and import binding, not plain pytest discovery.
- Phase 6 exit status: not green yet. First-green acceptance remains generated Rust `cargo test`
  passing the Rich-like `NullFile` fixture.

`Effect-TS/effect`:

- Manifest slice:
  - `packages/effect/src/Number.ts`
  - `packages/typeclass/src/data/Number.ts`
  - `packages/typeclass/test/data/Number.test.ts`
- `@effect/vitest` and `describe.concurrent` are no longer the first failures.
- Test file alone resolves namespace imports such as `NumberInstances`; remaining failures are in the exported
  constants/functions that those namespace members point at.
- Full slice now reaches `packages/effect/src/Number.ts` and can lower exported arrow-function
  constants plus object constants that group existing exports; remaining failures are exported
  non-primitive value expressions such as `dual(...)` helpers.
- Next blockers are remaining exported constant expression shapes outside the primitive folder and
  the Effect runtime subset.
- Phase 6 exit status: documented pass/fail required before close. Current first unsupported
  construct remains exported non-primitive value expressions such as `dual(...)`.

`encode/httpx`:

- Manifest slice:
  - `httpx/_status_codes.py`
  - `tests/test_status_codes.py`
- Pytest unannotated test discovery is no longer the first failure.
- Library file still fails on untyped `cls` in `codes.__new__` and unresolved class name `codes`.
- Test file alone can resolve direct package namespace members such as `httpx.add(...)`; deeper
  class-level member calls such as `httpx.codes.get_reason_phrase(...)` still need object model work.
- Next blockers are `IntEnum`/class body handling, classmethod/`cls`, and class-level member-call lowering.
- Phase 6 exit status: documented pass/fail required before close. Current first unsupported
  construct remains class body/classmethod support around `codes.__new__`, `cls`, and class-level calls.

## Priority 0: Existing Workspace Health

- [x] Fix `smelt-codegen-rust::tests::emits_typescript_tuple_index`.
- [x] Fix clippy failures in `crates/smelt-frontend-ts/src/lowering.rs`.
- [x] Confirm `cargo test` passes.
- [x] Confirm `cargo check` passes.
- [x] Confirm `cargo clippy` passes.

Acceptance:

```bash
cargo test
cargo check
cargo clippy
```

all pass before external test suites become CI signal.

## Priority 1: Test Discovery And Test API Imports

TypeScript:

- [x] Recognize `vitest` imports as test-framework builtins:
  - `describe`
  - `it`
  - `test`
  - `expect`
  - `beforeEach`
  - `afterEach`
- [x] Recognize `@effect/vitest` as Vitest-compatible for:
  - `describe`
  - `describe.concurrent`
  - `it`
  - `test`
- [x] Treat these symbols as test-framework builtins during test lowering, not normal unresolved runtime imports.

Python:

- [x] Recognize pytest-style test files:
  - `test_*.py`
  - `*_test.py` if needed later
- [x] In pytest mode, allow top-level `def test_*():` with no return annotation and treat it as `-> None`.
- [ ] Recognize pytest APIs and marks:
  - [x] `pytest.raises`
  - [x] `pytest.mark.parametrize`
  - [x] `pytest.fixture`
  - [x] `pytest.mark.skip`
  - [x] `pytest.mark.skipif`
  - [x] `pytest.mark.xfail`

## Priority 2: Minimal Native Rust Test Emission

Add `smelt test` later. First implement the lowering units needed by codegen.

TypeScript lowering:

- [x] Lower this shape to one Rust `#[test]`:

```ts
test("name", () => {
  expect(value).toBe(expected)
})
```

- [x] Lower this shape by flattening the group name into a stable Rust test function name:

```ts
describe("group", () => {
  it("name", () => {})
})
```

- [x] Ignore `describe` as runtime structure. It is a test organization construct.

Python lowering:

- [x] Lower this shape to one Rust `#[test]`:

```py
def test_name():
    assert expr
```

- [ ] Preserve source span metadata where available for failure messages.

## Priority 3: Assertion API Coverage

Extend `smelt-test`.

TypeScript helpers:

- [ ] `to_be_instance_of`
- [x] `to_contain`
- [x] `to_have_length`
- [x] `to_have_property`
- [ ] `to_throw_with_type`
- [x] `to_throw_with_message`
- [x] `deep_strict_equal` for Effect-style helper assertions
- [x] Keep `to_strict_equal` as structural equality for v1.

Python helpers:

- [x] `raises_type`
- [x] `raises_message`
- [x] Context-manager-shaped lowering for `with pytest.raises(...)`
- [x] Identity assertions for `is` / `is not`
- [x] Boolean negation assertion support for `assert not expr`

## Priority 4: Parametrization And Fixtures

Python first:

- [x] Implement `@pytest.mark.parametrize` expansion.
- [x] Generate one Rust `#[test]` per parameter row.
- [x] Use stable generated names with sanitized parameter indexes:
  - `test_name__case_0`
  - `test_name__case_1`
- [x] Support simple literal parameter tables first:
  - numbers
  - strings
  - booleans
  - `None`
  - tuples/lists of those

Fixtures:

- [x] Start with simple named fixtures that return a value.
- [x] Accept `autouse` fixture syntax without rejecting valid pytest code.
- [x] Accept scoped fixture syntax without rejecting valid pytest code.
- [x] Implement actual function-level autouse fixture injection semantics.
- [ ] Implement actual non-function fixture scope caching semantics.
- [ ] Support common built-in/project fixtures discovered in Click and Requests:
  - `tmp_path`
  - `tmpdir`
  - `monkeypatch`
  - `capsys`
  - `capfd`
  - `recwarn`
  - project fixtures such as Click's `runner`

Parametrization follow-up:

- [ ] Support tuple/list parameter-name declarations:

```py
@pytest.mark.parametrize(("value", "expected"), [(1, 2)])
```

- [ ] Support function values and lambdas in parameter rows.
- [ ] Support row-level marks in `pytest.param(...)`.
- [ ] Support non-literal `pytest.mark.skipif(...)` conditions.
- [ ] Allow untyped helper functions in pytest files when they are local test helpers, not emitted
      Rust tests.

TypeScript table tests later:

- [x] Lower direct literal `test.each` / `it.each` rows into one Rust test per row.
- [x] Lower direct literal `describe.each` rows by flattening direct nested `it` / `test` calls.
- [x] Inline direct `beforeEach` / `afterEach` arrow callbacks into each generated test.
- [x] Support nested `describe` blocks with inherited lifecycle hooks.
- [ ] Support dynamic table sources.
- [x] Support Vitest type-test imports such as `expectTypeOf` / `assertType` as no-op compile-time
      assertions once type-only test files are in scope.

## Priority 5: Import Graph And Real Project Slices

Required for the date-fns first green target:

- [x] Resolve extensionful imports:
  - `./index.ts`
  - `../constants/index.ts`
- [x] Resolve named exported constants.
- [x] Resolve re-exports from index modules.
- [x] Ensure manifest entries can provide symbols to later entries.
- [x] Add a regression fixture based on date-fns-style imports:

```ts
// constants.ts
export const monthsInQuarter = 3;

// quartersToMonths.ts
import { monthsInQuarter } from "./constants";

export function quartersToMonths(quarters: number): number {
  return Math.trunc(quarters * monthsInQuarter);
}
```

## Priority 6: Runtime/Stdlib Features Exposed By Target Repos

TypeScript:

- [x] `unknown` as a boundary type distinct from `any`
- [x] `readonly unknown[]`
- [x] Tagged `unknown` runtime representation for executable guards.
- [x] `typeof value === "string" | "number" | "boolean" | "object"` unknown guards.
- [x] `Array.isArray(value)` unknown guard.
- [x] `value === null` / `value !== null` unknown guards.
- [x] User assertion functions declared as `asserts value is T`.
- [x] Checked `unknown -> T` extraction for TypeScript `as T` / `<T>value`.
- [x] `Math.trunc`
- [x] `Math.pow`
- [x] String `.toUpperCase`
- [x] String `.toLowerCase`
- [x] `Date` should be rejected clearly unless implemented.
- [x] `instanceof` for concrete class values.
- [x] `Infinity`
- [ ] Array iteration and readonly array parameters
- [x] Exported object constants
  - [x] Exported primitive literal constants.
  - [x] Exported and local literal object constants, including `{ ... } as const`, lower as
        importable/reusable record literals.
- [x] Arrow functions assigned to `const`
- [x] Function overload declarations should be ignored/merged with implementation when safe.
- [ ] `Iterable<T>` type references.
- [ ] Type queries such as `typeof TypeId` in exported type aliases.
- [ ] `TSFunctionType` annotations, especially rest args and callback/lazy evaluator shapes.
- [x] `never` in rest parameter type positions, required by Remeda's
      `StrictFunction = (...args: never) => unknown`; implement according to
      `specs/never-type-plan.md`.
- [ ] Function calls with spread arguments inside closure/callback bodies, required by Remeda's
      `lazyDataLastImpl.ts` data-last helper: `(data: unknown): unknown => fn(data, ...args)`.
- [x] Local arrow function constants declared after a function body that reads them, required by
      Remeda's `add.ts`: `add(...)` calls `purry(addImplementation, args)` before the
      `const addImplementation = (...) => ...` declaration appears.
- [x] Overload-aware call lowering, required by Remeda's `add.test.ts`: `add(10, 5)` should
      select the data-first overload instead of the implementation rest signature.
- [x] Call-expression callees for curried/data-last APIs, required by Remeda's `add.test.ts`:
      `add(5)(10)`.
- [x] Generic `Record<K, V>` where `K` is constrained to `PropertyKey`, required by Remeda's
      `internal/types/UpsertProp.ts` type-level helper.
- [x] Object spread properties in object literals, required by Remeda's `addProp.ts`.
- [x] Type-test `.test-d.ts` handling for Remeda's `addProp.test-d.ts`, including
      `{} as { ... }` call arguments and `expectTypeOf` no-op type assertions.
- [ ] `pipe.ts` runtime lowering gaps in Remeda: callback expression kind, destructured
      `for...of` binding, and field access on a non-record/class/interface type.
- [ ] Inference or safe acceptance for unannotated exported/test-adjacent function declarations
      when upstream TypeScript would infer the return type.
- [ ] Exported non-primitive const call expressions used by Effect, especially `dual(...)`-style
      helpers and typeclass instance constructors.
- [ ] Object spread properties in object literals, required by Ky.
- [ ] Optional class fields, required by Ky.
- [ ] `Symbol` and `Object` global constants/helpers, required by Ky.
- [ ] Callback binary operators and multi-statement callbacks in array/object helper calls, required
      by Ky.
- [x] Manifest import cycles should not be rejected by the dependency sorter. Unsupported cycle
      semantics should surface as normal lowering/codegen diagnostics.
- [ ] Forward class references inside one module, required by Neverthrow's `ok(...)` and `err(...)`
      functions before `Ok` / `Err` class declarations.
- [ ] Generic classes, required by Neverthrow's `Ok<T, E>` and `Err<T, E>`.
- [ ] Tuple rest types and conditional tuple element types, required by Neverthrow's type helpers.
- [ ] Fluent exported helper chains such as `Schema.Struct(...).annotations(...)`.
- [ ] Class inheritance and mixin/factory extends expressions such as
      `class X extends Schema.TaggedError<X>(...)(...)`.
- [ ] Block-bodied callbacks with normal statement flow, not only a single `return` statement.
- [ ] Namespace import member resolution against external package stubs, for example
      `Predicate.hasProperty`.

Python:

- [x] pytest-mode untyped `test_*`
- [x] `is` / `is not`
- [x] Ternary expressions
- [x] `try` / `except`
- [ ] Context manager protocol
- [x] `IntEnum`
- [x] `classmethod`
- [x] `__all__`
- [ ] Dunder methods used by tests:
  - `__str__`
  - `__iter__`
  - `__next__`
  - `__enter__`
  - `__exit__`
- [ ] General member/method calls beyond top-level functions, class constructors, and `print()`.
- [ ] Complex generic base class expressions such as Click's `Sentinel(...)`.
- [ ] Type variable names and aliases used at runtime-adjacent positions, such as Click's `t`.
- [ ] `TYPE_CHECKING` as a recognized type-only constant.
- [ ] Dict/list/set comprehensions.
- [ ] `*args` and `**kwargs` in function signatures and call forwarding.
- [ ] Primitive conversions over enum/class values exposed by HTTPX status codes.

## Repo-Specific First Green Targets

### First TS Target: date-fns

Target files:

- `src/constants/index.ts`
- `src/quartersToMonths/index.ts`
- `src/quartersToMonths/test.ts`

First success means:

- [x] Smelt emits Rust tests for `quartersToMonths/test.ts`.
- [x] Generated crate runs via `cargo test`.
- [x] The four Vitest `it(...)` cases pass.

Status: green as of the 2026-05-11 external rerun.

#### date-fns Compatibility Probe: 2026-05-11

Probe roots:

- Fresh checkout: `/tmp/smelt-reclone-rerun-20260511-145915/repos/date-fns`
- File-level logs: `/tmp/date_fns_compat_20260511_150833`
- Sibling-index slice logs: `/tmp/date_fns_slice_compat_20260511_150933`
- Latest rerun logs after additional stdlib/frontend work: `/tmp/date_fns_slice_compat_20260511_180557`
- Latest rerun logs after Date/runtime work: `/tmp/date_fns_slice_compat_20260511_184420`
- Latest `src/types.ts` direct probe: `/tmp/date_fns_types_probe_20260512_094830`
- Latest sibling slices with `src/types.ts`: `/tmp/date_fns_with_types_compat_20260512_094924`
- Latest full `src/**/*.ts(x)` manifest probe after import refactor:
  `/tmp/date_fns_full_compat_20260512_152219`
- Latest full manifest probe after chain-argument wiring:
  `/tmp/date_fns_full_compat_20260512_160135`
- Latest full manifest probe with `src/types.ts` forced first:
  `/tmp/date_fns_full_types_first_20260512_160330`
- Latest full manifest probe after chain/import/Date receiver work:
  `/tmp/date_fns_full_compat_20260512_163625`
- Latest full manifest probe with shared type files forced first:
  `/tmp/date_fns_full_types_first_20260512_163841`
- Latest full manifest probe after locale type, optional chain, Node probe, and typed-arrow work:
  `/tmp/smelt_date_fns_full_check_BLgUAn`

Compatibility numbers:

| Measurement | Result | Notes |
|---|---:|---|
| TS/TSX files under `src` | `1536` | Raw source corpus size in the latest checkout. |
| Vitest-style `test.ts` files | `250` | Direct date-fns test files under `src`. |
| Full `src/**/*.ts(x)` manifest `smelt check` | fail | Gets past `src/locale/types.ts`, Node probe files, `.d.ts` ambient declarations, and `en-US/_lib/formatDistance`; current wall is returned inline arrow functions in `src/locale/_lib/buildFormatLongFn/index.ts`. |
| Full manifest with shared type files forced first `smelt check` | fail | Superseded by the sorted full-manifest rerun above; the locale type-surface blockers are now cleared. |
| Isolated non-test file lowering | `7 / 1237` | Pessimistic lower bound because imports are missing in single-file mode. |
| Isolated test file lowering | `0 / 254` | Pessimistic lower bound because tested functions are unavailable. |
| Sibling `index.ts` + `constants` + `test.ts` slices passing `smelt check` | `21 / 250` | Comparable sibling-index `test.ts` sweep. `isExists` newly reaches Rust emission. |
| Same slices passing `smelt build` | `21 / 250` | Every check-green slice emitted Rust. |
| Same slices passing generated `cargo test` | `20 / 250` | `isExists` builds but one invalid-date test panics at runtime. |
| `src/types.ts` direct `smelt check` / `smelt build` | pass / pass | Shared date-fns type file now lowers by itself. |
| Sibling slices with `src/types.ts` included passing `smelt check` | `23 / 250` | Shared type wall moved into dependency closure/runtime gaps. |
| Same `src/types.ts` slices passing `smelt build` | `23 / 250` | Every check-green slice emitted Rust. |
| Same `src/types.ts` slices passing generated `cargo test` | `23 / 250` | All build-green `with_types` slices passed generated Rust tests. |
| Approx direct Vitest cases covered | `78 / 2882` | Heuristic text count of direct `it(...)` / `test(...)` calls. |

Latest rerun status: the full sorted manifest now gets past the previous `isSaturday` chain,
`ContextOptions.in`, Date receiver blockers, `src/locale/types.ts`, the `addBusinessDays/basic.ts`
Node environment probe, ambient `.d.ts` declaration files, and the first locale
`formatDistance` formatter. The first failing file is now
`src/locale/_lib/buildFormatLongFn/index.ts`, where `buildFormatLongFn` returns an inline arrow
function expression typed by the declared `FormatLongFn` return type.

`src/types.ts`, `src/locale/types.ts`, `src/fp/types.ts`, `src/addBusinessDays/index.ts`,
`src/_lib/addBusinessDays/basic.ts`, and `src/locale/en-US/_lib/formatDistance/index.ts` pass
direct `smelt check` probes. With `src/types.ts` included in each sibling slice, `23` date-fns test
slices pass all the way through generated Rust `cargo test`.

Generic interface implementation status:

- [x] HIR has explicit generic type parameter types and metadata.
- [x] TypeScript interfaces can declare generic parameters with constraints and defaults.
- [x] Generic interface references lower with actual/default type arguments.
- [x] Generic interface inheritance substitutes parent fields and method signatures.
- [x] Generic function declarations create type-parameter scopes for annotations.
- [x] Generic type aliases lower and substitute through generic references.
- [x] Function type aliases and empty-object intersections such as `DateArg<Date> & {}` lower.

Shared type-file status:

- [x] `src/types.ts` lowers in direct `smelt check`.
- [x] `src/types.ts` emits Rust in direct `smelt build`.
- [x] Builtin `Date` heritage, interface construct/index signatures, `keyof`, and the shared
  `DateArg` / `ContextFn` aliases no longer globally block the file.
- [x] Full manifest import traversal reaches date helper source files instead of stopping on
  `src/types.ts`.
- [x] Full date-fns gets past `ContextOptions` / `options?.in` / `toDate(...).getDay()`.
- [x] Full date-fns gets past locale type-surface lowering/rejection strategy for
  `src/locale/types.ts`.
- [ ] Full date-fns needs closure-as-value lowering for returned inline arrow functions such as
  `buildFormatLongFn(...): FormatLongFn { return (options = {}) => { ... }; }`.

Full manifest first blocker:

| File | Unsupported feature | Current error shape |
|---|---|---|
| `src/locale/_lib/buildFormatLongFn/index.ts` | Returned inline arrow function expression | `expression kind is not lowered yet: ArrowFunctionExpression(...)`. |

Optional chaining surface in date-fns:

| Pattern | Count |
|---|---:|
| Files containing `?.` | `319` |
| Total `?.` matches | `462` |
| `options?.in` matches | `249` |

Top normalized latest failure messages for sibling `index.ts` + `constants` + `test.ts` slices:

| Count | Missing support | Representative location |
|---:|---|---|
| `196` | `DateArg` type reference lowering without `src/types.ts` in the manifest | `src/toDate/index.ts`, `src/addDays/index.ts`, and many Date helpers. |
| `152` | Shared option interfaces unavailable without `src/types.ts` | `ContextOptions` users across add/sub/start/end helpers. |
| `18` | Shared localized option interfaces unavailable without `src/types.ts` | `LocalizedOptions` users in week/format helpers. |
| `7` | Shared step option interfaces unavailable without `src/types.ts` | interval helpers. |
| `5` | Method calls only lowered for class values | Date-like receiver paths. |
| `5` | Inferred return type acceptance for source functions | `src/_lib/getRoundingMethod/index.ts`, parse/rounding helpers. |
| `5` | Shared rounding option interfaces unavailable without `src/types.ts` | rounding/difference helpers. |
| `4` | Destructuring declarations | `src/areIntervalsOverlapping/index.ts` and interval helpers. |
| `3` | Shared week option interfaces unavailable without `src/types.ts` | week helpers. |
| `2` | Object literal typing without explicit `Record<string, T>` | `src/setDefaultOptions/index.ts`, `_lib/defaultOptions`. |
| `1` | `typeof value === "function"` narrowing | `src/transpose/index.ts`. |
| `1` | `GenericDateConstructor` type reference without full shared types | `src/transpose/index.ts`. |

Top blockers when `src/types.ts` is included in every sibling slice:

| Count | Missing support | Representative location |
|---:|---|---|
| `75` | Missing dependency closure for imported helpers | `toDate` users across date helpers. |
| `38` | Destructuring declarations | `src/add/index.ts`, interval and difference helpers. |
| `14` | Unary plus lowering | `src/compareAsc/index.ts`, `src/closestIndexTo/index.ts`. |
| `11` | Missing dependency closure for default option helpers | `getDefaultOptions` users. |
| `10` | `LocalizedOptions` still unavailable in some slices | week/format helpers. |
| `6` | Method calls only lowered for class values | Date-like receiver paths. |
| `5` | Inferred return type acceptance for source functions | `src/_lib/getRoundingMethod/index.ts`, parse/rounding helpers. |
| `4` | Missing dependency closure for `constructNow` | relative/current-date helpers. |
| `2` | `typeof value === "function"` narrowing | `src/constructFrom/index.ts`, `src/transpose/index.ts`. |
| `2` | Qualified type references | Locale/namespace-style type references. |
| `2` | Object literal typing without explicit `Record<string, T>` | default options helpers. |

Current date-fns test slices that pass `smelt check`, `smelt build`, and generated `cargo test`:

- `src/_lib/addLeadingZeros/test.ts` with `src/types.ts`
- `src/daysToWeeks/test.ts` with `src/types.ts`
- `src/hoursToMilliseconds/test.ts`
- `src/hoursToMinutes/test.ts`
- `src/hoursToSeconds/test.ts`
- `src/isExists/test.ts` with `src/types.ts`
- `src/millisecondsToHours/test.ts`
- `src/millisecondsToMinutes/test.ts`
- `src/millisecondsToSeconds/test.ts`
- `src/minutesToHours/test.ts`
- `src/minutesToMilliseconds/test.ts`
- `src/minutesToSeconds/test.ts`
- `src/monthsToQuarters/test.ts`
- `src/monthsToYears/test.ts`
- `src/quartersToMonths/test.ts`
- `src/quartersToYears/test.ts`
- `src/secondsToHours/test.ts`
- `src/secondsToMilliseconds/test.ts`
- `src/secondsToMinutes/test.ts`
- `src/weeksToDays/test.ts`
- `src/yearsToDays/test.ts`
- `src/yearsToMonths/test.ts`
- `src/yearsToQuarters/test.ts`

Current date-fns test slices that pass `smelt check` and `smelt build` but fail generated
`cargo test`:

- `src/isExists/test.ts`: invalid-date case panics with `timestamp out of range`; Date lowering
  needs an invalid-date representation instead of panicking during construction.

Current date-fns TypeScript blockers with representative break locations:

| Missing support | Representative break location | Current error shape |
|---|---|---|
| TS conditional expression lowering | `src/_lib/addLeadingZeros/index.ts` via `src/_lib/addLeadingZeros/test.ts` | `ConditionalExpression` not lowered. |
| Inferred return type acceptance for source functions | `src/_lib/getRoundingMethod/index.ts` via `src/_lib/getRoundingMethod/test.ts` | `function declarations must have an explicit return type`. |
| Dependency closure manifests for real date-fns slices | `src/addDays/index.ts`, `src/_lib/getTimezoneOffsetInMilliseconds/index.ts` | unresolved helper imports such as `toDate`, `addMilliseconds`, `constructFrom`, `getDefaultOptions`. |
| Imported generic type aliases such as `DateArg` without `src/types.ts` | `src/addDays/index.ts`, `src/addHours/index.ts`, `src/addMilliseconds/index.ts`, and many date helpers | `type reference is not lowered yet: DateArg`. |
| Invalid Date value semantics | `src/isExists/test.ts` | Generated Rust panics with `timestamp out of range` instead of preserving an invalid Date value. |
| Unary plus | `src/compareAsc/index.ts`, `src/closestIndexTo/index.ts` | `unary operator is not lowered yet: UnaryPlus`. |
| Conditional type annotations | `src/clamp/index.ts`, `src/closestTo/index.ts` | `TSConditionalType` not lowered. |
| RegExp literals as values | `src/_lib/protectedTokens/index.ts`, `src/parse/index.ts` | `RegExpLiteral` expression not lowered. |
| Result type aliases | `src/clamp/index.ts`, `src/closestTo/index.ts` | `type reference is not lowered yet: ClampResult` / `ClosestToResult`. |
| Destructuring declarations | `src/areIntervalsOverlapping/index.ts` | `destructuring declarations are not lowered yet`. |
| Object literals without explicit `Record<string, T>` annotation | `src/_lib/defaultOptions/index.ts`, `src/_lib/tzOffsetTransitions.ts` | `object literals currently require a Record<string, T> annotation`. |
| `Date` constructor overloads beyond numeric timestamp | `src/_lib/getTimezoneOffsetInMilliseconds/test.ts`, `src/clamp/test.ts`, `src/closestIndexTo/test.ts`, `src/closestTo/test.ts` | `new Date() currently supports exactly one numeric timestamp argument`. |
| `new Date(timestamp)` with non-numeric/static-unknown argument | `src/_lib/tzOffsetTransitions.ts` | `new Date(timestamp) requires a numeric timestamp`. |
| `instanceof` where the left operand is not known as a concrete class value | `src/_lib/tzOffsetTransitions.ts` | `TypeScript instanceof requires a concrete class-typed left operand`. |
| Method receiver type recovery for Date-like values | `src/_lib/tzOffsetTransitions.ts` | `method receiver class is unknown`. |
| Type aliases with object/union members | `src/_lib/tzOffsetTransitions.ts` | `TSTypeAliasDeclaration` statement not lowered. |
| Optional chaining / chain expressions | `src/_lib/addBusinessDays/basic.ts`, `src/_lib/eachDayOfInterval/basic.ts`, `src/_lib/parseISO/samoa.ts`, `src/_lib/parseISO/sydney.ts` | `ChainExpression` not lowered. |
| `process.version` / Node environment globals | `src/_lib/addBusinessDays/basic.ts`, `src/_lib/parseISO/samoa.ts`, `src/_lib/parseISO/sydney.ts` | unresolved identifier `process`. |
| Node/assert-style helper identifiers in date-fns internal timezone probes | `src/_lib/addBusinessDays/basic.ts`, `src/_lib/eachDayOfInterval/basic.ts`, `src/_lib/parseISO/samoa.ts`, `src/_lib/parseISO/sydney.ts` | unresolved identifier `assert`. |
| Nested/complex `describe` contents beyond direct `it` / `test` calls | `src/areIntervalsOverlapping/test.ts` in isolated test-file mode | `describe blocks only support direct it/test calls for now`. |

Most failing date-fns feature slices hit library-source type/runtime gaps before Vitest API gaps.
The next highest-leverage TS work for this repo is real import dependency closure for date-fns
function slices, then destructuring declarations and unary plus. `src/types.ts` itself is no longer
the wall.

### First Python Target: Rich

Target files:

- `rich/_null_file.py`
- `tests/test_null_file.py`

First success means:

- Smelt treats `test_null_file()` as `-> None`.
- Smelt lowers plain asserts.
- Smelt supports enough class/context-manager/iterator behavior for `NullFile`.
- Generated crate runs via `cargo test`.

Current first blocker from the 2026-05-11 rerun:

- `_null_file.py` still hits the Python member/method call limit: only top-level functions, class
  constructors, and `print()` are accepted.

### Second TS Target: Effect

Target files:

- `packages/typeclass/src/data/Number.ts`
- `packages/typeclass/test/data/Number.test.ts`

First success means:

- `@effect/vitest` imports are recognized.
- `describe.concurrent` is accepted.
- `U.deepStrictEqual` lowers to `smelt_test::ts::deep_strict_equal` or equivalent.
- A simple numeric semigroup test passes.

Current first blockers from the 2026-05-11 rerun:

- `packages/effect/src/Number.ts` still requires exported non-primitive const expressions and
  calls, especially Effect/typeclass helpers.
- `Iterable<T>` type references are unsupported.
- Some helper values/functions remain unresolved because the exported const/call layer is skipped.

Current first blockers from the 2026-05-12 rerun:

- Focused numeric slice still fails in `packages/effect/src/Number.ts` before generated Rust tests
  can be emitted.
- Full `packages/**/*.ts(x)` manifest fails earlier in `packages/ai/ai/src/AiError.ts`.
- Full-repo first wall includes `typeof TypeId` type queries, external namespace member resolution
  for `Predicate.hasProperty`, fluent Schema helper chains, `Schema.TaggedError(...)` class
  inheritance, and block-bodied callbacks with ordinary statement flow.
- Effect test APIs are not the current blocker: `@effect/vitest`, `describe.concurrent`, and
  `U.deepStrictEqual(...)` are already past the first failure point.

### Second Python Target: HTTPX

Target files:

- `httpx/_status_codes.py`
- `tests/test_status_codes.py`

First success means:

- pytest test functions lower despite missing return annotations.
- `IntEnum` or a targeted enum mapping is supported.
- Class methods such as `codes.get_reason_phrase(...)` lower.
- Generated tests for simple status code behavior pass.

Current first blockers from the 2026-05-11 rerun:

- Primitive conversions over enum/class values in `_status_codes.py`.
- Class/member calls in status code helpers still hit the general Python call limit.

### Additional External Slices

These are not primary acceptance targets, but they are useful compatibility probes from the local
eight-repo rerun.

`pallets/click`:

- Slice:
  - `src/click/_utils.py`
  - `tests/test_utils.py`
- First blockers:
  - complex generic base class expression for `Sentinel`
  - unresolved `Sentinel`
  - unresolved type variable `t`

`TanStack/query`:

- Slice:
  - `packages/angular-query-experimental/src/infinite-query-options.ts`
  - `packages/angular-query-experimental/src/__tests__/infinite-query-options.test.ts`
- First blocker:
  - function declaration missing explicit return type

`remeda/remeda`:

- Slice:
  - `packages/remeda/src/purry.ts`
  - `packages/remeda/src/pipe.ts`
  - `packages/remeda/src/toUpperCase.ts`
  - `packages/remeda/src/toUpperCase.test.ts`
- First blocker:
  - Superseded by 2026-05-13 overload rerun:
    - focused log root: `/tmp/smelt-alt-ts-probe-20260512_165432/runs/remeda_to_upper_after_never_step1_180952`
    - full log root: `/tmp/smelt-alt-ts-probe-20260512_165432/runs/remeda_full_after_never_step1_180952`
    - previous blocker fixed: `packages/remeda/src/internal/utilityEvaluators.ts` now lowers
      literal `{ ... } as const` object constants, local `EMPTY_PIPE` reads, and the unannotated
      generic `lazyIdentityEvaluator` arrow const.
    - previous blocker fixed: `packages/remeda/src/add.ts` can read `addImplementation` before the
      local arrow const declaration appears.
    - previous blocker fixed: `packages/remeda/src/add.test.ts` now lowers overload-aware calls for
      `add(10, 5)` and call-expression callees for `add(5)(10)`.
    - previous blocker fixed: `packages/remeda/src/internal/types/UpsertProp.ts` now preserves
      generic `Record<K, V>` keys for later type-parameter substitution after first trying the
      concrete key lowering path.
    - previous blocker fixed: `packages/remeda/src/addProp.ts` now lowers object spread literals,
      computed property keys, and `addPropImplementation`.
    - previous blocker fixed: `packages/remeda/src/addProp.test-d.ts` now lowers type assertion
      call arguments such as `{} as { ... }` and erases Vitest `expectTypeOf` type assertions.
    - new blocker: `packages/remeda/src/pipe.ts` needs callback expression support,
      destructured `for...of` bindings, and field access on a non-record/class/interface type.

`psf/requests`:

- Slice:
  - `src/requests/hooks.py`
  - `tests/test_hooks.py`
- First blockers:
  - dict comprehension
  - `*args` / `**kwargs`
  - unresolved `TYPE_CHECKING`

## Acceptance Criteria

- [x] Root `Test-TODO.md` is the main implementation checklist for `smelt test`.
- [x] `specs/test-framework-repo-survey.md` uses date-fns, Effect, Rich, and HTTPX as primary survey targets.
- [x] Zod and Pydantic are documented only as deferred stress targets.
- [x] The first external TypeScript target is date-fns `quartersToMonths`.
- [x] The first external Python target is Rich `NullFile`.
- [ ] Every unsupported source test feature listed above either lowers correctly or produces a clear source-located Smelt error.
