# Spike: using Astral's `ty` as Smelt's Python type source

**Status:** feasibility spike (branch `claude/python-ty-spike`, crate
`crates/smelt-py-ty-spike`). Not wired into the real frontend yet.

## Motivation

Smelt's TypeScript frontend leans on `oxc` (a crates.io parser) plus its own
type lowering/inference. The Python frontend (`smelt-frontend-py`) currently uses
ruff *only as a parser* — `ruff_python_parser` produces the AST and Smelt
recovers types from source annotations (`[strict] python = true` makes them
mandatory). Real Python type inference is the hard part: call-result types,
narrowed unions, generics, and — crucially — types that live in **typeshed
stubs** rather than user source (`math.sqrt(...) -> float`, `dict.get`, etc.).
Re-implementing that is a large, perpetually-incomplete effort.

`ty` (Astral's Rust type checker, formerly red-knot) already does all of this and
is in the **same git repo at the same revision** Smelt already pins for the ruff
parser. This spike evaluates embedding it as a library to supply Python types,
the way the TS side could (in principle) lean on a checker.

## What the spike does

`crates/smelt-py-ty-spike/src/main.rs`:

1. Materializes a one-file Python project in a temp dir.
2. Builds a `ty` project database (`ProjectDatabase::use_defaults`) over an
   `OsSystem`, discovering metadata like the `ty` CLI does.
3. Runs the checker on the file (`ty_python_semantic::check_file`) and prints
   diagnostics — demonstrating **type checking**.
4. Builds a `SemanticModel` and, for each top-level binding, queries
   `HasType::inferred_type` on the value expression and prints
   `Type::display(db)` — demonstrating **type extraction**, including types that
   come from bundled typeshed (e.g. the result of `math.sqrt`).

## Key API surface (at rev `6c88390f…`)

| Need | Entry point |
| --- | --- |
| Database | `ty_project::ProjectDatabase::{use_defaults,fallible}(metadata, system)` |
| Project metadata | `ty_project::ProjectMetadata::{discover,from_config_file}` |
| Filesystem | `ruff_db::system::OsSystem` (real FS) or `MemoryFileSystem` (in-memory) |
| File handle | `ruff_db::files::system_path_to_file(db, path)` |
| Diagnostics | `ty_python_semantic::check_file(db, file) -> Result<Box<[Diagnostic]>, Diagnostic>` |
| Parsed AST (db-tracked) | `ruff_db::parsed::parsed_module(db, file).load(db).syntax()` |
| Type of an expression | `ty_python_semantic::HasType::inferred_type(&model) -> Option<Type>` |
| Display a type | `Type::display(db)` |

The AST nodes are the *same* `ruff_python_ast` types `smelt-frontend-py` already
walks, so a real integration would hand ty the file, then annotate the existing
lowering walk with `inferred_type` lookups keyed on the nodes it already visits.

## Dependency / build implications

- `ty_python_semantic` + `ty_project` pull in `salsa` (crates.io, `0.26.1`), the
  ruff db/index/source crates, and **`ty_vendored`** (bundled typeshed stubs —
  sizeable, but it's what makes stdlib inference work out of the box).
- They do **not** pull the one git-only transitive dep (`lsp-types`, used by
  `ty_server`/`ruff_server`), so the closure stays inside the single
  `astral-sh/ruff` git source already pinned.
- The spike crate is deliberately **excluded from `default-members`** so this
  heavy tree does not affect the normal `cargo build`; only an explicit
  `-p smelt-py-ty-spike` builds it.
- Sandbox/local builds still need the egress workaround (vendor the ruff repo at
  the pinned rev + a local-only `[patch."https://github.com/astral-sh/ruff"]`).
  CI, which can fetch GitHub, builds the git deps directly — same as the existing
  ruff parser deps.

## If we adopt it — integration sketch

1. Build one `ProjectDatabase` per Smelt compilation (rooted at the source
   project), reusing the user's `pyproject.toml`/`ty` config if present.
2. During `smelt-frontend-py` lowering, replace annotation-only type recovery
   with `inferred_type` lookups on the nodes already being walked; keep
   annotations as the fallback when ty returns no type.
3. Map `ty_python_semantic::types::Type` → Smelt HIR `Type`. This is the real
   work: unions/narrowing, generics, callable signatures, and the typeshed
   nominal types need a translation layer (and a policy for types Smelt can't yet
   represent — likely `Unknown` with a diagnostic, mirroring the TS side).
4. Surface ty diagnostics as Smelt errors under `[strict] python = true`.

## Open questions / risks

- **Type-model mapping is the bulk of the effort**, not the embedding. ty's type
  lattice is richer than Smelt's HIR; we need a defined lossy-but-sound mapping.
- **Salsa lifecycle**: ty is incremental; Smelt is batch. Simplest is a fresh db
  per run (what the spike does). Reuse/caching is a later optimization.
- **Version pinning**: ty's internal API is unstable and moves with ruff. Bumping
  the pinned rev may require touching the mapping layer.
- **Build weight & typeshed**: `ty_vendored` adds compile time and binary size;
  acceptable for a compiler, worth measuring.

## How to run

```bash
cargo run -p smelt-py-ty-spike
```

(See the egress note above for local sandbox builds.)

## Spike result

Built and ran locally (vendored ruff at the pinned rev). Output:

```
== ty diagnostics ==
- Object of type `Literal["not an int"]` is not assignable to `int`

== inferred types (top-level bindings) ==
count : Literal[3]
label : Literal["hello"]
ratio : float
root  : int | float
mixed : list[int]
bad   : Literal["not an int"]
```

This confirms the two capabilities Smelt needs, from a single embedded library:

- **Checking** — the deliberate `bad: int = "not an int"` is reported with a
  precise message.
- **Inference**, including things Smelt's annotation-only path cannot derive
  today: `ratio` is `float` from integer division semantics, `mixed` is
  `list[int]` from element inference, and `root` comes from the **typeshed stub**
  for `math.sqrt` (no user annotation involved). Literal types (`Literal[3]`)
  are also available where Smelt would widen to `int`.

**Conclusion:** embedding `ty` as a Python type source is feasible and the API is
reasonable. The remaining effort is not the embedding but the
`ty` → Smelt-HIR **type-mapping layer** (see risks above).
