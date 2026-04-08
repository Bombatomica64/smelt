# M1: TypeScript Frontend — Core Subset

**Milestone:** v1.0
**Estimated duration:** 6–8 weeks
**Depends on:** M0

## Goal

Parse a strict subset of TypeScript and produce HIR. The frontend rejects anything outside the supported subset with a clear, source-located error.

## Why this matters

This is the first real piece of the compiler. It also defines what "strictly typed TypeScript" means for the rest of the project — every later milestone assumes the frontend produces well-formed, fully-typed HIR.

## Supported Constructs

### Types
- Primitives: `number`, `string`, `boolean`, `null`, `undefined`, `void`
- Arrays: `T[]` and `Array<T>`
- Tuples: `[T, U, V]`
- Objects via `interface` declarations
- Unions limited to `T | null`, `T | undefined`, and discriminated unions with a literal tag field
- Generics on functions and classes (no conditional types, no mapped types)
- `Promise<T>`

### Expressions
- Literals (number, string, boolean, null)
- Variable references
- Binary and unary operators
- Function calls
- Method calls
- Property access (only on known interfaces)
- Array literals, object literals
- Arrow functions and function expressions
- `await`

### Statements
- `const`, `let` (no `var`)
- `if` / `else`
- `while`, `for`, `for...of`
- `return`
- `throw` and `try`/`catch`/`finally`
- Function and class declarations

### Classes
- Fields with type annotations
- Methods (sync and async)
- Constructors
- Generics on classes
- Single inheritance only (no mixins)

## Explicitly Rejected

- `any`, `unknown`, `never`
- Conditional types, mapped types, template literal types
- Index signatures
- `eval`, `Function` constructor, `with`
- Decorators (deferred to v1.1)
- Namespaces
- `var`
- JSX/TSX

## Implementation Notes

- Use `tree-sitter-typescript` for parsing. Consider `oxc` if richer semantic analysis is needed; decide early in the milestone.
- Strict-mode verification: shell out to `tsc --noEmit --strict` or embed equivalent checks. Decision and rationale documented in a sub-issue.
- Walker is implemented as a visitor that produces `smelt_hir::Module`.
- Naming: convert `camelCase` to `snake_case` at the boundary, preserve original names in a side table for diagnostics.

## Exit Criteria

- [ ] All supported constructs parse to valid HIR.
- [ ] All explicitly-rejected constructs produce errors with file:line:col locations and clear messages.
- [ ] 50+ snapshot tests covering the supported subset.
- [ ] 20+ negative tests asserting that rejected constructs error.
- [ ] HIR validator (from M2) passes on every snapshot test output.
- [ ] Performance: parsing a 500-line TS file completes in <500ms on a developer laptop.

## Out of Scope

- Lowering to MIR (M3).
- Code generation (M4).
- Anything async-runtime-specific (M5).
- Standard library mapping beyond what's needed for the snapshot tests (M6).
