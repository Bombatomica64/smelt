# H31 — a write through a projected receiver was silently lost

Round 11, item 1. The only family in `hono-phase2-generated-rust.md` with NO
rustc error: the generated Rust type-checked, compiled, ran, and printed a
different answer than Node.

```ts
class Node {
  children: Record<string, Node> = {};
  pattern = '';
  insert(key: string, pattern: string): void {
    const child = new Node();
    this.children[key] = child;   // wrote into a COPY of this.children
    child.pattern = pattern;
  }
  patternOf(key: string): string {
    const child = this.children[key];
    return child ? child.pattern : 'missing';
  }
}
const root = new Node();
root.insert('a', 'p1');
console.log(root.patternOf('a'));   // Node 22: p1   Smelt: missing
```

## Why

A MIR place is rooted at a LOCAL, so `this.children[key] = child` cannot name
`this.children` as its base: `lower_place` materialized the receiver into a
temporary and wrote through that.

```rust
_smelt_tmp_5 = self.children.clone();
_smelt_tmp_5.insert(key.clone(), child.clone());
// ... and _smelt_tmp_5 is dropped
```

Whether that write reaches `this` depends entirely on the receiver's
REPRESENTATION, which is a codegen choice MIR cannot see:

* a Smelt collection HANDLE (`SmeltList`, `SmeltRecord`, a reference class)
  shares its storage across clones, so the write lands in the original — this is
  the property the `a.b[i] = v` path has always relied on, and it is why the
  bug went unnoticed;
* a VALUE representation (a plain `HashMap`/`Vec` field, a value-class struct)
  is deep-copied by `.clone()`, so the write goes nowhere.

`lower_place`'s own comment asserted the first case as if it were the only one.

## The rule

The assignment path now does what the collection-mutation path has always done
for `a.b.push(x)` (`lower_mutation_receiver`): it records the projection it
copied and stores the temporary back through it once the write is emitted.

```rust
_smelt_tmp_4 = self.branches.clone();
_smelt_tmp_4.insert(branch.clone(), _smelt_tmp_5);
self.0.borrow_mut().branches = _smelt_tmp_4;   // ← the commit
```

Correct for both representations, and it needs no answer to the representation
question — committing a handle stores an equal handle; committing a value commits
the copy. That is the same reasoning `Place::Global` (H6) used for a mutable
global's cell, one level down.

Three pieces, all in `crates/smelt-mir/src/lower/`:

* `PlaceWritebacks` (`mod.rs`) — the copies a place write must commit, plus the
  rationale above.
* `place_base_local` (`place.rs`) — roots a projection at a local and returns the
  writeback. It recurses through `lower_place`, not `lower_expr`, which is what
  makes DEPTH work: `this.branches[b].leaves[k] = leaf` contributes one entry per
  level, replayed innermost first so each commit lands in a base that is still
  live.
* `write_back_place_receivers` (`expr.rs`) — the replay, called by both
  assignment sites (`HirStmt::Assign` and the C-style `for` update latch).

`narrowed_receiver_base` (the optional-receiver path H19 added) roots its
receiver the same way, and the narrowing unwrap itself gets a writeback: the
unwrapped value is a copy of what the optional holds, so it is stored back into
the optional place, which the emitter re-wraps for an optional destination.

### Two spellings that deliberately keep the old path

* **`TypeAssert` / `UnknownCast` receivers.** `bucket![0] = 9` roots at the local
  `bucket`, whose declared type is still `number[] | undefined`; an
  optional-typed place base has no index-write spelling, and the write was
  silently DISCARDED (`let _ = 9.0;`) when I first routed these through
  place lowering. Their `lower_expr` performs the narrowing the write needs, so a
  wrapped receiver keeps the materialize-a-value path. Caught by H19's own
  runtime fixture — the reason that tier exists.
* **A mutable global receiver.** `Place::Global` is already the assignment root
  and has no lvalue fragment to project onto; it keeps its own path.

## Evidence

`crates/smelt-codegen-rust/tests/projected_receiver_place_runtime.rs` —
`a_write_through_a_projected_value_receiver_lands_in_the_original`, five cases:
the one-level `this.children[key] = child`; two levels
(`this.branches[b].leaves[k] = leaf`, and a field and an index write through the
same nested receiver); a second insert that must not lose the first; a nested
list write (`this.rows[r][c] = v`); and the same shape with no `this` in sight
(`outer.a['k'] = 5`). The tier lowers the fixture to a crate and runs it, so a
green run means every `expect(...)` held at runtime.

Diffed against Node: the same file run under `vitest` in Node 22 passes all five,
and the reduced `Node`/`patternOf` program above now prints `p1` in both (`cargo
run` and `tsx`), where Smelt printed `missing`.

## The `SmeltUnknown` report

es-toolkit's avoidable erasure moves **32396 → 32401 (+5)** and its total
occurrences **73886 → 73938**, entirely from the commit statements this change
adds. The shape-by-shape diff shows what the +5 is:

| delta | shape | what changed |
| ---: | --- | --- |
| −6 / +6 | `let _smelt_tmp_N: SmeltUnknown;` → `let mut _smelt_tmp_N: SmeltUnknown;` | the same declaration, now written through |
| −3 / +3 | `let result: SmeltUnknown;` → `let mut result: SmeltUnknown;` | same |
| −2 / +2 | `self.props` → `self.0.borrow().props` | a class the new write reclassifies from value to reference |
| +6 | a receiver copy typed as its own receiver | an erased receiver's copy is erased; no other type exists for it |

No new `SmeltUnknown` CONVERSION is introduced — the change is in MIR place
lowering and mentions no type at all; a copy of a receiver is typed exactly as
the receiver, so there is no concrete type, union, or scoped generic available to
carry it. The 48 new `legitimate-boundary` occurrences are the erased-object
write arms of those same commit statements. The baseline is re-snapshotted in
this commit with this accounting rather than the increase being accepted
silently.

## Still open, one level further

A write through an optional receiver whose inner value is a VALUE CLASS commits
back through `Place::Local` of the optional, which is correct; a write through
an optional receiver that is itself reached through a *chain of optional
projections whose intermediate values are value classes* has one copy per level
and only the levels that are places are committed. Every shape the fixture
covers is committed; the residue needs `Place` to nest (the 328-site refactor
this round deliberately did not take), and is recorded here rather than in a
comment nobody reads.
