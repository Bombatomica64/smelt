# Standards round 10: the two silent-wrong-value defects

Date: 2026-09-07. Previous round: `blocker-logs/standards-round9-progress.md`.
Plan: `blocker-logs/standards-tier-plan.md` §4. Demand:
`blocker-logs/hono-fetch-demand.md` §4.

## 1. What landed

| item | state |
| --- | --- |
| 1. module-level `const` holding a modeled host value | **landed** (`ead1cfb3`) |
| 2. `instanceof <concrete host class>` narrowing | **landed** (`6bd82d70`) |
| 3. `FormData` | not started; design still stands from round 9 §5 |
| 4. `crypto` | not started; design still stands from round 9 §6 |
| 5. `AbortController` / `AbortSignal` | not started; inventory still stands from round 9 §7 |
| 6. erased-cast `.size` collision | not started, and re-scoped — see section 4 |

Both landed items were *silent wrong values*, and in both cases the fix was as
much about deleting a silent fallback as about adding a capability. That is the
through-line of this round and the reason the two took a full round between
them: each fallback was load-bearing for shapes that had nothing to do with the
defect, so the work was in finding the narrowest rule that fixed the defect and
moved nothing else.

## 2. Item 1: the module-const slot, and three scopings

The defect and the mechanism are in the commit message. What is worth carrying
forward is how much the SCOPE had to be narrowed, because each widening broke
something real and each break named a rule:

* **Lift only what is read from a hoisted item body.** Lifting every
  class-typed module binding moved 17 frontend goldens for shapes that were
  never broken — a binding used only in module-body statements already reaches
  its ordinary local, because name resolution checks `self.scope` before any
  global path. The condition to lift on is therefore "there is no module-body
  local to read through", which is exactly the set
  `collect_mutated_names` already walks for the write direction. Reusing that
  traversal for reads is what confined the change.
* **Subtract the names the item binds itself.** The first read scan recorded
  every identifier, so a function whose PARAMETER shares a module binding's
  name counted as reading it. `40_computed_method_over_known_members` has
  precisely that (`function read(body, key)` next to `const body = new Body()`)
  and was lifted by it. The subtraction is deliberately not block-accurate, so
  the residual error is one-directional and lands on the named blocker rather
  than on a wrong value.
* **Lift only a MODELED class.** `type C = A & B` — an intersection alias —
  also lowers to a nominal `Type::Class`, and the empty erased record is its
  existing representation; claiming it broke
  `wraps_concrete_records_when_casting_to_erased_intersection_aliases`. The
  registry's "is this a modeled stdlib class" is the right key: it is exactly
  the set with a concrete generated representation carrying a reference
  identity.
* **Leave a binding another const path owns.** `const P = /x/g` read from two
  functions deliberately rebuilds its `SmeltRegExp` at each use site, because
  `lastIndex` is observable per object;
  `blocker-logs/module-const-construction-cost.md` measured that decision and
  `module_level_regex_const_compiles_its_pattern_once` pins it. A slot is
  arguably the more faithful reading of `const P = /x/g` — JavaScript really
  does have one object there — but that is that note's decision to revisit.

### A symbol leak fixed on the way

The synthesized initializer's name embedded the module's ABSOLUTE path
(`smelt_global_init__encoder__module__tmp_claude_0__home_user_smelt_…`). It put
a build machine's filesystem path into every generated crate, and it made any
golden containing the symbol unreproducible outside the directory it was
generated in — which is how it was found, since fixture 47 is the first golden
to contain one. Round 8 introduced the expression-initializer path but no
example golden had exercised it. The name now carries the global's HIR item
index.

## 3. Item 2: what the narrowing exposed

The narrowing itself is a general rule keyed on a new registry capability,
`StdlibClass::narrows_from_erased` — true exactly for the classes whose runtime
type declares a `SmeltFromUnknown` adapter alongside a host marker, because the
marker makes the check possible and the adapter makes the conversion possible.

Removing codegen's `false` guess for an unrecognized `instanceof` pair exposed a
gap that should be the next round's cheapest real win:

> **Six modeled classes have no erasure adapter and no marker, so none of them
> can answer `instanceof` on an erased value:** `TextEncoder`, `TextDecoder`,
> `EventEmitter`, `HttpServer`, `IncomingMessage`, `ServerResponse`.

