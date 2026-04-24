# Python Frontend (`smelt-frontend-py`)

The Python frontend turns a `.py` source file into a `smelt_hir::Module` — the **same** type the TS frontend produces. If something doesn't fit, that's a signal HIR needs to grow, not that Python needs its own IR.

## Inputs and Outputs

```
fn check(path: &Path) -> Result<(), Vec<SmeltError>>
fn to_hir(source: &str, file_id: FileId, ctx: &mut HirCtx) -> Result<ModuleId, Vec<SmeltError>>
```

`HirCtx` is the same arena-owning context the TS frontend uses. Both frontends contribute to one shared `smelt_hir::Crate`.

## Pipeline

```
.py source
   │
   ▼
1. ty               — type-check + lint in one pass (strict mode)
   │
   ▼
2. smelt rules      — reject AST shapes with no HIR representation
   │
   ▼
3. HIR construction — walk the typed AST → smelt_hir::Module
```

Phase 1 substitutes for both phase 1 and phase 2 of the TS pipeline (`ty` does both lint and type-check).

### Phase 1: ty

`ty` is a Rust-native Python type-checker (Astral). **Prefer embedding** over shelling out — it's the same language, the API exists, and we want structured access to inferred types for HIR construction.

If embedding turns out to be impractical at the start of M8, fall back to shelling out and parse JSON diagnostics. Decision is recorded as part of M8.

Strict-mode requirements (`ty` must report zero errors and the file must satisfy):

- Every function parameter and return has a type annotation.
- Every module-level variable has an annotation.
- Every class attribute is annotated (in `__init__` or as a class-level annotation).
- No `Any`, `object` as a fallback type, `cast`, `# type: ignore`.

Failures at this phase produce `SmeltError`s with file:line:col spans from `ty`'s diagnostics.

### Phase 2: smelt rules

Visitor over the `tree-sitter-python` parse tree (already type-annotated by phase 1's binding to ty's symbol table). Each rule produces zero or more `SmeltError`s.

| Rule code                          | Rejects                                                      |
|------------------------------------|--------------------------------------------------------------|
| `smelt::no-any`                    | `Any`, `object` used as a fallback type, `# type: ignore`    |
| `smelt::no-cast`                   | `typing.cast(...)`                                           |
| `smelt::no-eval`                   | `eval`, `exec`                                               |
| `smelt::no-dynamic-attr`           | `getattr`/`setattr`/`hasattr`/`delattr` with non-literal name|
| `smelt::no-metaclass`              | metaclass other than `type`                                  |
| `smelt::no-multiple-inheritance`   | `class C(A, B):` with more than one non-`Protocol` base      |
| `smelt::no-decorators`             | any decorator outside the v1.0 allowlist                     |
| `smelt::no-varargs`                | `*args`, `**kwargs` in v1.0                                  |
| `smelt::no-dynamic-imports`        | `importlib`, `__import__`                                    |
| `smelt::no-module-side-effects`    | top-level statements that aren't definitions or `if __name__`|

Decorator allowlist (v1.0): `@dataclass`, `@property`, plus FastAPI route decorators recognized by M9.

### Phase 3: HIR construction

Walk the parse tree and produce HIR. Per-construct mapping below.

#### Module-level

- Top-level statements outside of definitions and `if __name__ == "__main__":` are rejected by `smelt::no-module-side-effects`.
- `import x`, `from x import y` → `smelt_hir::Import`.
- `def`, `async def` → `Item::Function`.
- `class C:` → `Item::Class`.
- Type aliases via `TypeAlias` (PEP 695) or `X = list[int]` at module level → `Item::TypeAlias`.
- Module-level annotated constants → `Item::Const`.

#### `with` desugaring

Python `with f as h: body` is desugared *before* HIR construction to:

```python
h = f.__enter__()
try:
    body
finally:
    f.__exit__(None, None, None)
```

The desugarer runs as a tree-sitter rewrite step. The HIR walker only ever sees the desugared form.

#### Comprehensions

Stay as `ExprKind::Comprehension`. Lowering to iterator chains happens in MIR.

#### Dataclasses

`@dataclass` is recognized in the frontend: the decorated class becomes an `Item::Class` whose fields are populated from the class body's annotated attributes. The synthesized `__init__` is materialized into HIR explicitly so MIR doesn't need to know about `@dataclass`.

#### `__init__`

Class constructors become a method named `__init__` (canonical HIR convention shared with the TS frontend). `self` is parameter 0.

