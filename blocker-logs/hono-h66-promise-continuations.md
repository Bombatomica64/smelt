# H66 — a promise's continuations lost their value

Round 27, item 3. Found in round 26 while reproducing the radash regression;
**two of its three layers fixed**, with the third measured and numbered.

## What was measured

```ts
async function work(kind: string): Promise<string> {
  if (kind === 'bad') { throw new Error('failed:' + kind) }
  return 'ok:' + kind
}
```

| source | JavaScript | before |
| --- | --- | --- |
| `await work('bad').catch(e => e instanceof Error ? 'typed:' + e.message : 'typed:other')` | `typed:failed:bad` | **empty** |
| `await work('kept').catch(() => 'not reached')` | `ok:kept` | `ok:kept` |
| `await work('mapped').then(v => v + '/then')` | `ok:mapped/then` | correct |
| the same three through an ERASED receiver (`(erase(work(..)) as any).then/.catch`) | as above | **empty / nothing** |

## Layer 1 — the typed `catch` discarded its handler's value

`AsyncOp::Catch` emitted

```rust
Err(smelt_error) => { let _ = {invocation}; Ok::<_, _>({default_value}) }
```

so it called the handler and answered the output type's DEFAULT. `catch`
recovers: the promise it answers settles with the handler's value, exactly as
`then`'s does. The arm now uses the same settle/flatten pair the `Then` arm
uses (a handler returning a promise is flattened, so `catch` never answers
`Promise<Promise<T>>`), and converts the handler's value to the output type.

It also handed the handler `SmeltUnknown::String(smelt_error.to_string())` —
the rejection's message TEXT. A `catch` binding sees the value that was thrown,
so the handler now receives `smelt_thrown_value(&*smelt_error)`: `error
instanceof Error`, `error.name` and a non-`Error` rejection reason all answer as
they do in the source.

## Layer 2 — an erased promise had no continuation members

The typed spelling lowers to an `AsyncOp` only when the receiver's type is
`Type::Future(_)`. A promise that arrives as `SmeltUnknown` — the result of
calling an erased callable, which is what radash's `TFunction extends () => any`
parameter is — read its member through `smelt_get_unknown_field`, whose match
had no `Promise` arm at all: `p.catch` answered `undefined`, the call answered a
default, and the chain's value was lost with no diagnostic.

`smelt_get_unknown_field` now answers a modeled callable for `then`, `catch`
and `finally` on the promise carrier (`smelt_promise_member`), built on the same
pieces the typed path uses: `SmeltPromise::from_future` for the derived promise
(lazy, as every other promise here is), `smelt_thrown_value` for the rejection
value, and `smelt_await_flatten` for a handler that returns a promise. This is
the dynamic boundary's own answer, so every dynamic spelling gets it —
`p['catch'](f)`, `const c = p.catch; c(f)`, a promise flowing through an erased
record.

Adding it grew the shared prelude by 25 lines in the 42 example crates that
already emit the erased carrier, and **no golden's stdout moved**.

## Layer 3 — still open: a throw through an awaited erased call escapes

The radash-shaped two-module reproduction (`guard` imported, an async arrow
passed through it) now runs its continuations, and stops on something else: the
rethrow inside `_guard` leaves through the awaited erased call and is not caught
by the source's own `try`/`catch` — the program exits 101 with no output where
JavaScript prints `user1|default-user|unknown error`.

That is the same FAMILY as round 26's optional-chain gap (a throwing callee
reached through a path the throwing propagation does not see), now on the
`await` of an erased call. Numbered **H70**, with this reproduction:
`guard.ts` holding radash's `guard` verbatim and a `main.ts` with
`makeFetchUser`/`fetchUser`/`run` as `async.test.ts` writes them.

Radash's own suite stays at 84/0 throughout, because its `guard` test asserts
the values this fix restores rather than the rethrow path.

## Guarded by

`examples/typescript/end-to-end/84_promise_continuations`, verified to fail with
the emitter change reverted. It covers both spellings of `then` and `catch`, a
rejection and a pass-through for each, and asserts the thrown value reaches the
handler (`error instanceof Error`).
