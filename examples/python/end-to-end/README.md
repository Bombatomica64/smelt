# Python End-to-End Examples

Each example shows the currently supported path:

```text
Python input -> HIR -> optimized MIR
```

Files:

- `input.py`: source program.
- `Smelt.toml`: manifest for building the example on its own.
- `expected.hir`: `smelt dump-hir input.py` output.
- `expected.mir`: `smelt dump-mir input.py` output.

The two goldens are checked by `python_end_to_end_examples_match_expected_dumps`
in `crates/smelt-transpiler/tests/hir_cli_cross_language_tests.rs`. Regenerate
one after an intended change with the path exactly as written below — the module
header in the dump contains the path it was invoked with:

```sh
cargo run --bin smelt -- dump-hir examples/python/end-to-end/01_number/input.py \
  > examples/python/end-to-end/01_number/expected.hir
```

Unlike the TypeScript corpus these cases have no `expected.rs` or
`expected.stdout` tier yet, so generated-Rust and runtime behaviour for Python
is still unasserted here. Adding those means a `smelt build` plus a `cargo run`
per case, the way `verify_end_to_end_example` does it for TypeScript.
