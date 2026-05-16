# M0: Project Setup & End-to-End Spike

**Milestone:** v1.0
**Estimated duration:** 2–3 weeks

## Goal

Stand up the repository, choose dependencies, and prove the toolchain works end-to-end with a throwaway prototype that handles the simplest possible input.

## Why this matters

Before any real design work, we want a working pipeline — even a stupid one — so that every subsequent milestone is "make this part better" rather than "build this part from scratch and hope it integrates." This catches integration problems early and gives us a CI baseline.

## Scope

- Cargo workspace with all the v1.0 crates created as empty stubs:
  - `smelt-frontend-ts`
  - `smelt-frontend-py`
  - `smelt-hir`
  - `smelt-mir`
  - `smelt-codegen-rust`
  - `smelt-runtime`
  - `smelt-cli`
- A spike implementation that takes a single hardcoded TypeScript expression — `const x: number = 1 + 2;` — and emits a Rust file containing `let x: f64 = 1.0 + 2.0;` that compiles under `cargo build`.
- The spike does **not** need to use the real HIR or MIR. It can be a string-to-string hack. Its only job is to prove the directory layout works and the CLI can shell out to `cargo`.
- CI configured (`cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`).
- Basic README pointing at the specs.

## Exit Criteria

- [ ] `cargo test` passes on a fresh clone.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] Running the CLI on the hardcoded input produces a Rust file that `cargo build`s.
- [ ] CI runs on every push and PR.
- [ ] README links to the architecture spec, HIR spec, and config spec.

## Out of Scope

- Real parsing.
- Real HIR or MIR.
- Configuration files (M0 takes input via hardcoded string).
- Anything Python-related.

## Notes

The spike code can be deleted in M1 once the real frontend lands. Don't get attached to it.
