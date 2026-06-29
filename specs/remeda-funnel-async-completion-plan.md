# Remeda `funnel` async showcase — completion plan

Status: **3 of the original 8 remeda generated-test failures remain**, all in
`__smelt_module_funnel_reference_batch_test`:

- `test_showcase_results_as_object`
- `test_showcase_results_as_array`
- `test_showcase_error_handling`

The other five (`isPromise`, `isShallowEqual` promises, `sortBy` data-last
multi-rule, `mapWithFeedback`, `constant`) are fixed and merged to `main`. This
document is the plan for the remaining async cluster.

## UPDATE (2026-06-29): corrected root cause — it is NOT scheduler delivery

Instrumenting the generated funnel test (`test_showcase_results_as_array`) showed
the three burst timers are **armed** (`[set_timeout] due_ms=0` ×3) but
**never fire** — `drain_due_timers`/`drain_promise_tasks` are never reached, and
`smelt_await` (the `SmeltPromise` poll-loop, `main.rs` ~1514) is emitted but
called **zero** times. So the cooperative scheduler is fine; it is simply never
driven.

The real blocker: **`await` never flattens an erased `SmeltUnknown::Promise`.**
The funnel's `call: async (...) => new Promise(...)` lowers the inner promise to
`SmeltUnknown::Promise(SmeltPromise::from_future(…))` (the cluster-b
representation DID land). But `Promise.all` lowers to
`for f in prepared { values.push(f.await?); }` — it awaits each *outer* `api.call`
future, gets the **unresolved** `SmeltUnknown::Promise`, and pushes it as-is. The
inner promise's polling future (spawned via `from_future`) is never awaited, so
`sleep_ms` never runs, the timers never fire, and `Promise.all` yields promise
objects instead of values. `short_result` ends up being a `SmeltUnknown::Promise`,
so `.toBe(5)` fails.

This is the same shape the Step-1 reproductions hit as a compile error (an
`async` fn that **returns** a `Promise` needs the returned promise awaited /
flattened). It compiles in the funnel only because the inner future is erased to
`SmeltUnknown`, so the type checks while the value is lost.

**Corrected fix direction (supersedes Step 3 below):** route `await` through a
flatten step — after `fut.await?`, if the value is `SmeltUnknown::Promise(p)`,
`p.smelt_await().await?` to drive it to its resolved value (recursively). Apply
at every await site: `AsyncOp::Await`, each `AsyncOp::All` element, and the
`async`-fn-returns-`Promise` return path. This wires the already-present
`smelt_await` into the await path; the scheduler work in Step 3 below is likely
unnecessary once flattening drives `sleep_ms`. Keep the same regression gate
(full remeda suite + smelt unit tests after every change).

Sub-problem 1 (the `setTimeout` Promise-executor value drop) is **fixed and
committed** separately (`fix(async): thread resolved value through setTimeout
Promise executors`); it was not on the funnel path, as predicted.

## Why these three are the hard tail

They are **not one bug** — they sit at the intersection of several async-runtime
mechanisms that all have to work together for this one pattern. The pattern (from
`packages/remeda/src/funnel.reference-batch.test.ts`, the `batch` reference
implementation):

```ts
call: async (...params) =>
  new Promise<Result>((...promiseCallbacks) => {
    batchFunnel.call({ promiseCallbacks, params });   // stores [resolve, reject], arms a timer
  });
// funnel timer callback (fires at end of burst, maxBurstDurationMs = 0):
(requests) => {
  callback(requests.map(r => r.params))               // async batch executor (mockApi)
    .then((response) => { for (…requests…) resolve(extractor(response, …)); })
    .catch((error)  => { for (…requests…) reject(error); });
}
// test:
const [a, b, c] = await Promise.all([api.call("short"), api.call("medium"), api.call("…")]);
```

Important baseline fact: **the core `funnel` works** — `funnel_test`,
`funnel_lodash_*`, and `funnel_remeda_debounce_test` all pass. So timers, the
funnel's captured mutable state (burst timer id, reducer accumulator), and
`triggerAt:"end"` scheduling are fine. The gap is the **async result-delivery
layer** layered on top: a `new Promise` executor that stashes `resolve`/`reject`,
which are invoked later from a timer-fired `.then().catch()` chain, with the
results harvested by `Promise.all`.

## How the runtime currently models this

Cooperative single-threaded scheduler emitted in
`crates/smelt-codegen-rust/src/lib.rs`:

