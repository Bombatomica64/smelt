# H47 — an out-of-range element read defaults instead of answering `undefined`

Round 20, item 2. Numbered and recorded, not fixed: the fix needs a ruling
about which of two behaviours Smelt should have, and they are not equivalent.

## What happens

```ts
type Route = [string, string, number];
const oneRoute: Route[] = [["PATCH", "/maybe", 4]];
console.log(record(...oneRoute[1]));   // index 1 of a 1-element array
```

prints

```
  @0
```

The read `oneRoute[1]` produced `("", "", 0.0)` — a fully defaulted tuple — and
the spread then distributed those defaults as three arguments. JavaScript throws
`TypeError: undefined is not iterable (cannot read property Symbol(Symbol.iterator))`,
because `oneRoute[1]` is `undefined`.

Found while building round 19's spread fixture: it is the reason that fixture's
"absent" case was withdrawn.

## Where it comes from

`emitter::types::array_hole_value` decides what a missing element reads as:

```rust
if matches!(self.mir.types.get(item_ty), Some(Type::Unknown)) {
    return Ok("SmeltUnknown::Undefined".to_owned());
}
self.default_value(item_ty)
```

For an `unknown`-element list the answer is `SmeltUnknown::Undefined`, which is
truthful. For a list with any CONCRETE element type there is no `undefined` in
that type, so it answers the type's default instead — `0.0` for a number, `""`
for a string, a fully defaulted tuple here. The list-index read path then uses
that value through `.cloned().unwrap_or_else(|| <hole>)`.

So the value is wrong before any spread sees it. This is not part of the spread
family (H46) and was deliberately not folded into it.

## Why it is not simply a bug to fix

Two defensible answers, and the choice is a real design decision:

1. **Throw.** Reading past the end yields `undefined`, and everything that then
   consumes it as the element type is a `TypeError` at the point of use. This is
   what JavaScript does and what the north star ("what would a hand-writing Rust
   team do") argues for: they would return `Option<T>` and let the caller deal
   with `None`, not silently substitute `0.0`.
2. **Block.** For a TUPLE-typed index the read is a compile-time error in
   TypeScript — `oneRoute[1]` on a `Route[]` is fine, but `route[3]` on a
   `Route` is `error TS2493`. Where `tsc` already rejects the program, Smelt's
   "frontend validation boundaries" rule says it need not re-check, and a
   blocker is cheaper than a runtime tag.

They apply to different spellings, which is what makes this worth a ruling
rather than a patch:

| spelling | `tsc` | today | option 1 | option 2 |
| --- | --- | --- | --- | --- |
| `arr[i]`, `i` a variable, `arr: T[]` | accepted | defaults | throws at use | must still answer |
| `arr[5]`, literal, `arr: T[]` | accepted (no length tracking) | defaults | throws at use | must still answer |
| `tup[3]`, literal, `tup` a 3-tuple | **rejected** (TS2493) | defaults | throws | blocker |

Only the third row is `tsc`-rejected, so option 2 covers one row of three and
option 1 is needed regardless. The cheap, honest first step is therefore option
1 for the general case — make the hole read `Optional(T)` rather than `T`'s
default and let the existing optional machinery force callers to handle it —
with option 2 available as a blocker for the tuple-index row if that turns out
to be noisy.

## Cost of option 1, honestly

It changes the TYPE of every list index read from `T` to `Option<T>`, which is a
wide change: every consumer of an index read either narrows or propagates. That
is the same shape of work as H42 (a type decision that has to be threaded), and
it will move the corpora. It should be measured on es-toolkit and remeda before
it is committed to — a suite that currently passes 1055/4 may well be relying on
a defaulted read somewhere, and if so that is itself worth knowing.

## Recommendation

Own round. Not folded into H46, not attempted alongside it. The runtime evidence
above is enough to schedule it; what it needs first is the ruling on 1 versus 2
and a measurement of option 1 against both corpora.
