# H52 — a generated `Set` does not iterate in insertion order

Found while landing Hono blocker 4 (`new Set(string)`, round 23). **Not fixed.**
Older and wider than that rule: it applies to every `Set`, however built.

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
