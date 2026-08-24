# SMELT

> Smelt your TypeScript and Python into Rust.

[![crates.io](https://img.shields.io/crates/v/smelt-transpiler.svg)](https://crates.io/crates/smelt-transpiler)
[![license](https://img.shields.io/crates/l/smelt-transpiler.svg)](LICENSE)

Install the `smelt` CLI from crates.io:

```sh
cargo install --locked smelt-transpiler
```

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

## Installation

Install the `smelt` CLI from the **`smelt-transpiler`** package on crates.io:

```sh
cargo install --locked smelt-transpiler
smelt --help
```

The default installation includes the TypeScript and Python frontends, with ty
support enabled for Python.

Publishing to crates.io is handled by [`scripts/publish-crates.sh`](scripts/publish-crates.sh)
(`--execute` to publish for real; a bare run does a dry run). It publishes the
crates leaf-first, including `smelt-py-types` before the Python frontend.
Maintainers can also run the manual **Publish crates.io release** workflow after
committing the new workspace version. The `crates-io` GitHub environment must
provide a `CRATES_IO_TOKEN` secret; the dispatch requires the matching version
and the confirmation text `publish`.

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

## Documentation

- [`docs/metaprogramming.md`](docs/metaprogramming.md) — what metaprogramming
  Smelt can specialize (metaclasses, decorators, descriptors, dataclasses, …),
  what fails loud, and what is rejected by design.
- [`docs/host-runtime-specialization.md`](docs/host-runtime-specialization.md) —
  how the sandboxed build-time partial evaluator works.

## Repository Layout

```
crates/
  smelt-frontend-ts/    TypeScript parser → HIR
  smelt-frontend-py/    Python parser → HIR
  smelt-hir/            High-level IR types and validation
  smelt-mir/            Mid-level IR (SSA-ish, ownership-naive)
  smelt-codegen-rust/   MIR → Rust source
  smelt-runtime/        Runtime helpers that transpiled code depends on
  smelt-transpiler/     The `smelt` binary
specs/                  Design documents
examples/               Example projects (Express demo, FastAPI demo)
```

## License

[GNU General Public License v3.0](LICENSE)

## AI use

This project leans heavily on AI, and that now includes the code. A large
portion of the source is AI-generated — as are commit messages, issues, and
documentation (READMEs, design docs, comments). AI-generated PRs are welcome as
long as they stay reasonably sized and make sense. Human review and the test
suite are the quality gate: anything that lands has to build, pass
`cargo test`/`clippy`, and be understandable.

## Contributing

Fork the repo and submit a pr :)

<!-- COVERAGE:START -->
## Coverage

![Coverage](coverage.svg)

### Workspace

| Metric | Coverage |
| --- | ---: |
| Functions | 81.69% |
| Lines | 76.65% |
| Regions | 74.93% |
| Branches | 64.90% |

### Per Crate

| Crate | Functions | Lines | Branches |
| --- | ---: | ---: | ---: |
| `smelt-asyncio` | 100.00% | 90.91% | 0.00% |
| `smelt-codegen-rust` | 85.04% | 80.23% | 62.79% |
| `smelt-frontend-py` | 79.61% | 76.05% | 68.28% |
| `smelt-frontend-ts` | 84.00% | 76.82% | 65.86% |
| `smelt-hir` | 69.29% | 38.49% | 59.44% |
| `smelt-mir` | 77.58% | 76.57% | 69.46% |
| `smelt-py-ty-spike` | 0.00% | 0.00% | 0.00% |
| `smelt-py-types` | 84.21% | 91.95% | 70.37% |
| `smelt-runtime` | 98.48% | 99.71% | 90.00% |
| `smelt-specialize` | 64.99% | 75.24% | 57.76% |
| `smelt-stdlib` | 84.13% | 74.54% | 100.00% |
| `smelt-test` | 90.52% | 90.25% | 61.36% |
| `smelt-transpiler` | 75.12% | 74.60% | 59.38% |
<!-- COVERAGE:END -->

































































































































































































