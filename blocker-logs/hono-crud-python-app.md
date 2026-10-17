# hono_crud with a Python app over the TypeScript Hono library: design and cost

Question: can the `hono_crud` showcase keep Hono (TypeScript) as the library and rewrite the
*app* modules (`app`, `routes`, `repository`, `model`, `db`, `main`) in Python, all transpiled by
Smelt into one Rust crate? What exists today, what is missing, and what would it cost?

Method: read-only source investigation on branch `claude/hono-crud-perf` (2026-10-16). Nothing
was compiled or run for this report; every "works" claim below is backed by an existing test or by
code reading, and is marked as such. Anything not established is marked **unverified**.

## Summary verdict

**The plumbing for one mixed-language crate exists and is tested, but only for the thin case of a
Python module calling a free TypeScript function with primitive types.** Both frontends write into
one shared `smelt_hir::Crate` in a single build, the manifest walks imports in both directions, and
MIR already applies per-file language semantics. A Python→TS function call and a TS→Python function
call both build and run end to end.

**What Hono needs is missing in four places:**

1. **Import binding.** A Python `from hono import Hono` does not resolve a bare package
   (Python imports only look next to the importer), and even when the TS module is in the crate the
   Python frontend binds imported names by a flat, last-wins name table across the whole crate, not
   by the TS module's exports.
2. **TS type surface.** The TS-only facts Hono's API relies on (overload sets, callable-interface
   call signatures, callable fields, rest params, class type-parameter defaults, cross-module type
   renames) live in TypeScript-only side state, not in HIR, so the Python frontend cannot see them.
   `app.get(...)` (a field typed by an overloaded callable interface), `c.json(...)` (a getter
   returning an overloaded callable interface) and `c.req.param('id')` (an overloaded method) are
   exactly these shapes.
3. **Callbacks and async.** Hono handlers are closures over the repository and half of them
   `await c.req.json()`. The Python frontend rejects nested `async def` closures.
4. **Boundary values.** Python `int` vs TS `number` (`Float`), Python `object` vs TS `unknown`,
   and Python exceptions vs JS `Error` (`instanceof Error` in `onError`) have no cross-language
   adapters yet.

The general design (Option B below: one call-resolution surface in HIR that any frontend reads) is
the one compatible with CLAUDE.md's "no special cases". **Estimate: 27-49 engineer-days** for the
showcase (31-57 with optional `.pyi`/IDE stubs), in seven
independently testable phases; the dominant risk is phase 3 (TS overload/callable-interface
resolution from Python).

## What exists today

### One crate, two frontends, one build

- `SourceLang` distinguishes TS, `.d.ts`, Python and `.pyi` by extension
  (`crates/smelt-transpiler/src/lowering.rs:24-60`).
- `lower_ordered_manifest_sources` creates **one** `smelt_hir::Crate` and lowers every manifest
  source into it in order, threading the crate through each file
  (`crates/smelt-transpiler/src/lowering.rs:931-972`). The module doc calls this "mixed-language
  manifests" (`lowering.rs:1`).
- `lower_manifest_source` dispatches per file: TS files get a `smelt_frontend_ts::HirCtx` rebuilt
  from ~25 TS-only side tables (`lowering.rs:1060-1137`); Python files get only the crate plus
  `py_module_namespaces` and `py_enum_members` (`lowering.rs:1139-1150`,
  `crates/smelt-transpiler/src/python.rs:89-122`). The TS side tables pass through a Python file
  untouched (`lowering.rs:1151-1177`), so they survive, but **Python never reads them**.
- The cross-file TS-only state is `FrontendLoweringState` (`lowering.rs:78-157`): overloads
  (`:97`), rest params (`:99`), class type-param defaults (`:108`), interface call/construct
  signatures (`:120-122`), callable fields (`:124`), type renames for ambiguous class names
  (`:150`), module exports by path (`:83`). HIR `Interface` has no call-signature field
  (`crates/smelt-hir/src/item.rs:251-264`), which is why these live outside HIR.
- Pre-passes are TS-only: type predeclaration (`lowering.rs:885`), host-global scanning
  (`lowering.rs:704-707`), type renames (`lowering.rs:761`).
