# H29, second half — a tuple has no erasure adapter

Round 13, item 5. **39 → 28 slice errors.**

## The site

`record_field_unknown_text` erases a value to `SmeltUnknown` for a record field
or a prototype-carrier return. Every structural shape it handles is erased
ELEMENT-WISE — a list maps its items, a string-keyed dict maps its values — but
`Type::Tuple(_)` was grouped with the shapes that have an `IntoSmeltUnknown`
impl:

```rust
Some(Type::Dict(_, _) | Type::JsMap(_, _) | Type::Tuple(_) | Type::Class { .. }) => {
    format!("({value_text}).into_smelt_unknown()")
}
```

A Rust tuple has no such impl. The prototype carrier for
`RegExpRouter#buildAllMatchers`, whose return type is
`Matcher<T> = [RegExp, HandlerParamsSet<T>[][], …]`, asked for it and got "no
method named `into_smelt_unknown` found for tuple" — 6 reports across the
slice's instantiations.

## The fix

A tuple erases element-wise to a JS array, which is what the emitter already
spells at its other tuple-erasing sites:

```rust
{ let smelt_tuple = <value>; SmeltUnknown::Array(vec![<e0>, <e1>, …].into()) }
```

Each element recurses through the same function, so a tuple of tuples or a tuple
of lists erases correctly too. The value is bound to a local first because
`value_text` is an arbitrary expression here and indexing it per element would
evaluate it once per element.

## The slice after this

| | errors |
| --- | ---: |
| round 12 end | 58 |
| after H29 (generic class bounds) | 39 |
| after H29b | **28** |

Remaining, by count:

| errors | shape |
| ---: | --- |
| 8 + 8 | a generic-`T` record rebuilt from an erased iterator — `SmeltRecord<String, SmeltList<T>>` collected from `Iterator<Item=(String, SmeltList<SmeltUnknown>)>` (E0308 + E0277, one family in two reports). The largest remaining family by a wide margin, and the natural next item. |
| 3 | E0615 `match_` used as a field where it is a method |
| 2 | `expected SmeltUnknown, found SmeltRecord<String, SmeltRegExp>` |
| 2 | `SmeltList<(T, …)>` vs `SmeltList<(SmeltUnknown, …)>` |
| 1 | `SmeltFromUnknown` not implemented for `(T, SmeltRecord<String, f64>)` — the READ direction of this same tuple-adapter gap |
| 1 each | a `Result` receiver needing `?`; `expected bool, found String`; `expected String, found (String, String, T)`; `expected SmeltRegExp, found SmeltList<SmeltUnknown>` |

That last single error is worth naming: `SmeltFromUnknown for (T, …)` is the
recovery direction of exactly the gap fixed here. The write side now erases a
tuple element-wise; the read side still wants a `SmeltFromUnknown` impl a tuple
does not have. Same shape, opposite direction — a good pair to take together.

## Gates

cargo test 105 result lines / 0 failures / 2661 passed; clippy `--lib` clean and
`--all-targets` clean outside `smelt-specialize`; end-to-end goldens
byte-identical; examples SmeltUnknown avoidable 0 (+0); es-toolkit compiles with
0 errors, generated tests 1055/4, ratchet +0 (32517); remeda 1789/0.
