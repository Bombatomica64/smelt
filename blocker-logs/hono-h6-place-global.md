# H6 write-through: `Place::Global` design

Owner: Hono implementer. Round 3, item 1. Date: 2026-09-06.
Ruling: `Place::Global` accepted; the store-back variant is explicitly rejected.

Background and the handle-vs-value analysis that led here are in
`hono-h6-module-mutable-globals.md` §8. Short version: `GlobalGet` emits
`NAME.with(|value| value.borrow().clone())`, which clones a *handle* for a
`SmeltRecord`/`SmeltJsMap` (so a write through it is already visible) and a
*deep copy* for a `HashMap` (so the write is lost). The frontend cannot tell
those apart without predicting a backend decision, so the fix must not depend
on the distinction at all.

## 1. What is wrong with the shape available today

`Place` is rooted at a local in all three of its variants:

```rust
pub enum Place {
    Local(LocalId),
    Field { base: LocalId, field: Symbol },
    Index { base: LocalId, index: Box<Operand>, negative: NegativeIndex },
}
```

So `cache[key] = value` where `cache` is a module-level mutable global can only
be expressed by first materialising the global into a local — which is the copy.
There is no way to say "the assignment target is *inside the cell*".

## 2. The variant

One new variant, carrying the projection so the global root and the projection
travel together:

```rust
/// A field or index projection rooted at a module-level mutable global.
///
/// Distinct from the local-rooted variants because the base is not a local at
/// all: it is a `thread_local!` cell, and the whole point is to mutate the
/// value INSIDE the cell rather than a copy read out of it.
Global {
    /// Index into `Mir::globals`.
    base: u32,
    /// What is being written inside the cell.
    projection: GlobalProjection,
},
```

```rust
pub enum GlobalProjection {
    Field(Symbol),
    Index { index: Box<Operand>, negative: NegativeIndex },
}
```

Deliberately **one** variant rather than two (`GlobalField` / `GlobalIndex`):
every consumer that cares about "is this rooted at a global" asks once, and the
projection is a second, smaller question. It also keeps the exhaustive-match
churn to a single new arm per site.

A whole-global assignment is **not** part of this: `x = e` already lowers to
`ExprKind::GlobalSet` and stays there. `Place::Global` is only for a write
*through* the binding.

## 3. Codegen

The whole assignment becomes one `with` closure, so the borrow lives exactly as
long as the mutation and no copy is made:

```rust
// cache[key] = value
CACHE.with(|cell| {
    let mut slot = cell.borrow_mut();
    let smelt_assign_index = /* key */;
    slot.insert(smelt_assign_index, /* value */);
});
```

Two consequences worth stating because they are easy to get wrong:

- **The operands must be evaluated *before* `borrow_mut()`.** If the index or
  the right-hand side themselves read the same global (`cache[cache.size] = 1`),
  evaluating them inside the borrow is a `RefCell` double-borrow **panic at
  runtime**, not a compile error. So the emitted code hoists both operands to
  temporaries above the `with`, and the fixture covers this shape.
- **A `Cell` global never reaches here.** `Cell` holds only `Copy` primitives
  and a primitive cannot be indexed or have a field written, so the
  `Place::Global` path is `RefCell`-only by construction. The emitter asserts
  that rather than assuming it.

## 4. Frontend

`assignment_target_mutable_global` currently matches only
`AssignmentTargetIdentifier`. It gains the member and computed-member cases:
when the *root* of the target is a mutable global and the projection is a single
field or index, the target lowers to the new place instead of to an
`Index`/`Field` over a `GlobalGet` temporary.

**Only a single projection level** is handled. `cache[a][b] = v` keeps the
existing blocker, because the inner `cache[a]` still has to produce a value and
whether that value shares with the cell is the same handle-vs-value question
this design exists to avoid answering. A nested write is a different, larger
change; the blocker for it stays specific.

## 5. Removing the guard

The declaration-time guard in `module_init.rs` (`is written through … only
whole-value reassignment … is lowered`) is narrowed, not deleted: it still
fires for the shapes §4 does not cover (nested projections, and the mutating
*method* call hole named in `hono-h6-module-mutable-globals.md` §7 which this
change does not close either). Deleting it outright would turn those into
silent lost writes, which is the outcome the whole family is about.

## 6. Fixture

Runtime tier, per the ruling: a module-level `Record<string, number>` mutated
from **two different functions**, with a third function reading it, asserting
the mutation from function A is visible to function B — that is the property a
copy would break and a type-level test would not catch. Plus the
`cache[cache_dependent_key()] = v` shape from §3 so the double-borrow panic is
covered.

---

## 7. Outcome, and the two things the design note did not predict

Landed. Hono's probe went **8 -> 7 occurrences / 6 -> 5 files**: the
`wildcardRegExpCache` blocker is gone. Seven fixtures in
`module_global_shapes_runtime` pass, including the double-borrow one.

Adding the variant was the easy half. `Place` is matched exhaustively almost
everywhere, so the compiler enumerated **17 sites in `smelt-mir` and 15 in
`smelt-codegen-rust`** and each got a decision rather than a `_` arm. Three
kinds of answer recurred, and they are worth naming because a wildcard would
have silently picked the wrong one:

- *"no base local"* — analyses that collect the local a place is rooted at
  (`erased_record_promote`, `classes`, `classify`, `opt/mod`) contribute
  nothing for a cell-rooted write;
- *"but the index operand still reads one"* — `local_use::place_reads_local`,
  `move_on_last_use::collect_statement_reads`, `throw::local_read_counts`,
  `opt/mod::rewrite_place`. Answering "reads nothing" here would let an
  optimisation retire or move a local the write depends on. This is the arm a
  `_ => false` would have got wrong;
- *"this is a read path, so reaching it is a compiler bug"* — `place_text`,
  `place_ty`, `assignment_place_text`, `lower/expr::local_operand`. Reported as
  internal errors rather than given a best-effort spelling.

### Two bugs the design note missed

**A binding that is only ever written *through* was not lifted at all.**
`collect_mutable_globals` returned early unless the name appeared as a
whole-binding reassignment, so `let cache: Record<string, number> = {}` whose
only mutation is `cache[k] = v` stayed an ordinary module binding. Before this
change that was harmless — the shape could not be lowered anyway — but with
`Place::Global` in place it would have left the write on a module-local copy,
which is precisely the defect this family exists to prevent. The lift condition
is now "mutated at all", write-through included. Hono's own global happens to
be reassigned as well, which is why the blocker fired there and hid this.

**The emitter arm has to `return`.** `emit_assign_place_statement`'s arms fall
through to a shared tail that asks for `place_ty(place)`, and a global place
deliberately has none, so the first real transpile failed with the internal
error rather than emitting anything. Cheap to fix, and the reason it was caught
is that the internal error was an error instead of a default.

### Fixture note

The first version of the sharing fixture asserted `load(missing) === -1` on a
`Record<string, number>`. TypeScript types that read `number`, not
`number | undefined`, so the guard is a comparison the type system has already
ruled out and Smelt folds it away — the fixture was testing unsound source
rather than the feature. Every read in the committed fixtures is of a key that
was written first.