`const x: unknown = new TextEncoder(); if (x instanceof TextEncoder)` emitted
`if false` and silently deleted the branch; it is now a named blocker. Erasing
one of these goes through the generic struct path that stamps
`__smelt_class: "TextEncoder"` rather than a registry marker, so its erasure
and its `instanceof` disagree. The fix per class is a marker plus an
`IntoSmeltUnknown`/`SmeltFromUnknown` pair — the shape `Blob` and `Headers`
already have, and `smelt_blob_record`'s one-definition rule is the pattern to
copy so the direct and reflected forms cannot drift. Six runtime types and
their tiers, mechanical once the first is done, and it retires the blocker
this round introduced.

`instance_of_text` is also now ~280 lines of one arm per class, each
duplicating the same marker-probe shape. The registry already answers "what
markers mean this class" (`host_instance_markers`); collapsing the chain onto it
is a refactor that would pay for itself the next time a class gains a marker.
Not done here.

## 4. Item 6 re-scoped: it needs Dict provenance

`blocker-logs/standards-erased-cast-size-collision.md` is updated with the
sharper diagnosis. Round 9's "fix shape" said to make
`supports_stdlib_size(receiver_ty)` decline for a Dict that came from a declared
interface literal. That cannot be done as stated: the gate is given a `TypeId`
and a `Type::Dict(K, V)` carries no provenance — a `Map`, a `Record<K, V>` and
an inline interface literal are deliberately the same interned type
(`CLAUDE.md`, "Frontend validation boundaries"), and `type_has_known_field`
answers `true` for every Dict unconditionally.

The correct fix is Dict provenance in the type, honoured by every Dict
construction site; the cheap alternative only covers an annotated local and
leaves the repro (whose receiver comes from an `as` cast) broken. Either way the
collision is any interface-literal member that is also a modeled collection
member — `size` is just the one that takes no arguments, so it answers
silently instead of failing to type-check.

## 5. Items 3, 4, 5

Not started. Their designs from round 9 are unchanged and still the ones to
build: `standards-round9-progress.md` §5 (`FormData`: the pair-list shape, the
`string | SmeltBlob` value union, and the hand-written RFC 7578 parser with the
reason each candidate crate is the wrong shape), §6 (`crypto`: per-member crate
picks, and why `getRandomValues` must take the concrete byte view), §7
(`AbortController`: what is already in tree, so the next round starts from an
upgrade rather than from scratch).

One correction to §7 worth making now: it says to start by reading the existing
`abort_signal_runtime.rs` tier. Add to that — check whether `AbortSignal`'s
erased form carries `__smelt_abortsignal` through a real `IntoSmeltUnknown`, or
only through the constructor's record literal. `instance_of_text` special-cases
those two markers outside the host-object registry, which is the same
erasure/instanceof split section 3 describes, and the upgrade should close it
rather than inherit it.

## 6. Gate numbers

* examples end-to-end: **10/10**, new fixture `47_module_const_host_value`
  byte-identical to Node 22.
* `smelt-frontend-ts`: **1070 / 0**.
* `smelt-codegen-rust` lib: **1004 / 0** (three new regression tests).
* examples SmeltUnknown invariant: avoidable **0**, prelude +0, legitimate +0.
* es-toolkit: all **745** files still transpile; ratchet avoidable
  32416 -> **32396** (-20), legitimate 38961 -> 38866 (-95), prelude +0.
  Re-snapshotted. `cargo check` errors unchanged at **2**
  (`flatten.rs`/`flatten_1.rs`, a recursive-capture `Ref` deref-move emitted by
  `rendered_text_rewrite.rs`) — **verified pre-existing** by rebuilding `smelt`
  at the merge base and re-transpiling: byte-identical errors.
* radash: **84 / 0**.
* remeda: **1787 / 2** — the `groupByProp` symbol failures, unchanged and owned
  by the Hono stream this round.
* four example HIR goldens moved by type-table ORDER only, bodies identical.

## 7. One environment note for the coordinator

Disk was the binding constraint on this round, not compile time. The volume hit
100% (104K free) twice with the shared `/home/user/smelt/target` at 14G and a
sibling worktree's `target-priv` at 12G, which cost two false gate failures
(both ENOSPC surfacing as a test error, once as `generated crate failed` and
once as an empty log) and several full rebuilds after clearing my own target
directory to make room. `scripts/regen-example-dumps.sh`, added this round, cuts
one of the expensive loops: a type-table shift used to need one full
cross-language test cycle per affected example, because the test stops at the
first mismatch.
