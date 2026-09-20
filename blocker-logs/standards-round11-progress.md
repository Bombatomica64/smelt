# Standards round 11: every modeled host class erases through its own adapter

Date: 2026-09-07. Previous round: `blocker-logs/standards-round10-progress.md`.
Plan: `blocker-logs/standards-tier-plan.md` §4. Demand:
`blocker-logs/hono-fetch-demand.md` §4.

## 1. What landed

| item | state |
| --- | --- |
| 1. retire the round-10 `instanceof` blocker for six modeled classes | **landed** (`c8ba4ebc`) |
| relayed Hono demand: `new Headers(init)` in `src/context.ts` | **checked, already resolved** (`a0c70355`) |
| 2. `FormData` | not started; design unchanged from round 9 §5 |
| 3. `crypto` | not started; design unchanged from round 9 §6 |
| 4. `AbortController` / `AbortSignal` | not started; inventory unchanged from round 9 §7 |
| 5. item 6 (`.size` on an `as`-cast interface literal) | not started; corrected note stands (round 10 §4) |

One item, and it was the right size for one: what began as "add six markers"
turned into collapsing three separate hand-maintained lists onto the registry,
because those lists are the mechanism by which the six classes came apart from
the rest in the first place.

## 2. The shape of the defect, and why it recurred

The six classes — `TextEncoder`, `TextDecoder`, `EventEmitter`, the `node:http`
`Server`, `IncomingMessage`, `ServerResponse` — are backed by generated runtime
types whose state is closures and cells, not declared fields. Erasing one fell
through to the GENERIC STRUCT path, which reads declared fields and stamps
`__smelt_class`. A prelude type has no declared fields, so the erased value
carried no state and no identity.

The interesting part is not the fix but the *recurrence*. Three places already
knew "this class has its own adapter", each with its own list of class names:

* `is_fetch_runtime_class_type` (nine names) decided which erasures route to
  `.into_smelt_unknown()`;
* `instance_of_text` had one `if class_name == ".."` block per class;
* `reflected_construct_kind` carried two exclusion lists to stop a
  runtime-typed class being reflectively built as a marker record.

A class is only correct when all three agree, and nothing made them agree. The
third one is the sharpest illustration: its list already named `EventEmitter`,
so someone had noticed the hazard for exactly one of the six — and the other
five would silently have become reflectively constructible as records the moment
they gained a marker, answering `instanceof` correctly and then failing every
method called on them.

All three now ask the registry: `StdlibClass::erases_through_adapter`,
`narrows_from_erased`, `reflects_to_marker_record`. `instance_of_text` went from
285 lines to 239, and the arms that remain are the ones asking something OTHER
than marker presence — an error subclass compares the marker's VALUE, a promise
is not an `Object` variant, a concrete `Map`/`Set` has a static answer from its
storage — each now saying so.

**The lesson worth carrying:** a per-class list in an emitter is a latent
disagreement. When a second site needs the same question, the question belongs
in the registry, and the cost of not moving it is paid by whichever class is
added next.

## 3. Identity, not reconstruction

`Headers` and `Blob` round-trip STRUCTURALLY — their record carries the header
pairs or the bytes — so a recovery can rebuild an equal value. The six cannot:
there is nothing in a record to rebuild a listener list or a tokio shutdown
sender from.

What JavaScript does there is not rebuild anything. Erasing a value and
narrowing it back yields the SAME object:

```ts
const x: unknown = emitter;
(x as EventEmitter).on("data", f);   // the emitter's own listener list
```

So the erasure RETAINS the live value in a new `SMELT_HOST_ORIGINS` registry
under the erased record's object id, and the recovery hands that value back.
Two details are load-bearing:

