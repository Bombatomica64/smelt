# Spike: using Astral's `ty` as Smelt's Python type source

**Status:** productionized for **function/method return-type resolution**
(issue #93). The spike below proved feasibility; the working integration now
lives in `crates/smelt-py-types` and is consumed by `smelt-frontend-py` behind
its optional `ty` feature. See "Productionization (issue #93)" at the end.

The original spike crate `crates/smelt-py-ty-spike` is retained as the minimal
feasibility harness.

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

### Against a real probe library

Smelt's daily `library-probes` CI already exercises **5 Python libraries**
(`returns`, `result`, `more-itertools`, `funcy`, `toolz`) alongside 5 TS ones —
see `.github/compat/libraries.json`. So this isn't hypothetical: there's a real
Python corpus a ty-backed frontend would run against.

Pointing the spike at `rustedpy/result`'s `src/result/result.py` (the pinned
probe ref) — `smelt-py-ty-spike <project-root> src/result/result.py` — gives:

```
== ty diagnostics ==
3 diagnostic(s):
- Return type does not match returned value
- Return type does not match returned value
- Return type does not match returned value

== inferred types (top-level bindings) ==
T : TypeVar      E : TypeVar      U : TypeVar      F : TypeVar
P : ParamSpec    R : TypeVar      TBE : TypeVar
Result : <types.UnionType special-form 'Ok[T] | Err[E]'>
OkErr  : tuple[<class 'Ok'>, <class 'Err'>]
```

ty resolves the library's full type-level vocabulary — `TypeVar`s, a `ParamSpec`,
the `Result = Ok[T] | Err[E]` union alias, and the `OkErr` class tuple — exactly
the generic machinery Smelt's annotation-only path struggles with. The 3
diagnostics are `ty` being stricter than the source's own tooling (`result`
passes mypy/pyright), so they are candidates for ty false-positives or genuine
edge cases to triage — a reminder that **ty is still maturing**: adopting it
means inheriting its current inaccuracies on advanced generics and deciding how
to map/suppress them in Smelt's diagnostics policy.

So: not a dream — ty runs on the real probe repos today and yields usable types.
What's missing is the integration + mapping layer, not the checker.

### Against full applications — and a mypy reality check

Libraries are the easy case (small, dependency-free, heavily typed). The spike
also has a **directory mode** (`smelt-py-ty-spike <project-root>`) that scans
every `.py` file and buckets diagnostics by ty rule id, splitting
`unresolved-*` (environment noise) from real findings. Run against two real apps
whose third-party dependencies are **not installed**:

| App | files | total | `unresolved-*` (noise) | other ("real") |
| --- | ---: | ---: | ---: | ---: |
| `psf/black` (`src/black`) | 25 | 31 | 22 | **9** |
| `httpie/cli` (pkg dir) | 78 | 519 | 365 | 154 |
| `httpie/cli` (project root) | 133 | 952 | 266 | 686 |

Two things jump out:

1. **Most volume is dependency cascade, not bugs.** With deps absent, every
   `import requests`/`click` becomes `Unknown`, and operations on those values
   then trip `unsupported-operator` (309 on httpie), `not-subscriptable` (93),
   `unresolved-attribute` (138)… The dependency-free `result` library produced
   only 3 findings precisely *because* it has nothing to fail to resolve. **A ty
   integration is useless without resolving the project's own modules + its
   third-party deps** (installed env or vendored stubs) — same precondition mypy
   has.

2. **Even with the cascade removed, ty currently over-reports.** `black` is
   heavily typed and CI-enforced. Running **mypy** on the same 25 files
   (`mypy --ignore-missing-imports`) →

   ```
   Success: no issues found in 25 source files
   ```

   …yet ty flags **9** (`invalid-argument-type` ×3, `invalid-raise` ×2,
   `invalid-return-type`, `invalid-yield`, `invalid-assignment`,
   `invalid-ignore-comment`). On type-clean code those are **ty false
   positives** — ty is younger and stricter/buggier than mypy on real apps.