- `SMELT_TIMERS` + `SMELT_TIMER_NOW_MS` — virtual timer heap and clock.
- `SMELT_PROMISE_TASKS` — queue of spawned fire-and-forget futures.
- `smelt_set_timeout` (lib.rs ~1056) — pushes a timer due at `now + delay`.
- `smelt_drain_due_timers` (lib.rs ~1080) — fires all timers due at `now`.
- `smelt_drain_promise_tasks` (lib.rs ~1029) — polls queued tasks, **bounded to
  64 iterations**, re-queueing pendings each round.
- `smelt_sleep_ms` (lib.rs ~1103) — `drain_promise_tasks().await`, then loops:
  advance clock to next due timer, `drain_due_timers()`, `drain_promise_tasks().await`.

Relevant codegen:

- `AsyncOp::Promise` (emitter/call.rs ~124) — builds a `smelt_promise_result`
  `RefCell`, synchronous `smelt_resolve`/`smelt_reject` closures that fill it,
  runs the executor synchronously, and returns a future that **polls the RefCell
  in a loop, calling `smelt_sleep_ms(0.0)` each iteration** until it is filled.
- `AsyncOp::Then` / `AsyncOp::Catch` (emitter/call.rs ~150/165) — wrap the
  receiver future in `Box::pin(async { v = fut.await?; (cb)(v); … })`.
- `AsyncOp::SpawnLocal` (emitter/call.rs ~182) — a `Future`-typed expression
  statement is spawned via `smelt_spawn_promise_task` (see builder_part04.rs
  ~451, which wraps a `Future`-typed expr-statement in `SpawnLocal`). This is how
  the fire-and-forget `callback().then().catch()` chain gets queued.
- `Promise.all` (emitter/call.rs ~22, `AsyncOp::All`) over a homogeneous list
  awaits each future **sequentially** in a `for` loop.

Expected happy path: `Promise.all` awaits call-promise #1 → its poll loop hits
`sleep_ms(0)` → clock advances, funnel timer fires → funnel callback runs the
batch executor and spawns the `.then().catch()` chain → `drain_promise_tasks`
drives that chain → `resolve(result)` fills each call-promise's RefCell → the
poll loops observe the filled RefCells and complete.

The three tests prove this chain does **not** deliver values (results come back
`undefined`/`None`; the error test's rejection never propagates so
`rejects.toThrow` fails).

## Confirmed sub-problems (from probing)

1. **Single-`setTimeout` Promise-executor drops the resolved value.**
   `promise_constructor_expression` (builder_part09.rs ~1118) special-cases an
   executor whose body is exactly one `setTimeout(fn, ms)` call
   (`promise_executor_timer_call`, ~1221) and lowers the **whole** Promise to
   `AsyncOp::Sleep(ms)` — discarding `resolve(value)` and returning the output
   type's default. Verified: `new Promise<number>(r => setTimeout(() => r(42), 0))`
   generated `smelt_sleep_ms(0); Ok(0.0)`.
   - This is a real latent bug but is **not** on the funnel path (the funnel
     executor body is `batchFunnel.call(...)`, not a `setTimeout`), so fixing it
     alone will not turn these three green. Fix it anyway for correctness, with
     its own regression test.

2. **Mutable locals captured across nested closures fail to lower.** A faithful
   closure-based funnel probe (closure-captured `pending`/`scheduled`, a
   `new Promise` executor pushing `resolve`, a `setTimeout` resolving later) did
   not compile: `smelt_capture_pending` / `smelt_capture_scheduled` unresolved,
   and `smelt_set_timeout`/`smelt_sleep_ms` were not emitted for that shape. The
   real funnel avoids this because its mutable state lives inside the library
   `funnel()` (which already works), but any fix work that reduces the funnel to
   a smaller repro must account for this.

3. **Scheduler delivery ordering / depth (primary suspect).** Even when the
   structure compiles, the nested chain `Promise.all → call-promise.poll →
   sleep_ms(0) → timer → spawn(then→await mockApi→catch) → resolve(RefCell)` must
   converge within `drain_promise_tasks`’ 64-iteration bound and in the right
   order. The failing results strongly indicate the spawned `.then().catch()`
   continuation is not driven to completion (or its `resolve` write is not
   observed) before the awaiting `Promise.all` loop gives up / proceeds.

## Plan of attack (incremental, regression-gated)

Work in this order; after **every** step, regenerate and run the full remeda
suite plus `cargo test -p smelt-mir -p smelt-codegen-rust -p smelt-frontend-ts`,
and confirm zero regressions before moving on. Use
`smelt rust-test-report --focus funnel_reference_batch_test --baseline-report …`
to track the three targets specifically.

