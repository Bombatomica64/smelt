# `AbortSignal`: what the marker record still gets wrong, measured

> **Round 14 status.** Everything in the "silent wrong values" and "member
> inventory" sections below is FIXED, in the order this log gave: `reason`,
> `throwIfAborted()`, `AbortSignal.abort`, `AbortSignal.timeout`, and
> `Request.signal` as a dependent signal with its follow list. The tier is
> `crates/smelt-codegen-rust/tests/abort_reason_runtime.rs`. What is left is
> `signal.onabort =` (a property WRITE that synthesizes a listener),
> `AbortSignal.any([..])` (the same follow list, fanned in rather than out),
> and the marker-record-to-concrete-type upgrade — the last of which is now
> blocking an end-to-end fixture for this surface, since every local holding a
> signal is an erased local and the examples corpus must hold zero avoidable
> erasure. See "Why this surface has no end-to-end fixture yet" at the end.

The two markers are now host-object registry entries (round 13), which closes
the split round 10 recorded. This log is the rest of the surface, every line
diffed against Node 22, so the marker→concrete upgrade starts from measured
behaviour rather than from the spec text.

## Two silent wrong values in what already exists

`smelt_abort_method`'s `"abort"` arm calls `smelt_abort_signal_fire(&signal)`
and **discards its arguments**, and its `_ =>` fallthrough answers
`SmeltUnknown::Undefined` for any method it does not name — which includes
`throwIfAborted`, even though `smelt_host_method` routes that name to it.

```ts
const c = new AbortController();
c.abort("because");
console.log(c.signal.aborted);              // Smelt true       Node true
console.log(String(c.signal.reason));       // Smelt undefined  Node because
console.log(String(c.signal.throwIfAborted())); // Smelt undefined  Node THROWS "because"
```

Both are the silent-wrong-value class: the crate compiles, the corpus accepts
it, and a program that branches on `signal.reason` takes the wrong branch with
no diagnostic. `throwIfAborted` is worse than a wrong value — a cancellation
check that never throws means the cancelled work runs.

## The full member inventory, diffed against Node 22

| member | Node | Smelt today |
| --- | --- | --- |
| `signal.aborted` before abort | `false` | `false` |
| `signal.reason` before abort | `undefined` | `undefined` |
| `controller.signal` read twice | same object | same object (shared `Rc`) |
| listeners | insertion order, each once | insertion order, each once |
| second `abort()` | no-op, reason unchanged | no-op |
| listener added AFTER abort | never fires | never fires |
| `abort(reason)` | `signal.reason === reason` | **reason discarded** |
| `abort()` with no argument | `reason` is an `AbortError` `DOMException`, message `This operation was aborted` | **no reason at all** |
| `signal.throwIfAborted()` not aborted | `undefined` | `undefined` |
| `signal.throwIfAborted()` aborted | throws the reason | **returns `undefined`** |
| `removeEventListener(t, fn)` | listener does not fire | works |
| `signal.onabort = fn` | fires on abort | not modeled |
| `AbortSignal.abort(reason)` | already-aborted signal with that reason | not modeled |
| `AbortSignal.abort()` | already aborted, `AbortError` reason | not modeled |
| `AbortSignal.timeout(ms)` | aborts after `ms` with a `TimeoutError` `DOMException`, message `The operation was aborted due to timeout` | not modeled |
| `Object.prototype.toString.call` | `[object AbortController]` / `[object AbortSignal]` | matches |
| `instanceof` | `true` | matches (registry, since round 13) |
| `Object.keys(controller)` | `[]` | `[]` |
| `for (const k in controller)` | `["signal", "abort"]` — Node's Web API prototype members ARE enumerable | `[]` (internal keys now hidden; prototype names not enumerated) |

## Why `Request.signal` is not a wiring change

The obvious reading — store the `init.signal` record on the request and answer
it from `request.signal` — is observably wrong:

