# es-toolkit match/switch join-block emitter + first whole-crate transpile

This entry documents the Rust-emission blocker that gated es-toolkit's first
whole-crate transpile, the general fix, and the small emitter families cleared
afterward until `smelt build` at the es-toolkit root emitted `dist-smelt`.

## Primary blocker: match codegen join block

`smelt build` HIR- and MIR-lowered the whole crate but aborted in Rust emission
with:

```
EmitError { message: "match codegen requires all non-terminating arms to share one join block" }
```

### Root cause

A source `switch`/`match` lowers to a MIR `Terminator::Match` whose arms are
separate basic blocks (`crates/smelt-mir/src/lower/stmt.rs::lower_match`). Every
arm that does not otherwise diverge ends with `Goto(join)` where `join` is a
single continuation block created up front.

The emitter (`crates/smelt-codegen-rust/src/emitter/control_flow_match.rs`)
assumed each arm target's **direct** terminator was that `Goto(join)`. It scanned
the arm targets, collected their direct `Goto` targets, and required them all to
be equal so the join could be hoisted and emitted **once** after the `match`.

That assumption breaks whenever an arm body contains its own control flow. The
triggering function was `trimEnd` (`src/string/trimEnd.ts`):

```ts
switch (typeof chars) {
  case 'string': { if (chars.length !== 1) throw ...; while (...) endIndex--; break; }
  case 'object': { while (...) endIndex--; }
}
return str.substring(0, endIndex);
```

The `'string'` arm target ends in a `Switch` (the conditional throw), and the
`'object'` arm target ends in `Goto(<loop-header>)`. Neither directly `Goto`s the
real join (`return str.substring(...)`); they reach it only transitively through
their loops/branches. The synthesized default `Goto`s the join directly. So the
scan saw conflicting direct targets (`<loop-header>` vs `<join>`) and aborted,
even though all arm regions genuinely converge on one continuation.

### General fix

Emit each arm as a self-contained region that carries its own control-flow tail,
rather than assuming one hoistable join:

- `match_join` now returns `Some(join)` **only** when every arm target's direct
  terminator is either a `Goto` to one shared block or a diverging terminator
  (`Return`/`Throw`/`Unreachable`). If any arm ends in a `Switch`/`Call`/`Await`/
  `Match`, or two arms `Goto` different blocks, it returns `None` (previously it
  raised the hard error).
- When a single clean join exists, behavior is unchanged: arms drop their
  trailing `Goto` and the join is emitted once after the `match` (preserves
  existing tests / output).
- When there is no hoistable join, each arm (and the default) is emitted via the
  existing `emit_block`, which already lowers nested loops, branches, and
  returns and follows each arm's tail to its own continuation. The shared
  continuation is duplicated into every arm that reaches it — the same treatment
  arms ending in a non-`Goto` terminator already received.

This reuses the emitter's existing structured-control-flow machinery instead of
adding a bespoke join solver, and needs no MIR changes.

Regression tests (`crates/smelt-codegen-rust/src/tests/part_6_tests.rs`):
`emits_switch_with_heterogeneous_arm_successors` and
`emits_switch_arm_with_loop_and_conditional_throw` (the `trimEnd` shape). Both
previously aborted emission. End-to-end: a clean string-scrutinee variant was
transpiled, compiled, and executed, returning the JS-correct results
(`pick("loop",3)=6`, `pick("double",5)=10`, `pick("other",7)=7`).

## Subsequent families cleared (first-abort loop)

Each fix below was the next `smelt build` abort after the previous; all are
general emitter fixes, not per-file special cases.

1. **`array sort comparator must return a number`** — `sortKeys`
   (`src/object/sortKeys.ts`) passes an optional `(a, b) => number` comparator to
   `.sort()`. That optional callback is lowered through a synthesized wrapper
   closure whose call of the erased/optional inner callback yields `SmeltUnknown`,
   so the wrapper's declared return type erases to `unknown`. The frontend
   already admits this erased return and documents that "the emitter coerces the
   comparison result numerically", but the emitter still hard-required `Float`.
   Fix (`emitter/list_mutation.rs::list_sort_comparator_text`): a `number` return
   compares directly; an erased return (`unknown`/union/`never`/leaked
   non-scoped type parameter, all of which render as `SmeltUnknown`) is coerced
   through the existing `SmeltIntoF64` boundary adapter. This is a genuine dynamic
   boundary (the callback may be absent at runtime), not `SmeltUnknown`-to-compile.

2. **`set remove item must match the set element type`** — deleting a concrete
   value from a `Set<unknown>` (e.g. `Set<unknown>.delete(1)` in an `isEqualWith`
   spec). `set_add_text` already coerced inserted items to the element type via
   `value_at_type`, but `set_remove_text` used strict type equality plus a raw
   operand render. Fix (`emitter/set.rs::set_remove_text`): coerce the removed
   item to the set element type, mirroring add, so the lookup key erases to
   `SmeltUnknown` to match the stored keys. The now-unused
   `validate_set_item_operands` helper was removed.

3. **`dict set / dict remove / collection clear receiver must be a mutable local
   for now`** — a `Map`-backed class stores its map in a field and mutates it via
   `this.<field>.set/delete/clear(...)`, so the receiver operand is a `Field`
   place, not a bare `Local`. Fix (`emitter/map.rs`, `emitter/list_mutation.rs`):
   accept any place-rooted receiver and render its assignable lvalue via
   `assignment_place_text` (which returns `self.<field>` for class fields), so the
   mutation targets the stored collection in place instead of a temporary copy.

After these, `smelt build` at the es-toolkit root completed and emitted
`dist-smelt` (745 generated source files) — es-toolkit's first whole-crate
transpile.

## Whole-crate transpile status

`dist-smelt` **emits** but does **not** yet compile clean. `rust-diagnostics`
over the generated crate (see `estk-first-transpile-diagnostics.md`) reports
525 errors / 387 warnings, dominated by `E0308` mismatched types (394). Because
the crate does not compile clean, the `rust-test-report` prize step was not run
(it requires a compiling crate). The remaining generated-Rust diagnostics are a
separate, large body of work beyond this emitter blocker.

## Observations noted, not fixed (out of scope for this blocker)

- **`typeof`-switch scrutinee folding**: in `trimEnd`, `switch (typeof chars)`
  lowers with a constant scrutinee (`match "object" { ... }`) and folds
  `chars.includes(...)` inside the `'object'` arm to `false`. This is a
  `typeof`-switch narrowing/fold issue in the frontend, independent of the match
  emitter; it still emits valid (if runtime-incorrect) Rust.
- **switch-inside-loop loop recognition**: `block_exits_to_loop`
  (`emitter/control_flow.rs`) has no `Terminator::Match` arm (`_ => Ok(false)`),
  so a `for`/`while` whose body ends in a `Match` is not recognized as a loop and
  degenerates to a single-pass guard, and a match arm that should `continue`/
  `break` the surrounding loop does not. es-toolkit's first-abort loop did not
  reach this (it aborts earlier on the diagnostics families above), so it is
  documented here as the next control-flow item rather than fixed.
