# `.size` on a value cast from `unknown` to an interface literal reads the MAP size

Found 2026-09-07 while writing the `Blob` runtime tier. Pre-existing, unrelated
to how a blob erases, and not fixed in that round.

## The repro

```ts
const blob = new Blob(["hello"], { type: "text/plain" });
const erased: unknown = blob;
const record = erased as { size: number; type: string };
console.log(record.size);   // Node: 5    Smelt: 4
console.log(record.type);   // Node: text/plain   Smelt: text/plain
```

The cast lowers the erased value to a `SmeltRecord<String, SmeltUnion5>` — an
inline interface literal is a Dict — and `record.size` is then claimed by the
generic stdlib `size` path:

```rust
let record: SmeltRecord<String, SmeltUnion5> = match (erased).into_smelt_unknown() { ... };
let _smelt_tmp_10: f64 = record.len() as f64;
```

`4` is the entry count of the erased blob record (`__smelt_blob`, `type`,
`size`, `content`), not the blob's byte size. The sibling `type` read goes
through the ordinary field path and is right.

## Why

`CLAUDE.md`'s "Frontend validation boundaries" note says Map and Record
deliberately share `Dict` internally, because `tsc` rejects the confusable
programs before Smelt runs. This is the case where that is not true: `tsc`
happily accepts an interface literal with a `size: number` field, so the shared
representation makes `.size` ambiguous *after* lowering, and the map-size
interpretation wins over the declared field.

The same collision applies to any interface literal declaring a member that is
also a modeled collection member — `size` is the one with no arguments and so
the one that silently answers rather than failing to type-check.

## Fix shape

Revisited 2026-09-07 while scoping standards round 10, and it is BIGGER than
the sketch below first suggested. Recorded properly rather than started.

The sketch was: `supports_stdlib_size(receiver_ty)`
(`lowering/ty/annotations.rs`) gates the `size`/`length` stdlib member paths, so
make it decline for a Dict that came from a declared interface literal.

The obstacle is that the gate is given a `TypeId` and nothing else, and a
`Type::Dict(K, V)` carries NO PROVENANCE — a `Map`, a `Record<K, V>` and an
inline interface literal are the same interned type by design (`CLAUDE.md`,
"Frontend validation boundaries"). So the gate cannot answer the question it
would need to answer, and neither can `type_has_known_field`, which returns
`true` for every `Type::Dict` unconditionally
(`lowering/testing/matchers.rs`).

Two ways to give it the fact, neither small:

1. **Dict provenance in the type.** A Dict lowered from a declared
   interface/object type records its declared field names, and the stdlib
   member gate declines for a name that is among them. This is the correct fix
   — the declared shape IS the more specific fact — and it is a change to the
   type representation that every Dict construction site has to honour, so it
   wants its own round.
2. **Decide at the read site instead.** Where the receiver is a local with a
   declared annotation, the annotation is still in hand before it is interned,
   so the member read could be resolved against it directly. Narrower, but it
   only covers the annotated-local spelling and leaves the same collision for
   every Dict that arrives by inference — including the repro above, whose
   receiver comes from an `as` cast.

Either way the collision is not confined to `size`: it is any interface-literal
member that is also a modeled collection member. `size` is simply the one that
takes no arguments, so it silently answers instead of failing to type-check.

A regression test belongs next to the interface-literal key-spelling fixture
(`examples/typescript/end-to-end/38_interface_literal_key_spellings`).
