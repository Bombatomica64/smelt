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

The `size`/`length` stdlib member paths already gate on
`supports_stdlib_size(receiver_ty)`. For a Dict that came from a DECLARED
interface literal — as opposed to a `Map`/`Record` — a declared member of that
name must win: the declared shape is the more specific fact, and the frontend
has it at the read site. Concretely, the gate should decline when the receiver's
Dict was lowered from an interface/object type that declares the member being
read.

A regression test belongs next to the interface-literal key-spelling fixture
(`examples/typescript/end-to-end/38_interface_literal_key_spellings`).