- Ordering: dependency post-order DFS over the manifest sources
  (`crates/smelt-transpiler/src/manifest.rs:174-189`), so an imported TS module is lowered before
  the Python file that imports it, **when the import edge was discovered**.
- MIR already tells the languages apart per file: absent values print as `None` for Python and
  `undefined` for TS (`crates/smelt-mir/src/lower/context.rs:294-299`,
  `crates/smelt-mir/src/types.rs:596-614`), and negative indexing follows each language's rule
  (`context.rs:311-316`).
- HIR types are shared: `Bool/Int/Float/String/List/Dict/Optional/Union/Class/Function/Future/...`
  (`crates/smelt-hir/src/ty.rs:11-87`). TS `number` lowers to `Float`
  (`crates/smelt-frontend-ts/src/lowering/ty/annotations.rs:27`); Python `int`→`Int`,
  `float`→`Float`, `str`→`String` (`crates/smelt-frontend-py/src/lowering/types.rs:481-484`).
  Both `throw` and `raise` lower to `HirStmt::Throw` (`crates/smelt-hir/src/body.rs:279-282`,
  `crates/smelt-frontend-py/src/lowering/statement.rs:119-137`).

### Existing cross-language tests and demos

- `build_runs_python_entry_importing_typescript_function`: `from math import add` against
  `src/math.ts`, built and run, prints `5`
  (`crates/smelt-transpiler/tests/hir_cli_cross_language_tests.rs:14-52`). Note the caller must
  annotate `result: float` and pass floats.
- `build_keeps_one_main_when_python_entry_imports_typescript_main_module`: `from lib.main import add`
  discovers `src/lib/main.ts` **implicitly** from a Python entry (`:55-108`).
- `build_runs_typescript_entry_importing_python_function`: the reverse direction (`:255-298`).
- The GUI's default sample is "Cross-language add" (`crates/smelt-gui/src/examples.rs:72-134`),
  lowered TS-first then Python into one crate (`crates/smelt-gui/src/compiler.rs:1-18`).
- No test covers a Python module using a TS **class**, generic, method, callback, or async value.

### Module resolution

- Manifest import discovery splits by the *importer's* language
  (`manifest.rs:326-343`): TS importers use the oxc resolver plus workspace-package lookup
  (`manifest.rs:346-406`); Python importers use `python_import_base`, which is
  **importer directory + dotted path** (or the relative-level base) (`manifest.rs:454-476`).
- Python candidates already include TS files: `X.ts`, `X.d.ts`, `X/index.ts` before `.py`/`.pyi`
  (`manifest.rs:590-611`). That is why the tests above work. The TS resolver likewise accepts `.py`
  (`manifest.rs:479-492`).
- Workspace packages (how the staging tree resolves `hono` to `packages/hono/src/index.ts` via
  `package.json` `"source"`; `benchmarks/hono_crud/prepare-staging.sh:31-32`,
  `manifest.rs:666-712`) are consulted **only** for TS importers. A Python `from hono import Hono`
  in `src/routes.py` looks for `src/hono.ts`, `src/hono/index.ts`, `src/hono/__init__.py`, ... and
  finds nothing. `[sources].roots` is not used as a Python search path (**verified by reading
  `python_import_base`; no other Python resolution path found**).
- In the Python frontend, `from M import N` first looks in `module_namespaces[M]`, which only
  Python modules populate (`crates/smelt-frontend-py/src/lowering/module_imports.rs:71-93`,
  `:109-131`), then falls back to `items[N]`. `items` is seeded from **every named item in the
  crate, keyed by bare name, last write wins** (`crates/smelt-frontend-py/src/lowering/validate.rs:2-15`).
  So `from math import add` binds to "whichever item named `add` was lowered last" — it works in the
  test because there is one `add`. Hono's 63+ modules contain repeated names (`Context` is a known
  cross-kind collision, see `blocker-logs/hono-h71-cross-kind-type-name-collision.md`), so this
  fallback is not a sound binding rule for Hono.

### Type surfaces seen from Python

