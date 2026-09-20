# H28 — the switch fallback that deletes both branches' code

Round 12, item 2. Fixed here. Round 10 recorded this family from its 6 × E0308
as "a throwing call in statement position renders as `()`"; round 11 corrected
that to "the then-branch is emitted EMPTY and the continuation moved into an
`else`" and left the trigger open, with the instruction to start from the MIR.
That was the right place to start: **MIR is correct and complete.** The loss is
entirely in codegen's structured-control-flow reconstruction.

## The trigger

`emit_switch` (`crates/smelt-codegen-rust/src/emitter/control_flow.rs`) is a
chain of reconstruction arms, and it ends in an unconditional fallback:

```rust
out.push_str("    if {cond} {\n");
for statement in &then.statements { self.emit_statement(statement, out)?; }
out.push_str("    } else {\n");
for statement in &else_.statements { self.emit_statement(statement, out)?; }
out.push_str("    }\n");
```

It emits each block's OWN statements and **drops both terminators**, so
everything reachable through them is deleted. Anything that reaches this arm
loses code; nothing reports it.

Hono's `RegExpRouter#add` reaches it at `if (/\*$/.test(path))` — MIR
`bb17: switch move %52 ? bb18 : bb19`:

* `bb18` (then) has NO statements of its own; its content hangs off a `Call`
  terminator (`%53 = call fn1(copy %2) -> bb20`, `buildWildcardRegExp(path)`),
  so the then-arm emitted as `if _smelt_tmp_52 { }` — two loop nests, the
  `#insertPath` calls and every middleware write gone;
* `bb19` (else) is the continuation, whose statements landed in the `else`.

The 6 × E0308 was only the function then falling off its own end.

## Why every structured arm missed it

Both regions are self-contained: the then-region ends in the source's `return`
(`bb35: return undefined`), the else-region ends at the function's own end
(`bb53`). Neither rejoins the other, so no join exists to find — and the arm
that handles exactly that shape,

```rust
if self.block_eventually_terminates(then.id, ..)? && self.block_eventually_terminates(else_.id, ..)?
```

cannot fire, because `block_eventually_terminates` scores a back edge as
`false` and requires BOTH arms of a `Switch` to terminate. A loop header IS a
`Switch` whose body edge cycles back, so **one loop anywhere in a region poisons
that region to "does not terminate"**. `if (cond) { for (…) {…} return }` is
that shape, and it is the ordinary way to write an early-exit branch.

That is why round 11's 25-line reduction lowered correctly: it had a loop, but
the reduction's regions rejoined, so an earlier `Goto`-based arm claimed it. The
three candidates that note listed (the `re.test(p) && …push(…)` statement, the
`findMiddleware(..) || findMiddleware(..) || []` chain, the loop over an array
literal) were all innocent.

## The fix

`block_never_falls_through` — `block_eventually_terminates` with a back edge
answering `true` instead of `false`, because the two predicates ask different
questions. `block_eventually_terminates` asks "does control reach a `return`?",
and a cycle it cannot prove exits must answer `false`. The question that matters
for choosing a reconstruction is "does control ESCAPE this region and rejoin a
continuation?", and a path going round a loop has not escaped — it is still
inside the region, so it must not veto the answer.

A new arm sits immediately BEFORE the dropping fallback and emits each region in
full inside its own arm when neither falls through. Placing it there is
deliberate: it can only claim shapes whose code was previously discarded, which
is why no golden moved.

## Evidence

Hono routers slice: empty then-branches **2 → 0**. `RegExpRouter#add` now emits
the wildcard branch's `build_wildcard_reg_exp`, its loop nest and the source's
`return Ok(SmeltUnknown::Undefined)`, with the continuation in the `else` —
the source's shape.

The family is not Hono-specific. es-toolkit had **ten** functions whose real
bodies were being deleted, each left returning a default:

| file | was | now |
| --- | --- | --- |
| `filter`, `pullAllWith` | `return SmeltList::new(Vec::<SmeltUnknown>::new())` | real body |
| `range`, `rangeRight` | `return SmeltList::new(Vec::<f64>::new())` | real body |
| `some` | `return false` | both overload bodies |
| `lastIndexOf`, `reduce`, `reduceRight`, `result`, `sumBy` | default | real body |

