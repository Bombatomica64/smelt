# M4: Rust Codegen — Synchronous Subset

**Milestone:** v1.0
**Estimated duration:** 4–6 weeks
**Depends on:** M3

## Goal

Walk MIR and emit Rust source code that compiles. Synchronous code only — async lands in M5.

## Why this matters

This is the first milestone where the full pipeline runs end-to-end with real inputs. Every test in this milestone compiles and runs the generated Rust, which means we have ground truth for correctness.

## Scope

- Implement `smelt-codegen-rust` as a MIR walker that emits Rust source as strings.
- Use `quote!` if it works ergonomically; otherwise hand-rolled string emission with a small indent helper. Decide early.
- Generate a `Cargo.toml` for the output crate based on what was used.
- Write the output to `Smelt.toml`'s configured `output.target` directory.
- Handle:
  - Function definitions, including parameters and return types
  - All MIR statement and terminator kinds
  - Type lowering: `Type::Int` → `i64`, `Type::Float` → `f64`, `Type::String` → `String`, `Type::List(T)` → `Vec<T>`, `Type::Dict(K, V)` → `HashMap<K, V>`, `Type::Optional(T)` → `Option<T>`, etc.
  - Class lowering: emit a `struct` for fields and an `impl` block for methods.
  - `Result<T, E>` for functions that originally threw exceptions.
  - Iterator chains from comprehensions.

## Golden-File Tests

This milestone introduces golden-file testing as the primary correctness mechanism:

- 20+ small TS programs in `tests/golden/`.
- Each has an expected Rust output checked into the repo.
- Each generated Rust file is compiled with `cargo build` as part of the test.
- A subset have an `expected_stdout.txt`; the test compiles, runs, and diffs stdout.

When the codegen output changes, the golden files must be updated explicitly via `cargo test -- --update-goldens` or similar. No accidental updates.

## Exit Criteria

- [ ] End-to-end pipeline works: a sync TS file → MIR → Rust source → `cargo build` → working binary.
- [ ] 20+ golden tests passing.
- [ ] At least 5 golden tests verify runtime output, not just compilation.
- [ ] Generated `Cargo.toml` is valid and uses the dependencies declared in `Smelt.toml` plus any auto-detected ones.
- [ ] Generated Rust passes `cargo clippy` with default warnings.
- [ ] Generated Rust is `rustfmt`-formatted.

## Out of Scope

- Async / Futures (M5).
- HTTP frameworks (M7).
- Stdlib mapping for non-trivial methods (M6).
- Optimizing the generated code (v2.0).

## Notes on Code Quality

The generated Rust will not be idiomatic in v1.0. It will clone everywhere and use `String` for everything. That's fine. The goal is *correct* and *compilable*, not pretty. Resist the urge to optimize during this milestone — every optimization is a place to introduce bugs, and we have no test coverage for borrow correctness yet.