- Python annotations resolve names locally: a non-builtin name becomes
  `Type::Class { name, args: [] }` by bare name (`crates/smelt-frontend-py/src/lowering/types.rs:501-508`).
  So `c: Context` gets no type arguments (TS fills `Context<E, P, I>` defaults from
  `ts_class_type_params`), ignores the TS rename table, and treats TS interfaces/type aliases
  (e.g. `Env`) as classes. `object` becomes `Class("object")`, not `Type::Unknown` (`types.rs:486-493`).
- Python method calls resolve through `class_methods` built from crate items with
  `FunctionOwner::ClassMethod` (`validate.rs:18-42`, `crates/smelt-frontend-py/src/lowering/call.rs:1063`).
  That reaches ordinary TS class methods, but not Hono's route registrars: `get!`/`post!` are
  **fields** typed `HandlerInterface<...>` assigned in the constructor
  (Hono `src/hono-base.ts:116-124, 186-190`), and `HandlerInterface` is an interface of ~20
  overloaded call signatures (Hono `src/types.ts:127-142`). `c.json` is a getter returning the
  overloaded callable interface `JSONRespond` (Hono `src/context.ts:222-237, 789`), and
  `c.req.param` is an overloaded method (Hono `src/request.ts:97-99`). (Hono line numbers are from
  the pinned compat checkout at `.../claude-hono-v5-phase4/target/compat-repos/hono/src`.)
- `ty` is used only to fill missing Python annotations, one module at a time, in a fresh temp dir
  containing just that file (`crates/smelt-py-types/src/lib.rs:157-200`; search path = that temp
  root, `crates/smelt-py-types/src/db.rs:57-60`). It never sees sibling modules, let alone TS ones;
  unresolved imports yield `Unknown`, which is dropped (`lib.rs:140-156` doc). So **today `ty` adds
  nothing across a language boundary**: every value coming from TS must be annotated by hand.

### Stubs

- Every `smelt build`/`check` writes a `.d.ts` **and** a `.pyi` next to every lowered source,
  including TS sources (`crates/smelt-transpiler/src/stubs.rs:16-33`, called from
  `crates/smelt-transpiler/src/pipeline.rs:86-88, 124-126`). For the staging tree this would write
  `.pyi` files into `packages/hono/src/` (**unverified that this happens in the staging run; it
  follows from the code**).
- The `.pyi` content is minimal: module functions and classes with fields only, no methods, no
  generics (type args dropped), `Callable` without a signature, interfaces/aliases/consts skipped
  (`stubs.rs:193-243, 417-471`). Generated stubs are skipped when fed back in
  (`crates/smelt-frontend-py/src/lib.rs:419-449`); a hand-written `.pyi` would be lowered as
  ordinary Python, creating *new* Python items rather than binding to the TS implementation.

### Async, callbacks, exceptions on the Python side

- Top-level `async def` and `await` lower (test `async_function_and_await_lower`,
  `crates/smelt-frontend-py/src/tests/basic_tests.rs:188-240`); `asyncio` APIs are classified into
  runtime-neutral async ops (`crates/smelt-asyncio/src/lib.rs:1-30`). Python `async def` returns
  `Type::Future`, the same HIR type TS `Promise` lowers to.
- **Nested `async def` closures are rejected**: "async nested closures need async closure-body
  lowering" (`crates/smelt-frontend-py/src/lowering/statement.rs:730-735`). Nested sync `def`
  closures and lambdas with a type hint are supported (`statement.rs:720-760`,
  `crates/smelt-frontend-py/src/lowering/literals.rs:281, 317`).
- A Python `except ValueError as error` binding is used as a `str` in a test
  (`crates/smelt-frontend-py/src/tests/lang_features_tests.rs:55-59`), which suggests Python caught
  exceptions are message strings, whereas TS `catch`/`onError` sees an `Error` object. How a Python
  `raise` arrives at a TS `err instanceof Error` check is **unverified**.
- `crates/smelt-python` provides typed protocol traits (`PyInit`, `PyAdd`, ...) for Python classes
  (`crates/smelt-python/src/lib.rs:1-40`); nothing there is Hono/TS-specific, and no
  Python-specific runtime value type was found that would conflict with TS `String`/`Vec`/records
  (**generated-code compatibility for classes/records across languages is unverified**).

