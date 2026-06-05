# CallbackExpr Audit

`CallbackExpr` is now a narrow legacy expression tree. It must not be used as
callback-body storage for new HIR or MIR features.

## Persisted Uses

- HIR `ExprKind::ListSort.comparator` stores the remaining JavaScript
  `Array.prototype.sort` comparator tree.
- MIR `Rvalue::ListSort.comparator` carries that same comparator to Rust
  codegen.
- Rust codegen renders sort comparators through `callback_expr_text` and scans
  those trees for metadata needs such as Regex support.
- Rust literal metadata may still inspect `CallbackExpr` trees to find captured
  assignments while the sort-comparator representation exists.

## Transient Uses

- The TypeScript frontend can build `CallbackExpr` while converting callback
  expressions into normal closure CFG bodies.
- The Python frontend uses the same bridge for list-style callback lowering.
- These frontend uses are migration scaffolding, not a license to persist new
  callback trees in HIR or MIR.

## Boundary

- Do not add new HIR or MIR fields of type `CallbackExpr`.
- Do not emit `map`, `filter`, `forEach`, `some`, `every`, `find`, `flatMap`,
  or related callback bodies through `callback_expr_text`.
- Keep new callback behavior on `ClosureExpr` bodies unless the path is the
  existing sort comparator escape hatch.

## Removal Path

1. Lower sort comparators into normal closure CFG values and store the
   comparator as the same closure/operand form used by other callbacks.
2. Replace TypeScript and Python frontend `CallbackExpr` builders with direct
   closure-body builders.
3. Delete `CallbackExpr`, `CallbackCallArg`, `CallbackExprKind`, their
   validation/metadata traversals, and the direct Rust expression renderer.
