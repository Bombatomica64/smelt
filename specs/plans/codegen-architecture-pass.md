# Architecture Pass Plan — smelt-codegen-rust (post es-toolkit campaigns)

*(Fable planning pass, 2026-07-12. Sizes measured; clusters read.)*

## 1. Measured sizes and seam analysis
| File | Lines |
|---|---|
| emitter/core.rs | 5,087 |
| lib.rs | 3,596 |
| emitter/call_runtime.rs | 3,589 |
| emitter/list_query.rs | 2,340 |
| emitter/coercion.rs | 2,214 |
| emitter/control_flow.rs | 1,997 |
| emitter/call.rs | 1,898 |
| emitter/types.rs | 1,165 |

Frontend (secondary): stdlib/call_dispatch.rs 4,395 · expr/operators.rs 3,820 · testing/matchers.rs 3,648 · module_init.rs 3,246 · ty/annotations.rs 3,141 · new_expr.rs 3,039 · stdlib.rs 2,689 · decls/functions.rs 2,576 · guards.rs 2,462 · stmt/assignments.rs 2,317.

**core.rs (5,087)** — five cohabiting responsibilities: function-shell emission (`emit`, `emit_body`, preludes); local dataflow/mutability analysis (largest cluster: `local_binding_needs_mut`, assignment/use-before-assignment family); CFG reachability (`block_can_repeat/reach`); parameter/capture ownership (`parameter_needs_mutable_reference*`, `closure_capture_*`); erasure/dead-value + type-structure helpers (`structural_record_fields`, `substitute_type_params_in_type` — belong in types.rs).

**call_runtime.rs (3,589)** — rvalue dispatch (`rvalue_text_for_dest_inner` is a single ~1,150-line match, the worst function-level hotspot); JS operator semantics (~900 lines: optional/unknown binary, equality, relational); optional-chaining access; host/global/interop + field/method reference rendering.

**list_query.rs (2,340)** — misnamed: half is the closure rendering engine (`closure_text*`, `emit_closure_*`) plus a textual identifier-rewriting mini-lexer (`replace_shared_capture_uses`, `closure_shadow_intervals`, …) — a self-contained string-surgery module over already-rendered Rust text (fragility smell, see §3).

**coercion.rs (2,214)** — the erasure verb layer (`value_at_type*` ~430-line dispatcher, `erase*`, `extract*`, tag_check). Cohesive; its problem is interface, not size.

## 2. Incremental split (mechanical `pub(super)` moves; one commit per step; gate = check+clippy+full test with ZERO golden/snapshot diffs)
1. core.rs → keep shell emission (~1,200); extract `emitter/local_analysis.rs`, `emitter/capture_analysis.rs`, `emitter/cfg_queries.rs`; move type-structure helpers into types.rs.
2. call_runtime.rs → extract `emitter/binary_ops.rs`, `emitter/optional_access.rs`, `emitter/host_interop.rs`; split `rvalue_text_for_dest_inner` arms >40 lines into named per-arm methods (mechanical, big LLM-navigability win).
3. list_query.rs → extract `emitter/closures.rs` and `emitter/rendered_text_rewrite.rs` (with its unit tests); list_query shrinks to ~800.
4. Module docstring in the same commit as each move.
5. (Optional R3) lib.rs → `crate_layout.rs` + `derived_impls.rs`; KEEP runtime-prelude emission in lib.rs.
Frontend (budget permitting): split call_dispatch.rs by builtin family; matchers.rs by matcher family. Do NOT restructure ty/annotations.rs or guards.rs this pass (interleave with active type rules).

## 3. The coercion seam interface (Phase 2 of an existing design)
`emitter/rendered_value.rs` already defines `RenderedValue { text, TypeId, Precedence }` ("Phase 1 adopts this only at the coercion seam"). Adoption gap measured: meaningful use only in coercion.rs (5) + strings/call_runtime (2 each); bare-string `value_at_type_text`/`extract_value_text` call sites: coercion 47, call_runtime 34, core 24, list_query 14, ~7 files with 1-7.
- Convert verb-layer signatures to consume/produce `RenderedValue`; demote `*_text` to deprecated bridges; migrate call sites file-by-file in descending count order, one commit per file, goldens unchanged; the bridge-deletion commit is the enforcement point. This makes the "text + type must agree" invariant structural — the class of bug behind many past campaign fixes.
- Flagged for LATER (not this pass): replacing the rendered-text identifier surgery with RenderedValue-level pending substitutions or placeholder captures — changes emission order, churns goldens; feature-adjacent.

## 4. What NOT to touch
Runtime prelude emission in lib.rs (snapshot-load-bearing; mtime optimization); `write_if_changed`; templates/generated crates; rvalue dispatch reordering beyond arm extraction; frontend guards.rs/ty/annotations.rs; any move must show ZERO delta in smelt-unknown-report.

## 5. LLM-ergonomics rationale
CLAUDE.md: separate emission-helper modules; documented helpers. A 5,087-line core.rs mixing shell emission with five analysis families is where agents mis-place helpers and re-derive near-duplicates (the capture-analysis cluster is undiscoverable from the filename). Named shards turn "where does this go" into a filename decision. Finishing RenderedValue Phase 2 converts a prose invariant into a compiler-enforced one.

## 6. Campaign structure
- **R1 (2 Opus, disjoint files):** A = core.rs split (4 commits); B = call_runtime + list_query splits (3 commits). Gate per commit: check/clippy/full test, empty golden diff, smelt-unknown-report delta zero, orchestrator reviews moves are copy-exact.
- **R2 (1 agent, sequential — every step touches coercion.rs):** RenderedValue Phase 2, commit per migrated file, final bridge-deletion commit. Strictest review (precedence risk).
- **R3 (optional, 2 agents, disjoint crates):** lib.rs layout split + frontend call_dispatch/matchers shards.
Any golden diff at any step → revert that step and defer as feature-adjacent.
