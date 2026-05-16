# TypeScript End-to-End Examples

Each example shows the currently supported path:

```text
TypeScript input -> HIR -> optimized MIR -> Rust source
```

Files:

- `input.ts`: source program.
- `expected.mir`: `smelt dump-mir input.ts` output.
- `expected.rs`: generated `src/main.rs` body for `smelt build`.
- `expected.stdout`: expected runtime output for examples that print.

