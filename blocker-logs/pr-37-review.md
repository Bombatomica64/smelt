# PR #37 Review — Host-runtime specialization for Python and Node

**Branch:** `t3code/1e801c33` → `main`
**Scope reviewed:** diff `28eaecd..bc6cea0` (43 files, +9553 / -81)
**Status:** work-in-progress (phase 7 of 9)
**Method:** 8 independent finder angles (line-by-line, removed-behavior, cross-file, reuse, simplification, efficiency, altitude, CLAUDE.md conventions) + per-finding verification.

## Summary

Introduces the `smelt-specialize` crate, which launches sandboxed CPython/Node "guest"
processes to introspect real runtime objects (descriptors, metaclasses, Django models, JS
prototypes), emits a JSON specialization manifest, and feeds that into HIR lowering and Rust
codegen so metaclass/descriptor-generated shapes can be materialized statically.

The structure is sound. Sandbox policy validation, manifest reference-checking, and the
"materialized manifest is authoritative" trust model all hold up under scrutiny. The
findings below are the issues that survived verification, most-severe first.

## Confirmed findings

### High

1. **`crates/smelt-codegen-rust/src/emitter/call.rs:~337` — `ConsoleWrite`/`ConsoleErrorWrite` discard the format spec.**
   `console_arg_text()` returns `(format_spec, value)` where `format_spec` is `"{:?}"` for
   `List`/`Dict`/`Tuple`/`Optional` and `"{}"` otherwise. The arm destructures as
   `|(_, value)|`, dropping the spec, and emits a hardcoded `"{}"` macro. The sibling
   `ConsoleLog` arm threads the spec through correctly.
   **Failure:** a Smelt program writing a list/dict/tuple to stdout/stderr generates
   `print!("{}", value)`, which fails to compile because those types only implement `Debug`.
   Introduced by this PR.

2. **`crates/smelt-frontend-py/src/lowering/specialization.rs:457` (`merge_materialized_descriptor_state_class`) — class resolved by unqualified name across the whole crate.**
   `qualified_class` is stripped via `rsplit('.').next()` (line 453) then matched
   first-wins against every item in `self.ctx.krate.items` by bare name.
   **Failure:** two modules each defining e.g. `Config` → descriptor state fields merged
   into whichever appears first, corrupting an unrelated class's layout.
   Violates CLAUDE.md: *"qualified type references must preserve or resolve the full alias
   path instead of blindly turning `Namespace.Member` into `Class(Member)`."*

3. **`crates/smelt-frontend-py/src/lowering/specialization.rs:392` (`source_item_for_provenance`) — unguarded `ends_with` matches unrelated names.**
   `name == expected_name || name.ends_with(expected_name)` has no word/dot boundary.
   **Failure:** looking up provenance for `helper` also matches `my_helper`; the wrong
   function gets lowered as the descriptor callable → incorrect method binding.

### Medium

4. **`crates/smelt-frontend-py/src/lowering/specialization.rs:1102` (`capture_source_item`) — same unqualified-name lookup.**
   `rsplit('.').next()` then `self.items.get(source_name)`. Module-scoped, so lower risk,
   but two same-named callables visible in one module (imported from different namespaces)
   still capture the wrong one into the wrapper closure.

5. **`crates/smelt-specialize/guest/node.js:306` and `:547` — a throwing getter aborts the whole run.**
   `value[key]` (instance fields) and `classValue[name]` (statics) invoke user getters with
   no `try`/`catch`. An object with a throwing getter aborts specialization for the entire
   module. The Python guest has the analogous abort at `python.py:458`
   (`zip(co_freevars, closure, strict=True)`) and `:235` (`dataclasses.fields(value)`),
   though those are caught by the top-level handler (exit code 2). Since the feature exists
   to introspect arbitrary third-party objects, consider per-field recovery.

### Minor

6. **`crates/smelt-specialize/src/manifest.rs:598` — inconsistent trimming in adapter-id validation.**
   Empty-check uses `adapter.id.trim().is_empty()` (line 593); dedup uses the untrimmed
   `adapter.id.as_str()`. So `" x "` and `"x"` pass the empty check yet are treated as
   distinct keys, letting logically-duplicate adapter ids through.

7. **`crates/smelt-specialize/src/{python,node}.rs` — ~150 lines of duplication.**
   Error enums, `ScratchDirectory`, `write_if_changed`, and `decode_manifest` are
   near-identical between the two adapters. Candidate for a shared module — but per CLAUDE.md,
   defer the broad refactor until the feature phase stabilizes.

8. **`specialization.rs` `merge_materialized_class_fields` — silent field-type override.**
   Materialized fields overwrite source-annotated fields with no divergence diagnostic.
   Behaviorally correct (the manifest is ground truth), but the metaprogramming plan's own
   "keep diagnostics honest about what was specialized" note argues for a comment + a
   non-fatal warning when a source `int` becomes a materialized `str`.

## Refuted (investigated, not bugs)

- **Node guest flags** (`--permission`, `--no-addons`): both valid; node flags are consumed
  before the script and never appear in `process.argv`, so the guest's `argv.length === 3`
  check is satisfied.
- **Sandbox subprocess / process-limit validation** (`sandbox.rs:637`): the condition
  correctly rejects `subprocesses=false` with `process_limit != 1`.
- **Custom-metaclass and Django-model "bypasses" when a materialization exists**
  (`class.rs:65`, `:113`): intended design — the materialized manifest, derived from running
  real CPython, is the authoritative shape, so deferring to it is correct, not a hole.
