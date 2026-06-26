# Cluster B (Promise) — what's actually blocking it

Short problem statement. Full design + attempt log: `specs/remeda-cluster-b-promise.md`.

## The 4 failing tests
- `isStrictEqual_test::test_built_ins_promises_1273`
- `funnel_reference_batch_test::{test_showcase_results_as_array, _as_object, error_handling}`

## The core problem (one sentence)
A JS `Promise` lowers to `Type::Future` → Rust `Pin<Box<dyn Future>>`, which **is not
`Clone` and cannot be stored in `SmeltUnknown`**, so the moment a promise crosses an
erased boundary (stored as `unknown`, compared with `===`, awaited later) we lose it.

## Why the two obvious fixes each fail
1. **Erase to `Null`** (today's behavior): all promises become `Null`, so `===` is wrong
   and there's nothing to await.
2. **Erase to a `__smelt_promise` marker object** (tried, reverted, net-0): gives identity
   for `instanceof`/`===` but the marker carries neither the future nor the resolved value,
   so funnel's *store-a-deferred-promise-then-await-it* flow produces garbage. Futures
   aren't `Clone`, so a marker is the only shape that fit — and it's inert.

The real fix needs ONE representation that is simultaneously **awaitable**, **identity-
bearing**, and **`Clone`**: a `SmeltUnknown::Promise(SmeltPromise{ id, Rc<RefCell<Option<
Result<SmeltUnknown,_>>>> })`. The `Rc<RefCell<…>>` shared cell is settable by `resolve`/
`reject` and pollable by `await`; the `id` gives `===`; `Rc` makes it `Clone`.

## Why the implemented version was reverted (2026-06-26)
Phase 1+2 was fully built. The codegen crate and the e2e golden compiled, but the
**generated remeda crate's test harness had 11 compile errors** — and a worktree subagent
cannot compile the generated crate, so they were invisible until integration. Two parts:

- **(solved) Exhaustive-match churn is bigger than the prelude.** Adding a 10th enum variant
  breaks ~15 INLINE `match SmeltUnknown {…}` coercion templates the emitter writes into user
  code (to-number, to-i64, truthiness, to-string, property-key, `primitive_none`). Each needs
  a `Promise` arm (treat like Object: NaN / 0 / true / "[object Promise]" / None). This sweep
  was completed correctly.

- **(unsolved — the actual blockers) 3 codegen-emission bugs:**
  1. **`?` applied to a `Future`** — `funnel_reference_batch_test.rs:262/268` (E0277/E0282).
     The `from_future`/flatten emission applies `?` to a `Pin<Box<dyn Future>>` instead of its
     awaited `Result`, and the closure return type can't be inferred.
  2. **Move out of an `Fn` closure** — `funnel_reference_batch_test.rs:387/393` (E0507).
     `smelt_callback` (captured in an `Fn`) is consumed by-value by the await/flatten path;
     must borrow or clone.
  3. **Bare-move-on-erasure double-moves `data`** — `isShallowEqual_test.rs:456/463`,
     `isStrictEqual_test.rs:340` (E0382). The new erasure arm renders the operand as a bare
     move (because `type_contains_noncloneable(Type::Future)` is true), so a value erased
     twice moves it twice. This breaks NON-target tests from compiling, taking the whole
     crate down.

## What the next attempt must do differently
Drive it with **regen + `cargo test --no-run` on the generated remeda crate after every
step** (not just `cargo check` on the codegen crate, and not via a worktree agent). Land the
inline-match sweep first as a behavior-neutral commit, then fix the 3 emission bugs one at a
time, each verified by recompiling the generated test harness, before touching funnel runtime
behavior.
