# SMELT

> Smelt your TypeScript and Python into Rust.

**smelt** is a transpiler that takes strictly-typed TypeScript and Python source code and compiles it down to idiomatic Rust. The goal is not to transpile *all* TS/Python — it is to transpile the statically-typed subset where types actually mean something.

## Why

TypeScript and Python both have rich, expressive type systems that are mostly used for editor tooling and runtime validation. smelt takes those types seriously: if your code is fully typed and passes strict checks, smelt can lower it through a shared intermediate representation and emit Rust that compiles, runs, and is reasonably idiomatic.

The long-term vision is **language-interchangeable modules**: a Python file importing from a TypeScript file, both lowering to the same HIR, both becoming the same Rust crate.

## Status

Pre-alpha. See the milestones in github issues for the v1.0 roadmap.

Current external test milestone: Smelt can compile the focused
`date-fns/date-fns` `quartersToMonths` slice, including its Vitest test file,
into a generated Rust crate whose `cargo test` run passes. The probed files are:

- `src/constants/index.ts`
- `src/quartersToMonths/index.ts`
- `src/quartersToMonths/test.ts`

This is the first real third-party TypeScript test slice where source-language
tests lower into native Rust `#[test]` functions and pass under Cargo.

## v1.0 Goals

- A real Express app (TypeScript, strict mode) compiles to a working `axum` server.
- A real FastAPI app (Python, fully typed, passes `ty`) compiles to a working `axum` server.
- Both produce structurally similar Rust output, validating the shared HIR design.
- Configuration via `Smelt.toml`, CLI modeled after `cargo`.

## Non-Goals (for v1.0)

- LSP / editor integration.
- Ownership/borrow inference (v1.0 clones aggressively).
- Supporting dynamic features (`eval`, `getattr`, monkey-patching, `any`).
- Multiple Rust web framework backends. v1.0 targets `axum` only.
- Incremental compilation.

## Architecture at a Glance

```
TypeScript ──┐                                      ┌── Rust source
             ├──> Frontend ──> HIR ──> MIR ──> Codegen
Python ──────┘                                      └── Cargo.toml (generated)
```

See `specs/architecture.md` for the full picture.

## Repository Layout

```
crates/
  smelt-frontend-ts/    TypeScript parser → HIR
  smelt-frontend-py/    Python parser → HIR
  smelt-hir/            High-level IR types and validation
  smelt-mir/            Mid-level IR (SSA-ish, ownership-naive)
  smelt-codegen-rust/   MIR → Rust source
  smelt-runtime/        Runtime helpers that transpiled code depends on
  smelt-cli/            The `smelt` binary
specs/                  Design documents
examples/               Example projects (Express demo, FastAPI demo)
```

## License

[GNU General Public License v3.0](LICENSE)

## AI use

This project will use AI tool for everything but code gen. PRs that are AI generated will be accepted if not too big and if they make sense.
Commit messages, issues, documentation (like readmes, design docs, comments) can be AI generated.

## Contributing

Fork the repo and submit a pr :)

<!-- COVERAGE:START -->
## Coverage

![Coverage](coverage.svg)

### Workspace

| Metric | Coverage |
| --- | ---: |
| Functions | 77.89% |
| Lines | 71.66% |
| Regions | 70.32% |
| Branches | 61.10% |

### Per Crate

| Crate | Functions | Lines | Branches |
| --- | ---: | ---: | ---: |
| `smelt-asyncio` | 100.00% | 90.91% | 0.00% |
| `smelt-cli` | 76.02% | 75.76% | 61.78% |
| `smelt-codegen-rust` | 80.86% | 74.40% | 54.55% |
| `smelt-frontend-py` | 78.54% | 74.24% | 65.89% |
| `smelt-frontend-ts` | 80.61% | 73.70% | 64.42% |
| `smelt-gui` | 13.89% | 13.72% | 81.25% |
| `smelt-hir` | 55.14% | 35.09% | 50.62% |
| `smelt-mir` | 77.46% | 67.64% | 58.11% |
| `smelt-stdlib` | 80.95% | 69.71% | 100.00% |
| `smelt-test` | 90.52% | 90.25% | 61.36% |
<!-- COVERAGE:END -->






















































