# Parallel AST Preparation

## Goal

Speed up Smelt's transpilation path by preparing source files in parallel and by avoiding repeated frontend work.

The first implementation should focus on manifest-driven `check`, `dump-hir`, `dump-mir`, and `build` flows. It should keep existing HIR lowering semantics intact while moving file reads, import scanning, and AST parsing out of the serial lowering loop where possible.

## Motivation

Smelt currently does multiple related operations over the same source:

- reads each manifest source while discovering imports;
- scans imports for dependency ordering;
- reads or reuses source text during lowering;
- parses Python once for import discovery and again for lowering;
- parses TypeScript once during lowering, while import discovery uses a separate line-based scanner;
- lowers ordered files through one shared HIR crate and frontend state.

Ruff and Oxc do not appear to parallelize a single file's AST parse internally. Their useful pattern for Smelt is task-level parallelism: schedule independent files across workers, then combine results in deterministic order.

## Non-Goals

- Do not parallelize parsing within a single source file.
- Do not rewrite HIR IDs or merge independently lowered HIR crates in the first phase.
- Do not change dependency ordering semantics.
- Do not remove validation or frontend diagnostics to gain speed.
- Do not introduce a separate source graph implementation for each frontend.

## High-Level Design

Introduce a shared frontend preparation phase that turns source paths into reusable `PreparedSource` records.

Each record should contain the work products that later phases need:

- canonical or normalized path;
- source language;
- source text;
- import edges;
- parsed frontend AST when useful;
- lightweight frontend facts that can be computed without mutating shared HIR state;
- diagnostics from read, parse, or import-scan failures.

The manifest flow then becomes:

```text
root entries
  -> parallel source preparation and dependency expansion
  -> deterministic dependency ordering
  -> sequential HIR lowering using prepared source records
  -> MIR lowering, optimization, validation
  -> Rust codegen
```

The first phase should still lower HIR sequentially. The frontend contexts carry cross-file state such as TypeScript export aliases, object namespaces, object constants, overloads, Python module namespaces, and enum members. Keeping that phase sequential avoids a broad HIR merge refactor.

## Shared Preparation Model

Add a focused module, likely `crates/smelt-cli/src/source_prep.rs`, to own reusable source preparation.

The core types should look conceptually like:

```rust
/// Prepared source file data shared by manifest discovery and lowering.
struct PreparedSource {
    path: PathBuf,
    lang: SourceLang,
    source: String,
    imports: Vec<ManifestImport>,
    ast: PreparedAst,
}

/// Parsed frontend AST payload for source languages.
enum PreparedAst {
    TypeScript(TypeScriptPreparedAst),
    Python(PythonPreparedAst),
}
```

The exact AST payloads should respect frontend crate ownership and lifetimes:

- Python AST is owned and can likely be stored directly because Ruff's AST does not borrow from a bump allocator in the same way Oxc does.
- TypeScript Oxc AST uses allocator-tied lifetimes, so do not force it into a long-lived shared enum until the lifetime shape is clear.
- If TypeScript AST storage is awkward, the first TypeScript phase can still parallelize source reads and import scanning, then parse during lowering. Python can get parse reuse earlier.

Avoid duplicating frontend logic by giving each frontend a small adapter:

```text
SourcePrepFrontend
  read/parse source
  collect imports
  expose optional parsed AST for lowering
```

This is a conceptual interface, not necessarily a trait in phase one. Prefer simple functions if a trait creates lifetime friction.

## Dependency Expansion

Dependency closure discovery is currently recursive and mostly serial. Replace it with a queue-based graph builder that can prepare unseen candidates concurrently.

Required behavior:

- preserve stable final ordering for diagnostics and lowering;
- deduplicate by normalized path key;
- resolve Python package imports, relative imports, `__init__.py`, and extensionless candidates as today;
- resolve TypeScript imports through `oxc_resolver` as today;
- handle TypeScript barrel re-exports as today;
- tolerate import cycles as today and leave cycle-specific runtime errors to later phases.

Implementation approach:

1. Seed a queue with manifest entry paths.
2. Prepare a batch of unseen paths in parallel.
3. For each prepared source, resolve local import targets.
4. Add newly discovered target paths to the next batch.
5. Repeat until no unseen paths remain.
6. Run the existing dependency-first ordering over the completed graph.

The graph builder should collect all errors and report paths clearly. It should not let parallel execution make error output nondeterministic.

## Avoiding Repeated Work

The preparation phase should become the only place that reads source text for manifest sources.

Target de-duplication by phase:

- Phase 1: one file read per source for manifest builds.
- Phase 2: one Python parse per Python source by reusing the Ruff AST for import scanning and lowering.
- Phase 3: one TypeScript parse per TypeScript source if Oxc allocator lifetimes can be represented cleanly.
- Phase 4: one shared import collection path per language, not separate ad hoc scanners for manifest sorting and lowering.

For TypeScript, the existing line scanner is cheap but incomplete compared with Oxc. Replacing it with AST-based import extraction is desirable only if it does not make source prep more complex than the benefit warrants.

## Parallelism Strategy

Use a proven Rust parallelism library rather than custom thread management.

Preferred option:

- `rayon` for CPU-bound batch source preparation and AST parsing.

Why:

