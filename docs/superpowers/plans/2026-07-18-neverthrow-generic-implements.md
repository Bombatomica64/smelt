# Neverthrow Generic Implements Implementation Plan

> **For agentic workers:** Execute this plan task-by-task with focused red/green verification and a review gate before merge.

**Goal:** Lower generic TypeScript class `implements` clauses through general interface type-argument substitution so neverthrow advances beyond its current whole-crate blocker.

**Architecture:** Reuse HIR's existing `InterfaceHeritage { parent, args }` representation for class implementation edges instead of discarding generic arguments. The TypeScript frontend will resolve locally declared interfaces, instantiate their fields and methods with the existing generic substitution helpers, and treat imported or ambient interfaces without local structural definitions as opaque validation boundaries. MIR retains interface symbols because downstream code does not consume implementation arguments.

**Tech Stack:** Rust, oxc TypeScript AST, Smelt HIR/MIR, Cargo tests.

## Global Constraints

- Implement a general rule; do not special-case neverthrow, `IResult`, or `PromiseLike`.
- Preserve concrete type parameters and do not introduce or expand `SmeltUnknown`.
- Keep codegen helpers separated and documented; this phase does not require codegen changes.
- Run `cargo check --lib --no-default-features` and `cargo clippy --lib --no-default-features` during the TypeScript-only loop.

---

### Task 1: Preserve Generic Implements References

**Files:**
- Modify: `crates/smelt-hir/src/item.rs`
- Modify: `crates/smelt-hir/src/format/types.rs`
- Modify: `crates/smelt-mir/src/lower/mod.rs`
- Modify: `crates/smelt-frontend-ts/src/lowering/ty/interface_lookup.rs`
- Modify: `crates/smelt-frontend-ts/src/lowering/decls/functions.rs`

**Interfaces:**
- Consumes: `smelt_hir::InterfaceHeritage { parent: Symbol, args: Vec<TypeId> }` and `ModuleBuilder::type_argument_substitution`.
- Produces: `Class::implements: Vec<InterfaceHeritage>` and `implements_reference(...) -> Result<Option<InterfaceHeritage>, SmeltError>`.

- [x] **Step 1: Add focused failing frontend tests**

Add tests covering a concrete generic field/method implementation, a mismatched concrete instantiation, and an imported opaque generic interface.

- [x] **Step 2: Verify the tests fail at the generic-clause gate**

Run: `cargo test -p smelt-frontend-ts generic_implements -- --nocapture`

Expected: positive cases fail with `generic implements clauses are not lowered yet`.

- [x] **Step 3: Preserve the interface symbol and lowered type arguments**

Change the HIR class field to `Vec<InterfaceHeritage>`, lower each AST type argument with `ts_type_to_hir`, format both the parent and arguments, and project only `parent` into MIR's existing symbol list.

- [x] **Step 4: Instantiate local interface requirements during validation**

Clone the referenced interface, build substitutions from its declared parameters and the implementation arguments, then validate against `substituted_fields` and `substituted_methods`. Return `None` for references with no locally lowered interface so imported and ambient contracts remain opaque after TypeScript validation.

- [x] **Step 5: Verify focused tests pass**

Run: `cargo test -p smelt-frontend-ts generic_implements -- --nocapture`

Expected: all focused generic implementation tests pass.

### Task 2: Reprobe and Verify Neverthrow

**Files:**
- Generate ignored report: `blocker-logs/neverthrow-generic-implements-after.md`

**Interfaces:**
- Consumes: the generic implementation lowering from Task 1.
- Produces: a refreshed blocker inventory proving the current whole-crate wall moved.

- [x] **Step 1: Refresh the neverthrow probe**

Run: `cargo run --bin smelt -- probe --manifest-path target/library-probes/neverthrow/Smelt.toml --output blocker-logs/neverthrow-generic-implements-after.md`

Expected: `generic implements clauses are not lowered yet` is absent.

- [x] **Step 2: Run mandated TypeScript validation**

Run: `cargo check --lib --no-default-features`

Run: `cargo clippy --lib --no-default-features`

Run: `cargo test -p smelt-frontend-ts --lib`

- [x] **Step 3: Review, commit, and push**

Review the focused diff, stage only intended files, commit with `fix(frontend-ts): lower generic implements clauses`, and push `lorenzo/neverthrow-generic-implements`.
