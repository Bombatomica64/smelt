# H27 — an already-optional dict value read as `Option<Option<T>>`

Round 12, item 4, second family: **64 → 58 slice errors** (6 × E0308, gone).

## The shape

`matchers[method]` where hono's `MatcherMap<T> = Record<string, Matcher<T>>` and
`Matcher<T>` is itself optional. The destination is declared `Option<(…)>` and
the read produced one wrapper too many:

```rust
let _smelt_tmp_9: Option<(SmeltRegExp, …)> = matchers.clone().get(&smelt_property_key(…));
//                                           ^ found `Option<Option<(SmeltRegExp, …)>>`
```

`src/matcher.rs:37` and `:69`.

## The rule, and why one path already had it

TypeScript does not distinguish a MISSING key from a key holding `undefined` —
both read as `undefined` — so HIR collapses `Optional(Optional(T))` to
`Optional(T)`. A `get` returns `Option<V>`, so when `V` is itself optional the
emitted read has to collapse with it.

`place.rs` has flattened here all along:

```rust
if matches!(self.mir.types.get(*value), Some(Type::Optional(_))) {
    return Ok(format!("{base_text}.get(&{key_text}).cloned().flatten()"));
}
```

The shared helper for the same read — `dict_index_optional_read_text`, used by
optional dict index reads and by class index-signature store reads — took the
KEY type and not the value type, so it had nothing to test and could not
flatten. Two spellings of one read disagreed, and the one that could not see the
value type was wrong.

The fix passes the value type and applies the same test, so the two agree by
construction rather than by coincidence.

One consumer needed a matching adjustment: the class index-store place read
wraps the optional read in `.unwrap_or(default)`. Once the read is flattened its
result IS the value type, and that type's own default is `None`, so defaulting
again would `unwrap_or` an `Option<inner>` with an `Option`. That path now
returns the flattened read directly — a missing key and a stored `undefined` are
the same `None`, which is exactly what the value type says.

## Gates

cargo test 102 result lines / 0 failures / 2650 passed; clippy `--lib` clean;
end-to-end goldens byte-identical; examples SmeltUnknown avoidable 0 (+0);
remeda 1789/0 and advisory +0 (25065); radash 84/84. es-toolkit ratchet still
+115 from H28 with no further movement here; es-toolkit crate still 8 errors.

## The slice after this

| | errors |
| --- | ---: |
| round 11 end | 53 |
| after H33 | 55 |
| after H28 (deleted regions restored) | 425 |
| after H35 | 64 |
| after H27 | **58** |

Next by count is **H29** — a lifted type parameter used in a generated generic
class does not carry the bounds its derives and body need (`T: Clone + Default +
IntoSmeltUnknown + SmeltFromUnknown`, 12 errors) and a tuple has no
`into_smelt_unknown` adapter (6 errors). Deliberately not started here: the
bounds half changes the declaration of every generated generic class, so it
wants its own commit and its own golden review rather than being tacked onto a
six-error fix.
