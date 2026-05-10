# Architecture

This document describes the high-level architecture of smelt: the compilation pipeline, the role of each crate, and the design principles that guide them.

## Pipeline Overview

```
Source (.ts / .py)
      │
      ▼
┌──────────────┐
│   Frontend   │   tree-sitter parse + ty/tsgo type info
│ (per-lang)   │   normalize into shared HIR
└──────┬───────┘
       │
       ▼
┌──────────────┐
│     HIR      │   typed, language-agnostic, classes/exceptions intact
│ (validation) │
└──────┬───────┘
       │  lowering passes:
       │   - exception → Result
       │   - closure capture explicitness
       │   - desugar comprehensions / generators
       │   - resolve cross-language imports
       ▼
┌──────────────┐
│     MIR      │   SSA-ish, basic blocks, naive ownership (clone everything)
└──────┬───────┘
       │
       ▼
┌──────────────┐
│   Codegen    │   emit Rust source + generated Cargo.toml
└──────┬───────┘
       │
       ▼
   Rust crate ──> cargo build
```

Tests follow the same philosophy. Source-language tests should eventually lower into Rust `#[test]` functions and run through `cargo test`; see `specs/testing-strategy.md`.

## Design Principles

**Two IRs, not one.** HIR keeps high-level constructs (classes, methods, exceptions, comprehensions) so frontend lowering stays simple. MIR is closer to Rust's mental model so codegen stays simple. Lowering passes between them are where the interesting work happens.

**Strict-only input.** smelt refuses to transpile code that isn't fully typed. TypeScript must pass `strict: true` with no `any`. Python must pass `ty` strict mode. This is non-negotiable — it's the project's whole identity.

**Clone first, optimize later.** v1.0 emits Rust where every value is `Clone` and every binding owns its data. No `&str`, no lifetimes, no borrows. This is intentionally suboptimal Rust — it's correct, it compiles, and it gives us a baseline to optimize against later. Ownership inference is a v2.0 problem.

**One Rust target per concept.** v1.0 picks one Rust library per feature: `axum` for HTTP, `tokio` for async, `serde_json` for JSON, `reqwest` for HTTP clients. No backend abstraction layer in v1.0.

**Shared HIR across frontends.** This is what enables the long-term cross-language interop story. A TS module and a Python module produce the same HIR shape, so an import resolver can connect them after lowering without caring which language produced which side.

## Crate Responsibilities

### `smelt-frontend-ts`

- Runs the check pipeline: oxclint → tsgo --noEmit → smelt rules → HIR construction. See `specs/check-pipeline.md`.
- Walks the type-annotated AST and produces HIR nodes.
- Rejects unsupported constructs with clear, source-located errors.

### `smelt-frontend-py`

- Parses Python via `tree-sitter-python`.
- Embeds or shells out to `ty` for type-checking and inferred types.
- Walks the AST and produces HIR nodes — the **same** HIR types as the TS frontend.
- Rejects untyped or dynamic code.

### `smelt-hir`

- Defines the HIR data types (modules, items, expressions, types).
- Provides a validator that catches malformed HIR (untyped nodes, dangling references).
- Provides a pretty-printer for debugging (`smelt dump-hir`).
- Serde-serializable for snapshot tests.

### `smelt-mir`

- Defines the MIR data types (functions, basic blocks, statements, terminators).
- Implements lowering passes from HIR to MIR.
- Each lowering pass is its own module so they can be unit-tested independently.

### `smelt-codegen-rust`

- Walks MIR and emits Rust source as strings (via a small AST writer or `quote!`-style).
- Generates a `Cargo.toml` for the output crate based on detected dependencies.
- Maps stdlib calls (e.g. `Array.map` → iterator chain) via a stdlib-mapping module.

### `smelt-runtime`

- A small Rust crate that transpiled code depends on at runtime.
- Provides helpers for things that don't have a direct Rust equivalent and need glue (e.g. JS-style coercions, Python-ish dict helpers if needed).
- Kept as small as possible — anything that can be lowered inline should be.

### `smelt-test`

- A small Rust crate for generated tests, added when `smelt test` lands.
- Provides assertion helpers and limited pytest/Vitest/Jest compatibility glue.
- Does not embed pytest, Vitest, Node, or CPython; frontends lower supported test APIs into native Rust tests.

### `smelt-cli`

- Reads `Smelt.toml`.
- Discovers source files.
- Drives the pipeline.
- Surfaces errors with source locations in `file:line:col` format.
- Eventually drives `smelt test` by emitting Rust tests and invoking `cargo test`.

## Cross-Language Imports

This is a v1.x feature, not v1.0, but the architecture must not preclude it.

The HIR module graph is language-agnostic. When the resolver encounters an `import` from a Python file in a TypeScript file (or vice versa), it:

1. Looks up the target file by path.
2. Asks the appropriate frontend to produce HIR for it.
3. Resolves the symbol against the produced HIR's exports.
4. Normalizes naming (camelCase ↔ snake_case) at the import boundary; canonical form in HIR is snake_case.

After this point, lowering proceeds as if it had been one project all along.

## Error Reporting

All errors carry source spans. Errors are formatted as:

```
path/to/file.ts:12:5: error: unsupported construct: dynamic property access
```

This format is recognized by every editor's "click to jump to error" feature, which gives us 60% of LSP value for 1% of the cost. No actual LSP is planned for v1.0.

## What's Deliberately Not Here

- Ownership and borrow inference (v2.0).
- Generic monomorphization beyond trivial cases (v2.0).
- Multiple backend targets (v2.0+).
- Incremental compilation (v2.0+).
- LSP (not planned).
- Source maps for runtime debugging (v2.0+).
