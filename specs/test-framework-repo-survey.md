# Test Framework Repository Survey

Survey date: 2026-05-10.

This survey checks whether smelt can consume tests from real TypeScript and Python projects today, and records the missing pieces needed for `smelt test`.

The primary targets are typed libraries with real test suites. Zod and Pydantic are intentionally deferred because they mostly stress runtime schema systems, metaprogramming, and advanced type machinery rather than the first version of Smelt's typed-source test path.

## Repositories

Temporary clones used during the survey:

- `/tmp/smelt-repo-survey` for date-fns and Rich.
- `/tmp/smelt-repo-survey-next2` for Effect and HTTPX.

| Repo | Language | Test framework | Source files scanned | Test files scanned |
|---|---:|---|---:|---:|
| `date-fns/date-fns` | TypeScript | Vitest | 1538 `.ts`/`.tsx` | 250 TS test files |
| `Effect-TS/effect` | TypeScript | `@effect/vitest` / Vitest-compatible | 1765 `.ts`/`.tsx` | 702 TS test files |
| `Textualize/rich` | Python | pytest | 213 `.py` | 63 `test_*.py` files |
| `encode/httpx` | Python | pytest | 60 `.py` | 31 `test_*.py` files |

## TypeScript Findings

Both TS projects are useful targets for v1 test API lowering, but they serve different phases:

- date-fns is the first target because many tests are simple Vitest tests over deterministic utility functions.
- Effect is a later stress target because it is large, typeclass-heavy, and uses `@effect/vitest`.

Common matcher/helper counts from the cloned repos:

| Matcher/helper | date-fns | Effect |
|---|---:|---:|
| `.toBe` | 2057 | 3887 |
| `.toEqual` | 1065 | 596 |
| `.toThrow` | 99 | not prominent in top scan |
| `.toBeInstanceOf` | 349 | not prominent in top scan |
| `.toContain` | 0 | 109 |
| `.toHaveLength` | 0 | 36 |
| Effect-style helper assertions | 0 | required, e.g. `U.deepStrictEqual(...)` |

Immediate test-framework blockers:

- Imported Vitest globals are unresolved. Example: `describe`, `it`, `test`, and `expect` from `vitest` currently fail before any test body lowering.
- `@effect/vitest` must be treated as a Vitest-compatible source of test framework globals.
- `describe.concurrent` appears in Effect tests and must lower as a test grouping construct.
- Effect uses helper assertions such as `U.deepStrictEqual(...)`; test lowering must support assertion helper calls, not only `expect(...)`.
- `expect(...).toBeInstanceOf(...)` is common in date-fns and is not in `smelt-test` yet.
- `expect(...).toThrow(ErrorType)` and message matching are needed beyond the current `to_throw()` panic check.

Immediate source-language blockers:

- Import/export resolution is too weak for real module slices. A manifest containing date-fns `src/constants/index.ts`, `src/quartersToMonths/index.ts`, and `src/quartersToMonths/test.ts` still fails with unresolved imported constant `monthsInQuarter`.
- Real TS libraries use `Math.trunc`, `Math.pow`, `Date`, `isNaN`, `instanceof`, `Infinity`, string case methods, array iteration, and readonly array parameters.
- Effect requires exported object constants and arrow callbacks inside library code.
- Effect contains substantial typeclass/type-level code; initial runtime conformance should use an allowlisted subset instead of trying to compile the whole repo.

Representative failures:

- `date-fns/src/quartersToMonths/test.ts`: unresolved function `describe`.
- `date-fns/src/quartersToMonths/index.ts`: unresolved identifier `monthsInQuarter`, even with the constants module listed in the manifest.
- `Effect/packages/typeclass/src/data/Number.ts`: `dump-hir` exits successfully but produces effectively empty HIR because current lowering misses or export-skips most declarations in that file.
- `Effect/packages/effect/src/Number.ts`: same as above.
- `Effect/packages/typeclass/test/data/Number.test.ts`: unresolved identifier `describe`.

## Python Findings

Both Python projects are pytest-heavy:

- Rich is the first target because many tests are plain assertions around deterministic rendering/data behavior.
- HTTPX is a later target because it adds useful stdlib/protocol coverage while staying more aligned with typed application/library code than Pydantic.

