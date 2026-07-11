# JavaScript parseFloat Coercion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lower global and `Number.parseFloat` calls with JavaScript's string-coercion semantics for non-string inputs.

**Architecture:** Centralize argument validation/coercion in a `parse_float_operand` helper. Preserve string operands directly; otherwise emit `PrimitiveCastOp::ToString`, then a dedicated `PrimitiveCastOp::ParseFloat` whose Rust emission returns `NaN` for invalid text without changing generic/Python float conversion.

**Tech Stack:** Rust, Oxc TypeScript AST, Smelt HIR and Rust codegen.

## Global Constraints

- Do not add radash- or function-owner-specific special cases.
- Do not add or expand `SmeltUnknown`; source `any`/`unknown` is an existing legitimate dynamic boundary.
- Keep TypeScript lowering helpers in focused modules and document new helpers.
- Run `cargo check`, `cargo clippy`, and full `cargo test` before commit.

---

### Task 1: Coerce parseFloat inputs through strings

**Files:**
- Modify: `crates/smelt-frontend-ts/src/lowering/stdlib/numbers_math.rs`
- Modify: `crates/smelt-frontend-ts/src/lowering/stmt/assignments.rs`
- Test: `crates/smelt-frontend-ts/src/tests/part02_tests.rs`
- Test: `crates/smelt-codegen-rust/src/tests/part_2_tests.rs`

**Interfaces:**
- Produces: `ModuleBuilder::parse_float_operand(source_name, call, body) -> Result<ExprId, SmeltError>`.
- Consumes: `PrimitiveCastOp::ToString` and the new `PrimitiveCastOp::ParseFloat`.

- [x] **Step 1:** Add failing frontend coverage for global and `Number.parseFloat` calls receiving `any`, asserting `ParseFloat` consumes a `ToString` expression.
- [x] **Step 2:** Run the focused frontend tests and confirm the current string-argument diagnostic.
- [x] **Step 3:** Implement `parse_float_operand` and route both call spellings through it.
- [x] **Step 4:** Add codegen coverage proving erased input is string-coerced before parsing, then rerun the radash blocker scan.
- [ ] **Step 5:** Run repository gates, review the diff, commit, push, and open a draft PR.
