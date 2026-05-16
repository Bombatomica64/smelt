# M2: HIR Definition & Validation

**Milestone:** v1.0
**Estimated duration:** 3–4 weeks (overlaps with M1)
**Depends on:** M0

## Goal

Define the HIR data structures, ship a validator, ship a pretty-printer, and document the HIR design.

## Why this matters

HIR is the contract between the frontends (M1, M8) and everything downstream (M3+). Getting it stable early prevents thrash. Validation catches frontend bugs immediately rather than letting them propagate to mysterious crashes in MIR or codegen.

## Scope

- Define all HIR types in `smelt-hir`:
  - `Module`, `Item`, `Function`, `Class`, `TypeAlias`, `ConstItem`
  - `Type` enum (primitives, composites, user-defined, async)
  - `Expr` with resolved type on every node
  - `Stmt`, `Block`, `MatchArm`, `ExceptionHandler`
  - `VarId`, `Symbol`, `ClassRef`, `TypeVarId`
- Implement `serde::Serialize`/`Deserialize` for snapshot tests.
- Implement a pretty-printer (`smelt-hir::pretty::print(&module)`) that produces a human-readable text dump for debugging.
- Implement a validator pass that checks:
  - Every `Expr` has a non-`None` resolved type
  - Every `VarId` is defined in scope
  - Every `ClassRef` resolves
  - `Await` only appears inside `is_async: true` functions
  - No `Type::Union` is empty
  - Match arms cover all variants of a discriminated union
- Document the HIR in `specs/hir.md` (already drafted; finalize during implementation).
- Wire the validator into the pipeline so M1 snapshot tests automatically validate.

## Open Questions to Resolve

- Should `Symbol` be globally interned or per-module?
- How are decorators represented? (Recommendation: lower in the frontend, never enter HIR.)
- How are Python `with` blocks represented? (Recommendation: desugar to try/finally before HIR.)
- Are `TypeVar`s monomorphized in HIR or kept symbolic? (Recommendation: keep symbolic; monomorphize during HIR → MIR lowering.)

Each open question gets its own sub-issue and a decision recorded in the spec.

## Exit Criteria

- [ ] All HIR types defined and documented with rustdoc.
- [ ] Serde round-trip works for every type.
- [ ] Pretty-printer produces stable output (golden tests).
- [ ] Validator catches every documented invariant violation in unit tests.
- [ ] M1 snapshot tests run the validator and pass.
- [ ] `specs/hir.md` is finalized with all open questions resolved.

## Out of Scope

- Lowering passes (M3).
- MIR types (M3).
- Optimization or arena allocation (v2.0+).
