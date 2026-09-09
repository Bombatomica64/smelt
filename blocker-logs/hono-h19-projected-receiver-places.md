# H19: a MIR place needs a local root, and two sites demanded one instead of making one

Round 10, item 2. This was the gate to phase 2: source lowering was clean at
0 blockers, but whole-crate MIR lowering aborted, so the Hono crate could not be
emitted at all.

## What it was

A MIR `Place` is rooted at a `LocalId`. Two lowering sites needed a root and
*required* the source to have already provided one:

| site | source shape | diagnostic |
| --- | --- | --- |
| `lower_for` (`smelt-mir/src/lower/stmt.rs`) | `for (const child of node.#patterns)` — an iterable that is a field/index projection, a call result, or a constant | `field and index reads currently require a local receiver` |
| `lower_place` (`smelt-mir/src/lower/place.rs`) | `partOffsets[p] = offset` where `partOffsets: number[] \| undefined` | `only local, field, and index expressions can be assigned` |

Both are in `src/router/trie-router/node.ts`, one after the other, and both are
general: nothing about them is Hono's, and the private-field spelling is
incidental (the public one failed identically).

`lower_place` already knew the answer — `Field`, `Index` and `TupleIndex`
receivers all call `materialize_operand_local`, so `a.b[i] = v` works — and
`lower_for` simply did not use it.

The second site is subtler. `tsc` accepts `partOffsets[p] = offset` only because
the preceding `partOffsets = []` narrows the binding to `number[]`; the
frontend's narrowing does not reach the write target, so the target arrives as
`ExprKind::OptionalIndex` (and `row.count = 7` after `row = { count: 0 }` as
`OptionalField`). Rejecting those is refusing a program `tsc` proved sound.

## The rule

* `lower_for` materializes the iterable into a temporary
  (`materialize_operand_local`). JavaScript evaluates the iterable expression
  exactly once and then iterates that value, so one temporary is what the
  semantics ask for, not a workaround.
* `lower_place` gained `OptionalField` / `OptionalIndex` arms through a new
  `narrowed_receiver_base`, which materializes the receiver as a temporary of the
  optional's **inner** type. The emitter's existing narrowing unwrap
  (`Rvalue::Use` of an `Optional<T>` operand into a `T` destination) then emits
  `receiver.clone().expect("optional value was absent after narrowing")`, and the
  place is an ordinary field or index projection on that local — exactly what a
  hand-written Rust team writes.

Both rely on the property `a.b[i] = v` already relied on: Smelt's collection and
reference-class handles share their storage across clones, so a write through the
temporary lands in the original object. The runtime tier below is what proves it
rather than assuming it.

`local_operand`, the helper whose only purpose was to reject a non-local
receiver, is deleted: it had no other caller.

## Two things found on the way

1. **The rejection did not say what it refused.** `place_unsupported` emitted one
   message for ~300 `ExprKind`s, so identifying the shape took a debug build. It
   now names the variant (`... can be assigned, not `OptionalIndex``), which is
   how the second site above was diagnosed in one run.
2. **An optional-chain field read on a REFERENCE class did not compile.**
   `field_access_text`'s fallback emitted `handle.field.clone()`, but a reference
   class is an `Rc<RefCell<Inner>>` handle whose fields are reached through it
   (`handle.0.borrow().field.clone()`, which the ordinary place path already
   knew). E0609 in the generated crate. It was unreachable until the
   `OptionalField` write above stopped being rejected, and it is fixed here
   because the H19 fixture is what reached it.

## Tests

`crates/smelt-codegen-rust/tests/projected_receiver_place_runtime.rs` (new tier,
registered in the `values` shard of `.github/workflows/runtime-tiers.yml`):

* `a_for_of_iterates_a_receiver_that_is_not_a_local` — `for...of` over a public
  field, a private field, an index read and a call result, plus a body that
  mutates a projected collection and observes it in the original.
* `a_write_through_a_narrowed_optional_receiver_lands_in_the_original` — the
  index write (Hono's `partOffsets` loop, values checked against Node 22), a
  field write on a reference class, and a write through a definite assertion
  (`bucket![0] = 9`) observed through the original record.

## Aftermath

With H19 fixed, whole-crate MIR lowering of Hono completes and the build reaches
the EMITTER, where it stops on `new Headers(init)` — a standards-stream demand,
recorded in `blocker-logs/hono-fetch-demand.md`.
