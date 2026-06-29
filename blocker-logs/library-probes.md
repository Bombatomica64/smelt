# Bug-library transpile probes (TypeScript + Python)

_Generated 2026-06-29 by the `library-probes` workflow (`scripts/probe_libraries.py`)._

Each library is checked out at a pinned ref (see `.github/compat/libraries.json`), given its `.github/compat/<name>/Smelt.toml`, and run through `smelt build`. If a crate is emitted, its generated `cargo test` suite is run and counted. Otherwise every source/test file is scanned individually with `smelt dump-hir` to enumerate the full set of distinct blocker classes (single-file mode cannot resolve cross-file imports, so bare `unresolved name/identifier` errors are excluded as scan noise).

**Error categories:**
- **missing-stdlib** — a JS/Python builtin Smelt does not model yet (`Array`, `Number`, `Reflect`, `TextEncoder`, `Proxy`, ...).
- **non-working Rust** — a frontend/IR lowering gap: Smelt cannot lower the construct into Rust that compiles (missing return-type annotations, `try`/`except`, decorators, callback methods not lowered into closures, non-primitive exported `const`, ...).

## Summary

| Library | Lang | Transpile | Tests (pass/fail) | First abort | Blocker classes | Dominant |
| --- | --- | --- | --- | --- | ---: | --- |
| [es-toolkit](https://github.com/toss/es-toolkit) | TS | **no** | n/a | `src/array/chunk.spec.ts` | 82 | non-working Rust (80r/2s) |
| [radash](https://github.com/sodiray/radash) | TS | **no** | n/a | `home/runner/work/smelt/smelt/src/typed.ts` | 23 | non-working Rust (21r/2s) |
| [ts-pattern](https://github.com/gvergnaud/ts-pattern) | TS | **no** | n/a | `home/runner/work/smelt/smelt/src/types/Pattern.ts` | 15 | non-working Rust (15r/0s) |
| [valibot](https://github.com/fabian-hiller/valibot) | TS | **no** | n/a | `home/runner/work/smelt/smelt/library/src/utils/_getByteCount/_getByteCount.ts` | 32 | non-working Rust (31r/1s) |
| [neverthrow](https://github.com/supermacro/neverthrow) | TS | **no** | n/a | `home/runner/work/smelt/smelt/src/result-async.ts` | 11 | non-working Rust (11r/0s) |
| [returns](https://github.com/dry-python/returns) | PY | **no** | n/a | `tests/test_context/test_requires_context/test_context.py` | 33 | non-working Rust (33r/0s) |
| [result](https://github.com/rustedpy/result) | PY | **no** | n/a | `home/runner/work/smelt/smelt/src/result/result.py` | 8 | non-working Rust (8r/0s) |
| [more-itertools](https://github.com/more-itertools/more-itertools) | PY | **no** | n/a | `home/runner/work/smelt/smelt/more_itertools/recipes.py` | 10 | non-working Rust (10r/0s) |
| [funcy](https://github.com/Suor/funcy) | PY | **no** | n/a | `home/runner/work/smelt/smelt/funcy/primitives.py` | 15 | non-working Rust (15r/0s) |
| [toolz](https://github.com/pytoolz/toolz) | PY | **no** | n/a | `home/runner/work/smelt/smelt/toolz/itertoolz.py` | 15 | non-working Rust (15r/0s) |

## es-toolkit

- Source: `toss/es-toolkit` @ `e008a2818cd8`
- Transpile: **no** — `smelt build` aborts at `src/array/chunk.spec.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 1219 · with blockers: 330

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 46 | 46 | missing-stdlib (builtin class) | unresolved class `X` |
| 27 | 27 | non-working Rust | callback conditions must be boolean, optional, or supported truthy checks |
| 15 | 15 | non-working Rust | exported const values currently support primitive literals and foldable primitive expressi |
| 10 | 10 | non-working Rust | callback block statements must be const declarations, if guards, return, or throw |
| 10 | 10 | non-working Rust | callback method `X` is not lowered into closure bodies yet |
| 9 | 9 | missing-stdlib | TypeScript instanceof target `X` is not a lowered class |
| 9 | 9 | non-working Rust | unary operator is not lowered yet: Typeof |
| 8 | 8 | non-working Rust | expect(...).rejects.toThrow(...) actual value must be a Promise<T> |
| 7 | 7 | non-working Rust | array callback local callback `X` is not defined |
| 6 | 6 | non-working Rust | method calls are only lowered for class values for now |
| 6 | 6 | non-working Rust | array concat currently requires exactly one array argument |
| 6 | 6 | non-working Rust | array callback methods require exactly one callback argument |
| 5 | 5 | non-working Rust | object key ownership checks require a record receiver |
| 5 | 5 | non-working Rust | describe blocks only support direct it/test/describe calls for now |

## radash

- Source: `sodiray/radash` @ `4cab1900d08e`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/src/typed.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 19 · with blockers: 13

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 4 | 3 | non-working Rust | callback method `X` is not lowered into closure bodies yet |
| 3 | 2 | missing-stdlib (builtin class) | unresolved class `X` |
| 3 | 1 | non-working Rust | regex replacement requires string-compatible receiver, pattern, and replacement |
| 2 | 1 | non-working Rust | array fill value must match the array element type |
| 1 | 1 | non-working Rust | conditional expression branches must have the same lowered type (then: Some(Union([TypeId( |
| 1 | 1 | non-working Rust | method calls are only lowered for class values for now |
| 1 | 1 | non-working Rust | conditional expression branches must have the same lowered type (then: Some(Union([TypeId( |
| 1 | 1 | non-working Rust | conditional expression branches must have the same lowered type (then: Some(Function(Funct |
| 1 | 1 | non-working Rust | call argument kind is not lowered yet: UpdateExpression(UpdateExpression { span: Span { st |
| 1 | 1 | non-working Rust | Promise constructor lowering supports one arrow executor |
| 1 | 1 | non-working Rust | conditional expression branches must have the same lowered type (then: Some(List(TypeId(10 |
| 1 | 1 | non-working Rust | local `X` is not callable (Some(None)) |
| 1 | 1 | non-working Rust | parseFloat requires a string argument |
| 1 | 1 | non-working Rust | dynamic Date constructor calls require exactly one value argument |

## ts-pattern

- Source: `gvergnaud/ts-pattern` @ `c92ca435c7e1`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/src/types/Pattern.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 68 · with blockers: 20

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 4 | 4 | non-working Rust | spread call requires at least one argument |
| 2 | 1 | non-working Rust | callback method `X` is not lowered into closure bodies yet |
| 2 | 1 | non-working Rust | Boolean requires a primitive argument |
| 2 | 2 | non-working Rust | array callback methods require exactly one callback argument |
| 1 | 1 | non-working Rust | field access is only lowered for Record<string, T>, class, and interface values for now (r |
| 1 | 1 | non-working Rust | type annotation is not lowered yet: TSConstructorType(TSConstructorType { span: Span { sta |
| 1 | 1 | non-working Rust | exported const expression references unresolved const `X` |
| 1 | 1 | non-working Rust | dynamic computed and this-parameter interface methods are not lowered yet |
| 1 | 1 | non-working Rust | string prefix/suffix methods require string receiver and argument |
| 1 | 1 | non-working Rust | tuple element type is not lowered yet: TSSymbolKeyword(TSSymbolKeyword { span: Span { star |
| 1 | 1 | non-working Rust | property names must be static identifiers or string literals |
| 1 | 1 | non-working Rust | statement kind is not lowered yet: TSEnumDeclaration(TSEnumDeclaration { span: Span { star |
| 1 | 1 | non-working Rust | extended interface `X` is not declared |
| 1 | 1 | non-working Rust | method calls are only lowered for class values for now |

## valibot

- Source: `fabian-hiller/valibot` @ `1f9b18338ad5`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/library/src/utils/_getByteCount/_getByteCount.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 1083 · with blockers: 321

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 266 | 266 | non-working Rust | describe blocks only support direct it/test/describe calls for now |
| 8 | 8 | missing-stdlib (builtin class) | unresolved class `X` |
| 6 | 6 | non-working Rust | callback method `X` is not lowered into closure bodies yet |
| 4 | 4 | non-working Rust | new Map entry key and value types must be homogeneous |
| 3 | 3 | non-working Rust | extended interface `X` is not declared |
| 2 | 2 | non-working Rust | rest parameter type must resolve to an array type |
| 2 | 2 | non-working Rust | exported const declarations require an initializer |
| 2 | 2 | non-working Rust | this-parameter function types are not lowered yet |
| 2 | 2 | non-working Rust | expect(...).resolves/rejects actual value must be a Promise<T> |
| 2 | 2 | non-working Rust | array element kind is not lowered yet: FunctionExpression(Function { span: Span { start: 2 |
| 1 | 1 | non-working Rust | expect(...).toContain(...) requires a string, array, set, or tuple actual value with a mat |
| 1 | 1 | non-working Rust | array element kind is not lowered yet: AwaitExpression(AwaitExpression { span: Span { star |
| 1 | 1 | non-working Rust | local closure return type does not match its annotation: actual Some(Dict(TypeId(1), TypeI |
| 1 | 1 | non-working Rust | array element kind is not lowered yet: AwaitExpression(AwaitExpression { span: Span { star |

## neverthrow

- Source: `supermacro/neverthrow` @ `5ef3a018bda7`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/src/result-async.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 8 · with blockers: 6

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 4 | 2 | non-working Rust | exported const values currently support primitive literals and foldable primitive expressi |
| 3 | 2 | non-working Rust | property names must be static identifiers or string literals |
| 2 | 1 | non-working Rust | array callback local callback `X` is not defined |
| 1 | 1 | non-working Rust | callback method `X` is not lowered into closure bodies yet |
| 1 | 1 | non-working Rust | method calls are only lowered for class values for now |
| 1 | 1 | non-working Rust | local closure return type does not match its annotation: actual Some(Never), expected Some |
| 1 | 1 | non-working Rust | describe blocks only support direct it/test/describe calls for now |
| 1 | 1 | non-working Rust | only expect(...).not matcher modifiers are supported |
| 1 | 1 | non-working Rust | yield* generator delegation is not lowered yet |
| 1 | 1 | non-working Rust | String.match() requires exactly one RegExp argument |
| 1 | 1 | non-working Rust | call expression is not lowered yet |

## returns

- Source: `dry-python/returns` @ `04e820c71461`
- Transpile: **no** — `smelt build` aborts at `tests/test_context/test_requires_context/test_context.py`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 199 · with blockers: 180

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 610 | 141 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 51 | 38 | non-working Rust | class 'X': decorator 'X' is not supported |
| 44 | 31 | non-working Rust | callback expression is not supported yet |
| 37 | 27 | non-working Rust | class 'X': multiple inheritance is not supported |
| 26 | 18 | non-working Rust | class 'X': unsupported class body statement 'X' |
| 25 | 10 | non-working Rust | subscript access requires a list, set, dict, tuple, or string |
| 21 | 14 | non-working Rust | unknown class field `X` |
| 19 | 8 | non-working Rust | Callable first argument must be a list of param types, e.g. [int, str] |
| 18 | 10 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 17 | 11 | non-working Rust | nested closure bodies need a single return expression |
| 16 | 11 | non-working Rust | pytest.mark.parametrize names must be a string literal |
| 13 | 7 | non-working Rust | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 11 | 6 | non-working Rust | unsupported expression: ellipsis |
| 10 | 8 | non-working Rust | function 'X' must have an explicit return type annotation |

## result

- Source: `rustedpy/result` @ `0b855e1e38a0`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/src/result/result.py`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 5 · with blockers: 4

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 43 | 3 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 7 | 2 | non-working Rust | async nested closures need async closure-body lowering |
| 6 | 2 | non-working Rust | callback expression is not supported yet |
| 3 | 2 | non-working Rust | unknown class field `X` |
| 3 | 1 | non-working Rust | nested closure bodies need a single return expression |
| 2 | 1 | non-working Rust | class 'X': unsupported class body statement 'X' |
| 2 | 1 | non-working Rust | Callable first argument must be a list of param types, e.g. [int, str] |
| 2 | 1 | non-working Rust | unsupported statement: try |

## more-itertools

- Source: `more-itertools/more-itertools` @ `5d946b3590bf`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/more_itertools/recipes.py`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 6 · with blockers: 4

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 185 | 4 | non-working Rust | function 'X' must have an explicit return type annotation |
| 157 | 2 | non-working Rust | method 'X' must have an explicit return type annotation |
| 12 | 2 | non-working Rust | parameter 'X' in 'X' must have a type annotation |
| 6 | 2 | non-working Rust | class 'X': unsupported class body statement 'X' |
| 4 | 3 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 3 | 2 | non-working Rust | class 'X': decorator 'X' is not supported |
| 3 | 2 | non-working Rust | unsupported statement: try |
| 2 | 1 | non-working Rust | class 'X': multiple inheritance is not supported |
| 1 | 1 | non-working Rust | integer literal out of i64 range |
| 1 | 1 | non-working Rust | unsupported expression: lambda |

## funcy

- Source: `Suor/funcy` @ `9eb04473e31b`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/funcy/primitives.py`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 33 · with blockers: 31

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 202 | 21 | non-working Rust | function 'X' must have an explicit return type annotation |
| 148 | 16 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 39 | 5 | non-working Rust | nested closure return type must be explicit |
| 13 | 4 | non-working Rust | nested class definitions are not yet supported |
| 6 | 4 | non-working Rust | parameter 'X' in 'X' must have a type annotation |
| 4 | 1 | non-working Rust | attribute access is only supported on class instances |
| 4 | 3 | non-working Rust | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 3 | 2 | non-working Rust | unsupported statement: try |
| 3 | 2 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 2 | 2 | non-working Rust | method 'X' must have an explicit return type annotation |
| 2 | 1 | non-working Rust | all() and any() argument must be a bool list |
| 1 | 1 | non-working Rust | set(value) currently requires a set, list, or homogeneous tuple value |
| 1 | 1 | non-working Rust | class 'X': unsupported class body statement 'X' |
| 1 | 1 | non-working Rust | all() and any() currently support exactly one bool list argument |

## toolz

- Source: `pytoolz/toolz` @ `568c2b839397`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/toolz/itertoolz.py`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 31 · with blockers: 28

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 118 | 17 | non-working Rust | function 'X' must have an explicit return type annotation |
| 90 | 15 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 27 | 5 | non-working Rust | nested closure return type must be explicit |
| 8 | 5 | non-working Rust | nested class definitions are not yet supported |
| 5 | 2 | non-working Rust | unsupported statement: del |
| 5 | 3 | non-working Rust | class 'X': unsupported class body statement 'X' |
| 4 | 2 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 3 | 2 | non-working Rust | container constructors do not support keyword arguments yet |
| 3 | 2 | non-working Rust | method 'X' must have an explicit return type annotation |
| 2 | 1 | non-working Rust | parameter 'X' in 'X' must have a type annotation |
| 2 | 2 | non-working Rust | class 'X': decorator 'X' is not supported |
| 2 | 2 | non-working Rust | unsupported expression: lambda |
| 1 | 1 | non-working Rust | dict.update() requires exactly one dict argument |
| 1 | 1 | non-working Rust | sorted() currently supports exactly one list argument |

## Highest-leverage transpiler gaps (non-working Rust)

Lowering gaps blocking more than one probed library; fixing these unlocks the most surface.

| Libraries hit | Total occ. | Blocker class |
| ---: | ---: | --- |
| 5 (funcy, more-itertools, result, returns, toolz) | 895 | only calls to top-level functions, class constructors, and print() are supported |
| 5 (funcy, more-itertools, result, returns, toolz) | 40 | class 'X': unsupported class body statement 'X' |
| 5 (es-toolkit, neverthrow, radash, ts-pattern, valibot) | 23 | callback method `X` is not lowered into closure bodies yet |
| 4 (funcy, more-itertools, returns, toolz) | 515 | function 'X' must have an explicit return type annotation |
| 4 (funcy, more-itertools, result, returns) | 12 | unsupported statement: try |
| 4 (es-toolkit, neverthrow, radash, ts-pattern) | 9 | method calls are only lowered for class values for now |
| 4 (funcy, more-itertools, returns, toolz) | 9 | unsupported expression: lambda |
| 3 (es-toolkit, neverthrow, valibot) | 272 | describe blocks only support direct it/test/describe calls for now |
| 3 (funcy, more-itertools, toolz) | 162 | method 'X' must have an explicit return type annotation |
| 3 (funcy, returns, toolz) | 67 | nested closure return type must be explicit |
| 3 (more-itertools, returns, toolz) | 56 | class 'X': decorator 'X' is not supported |
| 3 (funcy, returns, toolz) | 25 | parameter 'X' must have an explicit type annotation |
| 3 (es-toolkit, neverthrow, radash) | 20 | exported const values currently support primitive literals and foldable primitive expressions |
| 3 (funcy, more-itertools, toolz) | 20 | parameter 'X' in 'X' must have a type annotation |
| 3 (es-toolkit, neverthrow, radash) | 10 | array callback local callback `X` is not defined |
| 3 (es-toolkit, ts-pattern, valibot) | 6 | extended interface `X` is not declared |
| 3 (es-toolkit, neverthrow, radash) | 5 | call expression is not lowered yet |
| 3 (es-toolkit, neverthrow, ts-pattern) | 5 | property names must be static identifiers or string literals |
| 2 (result, returns) | 50 | callback expression is not supported yet |
| 2 (more-itertools, returns) | 39 | class 'X': multiple inheritance is not supported |
| 2 (result, returns) | 24 | unknown class field `X` |
| 2 (result, returns) | 21 | Callable first argument must be a list of param types, e.g. [int, str] |
| 2 (funcy, toolz) | 21 | nested class definitions are not yet supported |
| 2 (result, returns) | 20 | nested closure bodies need a single return expression |
| 2 (funcy, returns) | 17 | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 2 (es-toolkit, ts-pattern) | 8 | array callback methods require exactly one callback argument |
| 2 (es-toolkit, valibot) | 8 | new Map entry key and value types must be homogeneous |
| 2 (returns, toolz) | 7 | binary operator 'X' is not supported |
| 2 (es-toolkit, valibot) | 5 | expect(...).resolves/rejects actual value must be a Promise<T> |
| 2 (es-toolkit, radash) | 4 | array fill value must match the array element type |
| 2 (es-toolkit, valibot) | 3 | this-parameter function types are not lowered yet |
| 2 (es-toolkit, valibot) | 2 | expect(...).toContain(...) requires a string, array, set, or tuple actual value with a matching expe |

## Missing stdlib builtins observed

Builtins referenced in `new`/`extends`/callback position that Smelt does not resolve.

| Library | Builtins (occurrences) |
| --- | --- |
| es-toolkit | `Array`×11, `Number`×9, `globalThis`×5, `Math`×3, `parseInt`×3, `Map`×3, `Reflect`×3, `AbortController`×3, `Function`×2, `Buffer`×2, `Blob`×2, `Promise`×2, `WeakMap`×1, `ArrayBuffer`×1, `File`×1, `Proxy`×1 |
| radash | `Proxy`×3, `Reflect`×1 |
| ts-pattern | `Reflect`×2 |
| valibot | `TextEncoder`×5, `Number`×1, `isFinite`×1, `Blob`×1, `File`×1 |

