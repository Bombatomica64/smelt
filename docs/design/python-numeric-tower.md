# Python's numeric tower: `float` slots and `int | float` values

Status: implemented. Code: `crates/smelt-frontend-py/src/lowering/literals.rs` (`union_member_dispatch`,
`union_binary_dispatch`), `crates/smelt-codegen-rust/src/emitter/union.rs` (`project_union_value`).

## 0. The question

Python's typing treats `int` as acceptable wherever `float` is declared (PEP 484's numeric tower), but at run time
the value stays an `int`:

```python
x: float = 2
print(x, x * 3)        # 2 6      (an int object in a float-annotated variable)
y: float = 2.0
print(y, y * 3)        # 2.0 6.0
```

So a `float` annotation describes a slot that may hold either kind of number, and the two print differently. A
transpiler has to decide what a `float` slot is.

## 1. Decision: a declared `float` is an `f64`

A slot the source declares `float` (a variable, parameter, return, field or container item) is a Rust `f64`. An
`int` stored into it is converted where it is stored: a literal becomes `2.0`, a `SmeltPyInt` goes through
`float(x)`'s conversion, and an `int | float` value has its `int` member widened (`project_union_value`: the
`SmeltPyInt` arm becomes `value.to_f64_lossy()` when the target is `float`).

This is the type a team porting the code by hand writes: the annotation is the contract, and `f64` is its precise
Rust type. The alternative, a `float` slot as the union `int | float`, models CPython exactly but turns every
`float` annotation (parameters and fields included, since any caller may pass an `int`) into a tagged enum whose
arithmetic dispatches at run time, for a difference that is only visible in how the number prints (`2` vs `2.0`),
in `type(x)` and in `isinstance(x, int)`.

**Divergence, accepted:** an `int` held by a `float`-declared slot prints as a float (`2.0`, not `2`), and `x * 3`
on it is `6.0`. Every arithmetic result is numerically equal to CPython's (an `int` that does not fit a double
excepted, as for any `float(x)`). A program that needs the `int` keeps it in a slot declared `int` or `int | float`.

`ty` spells the type of a call returning `int | float` as `float` (its display of the promotable union), so an
unannotated module variable bound to such a call is a `float` slot too: `m = pick(3, 2.5)` at module level holds
`3.0`. In a function the local takes the value's own type, `int | float`.

## 2. `int | float` values

A value whose type is the union `int | float` (a `max` of mixed arguments, a function annotated `-> int | float`,
an unannotated local bound to one) keeps the union: a `SmeltUnionN` enum with a `SmeltPyInt` and an `f64` arm.
Every operation on it runs on the member it holds, exactly as CPython dispatches on the object's type:

```text
v = value                          # evaluated once
if v holds int:   result = op(v as int)
else:             result = op(v as float)
result
```

`union_member_dispatch` builds this from the HIR member test (`ExprKind::UnionArmTest`) and the member projection,
lowering the operation once per member through the ordinary rule for that member type. Binary operators bind both
operands first, in source order, and split the union one (`union_binary_dispatch`); with two union operands the
right one is split inside each branch of the left. The result type is each branch's type: one type when all
agree (`int | float` times `2.5` is a `float`), else their union (`int | float` plus `1` stays `int | float`).

Covered: `+ - * / // % **`, the six comparisons, unary `-`, `abs` and one-argument `round`. A bitwise or shift
operator is refused, as CPython raises `TypeError` for its `float` member; so is a union with a `None` member
(test for `None` first).

## 3. `A | B | None`

A Python union with `None` among several other members is `Optional(Union[A, B])`, a Rust
`Option<SmeltUnionN>` (`union_from_types`), not the canonical union with a `None` arm, which has no concrete enum
and would erase a fully static value to `SmeltUnknown`. `is None` tests the option; `isinstance` tests the
union's members inside it.

## 4. Evidence

`crates/smelt-transpiler/tests/fixtures/python-parity/numeric_unions.py` exercises every operator above on both
members, mixed with `int` and `float` operands, the result types (an `int` result stays exact and prints without
`.0`), and `isinstance`/`is None` on `int | str | None`; its stdout equals `python3`'s. Planted defects are listed in
the PR that landed this.
