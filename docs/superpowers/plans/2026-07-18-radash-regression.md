# Radash Regression Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the pinned, fully passing Radash generated-Rust test suite as a blocking pull-request regression gate.

**Architecture:** Extend the existing GitHub Actions compatibility jobs with an independent Radash lane that builds Smelt from the candidate commit, clones the ref already pinned in `libraries.json`, applies the existing Radash fixture, exposes its ambient Jest-style test globals through an explicit Vitest import, transpiles, and runs the generated crate. Document Radash as a curated passing target while retaining its daily probe entry.

**Tech Stack:** GitHub Actions YAML, Smelt CLI, Cargo, TypeScript fixture manifests.

## Global Constraints

- Use `.github/compat/libraries.json` as the single source of truth for the Radash commit.
- Run all 84 generated tests as a blocking gate.
- Do not commit generated third-party distribution artifacts.
- Do not add or expand `SmeltUnknown` usage.

---

### Task 1: Add the Radash CI regression lane

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/compat/README.md`
- Test: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: the `radash` repository/ref entry in `.github/compat/libraries.json` and `.github/compat/radash/Smelt.toml`.
- Produces: the blocking `radash-regression` GitHub Actions job.

- [x] **Step 1: Confirm the current fixture requires explicit test discovery**

Run:

```bash
rg -n "import.*vitest|import.*chai" target/library-probes/radash/src/tests/typed.test.ts
```

Expected: the file imports Chai but has no explicit Vitest import.

- [x] **Step 2: Add the regression job**

Add a `radash-regression` job to `.github/workflows/ci.yml` that builds `smelt`, clones `sodiray/radash` at the JSON-pinned ref, copies `.github/compat/radash`, inserts `import { describe, test } from 'vitest'` after the Chai import, runs `smelt build`, and runs generated `cargo test --no-fail-fast` with warnings suppressed.

- [x] **Step 3: Document the new curated target**

Update `.github/compat/README.md` to state that both Remeda and Radash are blocking compatibility gates, while Radash remains part of the daily probe matrix.

- [x] **Step 4: Validate the workflow and generated suite**

Run:

```bash
actionlint .github/workflows/ci.yml
cargo test --manifest-path target/library-probes/radash/dist-smelt/Cargo.toml --no-fail-fast
```

Expected: `actionlint` reports no errors and Radash reports `84 passed; 0 failed`.

- [x] **Step 5: Run repository validation**

Run:

```bash
cargo check --lib --no-default-features
cargo clippy --lib --no-default-features
```

Expected: check passes; Clippy may stop only on the already documented current-main `single_match_else` failure in `emitter/map.rs`.

- [x] **Step 6: Commit and publish**

```bash
git add .github/workflows/ci.yml .github/compat/README.md docs/superpowers/plans/2026-07-18-radash-regression.md
git commit -m "ci: gate generated Radash tests"
git push
```

Expected: PR #159 receives the new commit and starts a `radash-regression` check.
