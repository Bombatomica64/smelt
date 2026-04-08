# M3: HIR → MIR Lowering (Ownership-Naive)

**Milestone:** v1.0
**Estimated duration:** 6–8 weeks
**Depends on:** M2

## Goal

Define MIR and implement the HIR → MIR lowering pipeline. v1.0 MIR is intentionally naive: everything clones, no borrows, no lifetimes.

## Why this matters

MIR is where the language-translation work actually happens. HIR still looks like TS/Python in shape; MIR looks like Rust. Lowering passes are where we desugar exceptions into `Result`, make closure captures explicit, and turn comprehensions into iterator chains. Keeping these as separate passes between two distinct IRs is what saves us in year 2 when we add ownership inference.

## Scope

### MIR Definition

- Functions as control-flow graphs of basic blocks.
- SSA-ish form: every assignment introduces a fresh local.
- Statements are simple: assignments, calls, no nested expressions.
- Terminators: `Return`, `Branch`, `SwitchInt`, `Call`, `Unreachable`.
- Locals carry their type and (in v1.0) are always owned.
- No lifetime annotations; no borrow expressions.

### Lowering Passes

Each pass is a separate module with its own tests:

1. **Exception lowering.** `try`/`catch`/`throw` → functions returning `Result<T, E>`. Synthesize an error enum per function based on the throw types.
2. **Closure capture explicitness.** Walk lambda bodies, identify free variables, build an explicit capture list. v1.0: every captured value is cloned at capture time.
3. **Comprehension desugaring.** List/dict/set comprehensions and Python generator expressions → iterator chain MIR.
4. **Method call resolution.** Resolve `receiver.method(args)` against the class definition; turn it into a direct function call with `self` as first argument.
5. **Generic monomorphization (trivial cases only).** For each concrete instantiation of a generic function, emit a specialized MIR function. v1.0 supports only fully-resolved generic parameters; bounded generics over traits are deferred.
6. **Async lowering preparation.** Mark functions as async; the actual Future lowering happens in M5.

### Validation

A MIR validator analogous to the HIR validator:
- Every basic block ends in exactly one terminator.
- Every local is assigned before use.
- Every type referenced exists.
- No unreachable blocks (warn, don't error).

## Exit Criteria

- [ ] All MIR types defined and documented.
- [ ] All six lowering passes implemented with unit tests.
- [ ] HIR → MIR snapshot tests for every M1 snapshot input.
- [ ] MIR validator catches every documented invariant.
- [ ] MIR pretty-printer for debugging.
- [ ] `specs/mir.md` written.

## Out of Scope

- Codegen (M4).
- Async runtime details (M5).
- Borrow inference, lifetimes, anything beyond clone-everywhere (v2.0).
- Generic monomorphization beyond trivial cases.

## Notes

The phrase "SSA-ish" in this issue is deliberate. Strict SSA with phi nodes is overkill for v1.0 — we just want enough single-assignment discipline that future optimization passes have a chance. Decide on the exact form (basic-block arguments vs phi nodes vs neither) early in the milestone and document the choice.
