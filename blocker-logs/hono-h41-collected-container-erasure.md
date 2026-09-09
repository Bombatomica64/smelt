# H41 — a collect names what its entries erase to

Round 14. **28 → 15 slice errors.**

Two independent fixes, both in the "erased value rebuilt at a generic type"
area that H29b opened.

## H41a — the collect turbofish disagreed with its own entries

The four map-and-collect coercion arms in `emitter/coercion.rs` build each entry
by coercing it to the target's key/value type, and coercion treats a `TypeParam`
target as ERASED. The collect's turbofish, though, was rendered with the
ordinary lexical substitution, which keeps a type parameter resolved in the
CALLER's scope. The two disagreed whenever a generic callee had not lifted its
own type parameter, and the disagreement surfaced as a `FromIterator` failure
rather than as a type-parameter bug:

```rust
// findMiddleware<T>(middleware: Record<string, T[]>, …), called from
// RegExpRouter<T>::add — the callee erased its own T in its signature
… .map(|(key, value)| (key.clone(), /* value erased to SmeltList<SmeltUnknown> */))
  .collect::<SmeltRecord<String, SmeltList<T>>>()
//                                          ^ kept the CALLER's T
```

`collected_container_type_text` renders the container with the erased
substitution instead. That is what the entries actually are, and it is identical
to lexical rendering for a target that mentions no type parameter, so it needs
no condition.

This writes `SmeltUnknown` into a turbofish where the lexical spelling said `T`,
which is worth being precise about: it erases nothing new. The entry mappers had
already coerced each key and value to the erased target; only the annotation was
lying about what the iterator held. The genuine dynamic boundary is the callee
that did not lift its own type parameter, decided upstream of this function.
Documented at the emit site.

## H41b — the read direction of the tuple gap

The mirror of H29b, and the single error that log named as "the recovery
direction of exactly the gap fixed here". A Rust tuple has no
`SmeltFromUnknown` impl, so the blanket `smelt_from_unknown(..)` an erased
method adapter reaches for does not compile against a tuple parameter. It is
now recovered element-wise from the erased array the write side spells.

## The slice after this

| | errors |
| --- | ---: |
| round 12 end | 58 |
| after H29 (generic class bounds) | 39 |
| after H29b (tuple erasure, write side) | 28 |
| after H41 | **15** |

Measured by regenerating the slice at this commit and running
`cargo check` in `third_party/hono/dist-routers`.

Remaining, by count:

| errors | shape |
| ---: | --- |
| 2 | E0615 `match_` used as a field where it is a method (`SmartRouter<T>`) |
| 2 | E0308 expected type parameter `T`, found `SmeltUnknown` |
| 2 | E0308 expected `SmeltUnknown`, found `SmeltRecord<String, SmeltRegExp>` |
| 2 | E0308 `SmeltList<(T, SmeltRecord<String, f64>)>` vs `SmeltList<(SmeltUnknown, …)>` |
| 2 | E0277 nested record-of-record collected from an iterator whose item still carries `(T, String)` |
| 1 | E0615 `build_all_matchers` used as a field on an anon class |
| 1 | E0599 `clone` on `Result<Router<T>, Box<dyn StdError>>` — unsatisfied bounds |
| 1 | E0308 expected `bool`, found `String` |
| 1 | E0308 expected `String`, found `(String, String, T)` |
| 1 | E0308 expected `SmeltRegExp`, found `SmeltList<SmeltUnknown>` |

The 8 + 8 family that dominated the round-13 table is gone. What is left of it
is the 2 E0277 above: the same collect shape one level deeper (a record of
records), where the INNER value type still renders a `(T, String)` tuple. H41a
fixed the outer container's annotation; the nested case needs the erased
substitution to reach through the inner container too, which is a distinct
change and the natural next item.

The table is now flat — no family larger than 2 — so the campaign has moved from
"one dominant shape per round" to a long tail. Three of the ten rows (the two
E0615s and the E0599) are not erasure bugs at all but method-vs-field and
receiver-mutability shapes, and are probably cheaper than the type-level rows.

## H40 not triggered

The brief made H40 conditional on the slice reaching single digits. At 15 it has
not, so H40 stays queued.

## Notes on process

H41a ships without a unit test, which is a deliberate and slightly
uncomfortable call. Its precondition is a callee that did NOT lift its own type
parameter — a crate-wide decision no small fixture reproduces. Every fixture
variant tried emitted the callee as generic, so the assertion passed with the
fix stashed and was proving nothing; it was removed rather than left in place as
green decoration. The evidence for H41a is therefore the measured slice delta
against the 8 changed turbofish spellings, not a test. H41b does have a
non-vacuous test (`an_erased_adapter_recovers_a_tuple_parameter_element_wise`,
confirmed failing with the fix stashed).
