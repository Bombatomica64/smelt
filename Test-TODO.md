# Test TODO

## Goal

Smelt should compile source-language tests from supported TypeScript and Python projects into Rust `#[test]` functions that run under `cargo test`. Generated tests should use `smelt-test` for public test API behavior.

For v1, Smelt should not try to transpile Vitest or pytest internals. The v1 goal is public API compatibility: tests written against common source-language testing APIs should lower into native Rust tests that assert against transpiled Rust code.

## Target Repos

| Priority | Repo | Why |
|---|---|---|
| 1 | `date-fns/date-fns` | Large typed TS utility library with many simple Vitest tests. |
| 2 | `Textualize/rich` | Real Python library with many plain pytest asserts. |
| 3 | `Effect-TS/effect` | Large typed TS library, good stress target after simple Vitest lowering works. |
| 4 | `encode/httpx` | Typed Python library with real pytest tests and useful stdlib/protocol coverage. |

## Current Baseline

- `cargo check`: passed in the latest implementation run.
- `cargo test`: passed in the latest implementation run.
- `cargo clippy`: passed in the latest implementation run.

Do not rely on external repo tests as a signal until the workspace's own checks are green.

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

`Textualize/rich`:

- Manifest slice:
  - `rich/_null_file.py`
  - `tests/test_null_file.py`
- Pytest unannotated test discovery is no longer the first failure.
- Library file now accepts generic base metadata such as `class NullFile(IO[str])`; `NULL_FILE =
  NullFile()` still needs module-level constructed constant support.
- Test file alone fails on local variable `file = NullFile()` being unresolved because imported module symbols are not available in that isolated run.
- Next blockers are Python class/protocol support and import binding, not plain pytest discovery.

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

`encode/httpx`:

- Manifest slice:
  - `httpx/_status_codes.py`
  - `tests/test_status_codes.py`
- Pytest unannotated test discovery is no longer the first failure.
- Library file still fails on untyped `cls` in `codes.__new__` and unresolved class name `codes`.
- Test file alone can resolve direct package namespace members such as `httpx.add(...)`; deeper
  class-level member calls such as `httpx.codes.get_reason_phrase(...)` still need object model work.
- Next blockers are `IntEnum`/class body handling, classmethod/`cls`, and class-level member-call lowering.

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
- [ ] Support Vitest type-test imports such as `expectTypeOf` / `assertType` as no-op compile-time
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

- [ ] `unknown` as a boundary type distinct from `any`
- [ ] `readonly unknown[]`
- [x] `Math.trunc`
- [x] `Math.pow`
- [x] String `.toUpperCase`
- [x] String `.toLowerCase`
- [x] `Date` should be rejected clearly unless implemented.
- [x] `instanceof` for concrete class values.
- [x] `Infinity`
- [ ] Array iteration and readonly array parameters
- [ ] Exported object constants
  - [x] Exported primitive literal constants.
- [ ] Arrow functions assigned to `const`
- [x] Function overload declarations should be ignored/merged with implementation when safe.

Python:

- [x] pytest-mode untyped `test_*`
- [x] `is` / `is not`
- [ ] Ternary expressions
- [ ] `try` / `except`
- [ ] Context manager protocol
- [ ] `IntEnum`
- [ ] `classmethod`
- [x] `__all__`
- [ ] Dunder methods used by tests:
  - `__str__`
  - `__iter__`
  - `__next__`
  - `__enter__`
  - `__exit__`

## Repo-Specific First Green Targets

### First TS Target: date-fns

Target files:

- `src/constants/index.ts`
- `src/quartersToMonths/index.ts`
- `src/quartersToMonths/test.ts`

First success means:

- Smelt emits Rust tests for `quartersToMonths/test.ts`.
- Generated crate runs via `cargo test`.
- The four Vitest `it(...)` cases pass.

### First Python Target: Rich

Target files:

- `rich/_null_file.py`
- `tests/test_null_file.py`

First success means:

- Smelt treats `test_null_file()` as `-> None`.
- Smelt lowers plain asserts.
- Smelt supports enough class/context-manager/iterator behavior for `NullFile`.
- Generated crate runs via `cargo test`.

### Second TS Target: Effect

Target files:

- `packages/typeclass/src/data/Number.ts`
- `packages/typeclass/test/data/Number.test.ts`

First success means:

- `@effect/vitest` imports are recognized.
- `describe.concurrent` is accepted.
- `U.deepStrictEqual` lowers to `smelt_test::ts::deep_strict_equal` or equivalent.
- A simple numeric semigroup test passes.

### Second Python Target: HTTPX

Target files:

- `httpx/_status_codes.py`
- `tests/test_status_codes.py`

First success means:

- pytest test functions lower despite missing return annotations.
- `IntEnum` or a targeted enum mapping is supported.
- Class methods such as `codes.get_reason_phrase(...)` lower.
- Generated tests for simple status code behavior pass.

## Acceptance Criteria

- [ ] Root `Test-TODO.md` is the main implementation checklist for `smelt test`.
- [ ] `specs/test-framework-repo-survey.md` uses date-fns, Effect, Rich, and HTTPX as primary survey targets.
- [ ] Zod and Pydantic are documented only as deferred stress targets.
- [ ] The first external TypeScript target is date-fns `quartersToMonths`.
- [ ] The first external Python target is Rich `NullFile`.
- [ ] Every unsupported source test feature listed above either lowers correctly or produces a clear source-located Smelt error.
