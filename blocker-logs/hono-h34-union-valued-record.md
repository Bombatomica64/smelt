# H34 retired, and the two defects that were actually blocking H26's fixture

Round 12, item 3. H34 was numbered in round 11 as "a union-valued record erases
its value type although `SmeltUnion*` exists for that union", recorded as the
reason H26 could not have a source-level test.

## H34 does not reproduce

It is not true at this head, and I could not find a spelling where it is.

```ts
type Pair = [string, number];
type Triple = [string, number, boolean];
const table: Record<string, Pair | Triple> = {};
```

HIR: `Dict<String, (String, Float) | (String, Float, Bool)>`.
Rust: `SmeltRecord<String, SmeltUnion8>`.

The value type is the concrete enum, not `SmeltUnknown`. The same holds for a
union of object interfaces (`Record<string, Leaf | Node>` →
`SmeltRecord<String, SmeltUnion6>`). Round 11's observation was mistaken —
probably reading the erasure of an OPERATION on the union (see H39 below) as
erasure of the record's value type. **H34 is retired.**

So H26's fixture was writable all along. Writing it found two real defects
instead, both silent, both fixed here.

## H37 — erasing a concrete union for inspection consumed it

`erase_concrete_union_text` emitted `{value}.into_smelt_unknown()`.
`into_smelt_unknown` takes `self` by value and `SmeltUnion…` is not `Copy`, so
erasing a PLACE moved out of it. Every one of the three callers erases a value in
order to INSPECT it, so none of them may consume the source. Reading one local
twice was enough:

```ts
const first = table['a'];
return first[0] + ',' + first.length;   // E0382: borrow of moved value: `first`
```

The clone belongs in the helper rather than at each call site, because the source
expression is a place read as often as it is a temporary. A caller that already
holds a clone is not cloned twice — `.clone().clone()` is not what a
hand-writing team would produce.

## H38 — concrete-union recovery discriminated by runtime tag alone

This one is a silent wrong value, and it is the more serious of the two.

`union_from_smelt_unknown_body_in_scope` guards each arm with the member's
runtime tag, plus a structural guard when the arm shares that tag with a
sibling. The structural guard only handled `Type::Class`, discriminating by field
names. A TUPLE member got no guard, so `Pair | Triple` — both tagged
`SmeltUnknown::Array` — recovered on the tag alone:

```rust
if matches!(value, SmeltUnknown::Array(_)) { return Self::M0(/* the 2-tuple */); }
```

The first arm won every time. `table['b'] = ['y', 2, true]` came back as the
2-tuple with its third element silently dropped:

| | output |
| --- | --- |
| Node 22 (`tsx`) | `x,y,2,3` |
| Smelt before | `x,y,2,2` |
| Smelt after | `x,y,2,3` |

A tuple's arity is part of its type, so the arity IS the discriminant — the
tuple counterpart of the class arm's field-presence guard. Emitted only when no
tag-sharing sibling has the same arity, so the check is decisive rather than
merely narrowing; ambiguous arms keep the tag-only guard, which is all their
type supports. `union_recovery_order` already ranks structurally-checkable arms
first, so it picks the new guard up with no change.

## Coverage

Two tests in `crates/smelt-codegen-rust/src/tests/part_7_tests.rs`, each
asserting its enabling condition before its claim, and both verified to FAIL
with the fixes reverted:

* `recovers_concrete_union_tuple_arms_by_arity` — asserts the tag-only guard
  (`if matches!(value, SmeltUnknown::Array(_)) { return`) is absent, which is
  what chose the wrong arm, then that the 2-tuple arm is guarded by
  `smelt_arms.len() == 2`;
* `erasing_a_concrete_union_for_inspection_does_not_move_it` — asserts the local
  is erased at BOTH sites (one erasure could never fail to compile) and that no
  erasure of it moves.

`examples/typescript/end-to-end/30_nullish_union_join/expected.rs` moved by one
line, the intended `.clone()`.

## H39 — why this family still cannot have a corpus fixture

The examples corpus is a HARD invariant at avoidable erasure 0, and the natural
fixture for this family scores **21**. I built it, measured it, and removed it
rather than break the invariant. The 21 come from two shapes, both avoidable by
the project's own rule:

**A tuple literal flowing into a union-valued slot round-trips through
`SmeltUnknown`.** `table['b'] = ['y', 2, true]` emits

```rust
SmeltUnion8::from_smelt_unknown(SmeltUnknown::Array(vec![
    SmeltUnknown::String("y".into()), SmeltUnknown::Number(2.0), SmeltUnknown::Bool(true)].into()))
```

where a hand-writing team writes `SmeltUnion8::M1(("y".to_owned(), 2.0, true))`.
The literal's shape is statically known and `inject_union_value_text` already
does exactly this injection — but the frontend types the array literal as the
UNION (from its contextual type) rather than as the member it structurally is,
so the emitter has a union-typed rvalue and no arm to pick. That is also why H38
existed at all: recovery-by-tag only runs because the injection erased first.

**Indexing or measuring a concrete tuple union erases it**, by H26's design
("to inspect a concrete union at run time, erase it first"). For a tuple union
the arms' element types are known, so `first[0]` could be a `match` over the
enum indexing the tuple.

Closing the first would make H38 unreachable for literals — the hono-relevant
case — and closing both would let this family have the runtime fixture it
deserves. Neither is in this commit's scope.
