# TypeScript HIR Examples

These examples are meant for frontend, HIR, and early MIR testing.

The async files can be inspected through HIR and MIR:

```bash
cargo run -q -p smelt-cli -- dump-hir examples/typescript/hir/09_async_function.ts
cargo run -q -p smelt-cli -- dump-mir examples/typescript/hir/09_async_function.ts
cargo run -q -p smelt-cli -- dump-hir examples/typescript/hir/10_async_class_method.ts
cargo run -q -p smelt-cli -- dump-mir examples/typescript/hir/10_async_class_method.ts
```

Or run the bundled helper:

```bash
just try-async-hir
```
