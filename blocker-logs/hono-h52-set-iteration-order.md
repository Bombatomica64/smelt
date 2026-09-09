# H52 — a generated `Set` does not iterate in insertion order

Found while landing Hono blocker 4 (`new Set(string)`, round 23); **ruled and
FIXED in round 24**. Kept as the record of the cause and of the design choice.

## Repro

```ts
const direct = new Set('hello');
const viaSpread = new Set([...'hello']);
console.log([...direct].join('|'));    // prints  l|e|o|h
console.log([...viaSpread].join('|')); // prints  h|o|e|l
console.log(direct.size);              // 4  — correct
console.log(viaSpread.size);           // 4  — correct
```

JavaScript specifies `Set` iteration as INSERTION order, so both lines must
print `h|e|l|o`. Contents and size are right; only order is wrong. Two
constructions of the same set in one program came back in two different orders,
so the backing container is hash-ordered rather than insertion-ordered.

## Why it matters

Order is observable through `for...of`, spread, `forEach`, `keys`/`values`/
`entries`, and anything built on them (`Array.from(set)`, `[...set].join()`).
Code that de-duplicates while preserving order — a common JS idiom — silently
scrambles its output. Nothing throws and sizes match, so a test only catches it
if it asserts a sequence.

This is why the end-to-end fixture `70_set_from_iterable` asserts `size` and
`has` rather than a joined string: pinning the current order into a golden would
freeze the wrong behaviour and make the eventual fix look like a regression.

## Where to look

The runtime `Set` container in the generated prelude (`SmeltSet`) and the
`SetProjection` emission. An insertion-ordered set is a small, well-understood
change: keep a `Vec` of keys beside the hash index, or use an ordered-map crate,
which the "prefer well-known Rust libraries" guidance in `CLAUDE.md` favours
over custom machinery. `Map`/`JsMap` and record iteration should be checked in
the same pass — JavaScript object key order and `Map` insertion order have the
same guarantee, and if `Dict` is hash-ordered the same class of silent scramble
exists there.

## Cost of leaving it

Unknown but bounded: es-toolkit and remeda pass their generated suites today,
so either they do not assert set order or their assertions are order-free. A
grep for `[...` over a `Set` in both corpora, plus Hono's own uses, would size
it before committing to the container change.

## What landed (round 24)

`SmeltPrimSet`: a `Vec` of entries beside a `HashSet` index — the record
store's shape — emitted for a source `Set` whose element type can key a Rust
hash map (`bool`, `i64`, `String`, and optionals/unions of those). Membership
stays hashed, iteration is insertion-ordered, and `add` on a member already
present does not move it (JS semantics).

Two containers, not one, and the measurement is why. `SmeltJsSet` was ALREADY
insertion-ordered (entries `Vec` plus slot index) and already backed every
element type Rust cannot hash, so the obvious fix was to route all sets through
it. That was implemented first and measured: its membership erases each element
through `IntoSmeltUnknown` for SameValueZero, which forces the whole
erased-value carrier into any program holding a set, and the whole-module
snapshot for a four-line `Set<string>` program went from **18 lines to over 450**
(it tripped the emitter snapshot's 450-line budget). For value-equality
primitives SameValueZero simply IS Rust equality, so `SmeltPrimSet` hashes the
values directly and pay-for-use survives: the same snapshot is 63 lines.

Two things came along with it, because the container has what a bare `HashSet`
lacked:

- a stable object id, so `setA === setB` compares identity as JavaScript does.
  A primitive set had no id and fell through to structural equality, which
  answered `true` for two distinct sets with the same members.
- an ordered erasure: a set erased to `SmeltUnknown` used to sort its members by
  hash key — a deterministic answer to a question the container could not
  answer — and now erases in insertion order.

Fixtures: `72_set_insertion_order` asserts the order through every observer JS
exposes it to (spread, `forEach`, `for...of`, `Array.from`, `values()`,
`keys()`), across mutation, for all three primitive element types, plus the two
identity answers; `70_set_from_iterable`'s assertions were re-tightened from
membership-only to order, which is what it wanted to assert in the first place.

Measured: es-toolkit 745 files with 1055 passed / 4 failed and the ratchet at
32438 (+0); remeda 391 files, `cargo check` 0 errors; examples invariant 0 with
every category +0; workspace 1041 codegen tests green (four asserted the old
`HashSet` spelling and now assert the container, with the reason at each);
`cargo clippy --all-targets` error-free.

## Still open, deliberately

`Map`/`JsMap` and record iteration were NOT audited in this round. `SmeltRecord`
is insertion-ordered already and `SmeltJsMap` keeps entries in a `Vec`, so both
look right, but "looks right" is not a measurement and JavaScript's object-key
order has its own integer-key rule on top. Worth a fixture of its own rather
than an assumption.