* **Keyed on the JS object id, not an `Rc` address.** The sibling callable
  registries key on an address and need a `Weak` guard against address reuse
  (see the long note at `SMELT_CALLABLE_KEY_GUARDS` — a recycled address is
  what made remeda's lazy `pipe` fail intermittently). Object ids are minted
  monotonically by `smelt_next_object_id` and never reused within a thread, so
  this registry needs no guard at all.
* **Pay-for-use.** Sixteen example goldens grew by the registry before it was
  gated on the four `needs_*` flags of the preludes that use it.

### `Recovery`: what a record with no origin means

The only per-type question left, and each answer is a real state rather than a
guess:

* `Structural` for the codecs — a codec's state besides its identity IS its
  encoding label, so the record is lossless and the registry is only an
  identity optimization there;
* `Empty` for the emitter, request and response — an emitter with no listeners
  and a response nothing has been written to are real states;
* `None` for the `node:http` `Server`. A server is a live listening socket with
  a handler closure; there is no empty one. Rather than fabricate a server that
  is not listening it emits NO recovery, and narrowing an erased server stays a
  named blocker. Splitting `narrows_from_erased` from `erases_through_adapter`
  is what let the five recoverable classes gain narrowing while the server did
  not — round 10 conflated the two and excluded classes whose `instanceof` was
  perfectly answerable.

## 4. A latent round-10 regression, caught by a tier

A concrete host value cast to a RECORD or COLLECTION fell straight through and
was assigned to a `SmeltRecord` (E0308). The adapters that walk a value into a
typed record are reached only when the source is already erased — and round
10's `instanceof` narrowing means that inside

```ts
x instanceof Blob ? (x as { type: string }) : …
```

the operand is now the concrete class where it used to be `unknown`. Such a cast
now erases through the adapter first, stated over `erases_through_adapter`.

Worth noting how it was found: the `blob_runtime` TIER caught it, not any unit
test, and round 10 did not run the tiers. **Run the standards tiers in any round
that touches narrowing or erasure** — they are the only gate that compiles and
executes the generated crate for these types.

## 5. Where the erasure test lives, and why not in `examples/`

The first draft of this round's verification was an end-to-end fixture,
`48_host_value_erasure_identity`. It pushed the examples corpus's avoidable
erasure from 0 to **53**, breaking the hard invariant.

Every one of those 53 lines was in that fixture, and every one traced to a
source-level `unknown` the fixture wrote on purpose. So the choice was to weaken
`classify_line` for every corpus, or to move the test. It moved: the examples
carry a hard "avoidable == 0" invariant precisely so that a rise there is a
readable lowering regression, and a fixture whose whole subject is the erased
boundary destroys that signal. The new `host_erasure_runtime` tier is not
scanned by the invariant and can additionally assert runtime behaviour a golden
cannot see — that the narrowed branch RUNS, and that a listener registered
before erasure fires through the narrowed handle.

The general rule: **the examples corpus is for lowering that should not erase;
a tier is for behaviour at the boundary.**

## 6. The relayed Hono demand item was already resolved

Full write-up in `blocker-logs/standards-hono-headers-init-resolved.md`. In
short: all four shapes the coordinator named are typed correctly and match
Node 22, and round 10's narrowing is what fixed it — the receiver is a
degenerate `Response | ResponseInit` union, and the `instanceof Response` guard
now recovers a real `SmeltResponse` from it.

`smelt check` over the whole Hono crate reports **zero** diagnostics. The build
aborts one stage later, in MIR lowering: `field and index reads currently
require a local receiver` at `src/router/trie-router/node.ts` — verified
identical at the round-10 head, so not new, and a general lowering family for
the Hono stream.

## 7. Items 2-5, unchanged and ready

Not started, and their designs need no revision:

* **`FormData`** — round 9 §5: the ordered pair list, the
  `string | SmeltBlob` value union (the reason it belongs after `Blob`), and
  the hand-written RFC 7578 parser with the reason each candidate crate is the
  wrong shape (`multer` is async over a `Stream` for a body Smelt has already
  buffered; `multipart` 0.18 is built for hyper 0.10/iron; the rest are
  unmaintained). One addition from this round: `FormData` is currently an
  identity-only registry entry, and `reflected_construct_kind` names it
  explicitly to stop a marker record standing in for the surface. When it gains
  a real runtime type it should join `erases_through_adapter`, and that name can
  come out of the explicit list.
* **`crypto`** — round 9 §6.
* **`AbortController`/`AbortSignal`** — round 9 §7, plus round 10's addition:
  its two markers are the last ones `host_instance_markers` answers from the
  subsystem table rather than from the host-object registry, so the upgrade
  should move them into the registry and close that split rather than inherit
  it.
* **item 6** — round 10 §4: needs Dict provenance; the cheap alternative covers
  only an annotated local.

Sequencing note for whoever picks these up: each is a full round on the
evidence of rounds 9-11 (one modeled type with its tier, fixture and corpus
gates has been the unit of work each time). `FormData` is the largest of the
four because it is the only one that also needs a parser.

## 8. Gate numbers

* examples end-to-end: **10/10**.
* `smelt-frontend-ts`: **1070 / 0**.
* `smelt-codegen-rust` lib: **1008 / 0** (four new regression tests).
* standards runtime tiers: **23 / 23** — `fetch_types` 8, `blob` 6,
  `text_codec` 5, and the new `host_erasure` 4, registered in the `standards`
  shard.
* examples SmeltUnknown invariant: avoidable **0**, legitimate +0, prelude +40
  for the six registry entries. Re-snapshotted.
* es-toolkit: all **745** files transpile; ratchet **+0 / +0 / +0**;
  `cargo check` errors unchanged at **2** (`flatten.rs`/`flatten_1.rs`, the
  recursive-capture `Ref` deref-move) — the Hono stream's, left alone as
  instructed.
* radash: **84 / 0**. remeda: **1787 / 2**, the pre-existing `groupByProp`
  symbol failures, unchanged.
* `cargo clippy --lib`: **0 errors**.
* example goldens: sixteen `expected.rs` move, all of them the shared runtime
  registry TABLES gaining the six new markers (the host-marker filter, the
  constructor-class table, the `toStringTag` table). That last one is the point:
  `Object.prototype.toString.call(erasedEncoder)` now answers
  `[object TextEncoder]`.

## 9. Session note

This round was interrupted mid-item by an account spend limit; the coordinator
committed the in-flight edits as a WIP checkpoint. Recovering was clean —
`git reset --soft` returned them to the working tree and the finished commit
replaced the checkpoint via `--force-with-lease`, after diffing both directions
to confirm the only WIP-only lines were the per-class arms the collapse
deliberately removed.

The reason it was recoverable is that the checkpoint was a coherent slice: the
markers, the registry, the codec adapters and the predicates, with the remaining
four adapters and the collapse still to come. Committing feature work in slices
that each answer one question is what made a mid-round cutoff cost nothing.
