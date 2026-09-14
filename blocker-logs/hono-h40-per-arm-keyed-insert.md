# H40 — withdrawn: the per-arm keyed insert is not implementable as designed

Round 17, item 3. Implemented, measured, reverted. **H40 should be withdrawn**,
and the precision loss it was meant to fix belongs somewhere else.

## What H40 proposed

`hono-h35-union-index-write.md` shipped an index write to a concrete union as a
round-trip — erase the union to `SmeltUnknown`, mutate the erased view with
`smelt_index_assign`, recover with `from_smelt_unknown`, commit to the place —
and called it "correct in SHAPE but not exact", because `from_smelt_unknown`
re-extracts every leaf into its arm's declared element types and so retypes
leaves the erased view had flattened (`36` coming back as `"36"`). It named the
exact lowering:

> a per-arm keyed insert — `match &mut results { M0(v) => v.insert(key, <value
> at M0's value type>), M1(v) => … }` — which never leaves the typed world. It
> needs the value operand re-rendered once per arm.

## Why it cannot work

That last sentence is the problem, and it is fatal rather than merely costly.

Only one arm is taken at runtime, but **every arm has to compile**. The written
value has ONE static type. Rendering it at each arm's value type therefore
demands a coercion into arms the value cannot inhabit — and the only reason the
union exists is that its arms *disagree* about the value type.

Implemented as specified and measured on the hono slice:

| | slice errors |
| --- | ---: |
| before | 9 |
| per-arm insert, as designed | **81** |

A fixture makes the mechanism plain. For `Counts | Labels` where
`Counts = Record<string, number>` and `Labels = Record<string, string[]>`, the
write `results[key] = 36` emits

```rust
match &mut results {
    SmeltUnion7::M0(smelt_arm) => { smelt_arm.insert(k, _smelt_tmp_6); }              // f64, fine
    SmeltUnion7::M1(smelt_arm) => { smelt_arm.insert(k, Into::<SmeltList<_>>::into(_smelt_tmp_6)); }
    //                                                 ^ a f64 into a SmeltList
}
```

## And the obvious guard makes it dead code

The natural fix is to take the per-arm path only when every arm wants the value
at the same type. That restores the slice to 9 — and is vacuous:

- string-keyed dict arms that all share one value type are all
  `Dict(String, V)` with the same `V`, so they are the SAME interned `TypeId`;
- identical members do not form a union.

So the guard's precondition cannot be satisfied by any union that exists. With
it in place the helper never fires — not in the hono slice (0 sites), and not
even in the purpose-built fixture above. Verified both ways before reverting.

## The conclusion, which is the useful part

**The round-trip is not an approximation of the per-arm insert. It is the
semantically faithful lowering, and the per-arm insert is the wrong one.**

At the moment of the write, JavaScript has no union: `results[key] = 36` stores
`36` whatever the runtime object is. The declared arm types are TypeScript
fictions the value need not satisfy. Erase-mutate-recover reproduces exactly
that, which is why it compiles for every arm shape; a typed per-arm insert
imposes a static constraint the source language does not have.

So the precision loss H40 was named for is **not in the write path at all**. It
is in `from_smelt_unknown`'s leaf extraction — the thing that turns `36` into
`"36"` on the way back in. That is where a fix belongs, it is independent of
index writes (any recovery through that path has it), and
`union_member_extraction_is_lossless` is already the test that documents the
behaviour.

## Recommendation

1. Withdraw H40. The note in `hono-h35-union-index-write.md` should point here
   rather than describing the per-arm insert as pending work; anyone who picks
   it up from that description will spend a round rediscovering the 9 → 81.
2. If the retyping matters to a real corpus failure, open it against
   `from_smelt_unknown` leaf extraction with a runtime fixture showing a wrong
   VALUE (not a wrong type name) after a recovery. Round 17 did not find such a
   failure: the slice's union index-write family now has zero sites, so nothing
   currently exercises the imprecision either way.
