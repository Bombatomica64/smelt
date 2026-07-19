# Complete Generator Protocol Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete Smelt's typed TypeScript generator protocol with sent values, explicit return/throw commands, stable completion, abrupt-completion delegation, and custom iterable runtime support.

**Architecture:** Replace the zero-argument resume ABI with a typed `SmeltGeneratorCommand<N, R>` channel consumed by both synchronous and asynchronous wrappers. Keep terminal state in the wrapper so calls after completion are stable, and lower `yield` as an expression that receives the next command. `yield*` drives typed built-in or custom iterator arms while forwarding normal, return, and throw commands according to the selected protocol.

**Tech Stack:** Rust, Oxc TypeScript AST, Smelt HIR/MIR, genawaiter, Tokio-backed `SmeltFuture`, emitted-crate runtime tests.

## Global Constraints

- Do not introduce `SmeltUnknown` for concrete generator yield, return, next, or error flow.
- Synchronous generators must never await async-only carriers.
- Generator calls remain lazy; every protocol call advances at most one suspension or completion.
- Use general type-directed lowering; no fixture or function-name special cases.
- Preserve generated-file mtimes and validate generated crates through `smelt rust-test-report`.

---

### Task 1: Bidirectional resume and stable terminal state

**Files:**
- Modify: `crates/smelt-hir/src/expr/kinds.rs`
- Modify: `crates/smelt-mir/src/types.rs`
- Modify: `crates/smelt-mir/src/lower/expr.rs`
- Modify: `crates/smelt-frontend-ts/src/lowering/decls/types_iface.rs`
- Modify: `crates/smelt-frontend-ts/src/lowering/stdlib/call_dispatch.rs`
- Modify: `crates/smelt-codegen-rust/src/lib.rs`
- Modify: `crates/smelt-codegen-rust/src/emitter/call_runtime.rs`
- Test: `crates/smelt-codegen-rust/tests/generator_runtime.rs`

**Interfaces:**
- Consumes: `Type::Generator { yield_ty, return_ty, next_ty, is_async }`.
- Produces: typed resume commands and `yield` expressions that evaluate to `N`; completed wrappers return stable `Complete(R)` results.

- [ ] Add emitted-runtime tests where `const received = yield 1` observes `.next(42)`, and repeated `.next()` after completion remains done.
- [ ] Run the focused runtime tests and confirm failure at frontend lowering or generated Rust compilation.
- [ ] Add the typed command carrier through HIR/MIR and use it as the generator producer's resume input.
- [ ] Make `.next(value?)` construct a normal resume command, preserving TypeScript's optional first argument behavior.
- [ ] Cache terminal completion in sync and async wrappers and rerun focused frontend/runtime tests.
- [ ] Commit the independently passing bidirectional-resume slice.

### Task 2: Explicit return and throw protocol methods

**Files:**
- Modify: `crates/smelt-frontend-ts/src/lowering/stdlib/call_dispatch.rs`
- Modify: `crates/smelt-codegen-rust/src/lib.rs`
- Modify: `crates/smelt-codegen-rust/src/emitter/control_flow.rs`
- Modify: `crates/smelt-codegen-rust/src/emitter/call_runtime.rs`
- Test: `crates/smelt-codegen-rust/tests/generator_runtime.rs`

**Interfaces:**
- Consumes: the typed resume-command carrier from Task 1.
- Produces: `.return(value)` terminal completion and `.throw(error)` resumption through generator control flow, for both sync and async wrappers.

- [ ] Add runtime tests for `finally` execution, `.return(value)`, caught `.throw(error)`, and uncaught `.throw(error)`.
- [ ] Run them and record the failing protocol dispatch boundary.
- [ ] Lower `.return()` and `.throw()` to typed generator commands rather than ordinary class calls.
- [ ] Route commands through suspended `yield` sites so generated `try/finally` and `try/catch` observe abrupt completion.
- [ ] Verify sync and async runtime cases and commit the passing protocol-method slice.

### Task 3: Complete `yield*` delegation

**Files:**
- Modify: `crates/smelt-frontend-ts/src/lowering/decls/types_iface.rs`
- Modify: `crates/smelt-codegen-rust/src/emitter/call_runtime.rs`
- Test: `crates/smelt-codegen-rust/tests/generator_runtime.rs`
- Test: `crates/smelt-frontend-ts/src/tests/part05_tests.rs`

**Interfaces:**
- Consumes: normal/return/throw resume commands and existing per-arm sync/async carrier selection.
- Produces: delegation that forwards sent values and abrupt completions, preserving delegate return unions and exact-once evaluation.

- [ ] Add runtime tests for `.next(value)` forwarding through `yield*`, delegated `.return()`, delegated `.throw()`, and missing delegate methods.
- [ ] Add custom `[Symbol.iterator]` and `[Symbol.asyncIterator]` emitted-runtime fixtures.
- [ ] Run focused tests and classify failures by built-in, sync custom, async custom, or union arm.
- [ ] Extend the delegate loop to select and forward each command per arm without yield or return erasure.
- [ ] Verify built-ins, custom iterables, heterogeneous unions, sync fallback, and async-only rejection.
- [ ] Commit the passing delegation slice.

### Task 4: Boundary validation and shipment

**Files:**
- Modify: `docs/superpowers/plans/2026-07-19-complete-generator-protocol.md`
- Create: `blocker-logs/generator-protocol.md`

**Interfaces:**
- Consumes: all completed generator protocol slices.
- Produces: reproducible generated-Rust report, unknown-erasure delta, repository validation evidence, and pushed commits.

- [ ] Run all focused frontend and emitted-runtime generator tests.
- [ ] Generate `blocker-logs/generator-protocol.md` with `smelt rust-test-report --full --diagnostics --suppress-warnings`.
- [ ] Run `smelt smelt-unknown-report` against the committed examples baseline and require avoidable delta `0`.
- [ ] Run `cargo check --lib --no-default-features` and `cargo clippy --lib --no-default-features`.
- [ ] Run `cargo clippy --all-targets` and `cargo test`, documenting only reproducible unrelated failures.
- [ ] Mark this plan complete, inspect the final diff, commit, and push the branch.
