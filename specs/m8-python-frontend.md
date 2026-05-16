# M8: Python Frontend — Core Subset

**Milestone:** v1.0
**Estimated duration:** 6–8 weeks
**Depends on:** M2 (HIR must be stable)

## Goal

Mirror of M1 for Python: parse strictly-typed Python and produce the **same** HIR that the TS frontend produces.

## Why this matters

This is the milestone that proves the architectural bet — that a single HIR can serve both languages. If HIR turns out to need language-specific extensions, we want to find out here, not in M9 when we're trying to compile FastAPI.

This milestone runs in parallel with M3–M7 once M2 is stable. It does not depend on the MIR or codegen work — it only needs HIR to be settled.

## Strict Mode Definition

Python is "strict" for smelt purposes when:

- All function parameters and returns have type annotations.
- All module-level variables have type annotations.
- All class attributes are declared with annotations (either in `__init__` with type hints or as class-level annotations).
- `ty` reports zero errors in strict mode on the file.
- No use of `Any`, `object` as a fallback type, `cast`, or `# type: ignore`.

The frontend rejects anything that fails these checks with a source-located error.

## Supported Constructs

### Types
- `int`, `float`, `str`, `bool`, `None`
- `list[T]`, `dict[K, V]`, `tuple[T, U, ...]`, `set[T]`
- `Optional[T]` and `T | None`
- `Union[T, U]` only when discriminated (a literal field discriminates the variants)
- `Awaitable[T]`
- User-defined classes including dataclasses and Pydantic-style models (Pydantic-specific support is in M9)
- Generics via `TypeVar` and `Generic[T]`

### Statements and Expressions
- All arithmetic, logical, comparison operators
- `if` / `elif` / `else`
- `for` / `while` / `break` / `continue`
- `try` / `except` / `finally` / `raise`
- `with` blocks (desugared to try/finally before HIR)
- `def` and `async def`
- `class` declarations with single inheritance
- `await`
- List/dict/set comprehensions and generator expressions

## Explicitly Rejected

- `Any`, `object` as a fallback, `# type: ignore`
- `eval`, `exec`, `getattr`/`setattr` with non-literal names
- Metaclasses (other than `type`)
- Multiple inheritance (mixins)
- Decorators in v1.0 except a small allowlist (`@dataclass`, `@property`, FastAPI route decorators recognized in M9)
- `*args` and `**kwargs` in v1.0
- Dynamic imports (`importlib`)
- Module-level code that does work (only definitions allowed at module level — execution must be inside `if __name__ == "__main__":` or a `main()` function)

## Implementation Notes

- Parse with `tree-sitter-python`.
- Use `ty` for type information. Either embed it (it's Rust!) or shell out — decide early in the milestone. Embedding is preferred.
- The walker produces `smelt_hir::Module` — the same type the TS frontend produces. If a construct doesn't fit, that's a signal HIR needs to grow, not that Python needs its own IR.
- Naming: Python is already snake_case, so the HIR canonical form needs no conversion. Class names stay PascalCase as in HIR.

## Exit Criteria

- [ ] All supported constructs lower to valid HIR.
- [ ] All explicitly-rejected constructs produce errors with file:line:col locations.
- [ ] 50+ snapshot tests covering the supported subset.
- [ ] 20+ negative tests for rejected constructs.
- [ ] At least 5 snapshot tests where a TS file and a Python file produce *equivalent* HIR (modulo names) — proving the shared-IR thesis.
- [ ] HIR validator passes on every snapshot output.

## Out of Scope

- FastAPI / Pydantic specifics (M9).
- Python stdlib mapping beyond what M6 already covers (M9 adds the Python-specific entries).
- Anything Python-3.13-specific.
