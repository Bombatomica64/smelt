# es-toolkit async/promise cluster — seven unmodeled values

Branch `claude/estk-async-retry2`, merged with `origin/main` @ `ec6c222`
(PR #231 landed mid-investigation; the branch was re-based on it and every
number below re-measured against it).

**es-toolkit: 972 passed / 87 failed → 988 passed / 71 failed.** 16 tests fixed,
zero new failures — verified by a failure-*name* diff against a baseline this
investigation measured itself, not by counts. Identical across 10 consecutive
runs (the virtual clock makes the suite deterministic).

My fixes and PR #231's five are disjoint: no test appears in both sets.

Every defect below is the same shape, the one this repo has now hit at a dozen
independent sites: **an unmodeled member silently becomes a value.** Not one of
them produced a diagnostic. Every one of them emitted Rust that type-checked,
that `compile_corpus` accepted, and that ran — producing a plausible wrong
answer, or quietly doing nothing at all.

## The seven root causes

### 1. An absent optional member became a callable that answers `false`

`crates/smelt-codegen-rust/src/emitter/optional_access.rs`

An optional-chain field read whose *static field type* is erased — an `unknown`
receiver, a union, an erased class, i.e. anything `field_access_type` answers
`Type::Unknown` for — was emitted as `Option::map`. The `Option` therefore
modeled only the **receiver's** nullishness, never the property's. A missing
property still had to be coerced to the destination type, and coercing "absent"
to a *callback* destination synthesizes a default closure — which returns
`false`.

So this, in `src/function/retry.ts`:

```ts
shouldRetry = _options?.shouldRetry ?? DEFAULT_SHOULD_RETRY;
```

bound a never-retry stub whenever the options object omitted `shouldRetry`, and
the `??` never got a chance to fire. `retry(func, { delay, retries: 2 })` threw
on the first failure. The read now propagates `None` for an absent property,
which is the answer JavaScript gives.

Note the discriminator: the *concrete-struct* spelling
(`function pick(options?: Options)`) was always correct — it emitted
`and_then(|v| v.should_retry.clone())`. Only a union or erased base took the
broken path. Both spellings are pinned by the new runtime tier.

### 2. `Promise.reject(reason)` compiled to nothing

`crates/smelt-stdlib/src/recognition.rs`, `crates/smelt-frontend-ts/src/lowering/guards.rs`

`Promise.reject` was simply **missing from the static-call recognition table**.
It never reached `promise_static_call` (so it never even hit that function's own
"not lowered yet" error), and fell through to the host-namespace path instead:
`smelt_builtin_namespace("Promise")`, a dynamic `.reject` field read that
answers `undefined`, and a callable coercion that substitutes a default closure
returning `null`.

The rejection did not exist. This whole function returned `"no-throw"`:

```ts
try { await Promise.reject(new Error("boom")); return "no-throw"; }
catch (error: any) { return "caught:" + error.message; }
```

`allKeyed`'s "should reject if any promise rejects" was asserting on a promise
that could not reject. Now lowered to `AsyncOp::Reject`, whose reason enters the
ordinary `throw` channel — `throw` and `Promise.reject` are the same operation
in JavaScript, so both render their payload through one shared helper.

### 3. A rejection reason was reduced to a string

`crates/smelt-codegen-rust/src/lib.rs` (`SmeltPromise`)

`SmeltPromise`'s settled state stored its rejection in a `String`. Every reason
that was not exactly its own `message` was destroyed: `Promise.reject({ status:
400 })` settled as `"[object Object]"` and was re-inflated on await as a
synthetic `{ __smelt_error, message }` record with `status` gone. The slot now
carries the payload `smelt_throw`/`smelt_thrown_value` already define — the same
ABI, kept whole.