Every one of the ten diffs is pure addition plus the removal of the synthesized
fallthrough default — ten silently-wrong library functions, none of which had a
diagnostic.

## What the restored code exposes

Restoring deleted code makes the type errors that code always had visible. This
is a count going UP because a correctness bug went away, so the families matter
more than the totals:

* **hono routers slice: 55 → 425 errors.** 360 of the 370 new ones are ONE
  shape — see H35 below. Family count barely moved.
* **es-toolkit: 7 → 8 errors.** The new one is H36 below.
* remeda 1789/0, radash 84/84, examples invariant 0 (+0), remeda advisory +0,
  workspace goldens byte-identical, 102 suites / 0 failures.

### H35 — an indexed WRITE into a concrete union crosses the erased helper

360 × `expected &mut SmeltUnknown, found &mut SmeltUnion164` at

```rust
smelt_index_assign(&mut _smelt_tmp_114, smelt_key, smelt_value);
// fn smelt_index_assign(target: &mut SmeltUnknown, key: String, value: SmeltUnknown)
```

This is H26's rule (a concrete generated union must cross `IntoSmeltUnknown` to
be inspected, and use its own value where a concrete type is expected) on the
index-WRITE path instead of the index-read path H26 fixed. Same rule, sibling
site. Largest family in the slice by a wide margin.

### H36 — `Array.isArray` narrowing emitted against a concrete list

`es-toolkit/pullAllWith.rs:76`:

```rust
_smelt_tmp_21 = values.clone().as_ref().is_some_and(|smelt_value| matches!(smelt_value, SmeltUnknown::Array(_)));
//                                                                ^ has type &SmeltList<SmeltUnknown>
```

`values` is already `Optional<SmeltList<SmeltUnknown>>`, so the runtime-tag test
has nothing to narrow: an `Array.isArray` guard over an already-concrete list
receiver should fold to `true` (or be dropped) rather than emit a
`SmeltUnknown::Array(_)` pattern against a `SmeltList`. One occurrence.

## Open: the es-toolkit ratchet moved, and it needs a decision

`blocker-logs/smelt-unknown-baseline-es-toolkit.json` is a BLOCKING ratchet and
this change takes avoidable erasure **32401 → 32516 (+115)** (legitimate
boundary +722).

Every one of those occurrences is inside the ten restored function bodies above
— verified by diffing the generated crate with and without this commit: 10 files
differ, each pure addition plus the removal of its bogus default return. No
emit site newly chose erasure; erasure that was always in those bodies stopped
being deleted along with them.

**The baseline is deliberately NOT re-snapshotted here.** CLAUDE.md's re-snapshot
path requires documenting a genuine dynamic boundary and reclassifying via
`classify_line`, and neither applies: these are the same ordinary avoidable
shapes the corpus already holds 32401 of, so reclassifying them would be false.
The choice is between keeping a code-deletion bug to hold a number flat and
re-snapshotting a blocking gate, which is above an implementer's authority.
Flagged for the coordinator: either re-snapshot with this file as the
justification, or hold H28 until H35 closes and re-measure (H35 is the dominant
family in the restored code, so it may absorb much of the +115).

## Ratchet decision (orchestrator, 2026-09-08)

Landing H28 moves the es-toolkit avoidable-erasure ratchet 32403 -> 32518 (+115). Every one of
the 115 lines sits inside the ten function bodies (`filter`, `lastIndexOf`, `pullAllWith`,
`range`, `rangeRight`, `reduce`, `reduceRight`, `result`, `some`, `sumBy`) that the switch
fallback was silently deleting; the diff of the generated crate with and without the commit is
ten files, each a pure addition of the restored body plus removal of its bogus default return.
No emit site chose erasure it did not choose before: the corpus grew by code that was always
supposed to be there, and that code carries the same ordinary shapes the corpus already holds.

The baseline is re-snapshotted at 32518 with this note as the justification. This is NOT the
CLAUDE.md reclassification path (there is no new dynamic boundary to document), and it is not an
accepted regression: holding a fix for silently deleted program code because the deleted code
contains erasure would invert the ratchet's purpose. Methodology gap recorded: the ratchet
measures lines, so a fix that restores dropped code reads as a rise; a per-emitted-function
density would not. Worth fixing in `unknown_report.rs` when the metric is next touched.
