# Bug-library transpile probes (TypeScript + Python)

_Generated 2026-07-02 by the `library-probes` workflow (`scripts/probe_libraries.py`)._

Each library is checked out at a pinned ref (see `.github/compat/libraries.json`), given its `.github/compat/<name>/Smelt.toml`, and run through `smelt build`. If a crate is emitted, its generated `cargo test` suite is run and counted. Otherwise every source/test file is scanned individually with `smelt dump-hir` to enumerate the full set of distinct blocker classes (single-file mode cannot resolve cross-file imports, so bare `unresolved name/identifier` errors are excluded as scan noise).

**Error categories:**
- **missing-stdlib** — a JS/Python builtin Smelt does not model yet (`Array`, `Number`, `Reflect`, `TextEncoder`, `Proxy`, ...).
- **non-working Rust** — a frontend/IR lowering gap: Smelt cannot lower the construct into Rust that compiles (missing return-type annotations, `try`/`except`, decorators, callback methods not lowered into closures, non-primitive exported `const`, ...).

## Summary

| Library | Lang | Transpile | Tests (pass/fail) | First abort | Blocker classes | Dominant |
| --- | --- | --- | --- | --- | ---: | --- |
| [es-toolkit](https://github.com/toss/es-toolkit) | TS | **no** | n/a | `home/runner/work/smelt/smelt/src/array/uniq.ts` | 55 | non-working Rust (53r/2s) |
| [radash](https://github.com/sodiray/radash) | TS | **no** | n/a | `home/runner/work/smelt/smelt/src/typed.ts` | 19 | non-working Rust (19r/0s) |
| [ts-pattern](https://github.com/gvergnaud/ts-pattern) | TS | **no** | n/a | `home/runner/work/smelt/smelt/src/types/Pattern.ts` | 12 | non-working Rust (12r/0s) |
| [valibot](https://github.com/fabian-hiller/valibot) | TS | **no** | n/a | `home/runner/work/smelt/smelt/library/src/utils/_getByteCount/_getByteCount.ts` | 61 | non-working Rust (60r/1s) |
| [neverthrow](https://github.com/supermacro/neverthrow) | TS | **no** | n/a | `home/runner/work/smelt/smelt/src/result-async.ts` | 8 | non-working Rust (8r/0s) |
| [returns](https://github.com/dry-python/returns) | PY | **no** | n/a | `(unknown)` | 26 | non-working Rust (26r/0s) |
| [result](https://github.com/rustedpy/result) | PY | **no** | n/a | `(unknown)` | 6 | non-working Rust (6r/0s) |
| [more-itertools](https://github.com/more-itertools/more-itertools) | PY | **no** | n/a | `(unknown)` | 10 | non-working Rust (10r/0s) |
| [funcy](https://github.com/Suor/funcy) | PY | **no** | n/a | `(unknown)` | 16 | non-working Rust (16r/0s) |
| [toolz](https://github.com/pytoolz/toolz) | PY | **no** | n/a | `(unknown)` | 15 | non-working Rust (15r/0s) |

## es-toolkit

- Source: `toss/es-toolkit` @ `e008a2818cd8`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/src/array/uniq.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 1219 · with blockers: 128

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 13 | 13 | non-working Rust | callback method `X` is not lowered into closure bodies yet |
| 13 | 13 | missing-stdlib (builtin class) | unresolved class `X` |
| 6 | 6 | non-working Rust | new expressions require a direct class name |
| 6 | 4 | missing-stdlib | TypeScript instanceof target `X` is not a lowered class |
| 5 | 5 | non-working Rust | exported const member expressions support well-known Number/Math numeric constants only |
| 5 | 5 | non-working Rust | array concat requires an array receiver |
| 4 | 4 | non-working Rust | callback conditional expression branches must have compatible lowered types |
| 4 | 4 | non-working Rust | switch case labels must be string, number, boolean, or null literals |
| 3 | 3 | non-working Rust | array push argument must match the array element type |
| 3 | 3 | non-working Rust | negative array/string bracket indexes are JavaScript property lookups; use .at(...) for ne |
| 2 | 2 | non-working Rust | method calls are only lowered for class values for now |
| 2 | 2 | non-working Rust | field access is only lowered for Record<string, T>, class, and interface values for now (r |
| 2 | 2 | non-working Rust | timer lowering supports setTimeout(milliseconds), setTimeout(callback, milliseconds), clea |
| 2 | 2 | non-working Rust | switch fallthrough is not lowered yet; each case must break, return, or throw |

## radash

- Source: `sodiray/radash` @ `4cab1900d08e`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/src/typed.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 19 · with blockers: 11

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 3 | 1 | non-working Rust | regex replacement requires string-compatible receiver, pattern, and replacement |
| 2 | 2 | non-working Rust | callback method `X` is not lowered into closure bodies yet |
| 1 | 1 | non-working Rust | conditional expression branches must have the same lowered type (then: Some(Union([TypeId( |
| 1 | 1 | non-working Rust | method calls are only lowered for class values for now |
| 1 | 1 | non-working Rust | conditional expression branches must have the same lowered type (then: Some(Union([TypeId( |
| 1 | 1 | non-working Rust | conditional expression branches must have the same lowered type (then: Some(Function(Funct |
| 1 | 1 | non-working Rust | call argument kind is not lowered yet: UpdateExpression(UpdateExpression { span: Span { st |
| 1 | 1 | non-working Rust | Promise constructor lowering supports one arrow executor |
| 1 | 1 | non-working Rust | Error constructor message must be a string |
| 1 | 1 | non-working Rust | local `X` is not callable (Some(None)) |
| 1 | 1 | non-working Rust | statement kind is not lowered yet: EmptyStatement(EmptyStatement { span: Span { start: 151 |
| 1 | 1 | non-working Rust | parseFloat requires a string argument |
| 1 | 1 | non-working Rust | dynamic Date constructor calls require exactly one value argument |
| 1 | 1 | non-working Rust | callback array spread elements are not supported yet |

## ts-pattern

- Source: `gvergnaud/ts-pattern` @ `c92ca435c7e1`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/src/types/Pattern.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 68 · with blockers: 19

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 4 | 4 | non-working Rust | spread call requires at least one argument |
| 2 | 1 | non-working Rust | callback method `X` is not lowered into closure bodies yet |
| 2 | 1 | non-working Rust | Boolean requires a primitive argument |
| 2 | 2 | non-working Rust | array callback methods require exactly one callback argument |
| 1 | 1 | non-working Rust | field access is only lowered for Record<string, T>, class, and interface values for now (r |
| 1 | 1 | non-working Rust | dynamic computed and this-parameter interface methods are not lowered yet |
| 1 | 1 | non-working Rust | string prefix/suffix methods require string receiver and argument |
| 1 | 1 | non-working Rust | tuple element type is not lowered yet: TSSymbolKeyword(TSSymbolKeyword { span: Span { star |
| 1 | 1 | non-working Rust | property names must be static identifiers or string literals |
| 1 | 1 | non-working Rust | statement kind is not lowered yet: TSEnumDeclaration(TSEnumDeclaration { span: Span { star |
| 1 | 1 | non-working Rust | method calls are only lowered for class values for now |
| 1 | 1 | non-working Rust | Symbol(...) description must be a string |

## valibot

- Source: `fabian-hiller/valibot` @ `1f9b18338ad5`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/library/src/utils/_getByteCount/_getByteCount.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 1083 · with blockers: 78

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 8 | 8 | non-working Rust | callback method `X` is not lowered into closure bodies yet |
| 5 | 5 | missing-stdlib (builtin class) | unresolved class `X` |
| 2 | 2 | non-working Rust | rest parameter type must resolve to an array type |
| 2 | 2 | non-working Rust | exported const declarations require an initializer |
| 2 | 2 | non-working Rust | new Set(iterable) currently requires an array argument |
| 2 | 2 | non-working Rust | expect(...).resolves/rejects actual value must be a Promise<T> |
| 2 | 2 | non-working Rust | describe blocks only support direct it/test/describe calls for now |
| 2 | 2 | non-working Rust | array element kind is not lowered yet: FunctionExpression(Function { span: Span { start: 2 |
| 1 | 1 | non-working Rust | empty nested arrays require an explicit type annotation |
| 1 | 1 | non-working Rust | expect(...).toContain(...) requires a string, array, set, or tuple actual value with a mat |
| 1 | 1 | non-working Rust | field access is only lowered for Record<string, T>, class, and interface values for now (r |
| 1 | 1 | non-working Rust | type assertion cannot construct a never value |
| 1 | 1 | non-working Rust | array element kind is not lowered yet: AwaitExpression(AwaitExpression { span: Span { star |
| 1 | 1 | non-working Rust | local closure return type does not match its annotation: actual Some(Dict(TypeId(1), TypeI |

## neverthrow

- Source: `supermacro/neverthrow` @ `5ef3a018bda7`
- Transpile: **no** — `smelt build` aborts at `home/runner/work/smelt/smelt/src/result-async.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 8 · with blockers: 5

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 3 | 2 | non-working Rust | property names must be static identifiers or string literals |
| 3 | 1 | non-working Rust | exported const member expressions support well-known Number/Math numeric constants only |
| 2 | 2 | non-working Rust | String.match() requires exactly one RegExp argument |
| 2 | 2 | non-working Rust | yield* generator delegation is not lowered yet |
| 1 | 1 | non-working Rust | method calls are only lowered for class values for now |
| 1 | 1 | non-working Rust | callback method `X` is not lowered into closure bodies yet |
| 1 | 1 | non-working Rust | local closure return type does not match its annotation: actual Some(Never), expected Some |
| 1 | 1 | non-working Rust | only expect(...).not matcher modifiers are supported |

## returns

- Source: `dry-python/returns` @ `04e820c71461`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 199 · with blockers: 180

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 357 | 89 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 67 | 67 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 44 | 31 | non-working Rust | callback expression is not supported yet |
| 16 | 5 | non-working Rust | subscript access requires a list, set, dict, tuple, or string |
| 14 | 9 | non-working Rust | unknown class field `X` |
| 14 | 8 | non-working Rust | nested closure bodies need a single return expression |
| 14 | 10 | non-working Rust | class 'X': unsupported class body statement 'X' |
| 12 | 10 | non-working Rust | class 'X': multiple inheritance is not supported |
| 6 | 4 | non-working Rust | nested closure parameters must have explicit type annotations |
| 6 | 4 | non-working Rust | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 5 | 4 | non-working Rust | unsupported expression: lambda |
| 4 | 2 | non-working Rust | Callable first argument must be a list of param types, e.g. [int, str] |
| 4 | 3 | non-working Rust | unsupported statement: try |
| 4 | 4 | non-working Rust | pytest.mark.parametrize names must be a string literal |

## result

- Source: `rustedpy/result` @ `0b855e1e38a0`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 5 · with blockers: 4

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 10 | 2 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 2 | 1 | non-working Rust | class 'X': unsupported class body statement 'X' |
| 2 | 1 | non-working Rust | Callable first argument must be a list of param types, e.g. [int, str] |
| 2 | 1 | non-working Rust | unknown class field `X` |
| 2 | 1 | non-working Rust | unsupported statement: try |
| 2 | 2 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |

## more-itertools

- Source: `more-itertools/more-itertools` @ `5d946b3590bf`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 6 · with blockers: 4

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 157 | 2 | non-working Rust | method 'X' must have an explicit return type annotation |
| 118 | 3 | non-working Rust | function 'X' must have an explicit return type annotation |
| 12 | 2 | non-working Rust | parameter 'X' in 'X' must have a type annotation |
| 6 | 2 | non-working Rust | class 'X': unsupported class body statement 'X' |
| 3 | 2 | non-working Rust | class 'X': decorator 'X' is not supported |
| 2 | 2 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 2 | 1 | non-working Rust | class 'X': multiple inheritance is not supported |
| 1 | 1 | non-working Rust | unsupported statement: try |
| 1 | 1 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 1 | 1 | non-working Rust | unsupported expression: lambda |

## funcy

- Source: `Suor/funcy` @ `9eb04473e31b`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 33 · with blockers: 31

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 174 | 19 | non-working Rust | function 'X' must have an explicit return type annotation |
| 146 | 15 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 39 | 5 | non-working Rust | nested closure return type must be explicit |
| 13 | 4 | non-working Rust | nested class definitions are not yet supported |
| 4 | 1 | non-working Rust | attribute access is only supported on class instances |
| 4 | 3 | non-working Rust | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 3 | 3 | non-working Rust | parameter 'X' in 'X' must have a type annotation |
| 3 | 2 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 2 | 2 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 2 | 2 | non-working Rust | method 'X' must have an explicit return type annotation |
| 2 | 1 | non-working Rust | all() and any() argument must be a bool list |
| 2 | 1 | non-working Rust | unsupported statement: try |
| 1 | 1 | non-working Rust | set(value) currently requires a set, list, or homogeneous tuple value |
| 1 | 1 | non-working Rust | class 'X': unsupported class body statement 'X' |

## toolz

- Source: `pytoolz/toolz` @ `568c2b839397`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 31 · with blockers: 28

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 94 | 14 | non-working Rust | function 'X' must have an explicit return type annotation |
| 84 | 13 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 26 | 4 | non-working Rust | nested closure return type must be explicit |
| 7 | 4 | non-working Rust | nested class definitions are not yet supported |
| 5 | 2 | non-working Rust | only `X` deletion is supported |
| 4 | 2 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 3 | 2 | non-working Rust | container constructors do not support keyword arguments yet |
| 3 | 3 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 3 | 2 | non-working Rust | class 'X': unsupported class body statement 'X' |
| 3 | 2 | non-working Rust | method 'X' must have an explicit return type annotation |
| 2 | 2 | non-working Rust | unsupported expression: lambda |
| 1 | 1 | non-working Rust | dict.update() requires exactly one dict argument |
| 1 | 1 | non-working Rust | sorted() currently supports exactly one list argument |
| 1 | 1 | non-working Rust | class 'X': decorator 'X' is not supported |

## Highest-leverage transpiler gaps (non-working Rust)

Lowering gaps blocking more than one probed library; fixing these unlocks the most surface.

| Libraries hit | Total occ. | Blocker class |
| ---: | ---: | --- |
| 5 (funcy, more-itertools, result, returns, toolz) | 599 | only calls to top-level functions, class constructors, and print() are supported |
| 5 (funcy, more-itertools, result, returns, toolz) | 75 | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 5 (es-toolkit, neverthrow, radash, ts-pattern, valibot) | 26 | callback method `X` is not lowered into closure bodies yet |
| 5 (funcy, more-itertools, result, returns, toolz) | 26 | class 'X': unsupported class body statement 'X' |
| 4 (funcy, more-itertools, returns, toolz) | 389 | function 'X' must have an explicit return type annotation |
| 4 (funcy, more-itertools, returns, toolz) | 9 | unsupported expression: lambda |
| 4 (funcy, more-itertools, result, returns) | 9 | unsupported statement: try |
| 4 (es-toolkit, neverthrow, radash, ts-pattern) | 5 | method calls are only lowered for class values for now |
| 3 (funcy, more-itertools, toolz) | 162 | method 'X' must have an explicit return type annotation |
| 3 (funcy, returns, toolz) | 66 | nested closure return type must be explicit |
| 3 (es-toolkit, neverthrow, radash) | 9 | exported const member expressions support well-known Number/Math numeric constants only |
| 3 (funcy, returns, toolz) | 8 | parameter 'X' must have an explicit type annotation |
| 2 (funcy, toolz) | 20 | nested class definitions are not yet supported |
| 2 (result, returns) | 16 | unknown class field `X` |
| 2 (funcy, more-itertools) | 15 | parameter 'X' in 'X' must have a type annotation |
| 2 (more-itertools, returns) | 14 | class 'X': multiple inheritance is not supported |
| 2 (funcy, returns) | 10 | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 2 (result, returns) | 6 | Callable first argument must be a list of param types, e.g. [int, str] |
| 2 (neverthrow, ts-pattern) | 4 | property names must be static identifiers or string literals |
| 2 (more-itertools, toolz) | 4 | class 'X': decorator 'X' is not supported |
| 2 (returns, toolz) | 3 | binary operator 'X' is not supported |

## Missing stdlib builtins observed

Builtins referenced in `new`/`extends`/callback position that Smelt does not resolve.

| Library | Builtins (occurrences) |
| --- | --- |
| es-toolkit | `Map`×4, `Buffer`×4, `Proxy`×3, `File`×3, `ArrayBuffer`×2, `WeakMap`×1 |
| valibot | `TextEncoder`×5 |

