# Round 28, item 0 — the examples zero-avoidable-erasure invariant, restored by construction

Owner: Hono implementer. The examples corpus (`blocker-logs/smelt-unknown-baseline.json`) is a
HARD invariant in `CLAUDE.md`: avoidable erasure stays 0, and CI enforces it with
`--fail-on-regression`. It was red on the integration head at **20 baseline / 71 current**.

## Why it went red

Nothing regressed in the emitter. The golden harness changed: `expected.rs` is now EVERY generated
file rather than `main.rs` alone (`scripts/regen-example-rust.sh`), so the scanner finally sees the
program bodies of fixtures whose lowering splits into modules. Fixtures written specifically to
exercise ERASED paths had been contributing nothing to the metric and now contribute all of it.

Measured split of the 71 at the start of this round:

| fixture | avoidable | owner |
| --- | ---: | --- |
| `85_promise_continuations` | 42 | this stream |
| `76_base64_globals` | 15 | standards stream |
| `75_callback_block_return_type` | 5 | this stream |
| `78_request_input_forms` | 4 | standards stream |
| `82_data_view_shared_buffer` | 3 | standards stream |
| `80_optional_chain_union_and_throw` | 2 | this stream |

## What landed, per fixture

### 75 — a general rule, not a fixture edit

`classified` in that fixture maps a block-bodied callback whose every `return` lives inside a
`switch`, and it typed `List<Unknown>`. The cause is `statement_terminates`
(`crates/smelt-frontend-ts/src/lowering/support.rs`), which callback return inference consults as
"can this body fall off its end" — it answered `false` for EVERY `switch`, so the caller's `Unknown`
fallback was kept and the whole mapped list erased.

The rule now covers a `switch`: it terminates when there is a `default` clause (so some clause is
always entered) and every clause the source can enter reaches a terminating statement. A clause with
an EMPTY consequent falls through to the next one and carries no requirement of its own, which is
why the LAST clause is required to be non-empty and terminating — it is the only clause with nowhere
to fall through to. `break` stays non-terminating, so the ordinary `case x: ...; break;` shape still
answers `false`, as it must.

Requiring every non-empty clause to terminate is stricter than JS semantics (a non-terminating
clause falls into the next, which may itself terminate) and deliberately so: `statement_terminates`
is conservative in one direction only — a `true` must mean control cannot reach the next statement,
while a `false` only keeps a caller's fallback, i.e. the pre-existing behaviour. That asymmetry is
now in the function's own docstring, because two callers depend on it (the switch lowering's "can
control reach the next case" and this inference).

Result: `fn(String, Int, List<String>) -> Unknown` is gone from the fixture's interned type table,
`classified` answers `List<String>`, and the fixture's own `unshift("start")` — which only
type-checks at the concrete element type — is the regression guard. 5 -> 0, no other golden moved.

### 80 and 85 — split, following the precedent of fixtures 66 and 74

Both fixtures' residual erasure is erased BY CONSTRUCTION: the subject of the rule is the dynamic
boundary itself, so there is no spelling of the same case that carries a concrete type. Keeping them
in the corpus spent the zero-avoidable-erasure budget on output whose whole point is erasure. Each
was split: the concrete half stays as the corpus fixture, the erased half moves to an `#[ignore]`d
runtime tier, where the assertion is the VALUE rather than the spelling — which is the stronger
assertion for these families anyway, since every one of them was a silent wrong value.

`80_optional_chain_union_and_throw` keeps the throwing-method-inside-an-optional-chain rule (a
concrete class, a concrete `Record`). Its union rules — a union receiver reaching runtime narrowing
through its `IntoSmeltUnknown` adapter, and the erased element being converted to the read's
declared result type — moved to `union_optional_element_read_narrows_at_runtime` in
`crates/smelt-codegen-rust/tests/union_receiver_runtime.rs`. An element read on a union whose arms
are not all indexable (`number | [string, number][]`) has no concrete Rust element type to carry.

`85_promise_continuations` keeps the typed continuations: `catch` recovering over a rejection,
`catch` not running over a resolved promise, `then` answering its handler's value, and a `then`
chained onto a `catch` so the recovery's value is what the next continuation receives. Two erased
halves moved to `crates/smelt-codegen-rust/tests/promise_value_fidelity_runtime.rs`:

* `a_typed_catch_handler_receives_the_thrown_value` — a `catch` handler that takes a PARAMETER. A
  rejection reason is a genuine dynamic boundary: JavaScript rejects with any value and the
  handler's annotation cannot narrow what was actually thrown, so the reason reaches the handler
  through the tagged value whatever the handler is annotated as (verified: annotating the parameter
  `Error` instead of `unknown` does not change the emitted `Rc<dyn Fn(&SmeltUnknown) -> String>`).
* `continuations_on_an_erased_promise_run_and_answer_their_handler` — H66, radash's `guard`: a
  promise arriving as source `unknown`/`any`, whose `then`/`catch` were read through the dynamic
  field path and answered `undefined`, so the continuation silently vanished.

Both tiers were already in the `.github/workflows/runtime-tiers.yml` matrix (`async` and
`references`/`functions` shards), so no workflow change was needed — the new cases join tiers CI
already runs.

## Numbers

| category | before | after |
| --- | ---: | ---: |
| runtime prelude | 49810 | 49610 |
| legitimate boundary | 877 | 609 |
| **avoidable erasure** | **71** | **13** |

The 13 that remain are the standards stream's three fixtures (`76` 6, `78` 4, `82` 3); this stream's
three are at **0**. `blocker-logs/smelt-unknown-baseline.json` is re-snapshotted at 13 rather than
left at 20, so the ratchet tightens rather than loosens; the standards stream re-snapshots to 0 when
its three land.

## Not done, and why

The remaining 13 are not mine to move. Two of them (`80`'s pair, before the split) exposed a
CLASSIFIER granularity artifact worth recording: inside one runtime-narrowing `match`, the arm
headers `SmeltUnknown::String(value) => {` and `SmeltUnknown::Array(values) => {` classify as
avoidable while the sibling `SmeltUnknown::Object(values) => match smelt_get_object_field(..)` on
the same match classifies as a legitimate narrowing guard, purely because the latter's line carries
a recognised marker and the former's does not. Teaching `classify_line` that a
`SmeltUnknown::Variant(binding) => {` arm header IS a narrowing arm would be a genuine accuracy fix,
not a whitewash — but it is a change to the gate itself, so it wants its own commit and its own
argument rather than riding along with a fixture split.