#### Names

Python is already snake_case. Class names stay PascalCase. No conversion at the boundary, but every name is still interned through `OriginalNameTable` so cross-language imports work uniformly.

## Type Mapping

| Python type                        | HIR type                                  |
|------------------------------------|-------------------------------------------|
| `int`                              | `Type::Int`                               |
| `float`                            | `Type::Float`                             |
| `str`                              | `Type::String`                            |
| `bool`                             | `Type::Bool`                              |
| `None` / `NoneType`                | `Type::None`                              |
| `list[T]`                          | `Type::List(T)`                           |
| `dict[K, V]`                       | `Type::Dict(K, V)`                        |
| `tuple[T, U, ...]`                 | `Type::Tuple([T, U, ...])`                |
| `set[T]`                           | (lowered to `Type::List(T)` w/ semantic note? open question — see below) |
| `Optional[T]` / `T \| None`        | `Type::Optional(T)`                       |
| `Union[T, U]` (discriminated)      | `Type::Union([T, U])`                     |
| `Awaitable[T]` / `Coroutine[…, T]` | `Type::Future(T)`                         |
| User class                         | `Type::Class(id, args)`                   |
| `TypeVar('T')`                     | `Type::TypeVar(id)`                       |
| `Callable[[A], R]`                 | `Type::Function(FunctionType)`            |

Open question: `set[T]` — does HIR get a dedicated `Type::Set`, or do we lower it to `List` with set-semantics encoded via the stdlib mapping? Decide during M8 implementation; default to a dedicated `Type::Set` and update the HIR spec.

## Cross-Frontend Equivalence

A success criterion of M8: at least 5 snapshot tests where a TS file and a Python file produce *equivalent* HIR (modulo names). Suggested fixture pairs:

- `add(a, b)` function on integers
- A `Point` class with `distance` method
- A discriminated union for a tagged event
- An async function that awaits another
- A list comprehension producing transformed values

These prove the shared-IR thesis.

## Errors

Same `SmeltError` type as the TS frontend. Errors carry file/line/col from `ty`'s diagnostics or from the `tree-sitter-python` node.

## Current Gaps

- `lib.rs` is one comment line. Nothing exists yet.
- No decision recorded on embed-vs-shell-out for `ty`.
- No `with` desugarer, no rule visitor, no walker.
- No `tree-sitter-python` integration even though the dep is in `Cargo.toml`.

## TODO (concrete, ordered)

### Foundation

- [ ] Wire `tree-sitter` and `tree-sitter-python` into a parsing helper that returns a tree + source bytes.
- [ ] Decide embed-vs-shell-out for `ty`. Default to embed; record the decision in `specs/check-pipeline.md`.
- [ ] Add a `PyTypeInfo` struct (or use `ty`'s native types directly) that lets the walker look up the inferred type at any tree-sitter node.
- [ ] Reuse the `SmeltError` and `Span` types from the TS frontend.

### Pipeline

- [ ] Implement `check(path)` that runs `ty` strict mode and returns `Vec<SmeltError>`.
- [ ] Implement the smelt-rules visitor; one rule per file under `src/rules/`. Unit-test each with positive/negative `.py` snippets.

### Pre-HIR rewrites

- [ ] Write the `with` → `try/finally` desugarer over tree-sitter. Round-trip tested against handwritten equivalents.
- [ ] Write the `@dataclass` materializer that synthesizes an explicit `__init__` in HIR.

### HIR construction

- [ ] Write `py_ty_to_hir(ty_ty, ctx) -> TypeId` for every supported Python type.
- [ ] Write the walker: module → items → bodies → statements → expressions. Mirror the TS walker's structure.
- [ ] Set `is_async = true` on `async def`s; treat `await` as `ExprKind::Await`.
- [ ] List/dict/set comprehensions and generator expressions → `ExprKind::Comprehension`.
- [ ] Class inheritance: capture `bases`; reject if more than one non-`Protocol`.

### Tests

- [ ] 50+ snapshot tests covering the supported subset.
- [ ] 20+ negative tests for rejected constructs.
- [ ] 5+ cross-frontend equivalence tests (TS + Python pairs producing matching HIR).
- [ ] HIR validator passes on every snapshot.

### Followups (M9 territory, listed for context)

- [ ] Pydantic model recognition.
- [ ] FastAPI route-decorator allowlist + extraction.
- [ ] Python stdlib mappings beyond what M6 covers.