1. **Build faithful, minimal reproductions** as `*.test.ts` fixtures (run via
   `rust-test-report`, the proven harness — program-`kind` `main` wrappers
   miscompile for `Promise`-returning entry points):
   - (a) `new Promise(r => { stash(r); })` where a separately-armed
     `setTimeout` later calls every stashed `r(v)`, harvested by a single
     `await`. Confirms the executor→timer→resolve→await delivery in isolation.
   - (b) the same, but the timer callback runs an `async` function and resolves
     inside its `.then(...)`. Confirms the spawned-continuation path.
   - (c) `Promise.all` over two of (b). Confirms multi-await harvesting.
   Each reproduction that fails localizes one layer without funnel/`batch` noise.

2. **Fix the `setTimeout` Promise-executor value drop (sub-problem 1).** Either
   stop treating a value-resolving `setTimeout` executor as a bare `Sleep`, or
   route it through `AsyncOp::Promise` (which already threads `resolve`). Add a
   compiler regression test (`new Promise(r => setTimeout(() => r(X), ms))`
   resolves to `X`). Keep the no-argument `setTimeout(r, ms)` delay-shim fast
   path if it is relied upon elsewhere.

3. **Make the spawned-continuation path deliver (sub-problem 3).** Likely the
   core fix. Investigate, in `smelt_sleep_ms` / `smelt_drain_promise_tasks`
   (lib.rs ~1029/1103):
   - that a task spawned *during* `drain_due_timers` is itself drained in the
     same `sleep_ms` invocation (the post-timer `drain_promise_tasks().await` at
     lib.rs ~1124 should cover this — verify it actually re-polls newly queued
     tasks rather than only the snapshot taken before timers fired);
   - that nested awaits inside a spawned task (the `await mockApi(...)` inside
     `.then`) make progress under the no-op waker without starving;
   - whether the 64-iteration bound is hit (instrument; raise or make the loop
     condition “drain until both the timer heap and task queue are empty or no
     progress is made”, not a fixed count).
   Prefer making the drain loop fixpoint-based (run until a full pass produces no
   state change) over bumping a magic constant.

4. **Verify `.then`/`.catch` callback application and `promiseCallbacks`
   destructuring.** The funnel callback destructures the stored
   `promiseCallbacks: [resolve, reject]` out of each request object and calls
   them. Confirm the stored function values survive erasure (they cross
   `SmeltUnknown`) and that `AsyncOp::Then`/`Catch` invoke the (non-trivial,
   loop-bodied) callbacks rather than assuming the callback is a bare `resolve`.
   The `mapWithFeedback` fix (callback member-store via closure-body fallback)
   and the object-identity fix (`promote_erased_mutated_records`) are likely
   prerequisites that now hold; re-check the generated funnel callback body for
   any silently-dropped statements.

5. **Error propagation for `test_showcase_error_handling`.** Once delivery works,
   confirm `reject(error)` fills the RefCell with `Err(...)` and that a
   `Promise.all` whose member rejects surfaces the error (the sequential
   `for … await?` loop in `AsyncOp::All` should short-circuit on the first
   `Err`). Verify the thrown `Error` message text round-trips
   (`Batch too big! [["hello"],["world"]]`).

6. **Full validation + merge.** Regenerate the probe, confirm
   `1789 passed; 0 failed`, run smelt unit tests, `cargo check` (clippy is
   unavailable in the sandbox toolchain), then commit per-layer and merge.

## Risk

This touches `smelt_sleep_ms` / `smelt_drain_promise_tasks` and the
`AsyncOp::{Promise,Then,Catch,All,SpawnLocal}` emitters — the runtime used by
**all** generated async code. Every generated async test is a potential
regression. The incremental, full-suite-gated approach above is mandatory; do not
batch these changes.

## Environment note (sandbox)

The remeda submodule clone and the `ruff` git build-dependency are blocked by the
egress policy. To build/run locally: fetch sources via `codeload.github.com`
(allowed), place the remeda source under `third_party/remeda`, copy
`.github/compat/remeda/Smelt.toml` into `target/compat-repos/remeda`, vendor
`ruff` at the pinned rev outside the workspace, and add a **local-only**
`[patch."https://github.com/astral-sh/ruff"]` to `Cargo.toml` pointing at it.
This patch must never be committed (revert `Cargo.toml`/`Cargo.lock` before any
commit).
