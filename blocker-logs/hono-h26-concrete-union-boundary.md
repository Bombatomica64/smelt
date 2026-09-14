# H26 — a concrete union is not a `SmeltUnknown`

Round 11, item 2, third family: 15 of the slice's errors.

A union every member of which has a concrete Rust type is emitted as a real
enum (`SmeltUnionNNNN`) with `IntoSmeltUnknown` / `from_smelt_unknown` as its
boundary adapters. Two erased-record operations assumed such a value was a
`SmeltUnknown` and used neither adapter. Hono's
`Matcher<T> = [RegExp, HandlerParamsSet<T>[][], ...] | [Record<string, ...>]`,
read out of `all[2]` in `reg-exp-router/prepared-router.ts`, is the shape.

## The two sites

**A missing-key read answered `undefined`.** The record's Rust value type is the
enum, so:

```rust
_smelt_tmp_76 = _smelt_tmp_75.get(&path2.clone()).cloned()
    .unwrap_or(SmeltUnknown::Undefined);
//             ^ expected `SmeltUnion127`, found `SmeltUnknown`   (E0308 ×9)
```

`undefined` is what a missing key answers in JavaScript, but only a value type
that RENDERS as `SmeltUnknown` can hold it. A concrete union falls to the same
`unwrap_or(default)` every other concrete value type already uses, and
`default_value` has had a concrete-union arm all along. A union WITHOUT a
generated enum still renders as `SmeltUnknown` and still answers `undefined`, so
the JS-visible behaviour changes only where the old spelling did not compile.

**An indexed read scrutinised the enum with `SmeltUnknown::` arms.**

```rust
match _smelt_tmp_76.clone() { SmeltUnknown::String(value) => .., SmeltUnknown::Array(..) => .. }
//    ^ this expression has type `SmeltUnion127`                              (E0308 ×6)
```

The erased index read (`unknown_index_text`) already erases its INDEX operand
through `erase_concrete_union_text`; its RECEIVER now crosses the same adapter,
at both call sites (the indexed place read and the erased tuple read). This is
the rule the truthiness path already follows: to inspect a concrete union at run
time, erase it first — the inspection is genuine runtime narrowing, only the
narrowed value escapes, and no static shape is lost.

## Effect on the slice

| | errors |
| --- | ---: |
| after H30 | 62 |
| after the record-default half | 59 |
| after the receiver-erasure half | **53** |

## Coverage, honestly

This family has NO source-level regression test, and I could not write one that
was not vacuous. In every TypeScript spelling I tried — `Record<string, Pair |
Triple>` with the members as tuple aliases, the same via a function return, the
local annotated with the alias — the RECORD's value type erased to
`SmeltUnknown` even though the same union has a generated `SmeltUnion` enum in
the same crate. So the fixture never reached the code path the slice exercises,
and a test asserting `into_smelt_unknown()` appears somewhere in the output
would have passed before the fix too. The evidence for the change is the slice
count above; the emit sites carry the reasoning as comments.

That failure to reproduce is itself a finding:

## H34 — a union-valued record erases its value type

`Record<string, Pair | Triple>` lowers its value type to `SmeltUnknown` while
`SmeltUnion*` exists for that very union. By the project's own rule that is
avoidable erasure: the concrete enum is exactly the "generated union" the
`SmeltUnknown` policy prefers over a tag. Whatever decides a union's
concreteness is not consulted for a record's value position. Numbered here; a
fix would also give H26 its fixture.