Common pytest feature counts:

| Feature | Rich | HTTPX |
|---|---:|---:|
| `pytest.mark.parametrize` | 35 | 44 |
| `pytest.fixture` | 5 | 7 |
| `pytest.raises` | 51 | 128 |
| `skip` / `skipif` / `xfail` | 39 | 1 |

Immediate test-framework blockers:

- Pytest test functions commonly omit `-> None`. smelt currently rejects them as untyped functions.
- `pytest.raises` needs context-manager lowering with optional exception type and message matching.
- `pytest.mark.parametrize` needs test-case expansion into multiple Rust `#[test]` functions.
- Fixtures are required, including named argument injection.
- Autouse fixtures and scoped fixtures should be rejected clearly at first unless explicitly supported.
- `skip`, `skipif`, and `xfail` need explicit Rust-side handling or early rejection.

Immediate source-language blockers:

- Rich uses standard-library protocols and classes such as `IO[str]`, context managers, iterators, `Optional`, `Iterable`, `Iterator`, and `Type`.
- Rich uses `TYPE_CHECKING`, forward/type-only imports, dunder names such as `__name__`, and many rich object equality comparisons.
- HTTPX uses `IntEnum`, `__all__`, `classmethod`/`cls` patterns, `is` comparisons, ternary expressions, `try` statements, and method/member calls.
- HTTPX status-code tests are a good second target because they are small and behavior-oriented once enum/classmethod support exists.

Representative failures:

- `rich/tests/test_null_file.py`: `function 'test_null_file' must have an explicit return type annotation`.
- `rich/rich/_null_file.py`: `class 'NullFile': complex base class expression not supported` for `class NullFile(IO[str])`.
- `rich/rich/align.py`: unsupported type annotation forms, unresolved `TYPE_CHECKING`, unresolved type-only aliases, unresolved `__name__`.
- `httpx/httpx/_status_codes.py`: untyped `cls` parameter in `codes.__new__`, unresolved `__all__`, unresolved `codes`.
- `httpx/tests/test_status_codes.py`: every top-level test function is rejected for missing explicit return annotation.
- `httpx/httpx/_utils.py`: unsupported `is`, ternary expressions, `try`, limited method/member calls, unresolved `typing`.

## Deferred Stress Targets

- `colinhacks/zod`: defer until Smelt is ready to stress advanced TypeScript type-level programming, runtime schema APIs, snapshots, `expectTypeOf`, `any`/`unknown`/`never`, conditional types, mapped types, indexed access types, and type predicates.
- `pydantic/pydantic`: defer until Smelt is ready to stress Python metaprogramming, validators, decorators, dynamic schema construction, generic type variables, and deep typing machinery.

## Suggested Backlog Order

1. Import test framework public APIs as known externals:
   - TS: `vitest` exports `describe`, `it`, `test`, `expect`, `beforeEach`, `afterEach`.
   - TS: `@effect/vitest` exports Vitest-compatible `describe`, `it`, and `test`.
   - Python: recognize pytest test files and allow unannotated `test_*` functions to mean `-> None`.
2. Lower the smallest test body shape:
   - TS: `describe` block containing `it` / `test` calls with sync closures.
   - Python: top-level `test_*` functions with plain `assert`.
3. Add matcher/helper coverage:
   - TS: `toBeInstanceOf`, `toContain`, `toHaveLength`, `toHaveProperty`, `toThrow` with expected type/message, Effect-style `deepStrictEqual`.
   - Python: `pytest.raises` context managers.
4. Add parametrization:
   - Python `@pytest.mark.parametrize` first because it appears in both Rich and HTTPX.
   - TS table tests later.
5. Fix real module imports before expecting project slices to work:
   - named exported constants
   - re-exports
   - extensionful `.ts` imports
   - test dependency imports from package-local modules
6. Use date-fns as the first TS external target and Rich as the first Python external target.
   - date-fns has simple arithmetic tests that look close once imports and Vitest globals work.
   - Rich has simple assertion tests, but needs relaxed return annotations for test functions and basic class/protocol support.

