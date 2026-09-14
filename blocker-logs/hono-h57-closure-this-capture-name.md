# H57 — a closure capturing `this` emits `let self = self.clone()`

Found while writing H54's fixture (round 25). **Not fixed**, and pre-existing:
it fails identically without the H54 change, which only altered a type.

## Repro

```ts
class Labeller {
  prefix = "p";
  private separator = ":";

  run(keys: string[]): string[] {
    return keys.map((key) => {
      const own = this.prefix;
      return key.length > 1 ? own + this.separator + key : key;
    });
  }
}
const labeller = new Labeller();
const out = labeller.run(["a", "bc"]);
out.unshift("first");
console.log(out.join("|"));
```

Transpiles with no blocker, and the generated crate does not compile:

```
error[E0424]: expected unit struct, unit variant or constant, found local variable `self`
    --> src/main.rs:2828:9
     |
2828 |     let self = self.clone();
     |         ^^^^ `self` value is a keyword and may not be bound to variables or shadowed
```

Two of them, one per closure that captures the receiver.

## Cause

A block-bodied callback that reads `this` cannot be modeled by the compact
callback IR, so it becomes a real HIR closure that CAPTURES `this`. The capture
prelude is emitted as `let {name} = {name}.clone();` with the name taken from
the captured local (`emitter/closures.rs`), and the local for `this` renders as
`self` — a Rust keyword, and already bound as the method receiver.

## Shape of the fix

The capture needs a distinct Rust binding name inside the closure while the
CLONE SOURCE stays the outer `self`: `let smelt_this = self.clone();`, with uses
inside the closure body rendering as `smelt_this`. The emitter already has the
two-name machinery for exactly this — the async and generator capture paths pass
a `clone_source` separate from the binding name, and `capture_aliases` exists —
so the work is to give the `this` local an alias at the point where a closure
body's locals are named, rather than to add a mechanism.

Worth checking in the same pass: whether the closure body's OTHER references to
the receiver (`self.0.borrow()...` field reads) go through the same local name,
because they have to move together.

## Why it matters

It is the H54 shape — a method mapping over something with a callback that reads
a field — so it is common, and it is the reason H54's fixture uses loops,
compound assignments and `try` blocks to reach the fallback path instead of the
`this` shape that motivated the fix. Hono's `buildRegExpStr` is a `this`-reading
callback, so the router slice will hit this the moment it gets far enough to
emit that function.
