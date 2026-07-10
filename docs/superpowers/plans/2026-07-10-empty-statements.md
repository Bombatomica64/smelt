# TypeScript Empty Statements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Treat TypeScript empty statements as semantic no-ops during block lowering.

**Architecture:** Add an explicit `Statement::EmptyStatement` arm to the central block statement dispatcher. Protect it with a frontend test containing empty statements at function and nested-block scope.

**Tech Stack:** Rust, Oxc TypeScript AST, Smelt HIR frontend tests.

## Global Constraints

- Do not add source-library-specific lowering.
- Do not introduce or expand `SmeltUnknown`.
- Run `cargo check`, `cargo clippy`, and full `cargo test` before commit.

---

### Task 1: Ignore empty statements

**Files:**
- Modify: `crates/smelt-frontend-ts/src/lowering/decls/types_iface.rs`
- Test: `crates/smelt-frontend-ts/src/tests/part02_tests.rs`

**Interfaces:**
- Consumes: Oxc `Statement::EmptyStatement` nodes.
- Produces: successful HIR lowering with no emitted statement for those nodes.

- [ ] **Step 1:** Add a failing frontend test with `;` in a function and nested block.
- [ ] **Step 2:** Run `cargo test -p smelt-frontend-ts lowers_empty_statements_as_noops -- --nocapture` and confirm the unsupported-statement diagnostic.
- [ ] **Step 3:** Add `Statement::EmptyStatement(_) => Ok(())` to `statement_in_block`.
- [ ] **Step 4:** Re-run the focused test and the radash probe scan.
- [ ] **Step 5:** Run repository gates, review the diff, and commit as `fix(frontend-ts): lower empty statements as no-ops`.