### Config and specs

- `[strict] typescript/python` are parsed but unused (`#[expect(dead_code)]`,
  `crates/smelt-transpiler/src/config.rs:236-247`); `specs/config.md:92-98,139` says both must be
  true in v1.0. They have no effect on mixed builds.
- `specs/architecture.md:25,52`: the shared HIR exists so "an import resolver can connect them after
  lowering"; `specs/check-pipeline.md:84`: cross-language import resolution is "a v1.x milestone",
  and the check pipeline has no cross-language phase. `specs/frontend-py.md:116`: names are interned
  through one table "so cross-language imports work uniformly".
- `specs/m9-fastapi-pydantic.md` plans a *Python-only* FastAPI/Pydantic demo mapped to axum, with a
  "cross-language convergence test" comparing the Express and FastAPI demos' HIR and HTTP behaviour.
  It does not plan Python code calling a TS library. No spec or blocker log covers that.

## Design

### Target app

Hono, the HTTP shim (`serve.ts` from the benchmark staging) and the in-memory store stay TS or move
to Python as the user prefers; the minimum interesting split is: `packages/hono/**` and `serve.ts`
in TS, everything under `src/` in Python. Handlers are plain functions; no decorators (decorator
routing would be a FastAPI-style recognizer, a different project).

```python
# src/todos/routes.py
from dataclasses import dataclass
from hono import Hono, Context          # bare package import into the TS workspace package
from todos.model import NewTodo, TodoPatch
from todos.repository import TodoRepository


def parse_id(raw: str) -> int | None:
    if not raw.isdigit():
        return None
    value = int(raw)
    return value if value > 0 else None


def validate_new_todo(body: object) -> NewTodo | str:   # object <-> TS unknown (JSON boundary)
    if not isinstance(body, dict):
        return "body must be a JSON object"
    title = body.get("title")
    if not isinstance(title, str) or title.strip() == "":
        return "title must be a non-empty string"
    done = body.get("done", False)
    if not isinstance(done, bool):
        return "done must be a boolean"
    return NewTodo(title=title, done=done)


def create_todos_router(repository: TodoRepository) -> Hono:
    router = Hono()

    router.get("/", lambda c: c.json(repository.list()))

    def get_one(c: Context) -> Response:
        todo_id = parse_id(c.req.param("id"))
        if todo_id is None:
            return c.json({"error": "id must be a positive integer"}, 400)
        todo = repository.get(todo_id)
        if todo is None:
            return c.json({"error": "todo not found"}, 404)
        return c.json(todo)

    async def create(c: Context) -> Response:      # needs nested async closures
        validated = validate_new_todo(await c.req.json())
        if isinstance(validated, str):
            return c.json({"error": validated}, 400)
        return c.json(repository.create(validated), 201)

    router.get("/:id", get_one)
    router.post("/", create)
    # patch / delete analogous
    return router
```

```python
# src/app.py
from hono import Hono, Context
from todos.repository import TodoRepository
from todos.routes import create_todos_router


def create_app(repository: TodoRepository) -> Hono:
    app = Hono()
    app.route("/todos", create_todos_router(repository))
    app.notFound(lambda c: c.json({"error": "route not found"}, 404))

    def on_error(err: Exception, c: Context) -> Response:
        return c.json({"error": str(err)}, 500)

    app.onError(on_error)
    return app
```

```python
# src/main.py
from serve import serve                 # TS shim, relative-path import
from app import create_app
from todos.repository import TodoRepository

app = create_app(TodoRepository())
serve(app.fetch, 3000)
```

`Response` is the TS host type returned by `c.json`; Python needs it nameable (see type surface).
The IdContext trick (`Context<Env, '/:id'>` making `param('id')` a `str`) cannot be spelled from
Python; without it `c.req.param("id")` is `str | None` per Hono's overloads, so the Python app
narrows it — that is a faithful translation, not a loss.

### How the import resolves

General rule (no Hono special case): **a Python importer may resolve a module through the same
project resolver TS uses, after the Python-native candidates fail.**

