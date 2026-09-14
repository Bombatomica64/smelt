# H16 — a destructured callback parameter is a projection, and projections nest

Round 9, Hono implementer. Site: `third_party/hono/src/request.ts:417` and `:437`.

## The source

```ts
get matchedRoutes(): RouterRoute[] {
  return this.#matchResult[0].map(([[, route]]) => route)
}
get routePath(): string {
  return this.#matchResult[0].map(([[, route]]) => route)[this.routeIndex].path
}
```

`#matchResult` is `Result<[H, RouterRoute]>`, so an element of `#matchResult[0]`
is `[[H, RouterRoute], ParamIndexMap]`: an array pattern whose first element is
itself an array pattern with the first slot elided.

## What was wrong

`bind_callback_param_pattern` (`crates/smelt-frontend-ts/src/lowering/callbacks/body_lowering.rs`)
bound exactly ONE level of a destructured callback parameter. An array element
had to be a bare identifier and an object property's value had to be a bare
identifier; anything deeper returned

```text
nested callback parameter destructuring needs closure-body lowering
```

and — unlike the sibling message `callback parameter destructuring needs
closure-body lowering` — this message was **not** in
`should_fallback_to_closure_body_for_callback`, so it was a hard blocker rather
than a retry. It aborted the whole-crate build at `src/request.ts`; it was
unmasked in round 8 when H13 (the symbol-keyed computed getter in the same file)
landed.

## Why one level was ever enough, and why the fix is not a new node

A destructured parameter is nothing but a projection of the argument:

| source | binding |
| --- | --- |
| `([k, v]) => ...` | `k = arg[0]`, `v = arg[1]` |
| `({ a, b }) => ...` | `a = arg.a`, `b = arg.b` |
| `([[, route]]) => ...` | `route = arg[0][1]` |
| `({ key: { name } }) => ...` | `name = arg.key.name` |

The compact callback IR already has the two projection nodes it needs —
`CallbackExprKind::Index { receiver: Box<CallbackExpr>, index }` and
`CallbackExprKind::Field { receiver: Box<CallbackExpr>, field }` — and both
lower recursively through `callback_expr_to_body_expr`, i.e. through the
ordinary expression path. The binder was the only non-recursive part.

## The rule that landed

`bind_callback_param_pattern` now builds the root projection (`Param(index)`)
and hands it to a new recursive `bind_callback_pattern_value(pattern, value,
params)`:

* a binding identifier binds the projection built so far;
* an array pattern projects `Index { receiver: value, index }` per element and
  recurses, carrying the element type from `Type::Tuple` (per position) or
  `Type::List` (the same element type for every position);
* an object pattern projects `Field { receiver: value, field }` and recurses,
  carrying the FIELD's own type from the same type table the one-level path
  already used (`Dict`/`JsMap` value, `class_field_type` for a class or
  interface, `Float` for `length` on a list or string, `Unknown` for an erased
  or type-parameter receiver, and a report for anything else).

The type is the load-bearing half. A nested binding typed as its container
compiles perfectly well and then answers a coerced value; the previous
one-level code already carried that scar in a comment (`({ length }) => length`
over `T[][]` typing `length` as `T[]`). The recursion carries the projected
type at every level and still reports — rather than invents — a type it cannot
resolve, so the caller retries through full closure-body lowering.

General, not Hono-shaped: it fires for any `map`/`filter`/`forEach`/`reduce`
callback over tuples or records, which is the ordinary `Object.entries` shape in
es-toolkit, remeda and radash.

### Deliberately not bound: `...rest`

`ArrayPattern::rest` / `ObjectPattern::rest` are still not bound. The compact IR
has no node for the tail of a list, an unreferenced rest costs nothing, and a
referenced one already fails as `unresolved callback identifier`, which IS in
the closure-body retry list. Documented at the binder.

## Test

`crates/smelt-codegen-rust/tests/callback_destructuring_runtime.rs` (new tier,
registered in the `functions` shard of `.github/workflows/runtime-tiers.yml`):
array-in-array with an elided slot (the Hono shape), object-in-object,
object-in-array, array-in-object, plus the two one-level shapes so the
recursion cannot change what they already answered. Expectations are what Node
22 prints for the same source.

```sh
cargo test -p smelt-codegen-rust --test callback_destructuring_runtime -- --ignored
```

## Probe

258 files / 3 with blockers / 3 occurrences -> 258 / 2 / 2. The whole-crate
build now aborts at `src/context.ts` instead of `src/request.ts`.

## Found while writing the fixture, NOT fixed — H17 candidate

An **inline (anonymous) object type annotation** lowers to
`Record<string, union-of-its-member-types>`, so every field read answers the
union instead of the member's declared type. It needs no destructuring at all:

```ts
interface Pair { key: { name: string; size: number }; value: number }
const sizes = (pairs: Pair[]): number[] => pairs.map((p) => p.key.size + 1)
```

emits `let _smelt_tmp_3: SmeltRecord<String, SmeltUnion6> = closure_arg_0.key`
and then reads `size` as the union, so `size + 1` lowers to STRING
concatenation and the result is parsed back to a float: Node prints `2,3`, the
generated crate produces the concatenation. Naming the inner type
(`interface Key { name: string; size: number }`) lowers correctly, which is why
the fixture above uses a named interface. A hand-writing Rust team would give
the anonymous type a struct; the fix is to lower an inline `TSTypeLiteral` with
named members to an anonymous generated shape (as interface literals already
are) rather than to a `Dict` of the joined member type. Silent wrong value, no
diagnostic.
