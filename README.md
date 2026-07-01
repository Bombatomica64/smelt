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
| Functions | 75.00% |
| Lines | 71.05% |
| Regions | 69.88% |
| Branches | 60.78% |

### Per Crate

| Crate | Functions | Lines | Branches |
| --- | ---: | ---: | ---: |
| `smelt-asyncio` | 100.00% | 90.91% | 0.00% |
| `smelt-cli` | 65.03% | 61.96% | 53.07% |
| `smelt-codegen-rust` | 80.75% | 74.53% | 56.11% |
| `smelt-frontend-py` | 78.03% | 73.70% | 65.61% |
| `smelt-frontend-ts` | 80.19% | 73.65% | 63.75% |
| `smelt-gui` | 13.89% | 13.72% | 81.25% |
| `smelt-hir` | 55.96% | 34.99% | 50.58% |
| `smelt-mir` | 79.39% | 74.71% | 66.19% |
| `smelt-py-ty-spike` | 0.00% | 0.00% | 0.00% |
| `smelt-specialize` | 44.06% | 48.52% | 36.47% |
| `smelt-stdlib` | 84.85% | 74.24% | 100.00% |
| `smelt-test` | 90.52% | 90.25% | 61.36% |
<!-- COVERAGE:END -->









































































































