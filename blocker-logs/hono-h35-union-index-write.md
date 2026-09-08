# H35 — an indexed write into a concrete union crosses the erased helper

Round 12, item 4, first family: **425 → 64 slice errors**, the largest single
family of the campaign so far.

Numbered in H28's note: restoring the code H28 had been deleting exposed 360
reports of one shape.

## The site

A keyed write whose receiver's type is `Unknown`, a union, or a type parameter
routes through the erased runtime helper:

```rust
fn smelt_index_assign(target: &mut SmeltUnknown, key: String, value: SmeltUnknown)
```

The arm matched `Type::Union(_)` without asking whether the union is CONCRETE. A
concrete union stores a tagged `SmeltUnion…` enum, so the receiver is not a
`&mut SmeltUnknown`:

```rust
smelt_index_assign(&mut results, smelt_key, smelt_value);
//                 ^^^^^^^^^^^^ expected `&mut SmeltUnknown`, found `&mut SmeltUnion164`
```

Hono's `src/utils/url.ts` (`results[name] = …` over a union-typed accumulator)
is the shape, reached from `url.rs`, `url_1.rs` and `prepared_router.rs`. Those
functions are generic over `T` and instantiated across the slice's spec modules,
which is how three emit sites produced 360 error reports.

## The rule

This is H26's rule on the WRITE path. H26 established that to operate on a
concrete union at run time you cross its boundary adapter first; a write needs
one more step than a read, because the mutation has to come back:

```rust
{ let smelt_key = …; let smelt_value = …;
  let mut smelt_erased = results.clone().into_smelt_unknown();
  smelt_index_assign(&mut smelt_erased, smelt_key, smelt_value);
  results = SmeltUnion164::from_smelt_unknown(smelt_erased); }
```

Cross out, mutate the erased view, cross back in, **commit to the place**. The
write-back is the point rather than a detail: mutating a copy and dropping it is
precisely the silent-write-loss failure this campaign keeps finding.

A union with no generated enum still renders as `SmeltUnknown` and keeps the
direct call, so nothing changes where the old spelling compiled.

## Precision, honestly — H40

The round-trip is correct in SHAPE but it is not exact. `from_smelt_unknown`
re-extracts every leaf into its arm's declared element types, which retypes
leaves that the erased view had flattened (the same effect
`union_member_extraction_is_lossless` documents, where `36` became `"36"`). With
H38's arity guard the ARM is now chosen correctly, so the remaining imprecision
is confined to leaf extraction.

The exact lowering is a per-arm keyed insert — `match &mut results { M0(v) =>
v.insert(key, <value at M0's value type>), M1(v) => … }` — which never leaves
the typed world. It needs the value operand re-rendered once per arm, so it is a
separate change; numbered **H40**. Shipping the round-trip first is a strict
improvement over code that did not compile at all, and it does not stand in
H40's way.

## The slice after this

| | errors |
| --- | ---: |
| round 11 end | 53 |
| after H33 (dropped calls became real calls) | 55 |
| after H28 (deleted regions restored) | 425 |
| after H35 | **64** |

Remaining families, by count — the next round's queue:

| errors | shape |
| ---: | --- |
| 8 + 8 | `SmeltRecord<String, SmeltList<T>>` built from an `Iterator<Item=(String, SmeltList<SmeltUnknown>)>` — a generic-`T` record rebuilt from an erased iterator (E0308 + E0277, one family in two reports) |
| 6 | `Option<Option<(SmeltRegExp, …)>>` where `Option<(SmeltRegExp, …)>` is expected — a double-wrapped optional |
| 5 + 1 | `into_smelt_unknown` not found for a tuple type — a tuple has no erasure adapter |
| 3 × 4 | `T: SmeltFromUnknown` / `IntoSmeltUnknown` / `Default` / `Clone` not implemented — missing bounds on a lifted type parameter (one family, four bounds) |
| 3 | E0615 `match_` used as a field, not a method |
| 2 | `expected SmeltUnknown, found SmeltRecord<String, SmeltRegExp>` |
| 2 | `SmeltList<(T, …)>` vs `SmeltList<(SmeltUnknown, …)>` |
| 1 each | `Node<T>` has no `==`; a `Result` receiver needing `?`; `expected bool, found String`; `expected String, found (String, String, T)` |

## Gates

cargo test 102 result lines / 0 failures / 2650 passed; clippy `--lib` clean;
end-to-end goldens byte-identical; examples SmeltUnknown avoidable 0 (+0);
remeda 1789/0 and advisory +0 (25065); radash 84/84. es-toolkit ratchet still
+115 from H28 with no further movement here; es-toolkit crate still 8 errors.
