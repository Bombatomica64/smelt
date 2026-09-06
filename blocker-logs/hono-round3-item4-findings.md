# Round 3 item 4: what reproduced, and what did not

Owner: Hono implementer. Round 3, item 4. Date: 2026-09-06.
**Status: investigated, not fixed.** Written so the next session starts from a
verified reproduction rather than a description.

The two bugs were reported by the standards agent. I reproduced one exactly and
could **not** reproduce the other from the description; the difference matters,
so both are recorded with the source I actually ran.

---

## (b) `const c: Config = {}` for an all-optional interface — REPRODUCES

Source:

```ts
interface Config {
  label?: string;
}

const emptyConfig: Config = {};

function labelOf(config: Config): string {
  const label = config.label;
  return label === undefined ? 'none' : label;
}
```

The struct itself is emitted correctly:

```rust
#[derive(Clone, Debug, Default, PartialEq)]
struct Config { label: Option<String> }
```

But the *literal* does not build it. It builds an erased record and then
reconstructs the struct out of it:

```rust
let _smelt_tmp_1: SmeltRecord<String, SmeltUnknown> = SmeltRecord::from([]);
let empty_config: Config = {
    let smelt_record_map = _smelt_tmp_1.clone();
    Config {
        label: smelt_record_map.get("label")
            .or_else(|| smelt_record_map.get("__smelt_proto:label"))
            .or_else(|| smelt_record_map.get("__smelt_method:label"))
            .cloned()
            .map(|value| match value.clone() {
                SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(),
                SmeltUnknown::Number(value) => value.to_string(),
                /* ... every other tag stringified ... */
            })
    }
};
```

Three separate things are wrong with that, in increasing order of seriousness:

1. **The `SmeltRecord<String, SmeltUnknown>` intermediate is pure avoidable
   erasure.** The target type is known at the literal — `Config` — so a
   hand-writing Rust team would emit `Config { label: None }` and nothing else.
   This is what the examples invariant measures.
2. **The reconstruction is a *stringifying* coercion.** Every `SmeltUnknown` tag
   is funnelled through `to_string()`. For `label?: string` that happens to be
   harmless, but the same emission for `count?: number` would turn a number into
   its decimal text. The shape is only accidentally correct here.
3. **It reads three keys per field** (`label`, `__smelt_proto:label`,
   `__smelt_method:label`) because it cannot tell a declared field from a
   prototype or method lookup — a consequence of having thrown the type away one
   line earlier.

It **compiles and runs correctly** for this fixture, so it is not a
compile-break; it is avoidable erasure plus a latent wrong-value coercion.

The fix belongs where an object literal is lowered with a known target type: if
that type resolves to a struct (class or interface), build the struct directly,
taking each declared field from the literal and `None` for an absent optional,
rather than materialising a record. The empty literal is just the degenerate
case, and it is the one that shows the bug most clearly.

Per the ruling this must bring the examples-invariant delta to **zero without
re-snapshotting**, which is the right constraint: re-snapshotting would record
the erasure as acceptable.

---

## (a) a typed arrow passed to `Array.prototype.find` — DOES NOT REPRODUCE as described

Reported as: the arrow emits an unwrapped `bool` into a `SmeltUnknown`
temporary, giving E0308 in the generated crate.

What I ran:

```ts
function firstBig(values: number[]): number {
  const found = values.find((value: number): boolean => value > 2);
  return found === undefined ? -1 : found;
}
```

The emitted crate **compiles cleanly** (`cargo check` on the generated crate is
silent) and the closure comes out typed, not erased:

```rust
pub(crate) fn first_big(values: SmeltList<f64>) -> f64 {
    let _smelt_tmp_2 = ::std::rc::Rc::new(
        |closure_arg_0: f64, closure_arg_1: i64, closure_arg_2: &SmeltList<f64>| { /* ... */ }
    );
    /* ... */
}
```

So the defect needs something my fixture does not have. Plausible differences,
none of which I am guessing at further without the original source: the
receiver being an erased list (`SmeltUnknown` elements) rather than
`SmeltList<f64>`; the predicate being a named function or a `const` alias rather
than an inline arrow; the arrow's return type being inferred rather than
annotated `: boolean`; or `find` on a union-typed receiver.

**Ask:** the exact source that produced the E0308, or the generated line. With
that this is likely a small fix; without it, anything I change here is a change
made blind, and the round-2 radash episode is a fresh reminder of what that
costs. I have deliberately not "fixed" it on the strength of the description.
