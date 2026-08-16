# TypeScript End-to-End Examples

Each example shows the currently supported path:

```text
TypeScript input -> HIR -> optimized MIR -> Rust source
```

Files:

- `input.ts`: source program.
- `expected.hir`: `smelt dump-hir input.ts` output.
- `expected.mir`: `smelt dump-mir input.ts` output.
- `expected.rs`: generated `src/main.rs` body for `smelt build`.
- `expected.stdout`: expected runtime output for examples that print.

All four are golden-checked by `end_to_end_examples_match_expected_outputs` in
`crates/smelt-transpiler/tests/hir_cli_cross_language_tests.rs`. Regenerate a
golden after an intended change with the paths exactly as written above — the
module header in the dump contains the path it was invoked with:

```sh
cargo run --bin smelt -- dump-hir examples/typescript/end-to-end/01_number/input.ts \
  > examples/typescript/end-to-end/01_number/expected.hir
```

