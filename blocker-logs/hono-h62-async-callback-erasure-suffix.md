# H62 — an erased ASYNC callback's call was `?`-unwrapped

Radash regression, found by the standards agent at the round-25 integration
head and fixed here. **Silent until compile time**: the generated radash crate
did not build.

## The error

```
error[E0277]: the `?` operator can only be applied to values that implement `Try`
  --> src/async_test.rs:2377
   | ... let smelt_future = (smelt_function_value)()?; SmeltUnknown::Promise(...
   the `?` operator cannot be applied to type `SmeltFuture<SmeltUnknown>`
```

## The rule that was wrong

`erase_value_text`'s function arm builds the adapter that makes a typed
callable usable as a `SmeltUnknown::Function`. For a promise-returning callee
it unwrapped the call with `?` whenever the callee `may_throw`:

```rust
let future_call = if function.may_throw { format!("{call_text}?") } else { call_text };
```

That is right for a **sync** function that returns a promise and can throw
before it does — its Rust call really answers `Result<Future, _>`, and erasing
the `Result` would double-wrap the future. It is wrong for an **async**
function: an async function carries its throw INSIDE the future
(`Future<Output = Result<..>>`) and its call yields the future itself, so there
is nothing to unwrap and the `?` lands on a `SmeltFuture`.

The condition is now `function.may_throw && !function.is_async`, which is the
same rule `throwing_call_suffix` already applies to a direct call.

## Why it surfaced in round 25

Radash's `guard` takes `TFunction extends () => any` and
`async.test.ts` passes it an async arrow:

```ts
const makeFetchUser = (id: number) => {
  return async () => { … }
}
const fetchUser = async (id: number) =>
  (await _.guard(makeFetchUser(id), isUserNotFoundErr)) ?? 'default-user'
```

H54 (a block-bodied callback's return type comes from its own returns) made
`makeFetchUser`'s block-bodied arrow future-typed instead of erased, so the
adapter's `Type::Future` arm started firing on a callee that is `may_throw` AND
async. The latent wrong condition then became a compile error. Radash was 84/0
before round 25 and is 84/0 again.

## Regression guard

The witness is the **radash corpus gate itself** (`cargo test` of the generated
crate, asserted at 84/0 in CI), and radash is now part of every gate run this
stream reports.

A single-module `source_for` reproduction does not emit the adapter at all:
the shape needs the generic `guard` in one module and the async arrow in
another, which is what an `examples/` fixture cannot express and what the
codegen unit tests cannot reach. Two attempts are recorded here so they are not
repeated:

| attempt | outcome |
| --- | --- |
| `source_for` with `guard` + `makeFetchUser` + `fetchUser` in one module | no adapter emitted; the assertion passes with the fix reverted, so it does not discriminate |
| a two-module project (`guard.ts` + `main.ts`) built and run | no adapter emitted either, and it exposed a DIFFERENT wrong value (below) |

## Found on the way: `guard` across a module boundary answers the fallback

The two-module attempt (`blocker-logs` has no fixture for it; the sources are
in the round-26 report) prints

```
default-user|default-user|no throw
```

where JavaScript prints `user1|default-user|unknown error`. Radash's own
generated suite passes this same test, so the difference is the module split:
with `guard` imported rather than local, the `isPromise(result) ? result.catch(_guard) : result`
dispatch takes the guarded arm for a resolved value and swallows the rethrow.
Numbered as **H66**, not fixed here — the radash fix had to go out on its own.