- Ruff uses Rayon for file-level command scheduling;
- deterministic collection is straightforward by sorting or preserving path order after parallel work;
- it avoids hand-rolled worker pools.

Do not add async I/O for this phase. Source files are local, parsing is CPU-bound, and the codebase is otherwise synchronous.

Thread count should default to Rayon defaults. A later config option can expose a limit if needed:

```toml
[performance]
jobs = 8
```

Do not add this configuration until there is a real need.

## Frontend Changes

### Python

Python is the best first target for AST reuse.

Add a lowering entrypoint that accepts a parsed module:

```rust
pub fn module_to_hir_with_path(
    module_ast: ModModule,
    file_id: FileId,
    path: &str,
    ctx: &mut HirCtx,
) -> Result<ModuleId, Vec<SmeltError>>
```

Then keep the existing `to_hir_with_path` as a wrapper:

```text
parse source -> module_to_hir_with_path(parsed, ...)
```

Import scanning can reuse the same parsed `ModModule` through the existing AST visitor.

### TypeScript

TypeScript should start with parallel source prep and possibly AST-based import extraction later.

Oxc parser AST lifetimes are tied to an allocator. Safe options:

- keep parsing in lowering for now;
- store source text and parse in a scoped lowering function with a per-file allocator;
- introduce a TypeScript prepared unit that owns both allocator and AST only if the lifetimes can be represented without unsafe code or self-referential structs.

Avoid unsafe self-referential wrappers for prepared Oxc ASTs.

If Oxc AST reuse is not clean, TypeScript gains still come from parallel dependency graph discovery, source reads, and future frontend fact extraction.

## HIR Lowering Boundary

Keep this invariant for the first implementation:

```text
Only one mutable HIR crate and one frontend state object are active during final lowering.
```

That means final lowering remains ordered and serial:

```text
for source in dependency_order {
    lower_prepared_source_into_shared_hir(source, state)
}
```

Parallel HIR lowering should be a separate architecture pass. It requires:

- independent HIR crate lowering per module or dependency component;
- stable ID remapping for modules, items, bodies, symbols, types, locals, and expressions;
- merge rules for frontend cross-file state;
- deterministic diagnostics;
- tests for cyclic imports and namespace exports.

## Incremental Implementation Plan

### Phase 1: Shared Source Text and Preparation Shell

- Move manifest source reading into a preparation helper.
- Keep cached source text in `ManifestSource` or its replacement.
- Ensure lowering never rereads manifest sources.
- Add tests around manifest builds with multiple linked files.

This phase is already partially started by storing source text in manifest source records.

### Phase 2: Python Parse Reuse

- Add `module_to_hir_with_path` to `smelt-frontend-py`.
- Change Python import scanning to accept a parsed module.
- Store parsed Python AST in prepared source records.
- Lower Python files from the stored AST.
- Keep the old source-based API for single-file callers and tests.

Acceptance criteria:

- Python manifest source is parsed once.
- Python parse diagnostics remain path-qualified.
- Existing Python package import tests still pass.

### Phase 3: Parallel Source Preparation

- Add `rayon` to the CLI crate.
- Convert dependency closure expansion to batch preparation.
- Prepare unseen files in parallel.
- Keep final source order deterministic.
- Keep error output deterministic.

Acceptance criteria:

- Multi-file manifest discovery uses parallel preparation.
- Import cycles behave as they do today.
- Dependency-first order is unchanged for existing fixtures.

### Phase 4: TypeScript Import Extraction Cleanup

- Decide whether to keep the line scanner or replace it with Oxc AST import extraction.
- If replacing, parse TypeScript once for import facts during preparation.
- Do not store Oxc AST across phases unless ownership is simple and safe.

Acceptance criteria:

- Type-only imports and re-exports used by existing tests still order correctly.
- Barrel re-export behavior remains unchanged.
- No unsafe self-referential AST storage is introduced.

### Phase 5: Instrumentation

Add optional timing output for transpiler phases:

```text
source prep
dependency ordering
HIR lowering
MIR lowering
MIR optimization
MIR validation
Rust codegen
```

This can be controlled by an environment variable first:

```text
SMELT_TIMINGS=1
```

Do not print timings by default.

## Testing Strategy

Add targeted tests for:

- Python AST reuse preserves import discovery;
- manifest dependency ordering remains deterministic;
- duplicate imports only prepare a source once;
- cyclic imports still do not recurse forever;
- mixed TypeScript/Python manifests still lower in dependency order;
- parse errors from parallel source prep are deterministic and path-qualified.

Run the standard checks:

```text
cargo test
cargo check
cargo clippy
```

## Risks

- Oxc allocator lifetimes may make TypeScript AST reuse impractical without a larger frontend API change.
- Parallel graph discovery can make diagnostics nondeterministic if errors are reported in worker completion order.
- Python AST reuse may require changing lowering APIs that tests call directly.
- Holding ASTs increases peak memory use. This is acceptable for phase 2 if it removes repeated parsing, but should be measured.

## Open Questions

- Is Python parsing a meaningful fraction of real Smelt build time, or is HIR/MIR/codegen hotter?
- Should `check` and `build` share a prepared-source cache inside one process if both are ever run together?
- Should source prep cache across process runs be part of the later package artifact/cache work?
- Should TypeScript import scanning stay cheap and line-based until Oxc AST reuse is needed for correctness?

