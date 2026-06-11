# CallbackExpr Audit

`CallbackExpr` is now a frontend-internal legacy expression tree. It must not
be used as callback-body storage for new HIR or MIR features.

## Persisted Uses

None. Removal step 1 is complete: `ExprKind::ListSort.comparator` and
`Rvalue::ListSort.comparator` carry a normal closure (`ExprId`/`Operand`) like
every other callback, and the Rust codegen `callback_expr_text` shadow renderer
plus its metadata scans have been deleted. `smelt-mir` and `smelt-codegen-rust`
contain zero `CallbackExpr` references.

## Transient Uses

- The TypeScript frontend can build `CallbackExpr` while converting callback
  expressions into normal closure CFG bodies.
- The Python frontend uses the same bridge for list-style callback lowering.
- These frontend uses are migration scaffolding, not a license to persist new
  callback trees in HIR or MIR.

## Boundary

- Do not add new HIR or MIR fields of type `CallbackExpr`.
- Do not render callback bodies through bespoke expression-tree emitters; all
  callback behavior lowers through `ClosureExpr` bodies.

## Removal Path

1. ~~Lower sort comparators into normal closure CFG values and store the
   comparator as the same closure/operand form used by other callbacks.~~
   Done: comparators are closure operands end to end and the Rust expression
   renderer is deleted.
2. Replace TypeScript and Python frontend `CallbackExpr` builders with direct
   closure-body builders (the `callback_expr_to_closure` bridge in
   `builder_part13.rs` and `frontend-py/src/lowering/list.rs` is the remaining
   transient use).
3. Delete `CallbackExpr`, `CallbackCallArg`, `CallbackExprKind`, and their
   remaining HIR validation traversals.
