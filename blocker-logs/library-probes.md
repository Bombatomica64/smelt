# Bug-library transpile probes (TypeScript + Python)

_Generated 2026-11-17 by the `library-probes` workflow (`scripts/probe_libraries.py`)._

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
| [valibot](https://github.com/fabian-hiller/valibot) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [neverthrow](https://github.com/supermacro/neverthrow) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [immer](https://github.com/immerjs/immer) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [rxjs](https://github.com/ReactiveX/rxjs) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [date-fns](https://github.com/date-fns/date-fns) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [returns](https://github.com/dry-python/returns) | PY | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [result](https://github.com/rustedpy/result) | PY | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [more-itertools](https://github.com/more-itertools/more-itertools) | PY | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [funcy](https://github.com/Suor/funcy) | PY | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [toolz](https://github.com/pytoolz/toolz) | PY | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [cachetools](https://github.com/tkem/cachetools) | PY | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |

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
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 1083 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## neverthrow

- Source: `supermacro/neverthrow` @ `5ef3a018bda7`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 8 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## immer

- Source: `immerjs/immer` @ `a3be9df762c1`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 24 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## rxjs

- Source: `ReactiveX/rxjs` @ `c15b37f81ba5`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 376 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## date-fns

- Source: `date-fns/date-fns` @ `cd53d2538cfa`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 1493 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## returns

- Source: `dry-python/returns` @ `04e820c71461`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 199 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## result

- Source: `rustedpy/result` @ `0b855e1e38a0`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 5 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## more-itertools

- Source: `more-itertools/more-itertools` @ `5d946b3590bf`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 6 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## funcy

- Source: `Suor/funcy` @ `9eb04473e31b`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 33 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## toolz

- Source: `pytoolz/toolz` @ `568c2b839397`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 31 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## cachetools

- Source: `tkem/cachetools` @ `340c474a11bd`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 20 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

