# remeda `pipe`: an identity-sequence-dependent intermittent failure

## Symptom

    remeda  uniqueBy > pipe get executed 3 times when take before uniqueBy
    remeda  uniqueBy > pipe gets executed until target length is reached
    panicked at src/map.rs: "unknown is not array"

Intermittent: roughly one run in five or six of the generated remeda suite,
on an unchanged binary.

## Root cause (measured, not hypothesised)

**The variable is test PARALLELISM, not hash iteration order.**

    cargo test ... --no-fail-fast                     ~1 run in 5 fails
    cargo test ... -- --test-threads=1                4 of 4 clean, always

Smelt's runtime identity state is `thread_local`: `smelt_next_object_id`,
`SMELT_LIST_IDENTITIES`, `SMELT_FUNCTION_IDENTITIES`, `SMELT_FUNCTION_ORIGINS`
and friends all live per thread. `cargo test` distributes tests across threads
nondeterministically, so which tests share a thread decides how many objects
were minted before a given test runs — and therefore what `id` its objects get.

The failing tests exercise remeda's LAZY `pipe`, which stores per-operation
state keyed by identity. Some id sequences break it. Everything that perturbs
allocation or hashing changes the probability without changing the underlying
defect.

## Why this matters beyond the one test

It has now blocked two separate improvements, and misdirected the diagnosis of
both:

1. **Erasure sharing** (`From<SmeltList<SmeltUnknown>> for SmeltArray` using
   `with_storage`). Recorded first as "does not reproduce" on the strength of a
   single green run, then reverted in #219 with the intermittency documented but
   the cause guessed as hash-iteration order. That guess is wrong.
2. **FxHash property keys.** A measured win (es-toolkit `unique` 25.4M -> 14.5M
   instructions, `partition` 136.8M -> 121.7M) that cannot land while this bug
   exists, because it raises the failure rate from "not seen in 10 runs" to
   about 1 in 5.

Neither change causes the defect. Both make it more likely to be observed.

## Localised: the panic is an un-erasure adapter, fed a non-array

`src/map.rs` line 16 is the adapter that rebuilds a `SmeltList<SmeltUnknown>`
from a `SmeltUnknown` for `map`'s array parameter. Its arms are `Null`,
`Undefined`, `Array`, `Object` (host buffer / `arguments` / `__smelt_map` /
`__smelt_set` / `Symbol.iterator`), and everything else panics.

The failing fixture's data is `[1, 2, 2, 5, 1, 6, 7]` — plain numbers. A
`SmeltUnknown::Number` reaching that adapter means an ITEM arrived where the
ARRAY was expected, which points at remeda's `purry` data-first/data-last
dispatch under the lazy protocol rather than at the adapter itself. `purry`
decides which form was called from the argument shape at runtime, so an erased
representation that answers its array-ness or arity check differently, for some
identity sequences, would route one item into the array slot exactly this way.
That is the thread to pull.

## A separate, independent defect found while localising this

The same conceptual conversion — erased value to typed list — is emitted with
DIFFERENT arm sets in different places. The `groupBy` adapter carries

    SmeltUnknown::String(value) => value.chars().map(..).collect(),

and this one does not, so a string that iterates fine in one lowering panics in
the other. Whatever the outcome of the identity bug, one lowering should not
have a hole its sibling does not; the arm set belongs in one shared emitter
helper. Not fixed here, and not the cause of this failure (the value in flight
is a number, not a string).

## Where to look

`src/map.rs` in the generated remeda crate, the `"unknown is not array"` arm —
an erased value reaching the map path is not an array when the lazy pipeline
expects one. Work backwards from there into `pipe`'s lazy-operation state and
what it keys on. The reproduction is cheap now that the mechanism is known:
run the suite multi-threaded in a loop, or force an adverse id sequence by
minting objects before the test body.

## What NOT to conclude

* Not a hash-ordering bug. A deterministic hasher (`SmeltFieldHasher`) did not
  remove the intermittency.
* Not a flake in the testing sense. It is a real defect with a probabilistic
  trigger; `--test-threads=1` hides it rather than fixing it.
* A single green run of the remeda gate proves nothing about any change that
  touches allocation, identity, or hashing. Run it at least ten times.
