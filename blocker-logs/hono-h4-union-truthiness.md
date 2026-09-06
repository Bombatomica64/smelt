# Hono family H4 — truthiness of a union whose every arm is an object

Probe: `smelt --manifest-path third_party/hono/Smelt.toml check --message-format json`
at `honojs/hono@eebdf7be39abf0a872671835ccce0c4f03ea497a`.
1 occurrence, 1 file.

## 1. The site

`src/router/reg-exp-router/matcher.ts:18`:

```ts
const staticMatch = matcher[2][path]
if (staticMatch) {
  return staticMatch
}
```

`matcher[2]` is `StaticMap<T> = Record<string, Result<T>>`, and

```ts
// src/router.ts
export type Result<T> = [[T, ParamIndexMap][], ParamStash] | [[T, Params][]]
```

so the guard's operand is a **union of two tuple types**.

## 2. Wrong output

Lowering rejects the file:

```text
condition expression must be boolean or optional (got Some(Union([TypeId(17), TypeId(20)])))
```

The reduced repro is four lines and reproduces the message verbatim:

```ts
type Result = [string[], string[]] | [string[]]
export const pick = (map: Record<string, Result>, key: string): string => {
  const hit = map[key]
  if (hit) { return 'hit' }        // <- rejected
  return 'miss'
}
```

The interesting comparison is the SAME guard over a single tuple, which has
always lowered:

```ts
export const pickTuple = (map: Record<string, [string, string]>, key: string): string => {
  const hit = map[key]
  if (hit) { … }                    // lowers to `!(false)`, i.e. always true
}
```

So the union arms added no falsy value — only a type shape nothing handled.

## 3. Responsible function

`ModuleBuilder::condition_expression` (the truthiness ladder ending at
`crates/smelt-frontend-ts/src/lowering/new_expr.rs:3138`) and the predicate it
consults, `type_is_always_truthy_object_surface`
(`new_expr.rs:3181`):

```rust
matches!(self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)),
    Some(Type::Class { .. } | Type::Function(_) | Type::List(_) | Type::Set(_)
       | Type::Dict(_, _) | Type::Tuple(_) | Type::Future(_)))
```

`Type::Union` is absent, so the branch that turns "present object" into a
presence test declined, and the ladder's last two rungs
(`is_nullishable_type` — false, the union has no `None` arm — and
`type_is_truthy_condition_surface`, whose union arm only accepts *primitive*
members) both declined too. The ladder then errored.

## 4. Design

JavaScript has exactly seven falsy values — `false`, `0`, `-0`, `NaN`, `''`,
`null`, `undefined` (plus `0n`) — and **none of them is an object**. So the rule
is compositional: a union has no falsy inhabitant exactly when none of its arms
does. `type_is_always_truthy_object_surface` now recurses through
`Type::Union`, requiring **every** arm to be always-truthy (`all`, not `any`: a
`string | T[]` union really can be falsy).

That alone would have routed the union into the existing "present object"
branch, which emits `cond != none`. It works for a single tuple only because the
emitter folds `tuple != none` to `false`; for a generated union it emits a real
presence check, and a generated union enum cannot answer one —
`matches!(v, SmeltUnknown::Null | SmeltUnknown::Undefined)` over a `SmeltUnion3`
is an E0308. So a second rung was added ABOVE it:

```rust
if !self.is_nullishable_type(cond_ty) && self.type_is_always_truthy_object_surface(cond_ty) {
    // the constant `true`
}
```

A type that cannot hold a nullish value and whose every inhabitant is an object
is truthy for every value it can take, so the guard *is* `true`. That is both
the precise answer and the one no runtime representation has to support. The
optional form (`Slots | undefined`) still falls through to the presence test,
which is what it should be.

No erasure is added: nothing about the union's representation changes, and the
guard becomes a constant rather than a tagged inspection.

## 5. Generality

The rule is stated as "a union is always truthy when every arm is", derived from
the closed list of JavaScript falsy values. It fires for any union of objects in
any boolean position — `if`, `while`, `&&`, `?:`, `!` — from any source.

## 6. Tests

* `crates/smelt-frontend-ts/src/tests/part04_tests.rs` —
  `lowers_truthiness_guard_over_a_union_of_object_arms`: the Hono tuple-union
  shape in both the plain and the optional form.
* `crates/smelt-codegen-rust/tests/truthiness_and_await_runtime.rs` —
  `a_union_of_object_arms_is_always_truthy` (extends the existing tier, already
  in the `async` shard): four fixtures that RUN — either arm is truthy, a tuple
  whose every member is the falsy `''` is still truthy, the negated guard
  reaches the other branch, and the optional form still distinguishes
  `undefined`. The negation and the `''` cases are what separate "always truthy"
  from the two ways the fold could be wrong.

  The fixtures assert the guard's OUTCOME and carry any payload alongside the
  union rather than reading it back out (see §7), so a failure is unambiguous
  about which rule broke.

## 7. Two adjacent gaps this exposed — NOT fixed, reported

Both are pre-existing, independent of this family, and hit by Hono's router at
phase 3. Neither is a truthiness problem.

1. **A `Record` index read is not optional.** `map[key]` on
   `Record<string, V>` lowers to
   `map.get(&key).cloned().unwrap_or(<Default>)` — a **fabricated** value for a
   missing key, not `undefined`:

   ```rust
   let hit: (String, String) = map.get(&key.clone()).cloned()
       .unwrap_or((String::new(), String::new()));
   let _smelt_tmp_3: bool = !(false);   // guard folds to `true`
   ```

   TypeScript without `noUncheckedIndexedAccess` types the read non-optional, so
   `tsc` accepts the source and Smelt is right to lower it — but the JS value
   for a missing key is `undefined`, and Hono's `if (staticMatch)` exists
   precisely to test for it. With the read non-optional the guard is a constant
   and a non-static path takes the static branch with a default-constructed
   result. A hand-writing Rust team would spell this `map.get(key)` returning
   `Option` and the guard `if let Some(..)`. Note this is **not** made worse by
   H4: the single-tuple value type already behaved this way. It is the same
   defect the union case would have had, had it lowered at all.

2. **Reading through a generated union arm, and constructing one from a
   literal.** `value[0]` on a `SmeltUnion3` of tuple arms emits a
   `match value { SmeltUnknown::String(..) => … }` over a value whose type is
   the generated enum (E0308, expected `SmeltUnion3` found `SmeltUnknown`), and
   an object literal at a discriminated-union type stays a
   `SmeltRecord<String, String>` where the enum is expected. Both were met while
   writing this tier's fixtures and are why they assert the guard alone.

## 8. Result

1 occurrence -> 0. `src/router/reg-exp-router/matcher.ts` leaves the blocker
list.
