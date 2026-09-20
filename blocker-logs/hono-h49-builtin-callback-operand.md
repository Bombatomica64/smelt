# H49 — a global builtin used as a callback erases its operand

Found while landing Hono blocker 3 (`xs.filter(Boolean as any)`, round 23).
**Not fixed**, and numbered here with its measurement.

## What happens

`String`, `Number`, `Boolean` used as VALUES lower to a synthesized
single-argument closure (`builtin_cast_closure_expression` in
`lowering/expr/references.rs`), and that closure's parameter is interned as
`Type::Unknown`:

```rust
let value_ty = self.ctx.krate.types.intern(Type::Unknown);
```

So `values.filter(Boolean)` on a `(string | undefined)[]` emits

```rust
Rc<dyn Fn(&SmeltUnknown) -> bool>
```

and every element is erased on the way in and re-tagged at the call:

```rust
(smelt_callback)(&(item.clone().map_or(SmeltUnknown::Undefined, |value| SmeltUnknown::String(value.into()))))
```

Measured: a four-line program that filters with `Boolean` and maps with
`Number` adds **17 avoidable erasures**. That is why the end-to-end fixture
`69_asserted_callback_name` keeps the builtin arm out and asserts it in
`crates/smelt-frontend-ts/src/tests/asserted_callback_name_tests.rs` instead —
the examples corpus is a hard invariant at avoidable == 0.

## Why it is avoidable

The precedent is already in the same file. `parseInt`/`parseFloat` build their
closure with a CONCRETE `string` parameter, and the docstring says exactly why:

> A concrete `string` parameter routes the cast through its real numeric-parse
> emission rather than the erased `unknown` fallback (which would yield a
> constant `0`).

A callback position knows the receiver's element type — `expected_param_tys` is
right there in `callback_identifier_argument` — so the same reasoning applies:
`Boolean` over a `string | undefined` list is
`Fn(&Option<String>) -> bool`, and the truthiness coercion on a concrete
`Option<String>` is already implemented (it was landed earlier in this campaign
at both coercion entry points).

## Shape of the fix

Thread an optional operand type into `builtin_function_value_expression` and
use it for the three cast arms only (the parse builtins keep `string`, the
numeric predicates keep `f64`). Value position (`const f = Boolean`) has no
element type and keeps `Unknown`.

## Why it is not in this round

It touches every `map(Number)` / `filter(Boolean)` in es-toolkit and remeda, so
it needs the full corpus gates rather than a fixture: the expected outcome is a
DECREASE in avoidable erasure on both, which would be a ratchet re-snapshot, and
the risk is a generated-Rust type error where a concrete operand meets a
`SmeltUnknown`-typed consumer. That is a round of its own, not a rider on a
callback-selection fix.
