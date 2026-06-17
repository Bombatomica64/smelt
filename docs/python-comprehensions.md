# Python Comprehensions

Status and design notes for lowering Python comprehensions
(`[...]`, `{...}`, `{k: v ...}`, and generator expressions) in the Python
frontend (`smelt-frontend-py`).

## Forms

| Source form            | Example                          | Status |
| ---------------------- | -------------------------------- | ------ |
| List comprehension     | `[f(x) for x in xs if p(x)]`     | supported (loop lowering) |
| Set comprehension      | `{f(x) for x in xs}`             | supported (loop lowering) |
| Dict comprehension     | `{k: v for k, v in items}`       | supported (loop lowering) |
| Generator expression   | `(f(x) for x in xs)`             | supported, eagerly materialized (see below) |

## Lowering strategy

Comprehensions lower to an **imperative accumulator loop** wrapped in an
`ExprKind::Block` value expression, rather than the legacy `map`/`filter`
`CallbackExpr` bridge (see `docs/callback-expr-audit.md`). For

```python
[elt for t0 in it0 if c0 for t1 in it1 if c1]
```

the frontend emits the HIR equivalent of

```rust
{
    let mut acc = Vec::new();          // SetLit / DictLit for set/dict forms
    for t0 in it0 {
        if c0 {
            for t1 in it1 {
                if c1 {
                    acc.push(elt);     // SetAdd / dict index-assign for set/dict
                }
            }
        }
    }
    acc
}
```

Because the element, key, value, and condition expressions are lowered through
the normal expression path (`expression`), they support the full expression
language — including nested comprehensions, f-strings, method calls, and
conditionals — not just the restricted callback subset.

Loop targets bind through `binding_pattern_from_target` and the iterable is
normalized through `for_iterable`, so comprehensions iterate over any iterable
the `for` statement already supports (lists, sets, dicts, strings).

### Generator expressions

Generator expressions lower identically to list comprehensions, i.e. they are
**eagerly materialized** into a list. This is correct for the common eager
sinks (`list(...)`, `sum(...)`, `" ".join(...)`, `for ... in (gen)`), but does
not preserve laziness. Truly lazy/infinite generators and short-circuiting
consumers (`any`/`next`) are therefore out of scope until the runtime grows a
lazy iterator type.

## Known limitations

These are rejected with a `smelt::unsupported-py` diagnostic rather than
mis-lowered:

- **Async comprehensions** (`[x async for x in ...]`) — needs async iteration.
- **Destructuring loop targets** (`for k, v in items`) — blocked by a more
  general MIR limitation: `Stmt::For` and `Stmt::Let` only lower *binding*
  (single-name) patterns to MIR today, so ordinary `for k, v in ...` loops are
  unsupported as well. Lifting this is tracked as a shared `for`/`let`/
  comprehension destructuring task, not a comprehension-specific one.
- **Walrus (`:=`) inside the comprehension body** — needs named-expression
  lowering.

## Implementation map

- `smelt-frontend-py/src/lowering/list.rs` — `list_comprehension`,
  `set_comprehension`, `dict_comprehension`, `generator_expression`, and the
  shared `comprehension_block` loop builder.
- `smelt-frontend-py/src/lowering/literals.rs` — routes `Expr::ListComp`,
  `Expr::SetComp`, `Expr::DictComp`, and `Expr::Generator`.
- `smelt-mir/src/lower.rs` — `ExprKind::Block` lowering (executes block
  statements, then yields the block tail operand).
</content>
</invoke>