**Takeaways for Smelt:**

- ty's **type inference** (the types of expressions) is the valuable, working
  part and is what a Smelt frontend should consume.
- ty's **diagnostics are not yet a trustworthy "real error" oracle** — they have
  false positives and are meaningless without dependency resolution. Don't
  surface them verbatim as Smelt errors yet.
- mypy is more accurate today but is a Python **subprocess** needing a venv +
  installed deps — not embeddable as a Rust library the way ty is. The trade is
  accuracy/maturity (mypy) vs in-process Rust embedding + inference API (ty).
- The earlier "only return-type mismatches, promising!" was an artifact of
  testing one tiny dependency-free library; full apps need deps resolved before
  any conclusion about ty's accuracy holds.

## Productionization (issue #93)

The spike is now productionized as a focused, opt-in increment: **`ty`-backed
return-type resolution** for functions and methods.

### Crates & wiring

- **`crates/smelt-py-types`** — owns the heavy `ty` dependency tree (salsa +
  vendored typeshed) and exposes one small, stable API:
  `resolve_module_types(source, stem) -> ResolvedModuleTypes`. It runs `ty` over
  the source (materialized into a temp dir + `OsSystem`, exactly as the spike
  proved works) and returns, keyed by **AST byte offset**, canonical Python
  type *spellings* (`"int"`, `"list[int]"`, `"str | None"`). It deliberately
  does **not** leak `ty`'s internal `Type` lattice (almost all `pub(crate)` and
  unstable) — it hands the frontend strings, which the frontend's existing
  `annotation_to_hir` already parses. Kept **out of `default-members`** so lean
  builds never pay for `ty`.
- **`smelt-frontend-py`** — new optional `ty` feature (`dep:smelt-py-types`).
  The `ty_resolve` module is a `#[cfg]` seam presenting the same API with or
  without the feature (a zero-cost stub when off). At each former
  "must have an explicit ... annotation" site, lowering now consults the
  resolver; only genuinely unresolved cases fall through to the (unchanged)
  error. `smelt-codegen-rust` and `smelt-cli` forward the feature.

### What it resolves — and the actual-vs-declared rule

- **Return types** (the dominant blocker: ~389 function + ~162 method
  annotations): derived from `ty`'s inferred types of the body's own
  `return <expr>` statements (unioned; `None` for a valueless function). This is
  the *actual returned* type. When a declared `-> T` annotation is present but
  diverges from `ty`'s inferred return, lowering **prefers `ty`'s inferred
  type**, per issue #93 (the annotation may be stale/wider/narrower).
- Literal narrowings are widened to primitives (`Literal[3]` → `int`), matching
  the frontend's HIR. Dynamic/`Unknown`/`Never`/`@Todo`/special-form spellings
  are dropped so the case stays an **explicit boundary** — never a blanket
  `SmeltUnknown`.

### Deferred (documented follow-ups)

- **Parameter inference.** `ty` reports an *unannotated* parameter as
  dynamic/`Unknown` (it does not infer a param's type from a default value or
  from call sites), so an unannotated parameter still requires a source
  annotation. The plumbing to consume a resolved param type exists
  (`resolved_param_ty`), so annotated/derivable params benefit, but the raw
  "parameter must have an explicit type annotation" blocker (~8-15 occ) is not
  removed by inference here.
- **Nested closure** return/param resolution (`statement.rs`) is unchanged.
- **Cross-module / third-party dep resolution.** As the spike notes, `ty` needs
  the project's own modules + installed deps to resolve non-stdlib types;
  isolated single-file resolution (what the frontend does today) leaves those as
  boundaries. This mirrors the existing "probe lowers files in isolation"
  caveat.

### Build note

The `ty` tree is large (first build ~3 min in this environment; cached
afterwards). It is only compiled when a crate is built with `--features ty`
(CI probes, the compile corpus's Python case, and an explicitly ty-built CLI).
