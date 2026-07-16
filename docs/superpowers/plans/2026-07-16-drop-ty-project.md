# Drop `ty_project` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Smelt's `ty`-backed Python frontend publishable by replacing the git-only `ty_project` database with a local database built from crates.io dependencies.

**Architecture:** Add a focused `SmeltTyDb` module that implements the public Ruff/ty/salsa database traits and initializes the ty `Program` with the temporary source root plus vendored typeshed. Keep type collection in `lib.rs`, changing only its concrete database type. Then remove publishing-time feature stripping because the complete dependency graph is publishable.

**Tech Stack:** Rust, salsa 0.28, Ruff/ty crates 0.0.4, Cargo packaging, shell publishing script.

## Global Constraints

- Use crates.io version `0.0.4` for Ruff/ty crates and `0.28.0` for salsa.
- Do not introduce `SmeltUnknown`.
- Preserve the frontend boundary as plain `u32` offsets.
- Put the database implementation in a separate documented module.
- Preserve unrelated working-tree changes.

---

### Task 1: Replace `ProjectDatabase`

**Files:**
- Create: `crates/smelt-py-types/src/db.rs`
- Modify: `crates/smelt-py-types/src/lib.rs`
- Modify: `crates/smelt-py-types/Cargo.toml`
- Test: existing `crates/smelt-py-types/src/lib.rs` unit tests

**Interfaces:**
- Consumes: `OsSystem`, `SystemPathBuf`, vendored typeshed, and ty's public database traits.
- Produces: `SmeltTyDb::new(system: OsSystem, source_root: SystemPathBuf) -> anyhow::Result<Self>`.

- [ ] **Step 1: Establish the dependency failure boundary**

  Replace git dependencies with crates.io dependencies and remove `ty_project`, then run `cargo check -p smelt-py-types`. Expected: unresolved `ty_project` imports until the local database is wired in.

- [ ] **Step 2: Implement the database module**

  Add a documented salsa database storing `OsSystem`, vendored filesystem, and `AnalysisSettings`; implement `ruff_db::Db`, `ty_python_core::Db`, `ty_module_resolver::Db`, `ty_python_semantic::Db`, and `salsa::Database`. Initialize `ProgramSettings` with the source root and vendored search paths. The semantic `check_file` diagnostic driver returns an empty vector because Smelt only performs inference queries.

- [ ] **Step 3: Wire type resolution to the local database**

  Replace `ProjectMetadata::discover` and `ProjectDatabase::use_defaults` with `SmeltTyDb::new(system, root)`, update collector database types, and revise module/function documentation to describe default settings rather than project config discovery.

- [ ] **Step 4: Verify type-resolution behavior**

  Run `cargo test -p smelt-py-types` and `cargo test -p smelt-frontend-py --features ty ty_resolution`. Expected: all existing inference and offset-boundary tests pass.

### Task 2: Publish the Complete `ty` Feature

**Files:**
- Modify: `crates/smelt-frontend-py/Cargo.toml`
- Modify: manifests/comments that describe `ty` as git-only
- Modify: `scripts/publish-crates.sh`
- Test: Cargo packaging dry run

**Interfaces:**
- Consumes: publishable `smelt-py-types` dependency graph from Task 1.
- Produces: published manifests retaining `smelt-py-types` and the `ty` feature.

- [ ] **Step 1: Update publish metadata**

  Set `smelt-py-types` to publishable, give its path dependency a workspace version, bump frontend Ruff parser dependencies to `0.0.4`, and remove obsolete git-only comments from dependent manifests and docs.

- [ ] **Step 2: Remove feature stripping**

  Update `scripts/publish-crates.sh` to include `smelt-py-types` in dependency order and stop deleting `ty` features/dependencies from staged manifests.

- [ ] **Step 3: Verify package construction**

  Run the publishing script in dry-run mode (or targeted `cargo package` commands if the script requires already-published dependencies). Expected: manifests contain no git dependencies and Cargo accepts the package graph.

### Task 3: Repository Verification and Delivery

**Files:**
- Modify: `Cargo.lock`
- Test: workspace checks required by `AGENTS.md`

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces: a committed and pushed feature implementation.

- [ ] **Step 1: Run required checks**

  Run `cargo check` and `cargo clippy`. Fix every failure attributable to this feature.

- [ ] **Step 2: Run targeted feature tests**

  Run the `smelt-py-types` and `smelt-frontend-py --features ty` test suites. Run full `cargo test` only immediately before committing, as required by `AGENTS.md`.

- [ ] **Step 3: Review scope**

  Inspect `git diff`, confirm unrelated site/third-party changes are excluded, and verify no `ty_project` or git-pinned ty dependencies remain in the publishable path.

- [ ] **Step 4: Commit and push**

  Stage only issue #157 files, commit with a clear message referencing the publishability change, and push the branch without including pre-existing modifications.
