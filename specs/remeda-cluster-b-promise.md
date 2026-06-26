# Cluster B — Awaitable + inspectable `SmeltUnknown::Promise`

Design plan (read-only investigation, 2026-06-26). Target tests:
- `isStrictEqual_test::test_built_ins_promises_1273`
- `funnel_reference_batch_test::{test_showcase_error_handling, test_showcase_results_as_array, test_showcase_results_as_object}`

## Decisive finding (revises the original marker plan)
The `__smelt_promise` marker is **already in the tree** and these 4 still fail:
- `coercion.rs:906` `Type::Future(_) => promise_unknown_sentinel_text()` (`:1222-1224`) →
  `SmeltUnknown::Object({"__smelt_promise": true})`.
- `call.rs:919-934` `instanceof Promise`; `coercion.rs:1437-1441` `UnknownKind::Promise` tag_check.
- The marker fixed `isPromise`/`isNot` but not these clusters.

Why insufficient:
1. **`isStrictEqual_1273`**: each promise binding erases to a FRESH `SmeltObject::new` (new id) at
   each erasure point; `isStrictEqual(p, p)` on the same binding erases `data` twice → two ids →
   `false`, but JS wants `true`. **Needs binding-stable identity**, like the array case.
2. **3 funnel showcases**: they `.await` real `Pin<Box<dyn Future>>`. `new Promise(...)` lowering
   (`funnel_reference_batch_test.rs:159`) builds the correct shared state (`Rc<RefCell<Option<Result>>>`
   + resolve/reject + polling future) but line 160 **discards that future and returns the inert
   marker** because the async block erases its `Type::Future` via `promise_unknown_sentinel_text()`.
   So `.call()` awaits a future wrapping the marker → resolved value lost → `toBe(5/6/17)` fails.

**Root cause: erasing `Type::Future` throws away the live future.** Fix = a `SmeltUnknown` variant
carrying the live shared state: awaitable (poll the cell), inspectable (variant), identity-bearing (id).

## Representation: `SmeltUnknown::Promise(SmeltPromise)`
Add a 10th variant to the prelude enum (`lib.rs:675-685`):
```rust
#[derive(Clone)]
pub struct SmeltPromise {
    id: usize,                                                   // stable JS reference identity
    state: Rc<RefCell<Option<Result<SmeltUnknown, SmeltErr>>>>,  // shared, settable, pollable
}
impl SmeltPromise {
    fn from_state(state) -> Self { id: smelt_next_object_id(), state }  // wrap new Promise's cell
    fn with_id(id, state) -> Self                                       // reuse identity (isStrictEqual)
    fn from_future(fut) -> Self                                         // spawn onto SMELT_PROMISE_TASKS, fill cell
    async fn smelt_await(self) -> Result<SmeltUnknown, ...> {          // poll loop
        loop { if let Some(r) = self.state.borrow_mut().take() { return r; }
               tokio::task::yield_now().await; smelt_sleep_ms(0.0).await; }
    }
}
```
Three axes: **Clone** via `Rc`+`Copy` id (the property a bare `Future` lacks — why the prior erasure
was net-zero); **awaitable** by reusing the exact poll loop `new Promise` already emits (`call.rs:144`)
which cooperates with the virtual-time executor (`smelt_drain_promise_tasks` `lib.rs:888-913`,
`smelt_drain_due_timers` `:939-960`); **identity** via `id` / `Rc::ptr_eq`. No new executor, no `block_on`.

## await lowering
- Operand statically `Type::Future` (the funnel awaits): UNCHANGED — keep `{operand}.await?`
  (`call_runtime.rs:1070-1074`). No regression to funnel await paths (what broke last time).
- Operand erased `SmeltUnknown` (e.g. `await someValue`): add
  `match {op} { SmeltUnknown::Promise(p) => p.smelt_await().await?, other => other }`.
  Additive; **no target test needs it** — defer.

