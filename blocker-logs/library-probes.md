# Bug-library transpile probes (TypeScript + Python)

_Generated 2026-08-23 by the `library-probes` workflow (`scripts/probe_libraries.py`)._

Each library is checked out at a pinned ref (see `.github/compat/libraries.json`), given its `.github/compat/<name>/Smelt.toml`, and run through `smelt build`. If a crate is emitted, its generated `cargo test` suite is run and counted. Otherwise every source/test file is scanned individually with `smelt dump-hir` to enumerate the full set of distinct blocker classes (single-file mode cannot resolve cross-file imports, so bare `unresolved name/identifier` errors are excluded as scan noise).

**Error categories:**
- **missing-stdlib** — a JS/Python builtin Smelt does not model yet (`Array`, `Number`, `Reflect`, `TextEncoder`, `Proxy`, ...).
- **non-working Rust** — a frontend/IR lowering gap: Smelt cannot lower the construct into Rust that compiles (missing return-type annotations, `try`/`except`, decorators, callback methods not lowered into closures, non-primitive exported `const`, ...).

## Summary

| Library | Lang | Transpile | Tests (pass/fail) | First abort | Blocker classes | Dominant |
| --- | --- | --- | --- | --- | ---: | --- |
| [es-toolkit](https://github.com/toss/es-toolkit) | TS | **yes** | 909 / 150 | — | — | — |
| [radash](https://github.com/sodiray/radash) | TS | **yes** | transpiled (counts unparsed) | — | — | — |
| [ts-pattern](https://github.com/gvergnaud/ts-pattern) | TS | **no** | n/a | `src/internals/helpers.ts` | 11 | non-working Rust (11r/0s) |
| [valibot](https://github.com/fabian-hiller/valibot) | TS | **no** | n/a | `library/src/storages/globalConfig/globalConfig.ts` | 15 | non-working Rust (14r/1s) |
| [neverthrow](https://github.com/supermacro/neverthrow) | TS | **no** | n/a | `src/result.ts` | 4 | non-working Rust (3r/1s) |
| [immer](https://github.com/immerjs/immer) | TS | **no** | n/a | `__tests__/spec_ts.ts` | 7 | non-working Rust (7r/0s) |
| [rxjs](https://github.com/ReactiveX/rxjs) | TS | **no** | n/a | `packages/rxjs/spec/Observable-spec.ts` | 23 | non-working Rust (22r/1s) |
| [returns](https://github.com/dry-python/returns) | PY | **no** | n/a | `(unknown)` | 26 | non-working Rust (26r/0s) |
| [result](https://github.com/rustedpy/result) | PY | **no** | n/a | `(unknown)` | 6 | non-working Rust (6r/0s) |
| [more-itertools](https://github.com/more-itertools/more-itertools) | PY | **no** | n/a | `(unknown)` | 21 | non-working Rust (21r/0s) |
| [funcy](https://github.com/Suor/funcy) | PY | **no** | n/a | `(unknown)` | 14 | non-working Rust (14r/0s) |
| [toolz](https://github.com/pytoolz/toolz) | PY | **no** | n/a | `(unknown)` | 14 | non-working Rust (14r/0s) |

## es-toolkit

- Source: `toss/es-toolkit` @ `e008a2818cd8`
- Transpile: **yes** — Rust crate emitted
- Generated `cargo test`: **909 passed / 150 failed**

## radash

- Source: `sodiray/radash` @ `4cab1900d08e`
- Transpile: **yes** — Rust crate emitted
- Generated `cargo test`: transpiled, but pass/fail counts could not be parsed

## ts-pattern

- Source: `gvergnaud/ts-pattern` @ `c92ca435c7e1`
- Transpile: **no** — `smelt build` aborts at `src/internals/helpers.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 68 · with blockers: 17

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 4 | 1 | non-working Rust | module-level function return type needs a supported default value |
| 4 | 4 | non-working Rust | spread call requires at least one argument |
| 2 | 1 | non-working Rust | Boolean requires a primitive argument |
| 2 | 1 | non-working Rust | too many generic type arguments |
| 2 | 2 | non-working Rust | array callback methods require exactly one callback argument |
| 1 | 1 | non-working Rust | field access is only lowered for Record<string, T>, class, and interface values for now (r |
| 1 | 1 | non-working Rust | string prefix/suffix methods require string receiver and argument |
| 1 | 1 | non-working Rust | tuple element type is not lowered yet: TSSymbolKeyword(TSSymbolKeyword { span: Span { star |
| 1 | 1 | non-working Rust | property names must be static identifiers or string literals |
| 1 | 1 | non-working Rust | statement kind is not lowered yet: TSEnumDeclaration(TSEnumDeclaration { span: Span { star |
| 1 | 1 | non-working Rust | Symbol(...) description must be a string |

## valibot

- Source: `fabian-hiller/valibot` @ `1f9b18338ad5`
- Transpile: **no** — `smelt build` aborts at `library/src/storages/globalConfig/globalConfig.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 1083 · with blockers: 35

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 8 | 8 | non-working Rust | callback method `X` is not lowered into closure bodies yet |
| 7 | 7 | non-working Rust | module-level mutable binding initializer must be a literal for now |
| 5 | 5 | missing-stdlib (builtin class) | unresolved class `X` |
| 2 | 2 | non-working Rust | exported const declarations require an initializer |
| 2 | 2 | non-working Rust | new Set(iterable) currently requires an array argument |
| 2 | 2 | non-working Rust | expect(...).resolves/rejects actual value must be a Promise<T> |
| 2 | 2 | non-working Rust | describe blocks only support direct it/test/describe calls for now |
| 2 | 2 | non-working Rust | new Map([...]) requires a Map<K, V> type annotation when annotated |
| 1 | 1 | non-working Rust | empty nested arrays require an explicit type annotation |
| 1 | 1 | non-working Rust | field access is only lowered for Record<string, T>, class, and interface values for now (r |
| 1 | 1 | non-working Rust | variable annotation `X` requires a diverging initializer |
| 1 | 1 | non-working Rust | too many generic type arguments |
| 1 | 1 | non-working Rust | Object.assign sources must be record or object-like values |
| 1 | 1 | non-working Rust | regex replacement supports only g/i/m/s RegExp literal flags |

## neverthrow

- Source: `supermacro/neverthrow` @ `5ef3a018bda7`
- Transpile: **no** — `smelt build` aborts at `src/result.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 8 · with blockers: 5

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 4 | 3 | non-working Rust | yield* requires a typed sync or async iterator method (received Some(Unknown)) |
| 1 | 1 | missing-stdlib | TypeScript instanceof requires a concrete class-typed left operand |
| 1 | 1 | non-working Rust | only expect(...).not matcher modifiers are supported |
| 1 | 1 | non-working Rust | yield* union member is not a typed iterable carrier |

## immer

- Source: `immerjs/immer` @ `a3be9df762c1`
- Transpile: **no** — `smelt build` aborts at `__tests__/spec_ts.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 24 · with blockers: 17

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 20 | 4 | non-working Rust | exported variable declarations must use const |
| 5 | 1 | non-working Rust | rest parameter type must resolve to an array type |
| 1 | 1 | non-working Rust | asserted call callee must be a function |
| 1 | 1 | non-working Rust | module-level mutable binding initializer must be a literal for now |
| 1 | 1 | non-working Rust | statement kind is not lowered yet: TSGlobalDeclaration(TSGlobalDeclaration { span: Span {  |
| 1 | 1 | non-working Rust | switch case labels must be string, number, boolean, or null literals |
| 1 | 1 | non-working Rust | exported const values currently support primitive literals and foldable primitive expressi |

## rxjs

- Source: `ReactiveX/rxjs` @ `c15b37f81ba5`
- Transpile: **no** — `smelt build` aborts at `packages/rxjs/spec/Observable-spec.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 376 · with blockers: 191

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 5 | 5 | non-working Rust | assignment target must be a local, field, or index expression |
| 3 | 3 | non-working Rust | asserted call callee must be a function |
| 3 | 1 | missing-stdlib | TypeScript instanceof target `X` is not a lowered class |
| 2 | 2 | non-working Rust | static fields require a concrete literal initializer |
| 2 | 1 | non-working Rust | expression kind is not lowered yet: SequenceExpression(SequenceExpression { span: Span { s |
| 1 | 1 | non-working Rust | statement kind is not lowered yet: TSImportEqualsDeclaration(TSImportEqualsDeclaration { s |
| 1 | 1 | non-working Rust | call argument kind is not lowered yet: ParenthesizedExpression(ParenthesizedExpression { s |
| 1 | 1 | non-working Rust | base class `X` is not declared |
| 1 | 1 | non-working Rust | call argument kind is not lowered yet: ParenthesizedExpression(ParenthesizedExpression { s |
| 1 | 1 | non-working Rust | field access is only lowered for Record<string, T>, class, and interface values for now (r |
| 1 | 1 | non-working Rust | field access is only lowered for Record<string, T>, class, and interface values for now (r |
| 1 | 1 | non-working Rust | call argument kind is not lowered yet: ParenthesizedExpression(ParenthesizedExpression { s |
| 1 | 1 | non-working Rust | call argument kind is not lowered yet: ParenthesizedExpression(ParenthesizedExpression { s |
| 1 | 1 | non-working Rust | call expression callee must return a function |

## returns

- Source: `dry-python/returns` @ `04e820c71461`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 199 · with blockers: 180

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 358 | 90 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 67 | 67 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 44 | 31 | non-working Rust | callback expression is not supported yet |
| 16 | 5 | non-working Rust | subscript access requires a list, set, dict, tuple, or string |
| 15 | 10 | non-working Rust | unknown class field `X` |
| 14 | 8 | non-working Rust | nested closure bodies need a single return expression |
| 13 | 10 | non-working Rust | class 'X': class-level assignment must be a single name bound to a literal |
| 12 | 10 | non-working Rust | class 'X': multiple inheritance is not supported |
| 7 | 5 | non-working Rust | function 'X' must have an explicit return type annotation |
| 6 | 4 | non-working Rust | nested closure parameters must have explicit type annotations |
| 6 | 4 | non-working Rust | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 5 | 4 | non-working Rust | lambda expressions need an expected `X` type from their context (annotate the assignment t |
| 4 | 4 | non-working Rust | pytest.mark.parametrize names must be a string literal |
| 4 | 1 | non-working Rust | pytest.mark.parametrize rows must be a list or tuple literal |

## result

- Source: `rustedpy/result` @ `0b855e1e38a0`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 5 · with blockers: 4

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 10 | 2 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 3 | 1 | non-working Rust | class 'X': decorator 'X' is not supported |
| 2 | 1 | non-working Rust | function 'X' must have an explicit return type annotation |
| 2 | 1 | non-working Rust | unknown class field `X` |
| 2 | 2 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 1 | 1 | non-working Rust | multiple `X` clauses are not supported; Smelt models a single catch handler |

## more-itertools

- Source: `more-itertools/more-itertools` @ `5d946b3590bf`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 6 · with blockers: 4

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 83 | 3 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 63 | 3 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 55 | 3 | non-working Rust | function 'X' must have an explicit return type annotation |
| 15 | 2 | non-working Rust | parameter 'X' in 'X' must have a type annotation |
| 5 | 2 | non-working Rust | nested closure return type must be explicit |
| 3 | 2 | non-working Rust | class 'X': decorator 'X' is not supported |
| 3 | 2 | non-working Rust | class 'X': class-level assignment must be a single name bound to a literal |
| 3 | 1 | non-working Rust | lambda expressions need an expected `X` type from their context (annotate the assignment t |
| 2 | 2 | non-working Rust | method 'X' must have an explicit return type annotation |
| 2 | 1 | non-working Rust | class 'X': multiple inheritance is not supported |
| 2 | 1 | non-working Rust | class 'X': unsupported class body statement 'X' |
| 2 | 1 | non-working Rust | integer literal out of i64 range |
| 1 | 1 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 1 | 1 | non-working Rust | list(value) currently requires a list, set, dict, or homogeneous tuple value |

## funcy

- Source: `Suor/funcy` @ `9eb04473e31b`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 33 · with blockers: 31

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 148 | 16 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 144 | 17 | non-working Rust | function 'X' must have an explicit return type annotation |
| 40 | 6 | non-working Rust | nested closure return type must be explicit |
| 31 | 9 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 13 | 4 | non-working Rust | nested class definitions are not yet supported |
| 4 | 1 | non-working Rust | attribute access is only supported on class instances |
| 4 | 3 | non-working Rust | parameter 'X' in 'X' must have a type annotation |
| 4 | 3 | non-working Rust | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 2 | 2 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 2 | 1 | non-working Rust | all() and any() argument must be a bool list |
| 1 | 1 | non-working Rust | set(value) currently requires a set, list, or homogeneous tuple value |
| 1 | 1 | non-working Rust | class 'X': class-level assignment must be a single name bound to a literal |
| 1 | 1 | non-working Rust | all() and any() currently support exactly one bool list argument |
| 1 | 1 | non-working Rust | lambda expressions need an expected `X` type from their context (annotate the assignment t |

## toolz

- Source: `pytoolz/toolz` @ `568c2b839397`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 31 · with blockers: 28

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 84 | 13 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 70 | 12 | non-working Rust | function 'X' must have an explicit return type annotation |
| 28 | 6 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 26 | 4 | non-working Rust | nested closure return type must be explicit |
| 7 | 4 | non-working Rust | nested class definitions are not yet supported |
| 5 | 2 | non-working Rust | only `X` deletion is supported |
| 4 | 2 | non-working Rust | parameter 'X' in 'X' must have a type annotation |
| 3 | 2 | non-working Rust | container constructors do not support keyword arguments yet |
| 3 | 3 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 3 | 2 | non-working Rust | class 'X': class-level assignment must be a single name bound to a literal |
| 2 | 2 | non-working Rust | lambda expressions need an expected `X` type from their context (annotate the assignment t |
| 1 | 1 | non-working Rust | dict.update() requires exactly one dict argument |
| 1 | 1 | non-working Rust | sorted() currently supports exactly one list argument |
| 1 | 1 | non-working Rust | binary operator 'X' is not supported |

## Highest-leverage transpiler gaps (non-working Rust)

Lowering gaps blocking more than one probed library; fixing these unlocks the most surface.

| Libraries hit | Total occ. | Blocker class |
| ---: | ---: | --- |
| 5 (funcy, more-itertools, result, returns, toolz) | 683 | only calls to top-level functions, class constructors, and print() are supported |
| 5 (funcy, more-itertools, result, returns, toolz) | 278 | function 'X' must have an explicit return type annotation |
| 5 (funcy, more-itertools, result, returns, toolz) | 75 | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 4 (funcy, more-itertools, returns, toolz) | 123 | parameter 'X' must have an explicit type annotation |
| 4 (funcy, more-itertools, returns, toolz) | 72 | nested closure return type must be explicit |
| 4 (funcy, more-itertools, returns, toolz) | 20 | class 'X': class-level assignment must be a single name bound to a literal |
| 4 (funcy, more-itertools, returns, toolz) | 11 | lambda expressions need an expected `X` type from their context (annotate the assignment target or c |
| 3 (funcy, more-itertools, toolz) | 23 | parameter 'X' in 'X' must have a type annotation |
| 3 (more-itertools, result, returns) | 7 | class 'X': decorator 'X' is not supported |
| 3 (more-itertools, returns, toolz) | 4 | binary operator 'X' is not supported |
| 2 (funcy, toolz) | 20 | nested class definitions are not yet supported |
| 2 (result, returns) | 17 | unknown class field `X` |
| 2 (more-itertools, returns) | 14 | class 'X': multiple inheritance is not supported |
| 2 (funcy, returns) | 10 | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 2 (rxjs, valibot) | 9 | callback method `X` is not lowered into closure bodies yet |
| 2 (immer, valibot) | 8 | module-level mutable binding initializer must be a literal for now |
| 2 (funcy, returns) | 5 | attribute access is only supported on class instances |
| 2 (immer, rxjs) | 4 | asserted call callee must be a function |
| 2 (ts-pattern, valibot) | 3 | too many generic type arguments |
| 2 (immer, rxjs) | 2 | switch case labels must be string, number, boolean, or null literals |

## Missing stdlib builtins observed

Builtins referenced in `new`/`extends`/callback position that Smelt does not resolve.

| Library | Builtins (occurrences) |
| --- | --- |
| valibot | `TextEncoder`×5 |

