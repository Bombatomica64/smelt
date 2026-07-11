# Bug-library transpile probes (TypeScript + Python)

_Generated 2026-07-11 by the `library-probes` workflow (`scripts/probe_libraries.py`)._

Each library is checked out at a pinned ref (see `.github/compat/libraries.json`), given its `.github/compat/<name>/Smelt.toml`, and run through `smelt build`. If a crate is emitted, its generated `cargo test` suite is run and counted. Otherwise every source/test file is scanned individually with `smelt dump-hir` to enumerate the full set of distinct blocker classes (single-file mode cannot resolve cross-file imports, so bare `unresolved name/identifier` errors are excluded as scan noise).

**Error categories:**
- **missing-stdlib** — a JS/Python builtin Smelt does not model yet (`Array`, `Number`, `Reflect`, `TextEncoder`, `Proxy`, ...).
- **non-working Rust** — a frontend/IR lowering gap: Smelt cannot lower the construct into Rust that compiles (missing return-type annotations, `try`/`except`, decorators, callback methods not lowered into closures, non-primitive exported `const`, ...).

## Summary

| Library | Lang | Transpile | Tests (pass/fail) | First abort | Blocker classes | Dominant |
| --- | --- | --- | --- | --- | ---: | --- |
| [radash](https://github.com/sodiray/radash) | TS | **no** | n/a | `home/lollo/Playground/smelt/src/async.ts` | 7 | non-working Rust (7r/0s) |

## radash

- Source: `sodiray/radash` @ `4cab1900d08e`
- Transpile: **no** — `smelt build` aborts at `home/lollo/Playground/smelt/src/async.ts`
- Tests passing: **n/a** (no Rust crate emitted)
- Files scanned: 19 · with blockers: 6

| Occurrences | Files | Category | Blocker class |
| ---: | ---: | --- | --- |
| 1 | 1 | non-working Rust | local `X` is not callable (Some(None)) |
| 1 | 1 | non-working Rust | dynamic Date constructor calls require exactly one value argument |
| 1 | 1 | non-working Rust | local closure return type does not match its annotation: actual Some(List(TypeId(0))), exp |
| 1 | 1 | non-working Rust | array reduce callback returns an unsupported type |
| 1 | 1 | non-working Rust | local `X` is not callable (Some(Dict(TypeId(7), TypeId(0)))) |
| 1 | 1 | non-working Rust | field access is only lowered for Record<string, T>, class, and interface values for now (r |
| 1 | 1 | non-working Rust | expression kind is not lowered yet: SequenceExpression(SequenceExpression { span: Span { s |