This is why `retry`'s `shouldRetry: err => err.status >= 500` saw `undefined >=
500`.

### 4. `Promise.resolve(v)` dropped `v`

`crates/smelt-frontend-ts/src/lowering/guards.rs`

It lowered to `AsyncOp::Sleep`, which keeps only the **type** of `v`. The
`Future<()>` → `Future<T>` coercion then invented a `T`. Every primitive default
is a plausible value, which is exactly why this was invisible for so long:

| source | settled as |
| --- | --- |
| `Promise.resolve(1)` | `0` |
| `Promise.resolve("hello")` | `""` |
| `Promise.resolve(true)` | `false` |

A new `AsyncOp::Resolve` carries the operand; its duration operand preserves the
microtask deferral `Sleep` was standing in for.

### 5. A `Promise.all` element expression was dropped

`crates/smelt-frontend-ts/src/lowering/expr/operators.rs`

Same bare `Sleep`, worse consequence. Any combinator-array element that was not
*statically* a `Future<_>` was replaced by a sleep of its type, so the element
expression — **and every side effect in it** — never ran:

```ts
await Promise.all([limitedCallback(), limitedCallback(), limitedCallback()]);
```

called `limitedCallback` zero times. All three `limitAsync` tests were observing
a function that was never invoked.

Keeping those calls alive reached a second gap: an indirect call whose callee is
erased has no static function type and was rejected outright with "indirect call
target is not a function" — even though `Rvalue::ClosureCall` already routes
exactly that callee through the run-time callable dispatch. Both call forms now
classify a callee the same way.

### 6. Adapted async callbacks lost their concurrency; a primed prefix owned the clock

`crates/smelt-codegen-rust/src/emitter/core.rs`, `crates/smelt-codegen-rust/src/lib.rs`

A callback adapter that re-types an inner async call's output emitted the inner
call **inside** a lazy `SmeltFuture::from_future` body. Calling a JavaScript
async function starts it — which Smelt models with `from_future_primed`'s eager
prefix poll — but only if the call happens. Deferring it to the adapter's own
first poll made a batch of adapted callbacks start one at a time as the
combinator awaited them: `Promise.all(array.map(cb))` in `mapAsync`,
`filterAsync`, `flatMapAsync` and `forEachAsync` observed a maximum concurrency
of **1** instead of 10. The call is now hoisted out of the future body.

Hoisting it exposed the other half, and this one is worth remembering:
**`smelt_sleep_ms` advances virtual time to its own deadline as soon as it is
driven.** A primed prefix containing `delay(1000)` therefore jumped the clock a
full second at call time, before any later deadline was armed, and
`withTimeout(() => delay(1000), 50)` could never time out. (This showed up as a
2-test regression in `withTimeout` in the intermediate state — caught by the
name diff, which counts alone would have hidden behind a net +12.) In JavaScript
a prefix schedules its timers without making time pass, so the sleep now
suspends immediately under `SMELT_PRIME_DEPTH` and defers all timekeeping to its
first real poll. This is the rule `SMELT_RACE_DEPTH` already states for a
`Promise.race` driver, applied to the other out-of-band poller.

### 7. `SmeltFuture` reduced a rejection to a string too

`crates/smelt-codegen-rust/src/lib.rs` (`SmeltFutureState::Rejected`)

The same defect as 3, in the other future type, and it survived the first fix
because the two types are separate. A synchronous prefix that throws is captured
by `from_future_primed`'s eager poll as `Ready(Err(..))`, and reducing that to
`error.to_string()` destroyed the payload:

```ts
const [error] = await attemptAsync(async () => { throw 'string error'; });
```

handed `error` a synthetic `{ __smelt_error, message }` record where JavaScript
hands it the string. The lazy path never had the problem — it propagates
`future.await?` unchanged — which is exactly why only a *pre-await* throw
misbehaved.

## Triage records: what was right and what was wrong

- **`allKeyed`: the recorded note was RIGHT.** It claimed every failing case
  builds its object with `Promise.resolve`/`Promise.reject` while the passing
  cases use `new Promise(...)` or plain values, and flagged itself NOT YET
  VERIFIED. Verified: the split is exact, and it is the correct discriminator —
  `new Promise((resolve, reject) => ...)` settles through `smelt_throw` and
  keeps its value, while `Promise.resolve` dropped its argument and
  `Promise.reject` did not exist. Four for four.
- **The `Promise.all` "sequential await" theory was HALF right**, and the record
  saying it is false needs qualifying. `Promise.all` really does await its
  futures in a sequential `for` loop. That is harmless *when the futures are
  already primed* — which is why the `allKeyed` concurrency test passed and the
  theory looked dead. It is not harmless when an adapter has un-primed them,
  which is root cause 6. The loop is still there; nothing on this branch changed
  it.

## Not fixed

- `retry` "should not retry when shouldRetry returns false" still fails.
  `await expect(...).rejects.toEqual(error)` lowers to a literal `()`:
  `vitest_async_expect_call` implements `rejects` only for `toThrow` /
  `toThrowErrorMatchingInlineSnapshot`, and for every other matcher it evaluates
  the arguments and returns a `Promise<void>` placeholder. **The assertion is
  silently deleted, and so is the call it was asserting on** — `retry` is never
  invoked, so `expect(func).toHaveBeenCalledTimes(1)` sees 0.

  This is the same defect class again, at the test-harness layer, and it means
  the suite's pass count is *overstated*: any `.resolves.X` / `.rejects.X` that
  is not `toThrow` is a no-op that always "passes". In es-toolkit that is 8 call
  sites (5 `.resolves.toBeUndefined`, 2 `.resolves.toBe`, 1 `.rejects.toEqual`).

  The general fix is to await the actual and delegate to the ordinary
  `expect(value).M(args)` lowering — no per-matcher special case — reusing the
  try/catch HIR construction `vitest_rejects_to_throw_call` already builds. Left
  undone here because it is a self-contained feature rather than a bug fix, and
  because turning 8 no-ops into real assertions may *raise* the failure count,
  which deserves to land on its own.