```ts
const c = new AbortController();
const r = new Request("https://a.test/x", { signal: c.signal });
console.log(r.signal === c.signal);   // Node: false
c.abort("stop");
console.log(r.signal.aborted, String(r.signal.reason));  // Node: true stop
```

The spec makes a request's signal a **dependent** signal: a NEW signal that
FOLLOWS the given one, which is `AbortSignal.any([signal])`'s relationship. So
`Request.signal` needs a follow list on the record (or the concrete type)
before it can be wired at all, and storing the given signal directly would make
`===` answer `true` where Node answers `false`.

Two more measured facts for that work:

* `new Request(url)` with no init still has a signal — a fresh, non-aborted
  one. It is never `null`, so the member's type is `AbortSignal`, not
  `AbortSignal | null`.
* `new Request(url, { signal: AbortSignal.abort("pre") })` is born aborted with
  reason `"pre"`, so the follow relationship has to be evaluated at
  construction and not only on a later abort.

## Why `AbortSignal.timeout` waits on `reason`

The static itself is small: build a signal record, and spawn a task that sleeps
`ms` on the virtual clock and then calls the existing
`smelt_abort_signal_fire`, which is already idempotent and already drains the
listener list. What makes it untestable today is that its whole observable
difference from `AbortSignal.abort()` is its REASON — a `TimeoutError` rather
than an `AbortError` — and reasons are discarded. A tier could only assert
`aborted` flipping, which does not distinguish the two statics.

So the order for the next round is: `reason` (a genuine dynamic boundary — a
reason is any JavaScript value), then `throwIfAborted`, then the two statics,
then the dependent signal that `Request.signal` and `AbortSignal.any` share.

## Where the code is

* `crates/smelt-frontend-ts/src/lowering/new_expr.rs` —
  `abort_controller_constructor_expression` builds the two records.
* `crates/smelt-codegen-rust/src/lib.rs` — `smelt_abort_signal_fire`,
  `smelt_abort_method`, `smelt_abort_signal_object`, and `smelt_host_method`'s
  routing of the five method names.
* `crates/smelt-stdlib/src/host_object.rs` — the two registry entries and
  `HostObject::helper_backed_methods`.
* `crates/smelt-codegen-rust/tests/abort_signal_runtime.rs` — the existing
  seven-case tier; every case stays green through the registry move.

## Why this surface has no end-to-end fixture yet

Round 14 wrote one, diffed it against Node 22 byte for byte, and then removed
it: `examples/typescript/end-to-end` is the corpus whose invariant is ZERO
avoidable erasure, and an `AbortController` is a marker record — so
`const controller = new AbortController()` is a `let controller: SmeltUnknown`,
and the fixture added 213 avoidable occurrences on its own.

That is the invariant working, not a fixture problem. The reclassification
escape hatch does not apply either: `classify_line` is textual, and a bare
`let controller: SmeltUnknown;` carries no marker for it to key on — the
signal's identity lives in the record's contents, which the shape normalizer
strips.

So the fixture is what the concrete-type upgrade unlocks, and it is the
concrete argument for doing it. Until then this surface's Node-diffed
verification is the tier, whose every expectation was diffed against Node 22
before being written down.

The upgrade also has a shape now: `aborted` is an `Rc<Cell<bool>>`, `reason` an
`Rc<RefCell<Option<SmeltUnknown>>>` (a reason is any JavaScript value, so that
one field stays a real boundary), the listener list is the same
insertion-ordered list `SmeltEventEmitter` already has, and the follow list is
a `Vec` of dependents — which is exactly the round-9 sketch, now with all four
of its consumers written and tested against the record.

One thing the upgrade must keep: the seven cases in `abort_signal_runtime`
exercise the ERASED member path on purpose (`signal?.addEventListener('abort',
..)` through an optional chain), because that spelling is how every
`AbortSignal`-aware helper reaches a signal. They pass today and have to keep
passing.
