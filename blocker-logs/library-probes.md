# Bug-library transpile probes (TypeScript + Python)

_Generated 2026-11-14 by the `library-probes` workflow (`scripts/probe_libraries.py`)._

Each library is checked out at a pinned ref (see `.github/compat/libraries.json`), given its `.github/compat/<name>/Smelt.toml`, and run through `smelt build`. If a crate is emitted, its generated `cargo test` suite is run and counted. Otherwise every source/test file is scanned individually with `smelt dump-hir` to enumerate the full set of distinct blocker classes (single-file mode cannot resolve cross-file imports, so bare `unresolved name/identifier` errors are excluded as scan noise).

**Error categories:**
- **missing-stdlib** — a JS/Python builtin Smelt does not model yet (`Array`, `Number`, `Reflect`, `TextEncoder`, `Proxy`, ...).
- **non-working Rust** — a frontend/IR lowering gap: Smelt cannot lower the construct into Rust that compiles (missing return-type annotations, `try`/`except`, decorators, callback methods not lowered into closures, non-primitive exported `const`, ...).

## Summary

| Library | Lang | Transpile | Tests (pass/fail) | First abort | Blocker classes | Dominant |
| --- | --- | --- | --- | --- | ---: | --- |
| [remeda](https://github.com/remeda/remeda) | TS | **yes** | transpiled (counts unparsed) | — | — | — |
| [es-toolkit](https://github.com/toss/es-toolkit) | TS | **yes** | transpiled (counts unparsed) | — | — | — |
| [hono](https://github.com/honojs/hono) | TS | **yes** | 444 / 10 | — | — | — |
| [radash](https://github.com/sodiray/radash) | TS | **yes** | transpiled (counts unparsed) | — | — | — |
| [ts-pattern](https://github.com/gvergnaud/ts-pattern) | TS | **yes** | transpiled (counts unparsed) | — | — | — |
| [valibot](https://github.com/fabian-hiller/valibot) | TS | **no** | n/a | `library/src/storages/globalConfig/globalConfig.ts` | 4 | non-working Rust (4r/0s) |
| [neverthrow](https://github.com/supermacro/neverthrow) | TS | **no** | n/a | `src/result.ts` | 1 | missing-stdlib (0r/1s) |
| [immer](https://github.com/immerjs/immer) | TS | **no** | n/a | `__tests__/spec_ts.ts` | 1 | non-working Rust (1r/0s) |
| [rxjs](https://github.com/ReactiveX/rxjs) | TS | **no** | n/a | `packages/observable/src/observable.ts` | 8 | non-working Rust (8r/0s) |
| [date-fns](https://github.com/date-fns/date-fns) | TS | **no** | n/a | `pkgs/tz/src/date/index.d.ts` | 2 | non-working Rust (2r/0s) |
| [returns](https://github.com/dry-python/returns) | PY | **no** | n/a | `(unknown)` | 29 | non-working Rust (29r/0s) |
| [result](https://github.com/rustedpy/result) | PY | **no** | n/a | `(unknown)` | 6 | non-working Rust (6r/0s) |
| [more-itertools](https://github.com/more-itertools/more-itertools) | PY | **no** | n/a | `(unknown)` | 22 | non-working Rust (22r/0s) |
| [funcy](https://github.com/Suor/funcy) | PY | **no** | n/a | `(unknown)` | 11 | non-working Rust (11r/0s) |
| [toolz](https://github.com/pytoolz/toolz) | PY | **no** | n/a | `toolz/itertoolz.py` | 13 | non-working Rust (13r/0s) |
| [cachetools](https://github.com/tkem/cachetools) | PY | **no** | n/a | `src/cachetools/_cached.py` | 7 | non-working Rust (7r/0s) |

## remeda

- Source: `remeda/remeda` @ `3c80f28bb394`
- Transpile: **yes** — Rust crate emitted
- Generated `cargo test`: transpiled, but pass/fail counts could not be parsed

## es-toolkit

- Source: `toss/es-toolkit` @ `e008a2818cd8`
- Transpile: **yes** — Rust crate emitted
- Generated `cargo test`: transpiled, but pass/fail counts could not be parsed

## hono

- Source: `honojs/hono` @ `0f5ad7b6bf0c`
- Transpile: **yes** — Rust crate emitted
- Generated `cargo test`: **444 passed / 10 failed**

## radash

- Source: `sodiray/radash` @ `4cab1900d08e`
- Transpile: **yes** — Rust crate emitted
- Generated `cargo test`: transpiled, but pass/fail counts could not be parsed

## ts-pattern

- Source: `gvergnaud/ts-pattern` @ `c92ca435c7e1`
- Transpile: **yes** — Rust crate emitted
- Generated `cargo test`: transpiled, but pass/fail counts could not be parsed

## valibot

- Source: `fabian-hiller/valibot` @ `1f9b18338ad5`
- Transpile: **no** — `smelt build` aborts at `library/src/storages/globalConfig/globalConfig.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 1083 · with blockers: 1050

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 1044 | 1044 | non-working Rust | module-level mutable binding initializer must be a literal for now |
| 4 | 4 | non-working Rust | string normalize requires a literal normalization form |
| 3 | 3 | non-working Rust | module-level binding `X` holds a `X` instance that cannot be read from here; a class-typed |
| 2 | 2 | non-working Rust | `X` is not modelled |

## neverthrow

- Source: `supermacro/neverthrow` @ `5ef3a018bda7`
- Transpile: **no** — `smelt build` aborts at `src/result.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 8 · with blockers: 8

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 8 | 8 | missing-stdlib | TypeScript instanceof requires a concrete class-typed left operand |

## immer

- Source: `immerjs/immer` @ `a3be9df762c1`
- Transpile: **no** — `smelt build` aborts at `__tests__/spec_ts.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 24 · with blockers: 23

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 48 | 16 | non-working Rust | rest parameter type must resolve to an array type |

## rxjs

- Source: `ReactiveX/rxjs` @ `c15b37f81ba5`
- Transpile: **no** — `smelt build` aborts at `packages/observable/src/observable.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 376 · with blockers: 354

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 644 | 322 | non-working Rust | local `X` is not callable (Some(None)) |
| 322 | 322 | non-working Rust | dynamic computed and this-parameter interface methods are not lowered yet |
| 322 | 322 | non-working Rust | property names must be static identifiers or string literals |
| 322 | 322 | non-working Rust | condition expression must be boolean or optional (got Some(Symbol)) |
| 322 | 322 | non-working Rust | statement kind is not lowered yet: TSGlobalDeclaration(TSGlobalDeclaration { span: Span {  |
| 19 | 19 | non-working Rust | unresolved package `X`: not a source file and not a modeled host module |
| 8 | 8 | non-working Rust | statement kind is not lowered yet: TSGlobalDeclaration(TSGlobalDeclaration { span: Span {  |
| 3 | 3 | non-working Rust | module-level variable type is not lowered for function reads yet |

## date-fns

- Source: `date-fns/date-fns` @ `cd53d2538cfa`
- Transpile: **no** — `smelt build` aborts at `pkgs/tz/src/date/index.d.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 1493 · with blockers: 203

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 194 | 194 | non-working Rust | declare methods are not lowered yet |
| 9 | 9 | non-working Rust | `X` is not modeled |

## returns

- Source: `dry-python/returns` @ `04e820c71461`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 199 · with blockers: 178

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 138 | 54 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 67 | 67 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 43 | 35 | non-working Rust | unknown class field `X` |
| 25 | 7 | non-working Rust | subscript access requires a list, set, dict, tuple, or string |
| 14 | 8 | non-working Rust | nested closure bodies need a single return expression |
| 12 | 10 | non-working Rust | class 'X': multiple inheritance is not supported |
| 7 | 5 | non-working Rust | function 'X' must have an explicit return type annotation |
| 6 | 4 | non-working Rust | nested closure parameters must have explicit type annotations |
| 6 | 4 | non-working Rust | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 5 | 4 | non-working Rust | lambda expressions need an expected `X` type from their context (annotate the assignment t |
| 4 | 4 | non-working Rust | pytest.mark.parametrize names must be a string literal |
| 4 | 1 | non-working Rust | pytest.mark.parametrize rows must be a list or tuple literal |
| 3 | 1 | non-working Rust | closure keyword arguments require a **kwargs parameter |
| 2 | 2 | non-working Rust | attribute access is only supported on class instances |

## result

- Source: `rustedpy/result` @ `0b855e1e38a0`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 5 · with blockers: 5

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 6 | 3 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 4 | 2 | non-working Rust | exception class 'X': only a `X`, `X` or docstring body is supported (no fields or methods) |
| 4 | 2 | non-working Rust | function 'X' must have an explicit return type annotation |
| 2 | 2 | non-working Rust | unsupported expression: yield |
| 2 | 2 | non-working Rust | nested closure bodies need a single return expression |
| 2 | 2 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |

## more-itertools

- Source: `more-itertools/more-itertools` @ `5d946b3590bf`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 6 · with blockers: 5

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 53 | 2 | non-working Rust | unknown class field `X` |
| 30 | 2 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 8 | 1 | non-working Rust | parameter 'X' in 'X' must have a type annotation |
| 3 | 3 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 3 | 1 | non-working Rust | nested closure bodies need a single return expression |
| 3 | 1 | non-working Rust | lambda expressions need an expected `X` type from their context (annotate the assignment t |
| 2 | 2 | non-working Rust | function 'X' must have an explicit return type annotation |
| 2 | 1 | non-working Rust | class 'X': multiple inheritance is not supported |
| 2 | 2 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 2 | 1 | non-working Rust | integer literal out of i64 range |
| 1 | 1 | non-working Rust | class 'X': class-level assignment must be a single name bound to a literal |
| 1 | 1 | non-working Rust | nested closure parameters must have explicit type annotations |
| 1 | 1 | non-working Rust | method 'X' must have an explicit return type annotation |
| 1 | 1 | non-working Rust | list(value) currently requires a list, set, dict, or homogeneous tuple value |

## funcy

- Source: `Suor/funcy` @ `9eb04473e31b`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 33 · with blockers: 32

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 138 | 13 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 64 | 20 | non-working Rust | function 'X' must have an explicit return type annotation |
| 31 | 16 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 24 | 5 | non-working Rust | nested closure parameters must have explicit type annotations |
| 11 | 3 | non-working Rust | nested class definitions are not yet supported |
| 4 | 3 | non-working Rust | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 3 | 1 | non-working Rust | nested closure bodies need a single return expression |
| 2 | 1 | non-working Rust | comprehension targets must be a single name (destructuring is not supported) |
| 2 | 1 | non-working Rust | all() and any() argument must be a bool list |
| 1 | 1 | non-working Rust | all() and any() currently support exactly one bool list argument |
| 1 | 1 | non-working Rust | lambda expressions need an expected `X` type from their context (annotate the assignment t |

## toolz

- Source: `pytoolz/toolz` @ `568c2b839397`
- Transpile: **no** — `smelt build` aborts at `toolz/itertoolz.py`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 31 · with blockers: 29

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 118 | 13 | non-working Rust | function 'X' must have an explicit return type annotation |
| 87 | 15 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 43 | 8 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 25 | 4 | non-working Rust | nested closure parameters must have explicit type annotations |
| 7 | 4 | non-working Rust | nested class definitions are not yet supported |
| 6 | 4 | non-working Rust | parameter 'X' in 'X' must have a type annotation |
| 3 | 3 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 3 | 1 | non-working Rust | only `X` deletion is supported |
| 2 | 1 | non-working Rust | class 'X': class-level assignment must be a single name bound to a literal |
| 2 | 2 | non-working Rust | lambda expressions need an expected `X` type from their context (annotate the assignment t |
| 1 | 1 | non-working Rust | unknown class field `X` |
| 1 | 1 | non-working Rust | nested closure bodies need a single return expression |
| 1 | 1 | non-working Rust | binary operator 'X' is not supported |

## cachetools

- Source: `tkem/cachetools` @ `340c474a11bd`
- Transpile: **no** — `smelt build` aborts at `src/cachetools/_cached.py`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 20 · with blockers: 20

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 30 | 4 | non-working Rust | function 'X' must have an explicit return type annotation |
| 14 | 14 | non-working Rust | parameter 'X' in 'X' must have a type annotation |
| 11 | 11 | non-working Rust | only calls to top-level functions, class constructors, and print() are supported |
| 2 | 2 | non-working Rust | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 2 | 2 | non-working Rust | unknown class field `X` |
| 1 | 1 | non-working Rust | parameter 'X' must have an explicit type annotation |
| 1 | 1 | non-working Rust | class 'X': decorator 'X' is not supported |

## Highest-leverage transpiler gaps (non-working Rust)

Lowering gaps blocking more than one probed library; fixing these unlocks the most surface.

| Libraries hit | Total occ. | Blocker class |
| ---: | ---: | --- |
| 6 (cachetools, funcy, more-itertools, result, returns, toolz) | 410 | only calls to top-level functions, class constructors, and print() are supported |
| 6 (cachetools, funcy, more-itertools, result, returns, toolz) | 225 | function 'X' must have an explicit return type annotation |
| 5 (cachetools, funcy, more-itertools, returns, toolz) | 78 | parameter 'X' must have an explicit type annotation |
| 5 (cachetools, more-itertools, result, returns, toolz) | 77 | definition-time metaprogramming for 'X' requires host-runtime specialization |
| 5 (funcy, more-itertools, result, returns, toolz) | 23 | nested closure bodies need a single return expression |
| 4 (cachetools, more-itertools, returns, toolz) | 99 | unknown class field `X` |
| 4 (funcy, more-itertools, returns, toolz) | 56 | nested closure parameters must have explicit type annotations |
| 4 (cachetools, more-itertools, returns, toolz) | 29 | parameter 'X' in 'X' must have a type annotation |
| 4 (funcy, more-itertools, returns, toolz) | 11 | lambda expressions need an expected `X` type from their context (annotate the assignment target or c |
| 3 (cachetools, more-itertools, returns) | 3 | class 'X': decorator 'X' is not supported |
| 2 (funcy, toolz) | 18 | nested class definitions are not yet supported |
| 2 (more-itertools, returns) | 14 | class 'X': multiple inheritance is not supported |
| 2 (funcy, returns) | 10 | pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals |
| 2 (result, returns) | 5 | exception class 'X': only a `X`, `X` or docstring body is supported (no fields or methods) |
| 2 (more-itertools, toolz) | 3 | class 'X': class-level assignment must be a single name bound to a literal |
| 2 (more-itertools, returns) | 2 | unsupported expression: starred |
| 2 (more-itertools, toolz) | 2 | binary operator 'X' is not supported |