1. `python_import_base` candidates (unchanged, `manifest.rs:454-476`), then
2. each `[sources].roots` entry + dotted path (Python's `sys.path` analogue), then
3. for a level-0 import whose first segment names a workspace package
   (`find_workspace_package_entry`, `manifest.rs:627-712`), that package's entry, with the
   remaining dotted segments mapped to `/` sub-paths (`from hono.http_exception import X` →
   `hono/src/http-exception.ts` needs the package's export map — **open question** for `-`/`_`).

The manifest records, per Python import, the **resolved source path**, and passes it into the
Python frontend (today it passes only source text, `python.rs:89-122`). The Python frontend then
binds `from M import N` by **module identity**: if `M`'s resolved path is a TS module, look `N` up
in `ts_module_exports[path]` (`lowering.rs:83`), honouring TS re-exports/barrels; if it is Python,
use `module_namespaces` as today. The flat `visible_items` fallback stays only for names with no
module (builtins), and a cross-language miss becomes a diagnostic instead of a by-name guess.

### How Python gets the TS type surface

| Option | What | General? | Verdict |
| --- | --- | --- | --- |
| **A. Generate `.pyi` from HIR, feed `ty`** | Extend `stubs.rs` to emit methods, generics, `@overload`, `Protocol.__call__` for callable interfaces; put the stub dir on `ty`'s search path so `ty` types the Python app | General, but **lossy**: TS conditional/template-literal types, defaults over generic constraints, and `this`-typed fluent returns (`HonoBase<E, S & ...>`) have no Python spelling; and `ty`'s answer comes back as a type *string* re-parsed by the frontend (`smelt-py-types/src/lib.rs:21-33`), so lowering would still need the HIR binding to emit calls | Useful for **IDE/`ty` checking of the app**, not sufficient for lowering |
| **B. Python frontend reads TS HIR (+ the TS call-resolution facts) directly** | Move the TS-only facts needed to *call* things (overload sets, interface call/construct signatures, callable fields, rest params, type-param defaults, renames) from `FrontendLoweringState` into HIR (or a shared read-only "foreign surface" view), and teach Python call/annotation lowering to resolve against them | General: one rule for "call a declared callee from another frontend"; also what TS→Python would need | **Recommended** for lowering |
| **C. Hand-written `hono.pyi`** | Write Hono's Python surface by hand | **Special case** per CLAUDE.md "Type lowering": a per-library spelling. Also does not work mechanically: a non-generated `.pyi` is lowered as ordinary Python (`frontend-py/src/lib.rs:419-449`) and would create *new* items, not bind the TS implementation | Rejected as a Smelt mechanism; acceptable only as a user-side IDE aid |

Recommended: **B for lowering, A (improved) as a follow-up for the developer experience**, with A
generated from the same HIR surface B introduces so the two cannot drift.

Concretely, B means:

- HIR `Interface` gains `call_signatures` / `construct_signatures` (today side tables
  `lowering.rs:120-122`); HIR functions/methods gain their overload signature list (today
  `lowering.rs:97`); callable intersection fields move likewise (`:124`). The TS frontend keeps
  reading them from HIR instead of its side tables (a refactor, not a behaviour change, guarded by
  the existing Hono/es-toolkit corpora and the `smelt-unknown` baselines).
- Python annotation lowering resolves a name through the import binding first, so `Context`
  becomes the right (possibly renamed) class symbol, with missing trailing type args filled from
  `TypeParamDef.default` (already in HIR, `item.rs:275-286`). Interface and alias names lower to
  their real HIR kinds, not `Class(name)`.
- Python call lowering: receiver type → class/interface → method, field-of-callable-interface, or
  getter-returning-callable → overload selection by argument HIR types (same algorithm the TS
  frontend uses; ideally the same code moved into a shared crate, e.g. `smelt-hir` helpers or a new
  `smelt-surface`), with generic instantiation from argument types.

### How values cross

| Concern | Rule | Status |
| --- | --- | --- |
| `str`/`string` | both `Type::String` | works (shared HIR) |
| `int`/`float` vs `number` | Python `int` argument to a TS `Float` parameter: implicit widening at cross-language call sites only (TS→Python `Float` to `int` is a narrowing and must be rejected or explicitly `int(...)`) | missing; today the tests sidestep it with `float` (`hir_cli_cross_language_tests.rs:42`) |
| `None` vs `undefined`/`null` | both `Type::None`/`Optional`; MIR prints per file language | works (`mir/src/lower/context.rs:294-299`) |
| `list`/`T[]`, `dict`/`Record` | both `List`/`Dict` | shared; **unverified** for Python `dict` literals against TS object-literal/interface parameter types (e.g. `c.json({"error": ...})` where TS infers a record type) |
| Python dataclass into `c.json(T)` | JSON serialization of a Python class value through Hono's `JSON.stringify` path | **unverified**; needs a test |
| TS `unknown` (`await c.req.json()`) | Map Python `object` at a cross-language boundary to `Type::Unknown` (genuine dynamic boundary: JSON), and make Python `isinstance`/`dict.get` narrowing operate on `Unknown` | missing (`object` → `Class("object")`, `types.rs:486-493`). This is a legitimate-boundary `SmeltUnknown` use and must be documented and tested per CLAUDE.md |
| async | Python `async def` → `Future(T)`, same as TS `Promise<T>`; a handler returning `Future(Response)` must fit Hono's `Response | Promise<Response>` handler union through the existing callback-into-union-function-arm rule | top-level async works; nested async closures rejected (`statement.rs:730-735`); union fit **unverified** |
| exceptions | One crate-wide exception value model: a Python `raise ValueError("x")` must be catchable by TS `catch (err)` and satisfy `err instanceof Error` with `.message`; a TS `throw new Error` must be catchable by Python `except Exception as e` | missing / **unverified** (Python bindings appear to be strings, `lang_features_tests.rs:55-59`) |

### Build and manifest

One `Smelt.toml`, both languages, no new section required:

```toml
[sources]
entries = ["src/main.py"]
roots = ["src"]
exclude = ["packages/hono/src/client/**", "packages/hono/src/helper/testing/**"]
```

Discovery walks `main.py` → `app.py` → `hono` (workspace package, via the new Python resolution
fallback) → Hono's TS closure. The existing post-order sort lowers Hono's TS modules before the
Python modules that import them (`manifest.rs:174-189`). The TS-only pre-passes
(`lowering.rs:885`, `704-707`, `761`) already run over all TS sources first. Required changes:
pass resolved import paths to the Python frontend; stop writing `.pyi`/`.d.ts` into vendored
package directories (or write them into the output dir) — **open question**.

## Phased plan

Each phase ends with a fixture under `crates/smelt-transpiler/tests/` (build + `cargo run` + exact
stdout, like `hir_cli_cross_language_tests.rs:14-52`) and an HIR type assertion, so both
behaviour and type preservation are checked (CLAUDE.md "Verification contracts"). Estimates are
engineer-days for one engineer familiar with the codebase.

| # | Phase | Work | Tests added | Estimate | Main risks |
| --- | --- | --- | --- | --- | --- |
| 1 | Resolution by module identity | Python import fallback to `roots` and workspace packages; pass resolved paths to the Python frontend; bind TS exports via `ts_module_exports` incl. barrels/re-exports; diagnostic on cross-language miss | `.py` imports a TS function from a **workspace package**; two TS modules export the same name, Python imports one and gets that one (defect check: revert to flat lookup → test fails); barrel re-export | 2-4 | dotted-name ↔ file mapping for packages (`-` vs `_`, subpaths) |
| 2 | TS classes from Python | Construct TS class (constructor default params), call methods, read/write fields, annotation names through import binding and rename table, interface/alias annotation kinds, generic default fill-in from `TypeParamDef.default` | `.py` constructs a TS `class Counter<T = number>` and calls methods; renamed class (`Context` collision analogue); interface-typed parameter | 4-7 | constructor ABI differences (Python `PyConstruct` vs TS `new`); private `#fields` |
| 3 | Callable surfaces in HIR | Move overloads, interface call/construct signatures, callable fields, rest params into HIR; TS frontend reads them back (no behaviour change); Python overload resolution + generic instantiation on calls | `.py` calls an overloaded TS function, a field typed by an overloaded callable interface, a getter returning one, a rest-param function; regressions: Hono `cargo check` and es-toolkit/remeda unknown baselines unchanged | 8-15 | largest refactor; TS frontend (~121k lines) depends on the side tables in many places; avoidable-`SmeltUnknown` must not rise |
| 4 | Callbacks and async | Python `def`/`lambda`/`async def` passed where TS expects a function type or a callable interface; nested `async def` closures (remove `statement.rs:730-735` rejection); fit into `Response | Promise<Response>` unions | `.py` passes a nested sync closure, a lambda and a nested async closure to TS higher-order functions; TS awaits a Python future and vice versa | 5-10 | async closure-body lowering in the Python frontend is new work; capture semantics |
| 5 | Boundary values | `int`→`number` widening at cross-language calls; `object`↔`unknown` with `isinstance`/`dict.get` narrowing on `Unknown`; unified exception value across languages; Python dataclass/dict through TS JSON paths | numeric widening (and rejected narrowing); JSON round-trip of a Python dataclass via a TS function; Python `raise` caught by TS `catch` with `instanceof Error` and `.message`, and the reverse | 5-8 | exception model unification touches both frontends' try/catch lowering; new `SmeltUnknown` sites need the CLAUDE.md justification + test |
| 6 | Hono showcase | Port `src/` to Python (in-memory store as in the staging tree), run the benchmark staging flow and `benchmarks/hono_crud/probe.sh` byte-for-byte conformance against the TS variant, then `bench.sh` | `examples/python/hono_crud_py` (or a staging variant) with route conformance and the 8 `todos.test` behaviours as pytest | 3-5 | Hono generic surfaces not covered by phases 2-5 (e.g. `route()` returning `HonoBase<E, S & ...>`), long generated-crate build times |
| 7 | (optional) `.pyi` for `ty`/IDE | Emit full stubs from the phase-3 HIR surface (methods, `@overload`, `Protocol.__call__`, generics) into the output dir; optionally put them on `ty`'s search path so missing annotations in the app can be inferred | `ty check` of the Python app against generated stubs has no unresolved-import/attribute errors | 4-8 | Python cannot spell several TS types; stubs would be approximations, so they must never feed lowering decisions |

**Total: 27-49 days for phases 1-6, 31-57 with phase 7**, i.e. roughly 6-11 engineer-weeks. Phases
1-2 alone (6-11 days) already give a credible "Python app calls a TS library class" demo; Hono
specifically needs phase 3.

## Open questions for the user

1. Which modules should be Python? All of `src/` (repository and model too), or only the routing
   layer with the data layer staying TS? All-Python makes phase 5 (dataclass→JSON) mandatory.
2. Import spelling: is `from hono import Hono` (Python resolves the TS workspace package) the
   desired surface, or is a relative `from .vendor.hono import Hono` acceptable for a first demo?
3. Should Python see TS camelCase names verbatim (`app.notFound`, `c.req.param`), or should Smelt
   offer a snake_case view (`app.not_found`)? Verbatim is the general, no-special-case default.
4. Is a nested `async def` handler a requirement, or are module-level `async def` handlers that
   read a module-level repository acceptable for the first demo (would defer part of phase 4)?
5. Generated `.pyi`/`.d.ts` are written next to every source today, including vendored library
   sources. Keep that, or move them to the output directory as part of this work?
6. Is the target the existing `node:http` shim (`serve.ts`) as the server entry, as in the
   benchmark staging tree, given the shipped app's `@hono/node-server` and `node:sqlite` still do
   not lower (`benchmarks/hono_crud/README.md`)?

## What this report does not establish

- No build, test or generated-Rust inspection was run; "works" claims rest on existing tests
  (cited) or on code reading.
- Not checked: how Python class values serialize through TS JSON code; how Python exceptions
  surface in TS `catch`; whether Python lambdas typed against TS function parameters lower
  without annotations; whether TS constructor default parameters are applied for Python
  `Hono()` calls.
- Estimates are judgement calls from the size of the affected code (TS frontend ~121.6k lines,
  Python frontend ~16.6k lines excluding tests), not measured.
