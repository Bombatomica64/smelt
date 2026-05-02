# Smelt Max TypeScript Before Python Checklist

## Status

- [x] Phase 0 baseline checked on current checkout.
- [x] Phase 1 TypeScript sync expression support.
- [ ] Phase 2 mutation and loops.
- [ ] Phase 3 classes, interfaces, constructors, methods.
- [ ] Phase 4 imports and multi-file TypeScript.
- [ ] Phase 5 async, await, and Promise lowering.
- [ ] Phase 6 standard library mapping.
- [ ] Phase 7 Express prep slice.

## Phase 0: Stability

- [x] Confirmed no pending stabilization work was present in this checkout.
- [x] Ran `cargo fmt --check`.
- [x] Ran `cargo test -q`.
- [x] Committed Phase 1 as `de44e6b Expand TypeScript sync expression support`.

## Phase 1: TypeScript Sync Core Expressions

- [x] Array literals.
- [x] Tuple literals with tuple annotation.
- [x] Record object literals with `Record<string, T>` annotation.
- [x] Index expressions.
- [x] Static member expressions for record field reads.
- [x] Unary `!` and `-`.
- [x] Logical `&&` and `||` as direct boolean rvalues.
- [x] General supported call arguments.
- [x] Type mapping for `number[]`, `Array<T>`, tuple types, `Record<string, T>`, and `T | null/undefined`.
- [x] MIR lowering for list, dict, tuple, index, field, unary, and logical expressions.
- [x] Rust codegen for list, dict, tuple, index, field, unary, and aggregate `console.log`.
- [x] End-to-end fixtures `06` through `11`.

## Phase 2: Mutation And Loops

- [x] Add shared assignment representation.
- [x] Add MIR place assignment support.
- [ ] Expand MIR places for field and index assignment.
- [x] TypeScript support for `x = expr`.
- [x] TypeScript support for compound assignments.
- [x] TypeScript support for statement-only increment/decrement.
- [x] TypeScript support for `while`.
- [ ] TypeScript support for `for...of`.
- [x] TypeScript support for C-style `for`.
- [ ] TypeScript support for `break` and `continue` inside loops.
- [ ] Switch with `break`, still rejecting fallthrough.
- [x] CFG lowering for `while`.
- [ ] Loop context stack for `break` and `continue`.
- [x] Rust codegen for structured `while`.
- [ ] Fixtures `12` through `17`.
- [x] Added fixtures `12_while_sum` and `14_c_for_loop`.
- [x] `cargo fmt --check`.
- [x] `cargo test -q`.
- [x] Committed partial Phase 2 slice as `Add TypeScript mutation and while loop support`.

Current Phase 2 limitation: nested `break`/`continue` inside branches still needs structured control-flow codegen before fixtures `15` and `16` can be claimed. Field/index assignment places exist in MIR, but Rust lvalue emission for those places is still pending.

## Later Phases

- [ ] Phase 3 object model.
- [ ] Phase 4 module linking.
- [ ] Phase 5 async model.
- [ ] Phase 6 stdlib mapping.
- [ ] Phase 7 Express recognizer and Axum codegen path.
