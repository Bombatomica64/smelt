# Bug-library transpile probes (TypeScript + Python)

_Generated 2026-08-13 by the `library-probes` workflow (`scripts/probe_libraries.py`)._

Each library is checked out at a pinned ref (see `.github/compat/libraries.json`), given its `.github/compat/<name>/Smelt.toml`, and run through `smelt build`. If a crate is emitted, its generated `cargo test` suite is run and counted. Otherwise every source/test file is scanned individually with `smelt dump-hir` to enumerate the full set of distinct blocker classes (single-file mode cannot resolve cross-file imports, so bare `unresolved name/identifier` errors are excluded as scan noise).

**Error categories:**
- **missing-stdlib** — a JS/Python builtin Smelt does not model yet (`Array`, `Number`, `Reflect`, `TextEncoder`, `Proxy`, ...).
- **non-working Rust** — a frontend/IR lowering gap: Smelt cannot lower the construct into Rust that compiles (missing return-type annotations, `try`/`except`, decorators, callback methods not lowered into closures, non-primitive exported `const`, ...).

## Summary

| Library | Lang | Transpile | Tests (pass/fail) | First abort | Blocker classes | Dominant |
| --- | --- | --- | --- | --- | ---: | --- |
| [es-toolkit](https://github.com/toss/es-toolkit) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [radash](https://github.com/sodiray/radash) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [ts-pattern](https://github.com/gvergnaud/ts-pattern) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [valibot](https://github.com/fabian-hiller/valibot) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [neverthrow](https://github.com/supermacro/neverthrow) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [immer](https://github.com/immerjs/immer) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [rxjs](https://github.com/ReactiveX/rxjs) | TS | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [returns](https://github.com/dry-python/returns) | PY | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [result](https://github.com/rustedpy/result) | PY | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [more-itertools](https://github.com/more-itertools/more-itertools) | PY | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [funcy](https://github.com/Suor/funcy) | PY | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |
| [toolz](https://github.com/pytoolz/toolz) | PY | **no** | n/a | `(unknown)` | 0 | non-working Rust (0r/0s) |

## es-toolkit

- Source: `toss/es-toolkit` @ `e008a2818cd8`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## radash

- Source: `sodiray/radash` @ `4cab1900d08e`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## ts-pattern

- Source: `gvergnaud/ts-pattern` @ `c92ca435c7e1`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## valibot

- Source: `fabian-hiller/valibot` @ `1f9b18338ad5`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## neverthrow

- Source: `supermacro/neverthrow` @ `5ef3a018bda7`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## immer

- Source: `immerjs/immer` @ `a3be9df762c1`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## rxjs

- Source: `ReactiveX/rxjs` @ `c15b37f81ba5`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## returns

- Source: `dry-python/returns` @ `04e820c71461`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## result

- Source: `rustedpy/result` @ `0b855e1e38a0`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## more-itertools

- Source: `more-itertools/more-itertools` @ `5d946b3590bf`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## funcy

- Source: `Suor/funcy` @ `9eb04473e31b`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

## toolz

- Source: `pytoolz/toolz` @ `568c2b839397`
- Transpile: **no** — `smelt build` aborts at `(unknown)`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 0 · with blockers: 0

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |

