# CallbackExpr Audit

Status: complete. `CallbackExpr`, `CallbackCallArg` and `CallbackExprKind` no
longer exist anywhere in the workspace (`grep -rn CallbackExpr crates` finds only
a historical comment in a test).

`CallbackExpr` was a private callback-body tree. It was removed in three steps:

1. Sort comparators became ordinary closure operands
   (`ExprKind::ListSort.comparator`, `Rvalue::ListSort.comparator`), and the Rust
   codegen `callback_expr_text` shadow renderer was deleted.
2. The TypeScript frontend stopped building it: a callback body lowers through
   `expression` into a normal closure CFG body, and a callback that only names a
   callable is a forwarding closure (`lowering/callbacks/forwarding.rs`).
3. The Python frontend's copy, `crates/smelt-frontend-py/src/lowering/callback_expr.rs`,
   was deleted in `d5376d51b` (2026-11-06): lambdas and single-return nested
   `def`s lower through expression lowering in their own closure body
   (`crates/smelt-frontend-py/src/lowering/closure.rs`).

## Boundary

- Do not reintroduce a callback-specific expression tree in HIR, MIR or a
  frontend: every callback lowers through a `ClosureExpr` body.
- Do not render callback bodies through bespoke expression-tree emitters.