## Producers (the seam that must change)
Single root edit: the `Type::Future` erasure arm (`coercion.rs:906`) wraps the live cell instead of the marker.
1. `new Promise` (`AsyncOp::Promise`, `call.rs:127-148`): hand its existing `smelt_promise_result`
   cell to `SmeltPromise::from_state` (don't rebuild+drop a throwaway future). Fixes the funnel `.call()` line.
2. `async fn` / `async move {}` (`core.rs:106-155`): `from_future(fut)` (self-driving task fills cell).
3. `Promise.resolve(x)` (`builder_part09.rs:937-995`, today `AsyncOp::Sleep(0)`): `from_future` w/ pre-settled `Some(Ok(x))`.
4. Funnel batching: with (1), `.call()`'s promise shares the cell that the flush's `resolve(result)`/
   `reject(error)` writes (`funnel_reference_batch_test.rs:73-74, 117-119`).
**No MIR/HIR/frontend structural change** — `Type::Future`, `AsyncOp::Promise`, `ExprKind::Await`,
`Promise.resolve` lowering, `instanceof Promise` all already exist. Change is confined to codegen erasure.

## Does it fix all 4?
- `isStrictEqual_1273`: distinct promises → distinct ids → `!==` ✓; same binding → build the
  `SmeltPromise` ONCE at the binding (both erasures clone same handle) + `promise_local_identity`
  keyed on storage addr (mirror `list_local_identity_key`/`smelt_list_identity` `coercion.rs:911-922`)
  → `===` ✓. Add `(Promise(l),Promise(r)) => l.id==r.id` to `same_js_key`/`js_strict_eq` (`lib.rs:574,595`).
- `results_as_array`/`_as_object`: (1) makes awaited values the real extractor outputs ✓.
- `error_handling`: reject path writes `Some(Err)`; awaited promise surfaces it → `did_throw` + message ✓.

## Seams to change
Prelude `lib.rs`: enum variant + Clone arm (`:687`); `SmeltPromise`; `same_js_key`/`js_strict_eq`
(`:574,595`); `smelt_unknown_structural_eq` (identity-only arm, no recursion, ~`:703`); typeof→"object",
truthiness→truthy, JSON→{}, to_string→"[object Promise]"; `into_smelt_unknown`. Compiler enumerates
every exhaustive `match SmeltUnknown` (same discipline as the `Undefined` rollout).
Emitter: `coercion.rs:906` (erase), `:1437-1441` (tag_check), `call.rs:919-934` (instanceof),
`call.rs:127-148` (AsyncOp::Promise), `call_runtime.rs:1070-1074` (erased await, deferred), new
`promise_local_identity`.

## Phased plan (gate each: rust-test-report --full + cargo test + clippy)
- **Phase 1 (behavior-neutral):** add the variant + all match arms, keep `coercion.rs:906` on the OLD
  marker → zero behavior change (de-risks the enum-match surface, like the `Undefined` re-land).
- **Phase 2 (high value):** switch `coercion.rs:906` to wrap live state; fix `AsyncOp::Promise`; add
  `from_future`/`smelt_await`; variant-based instanceof/tag_check. Flips the 3 funnel showcases.
  `isPromise`/`isNot` stay green (variant check).
- **Phase 3:** binding-stable promise identity + `(Promise,Promise)` eq arms → `isStrictEqual_1273`.
- **Phase 4 (optional):** erased-await arm, `.then`/`.catch`/`Promise.all` if a non-target needs it.

## Risks
- Async is pervasive & passing — Phase 2 must not perturb `funnel_test.rs` (552KB), `funnel_lodash_*`,
  `debounce`/`funnel_remeda_debounce`. Phase 1 behavior-neutral; gate Phase 2 on full report diff.
- Avoids the prior net-zero break: `SmeltPromise` IS Clone (Rc); concrete `Type::Future` await path untouched.
- Liveness: poll loop relies on executor draining tasks/timers (proven by existing `new Promise` loop);
  `smelt_drain_promise_tasks` caps at 64 iters (`lib.rs:892`) — verify self-driving task + awaiter don't starve.
- Exhaustive-match churn (10th variant) — Phase 1 isolates.
- `isDeepEqual`: two distinct promises deep-UNequal (identity arm), never structurally recurse.

## Attempt 2026-06-26 (reverted) — concrete failure modes for the next pass
A full Phase 1+2 implementation was built and reverted. It compiled the codegen crate
and the e2e golden, but the **generated remeda crate's TEST harness** had 11 compile
errors (a worktree agent can't compile the generated crate, so these slipped through —
**the next attempt MUST regen + `cargo test --no-run` the generated crate in the loop**).
Two categories:

1. **Exhaustive-match churn is bigger than the prelude.** The emitter emits ~15 INLINE
   `match SmeltUnknown {…}` coercion templates (NOT just prelude helpers) that enumerate
   every variant: to-number (`place.rs:414`, `types.rs:128/136/142/232`, `strings_io.rs:129`,
   `call_runtime.rs:1042/2724`, `coercion.rs:1511`), to-i64 (`coercion.rs:1514`), truthiness
   (`types.rs:70/255`, `coercion.rs:1508`), to-string (`strings.rs:479/500/668/759`,
   `coercion.rs:1517`, `core.rs:2893` property-key), and the `primitive_none` fragment
   (`call_runtime.rs:2747`). Each needs a `SmeltUnknown::Promise(_)` arm (treat like Object:
   NaN / 0 / true / "[object Promise]" / None). Templates with a trailing `_ =>` (the
   `extract`-to-Vec iterators `coercion.rs:1521/1526/1532`, field-access `call_runtime.rs:2467`)
   are already exhaustive — skip. **This sweep was completed and is correct**; the generated
   crate's library + the 27_optional_chains e2e golden then compiled cleanly. The golden
   refresh: only `27_optional_chains/expected.rs` embeds the enum; rebuild it via a temp
   project (name=example-app, crate-name=example_app, clone-strategy=aggressive) and diff —
   the delta should be ONLY Promise additions.

2. **Three real codegen-emission bugs (the actual blockers — unsolved):**
   - **`?` on a `Future` — `funnel_reference_batch_test.rs:262/268` (E0277/E0282).** The
     erasure `from_future(Box::pin(async move { Ok((<future>).await?…) }))` or the flatten
     emits a `?`/type-inference shape that applies `?` to `Pin<Box<dyn Future>>` directly
     (not to its awaited `Result`), and needs a type annotation. The `<future>` operand
     rendering + the wrapper interact badly when the funnel `.call()` body already produces a
     future.
   - **Move out of an `Fn` closure — `funnel_reference_batch_test.rs:387/393` (E0507).**
     `smelt_callback` (a captured var in an `Fn` closure) is consumed by-value by the
     flatten/`smelt_await` path; must borrow/clone instead.
   - **`use of moved value: data` — `isShallowEqual_test.rs:456/463`, `isStrictEqual_test.rs:340`
     (E0382).** The new erasure arm renders the operand as a **bare move** (because
     `type_contains_noncloneable(Type::Future)` is true), so a value erased twice double-moves.
     This breaks NON-target tests from compiling → takes the whole crate down. The erasure must
     clone (or the value must be a `SmeltPromise`, which IS Clone) at the erasure site rather
     than move the underlying future.

Recommendation: do the inline-template sweep first (Phase 1, mechanical, proven), commit it
behavior-neutral; then fix the 3 emission bugs ONE at a time with regen + `cargo test --no-run`
on the generated remeda crate after each, before attempting the funnel runtime behavior.
