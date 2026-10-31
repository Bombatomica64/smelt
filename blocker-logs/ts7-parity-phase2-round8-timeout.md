# TS7 parity, phase 2, round 8: timeout middleware

Hono v5 `0f5ad7b6`: **451 passed / 3 failed** of 454 (round 7: 448). The 3
timeout tests now pass and no test that passed before fails. Example 184
covers the rules; its output matches Node and it holds zero avoidable
`SmeltUnknown`.

## Nested primes do not advance the clock

Calling an async function primes it: its synchronous prefix runs eagerly.
A primed caller that awaits another primed call re-polls it inside the same
prime, so the prime depth can reach 6 in Hono's middleware chain. The
virtual-clock `smelt_sleep_ms` waited out only one yield of the prime, so a
sleep started deep in that chain advanced the clock past a `Promise.race`
deadline armed before it, and the handler won the race (200, not 504).

`smelt_sleep_ms` now yields while any prime is active (`while` instead of
`if`). Round 6 tried this and reverted it because `body.test`'s nesting
limit appeared to hang; that test takes 26 s on the baseline too, so the
hang did not reproduce.

Coverage gap: example 184's six-layer chain does not fail when the `while`
is turned back into an `if`. Hono's 3 timeout tests are the check that
detects that defect.

## `for...of` over `Headers`

An erased `Headers` value iterated with `for...of` panicked ("unknown is not
iterable"): its erased form stores its own `entries` key as an array, so the
host-method dispatch did not apply. A `for...of` whose iterable the checker
types as `Headers` now lowers to the typed `HeadersOp::Entries`.

## `ReadableStream` bodies keep their bytes

`new Response(original.body)` produced an empty body: the body erased
field-wise, and union recovery let the first object arm win.

- `SmeltBody` has boundary adapters: it erases to a host-origin object
  tagged `ReadableStream` and is restored from that origin (empty otherwise).
- `ReadableStream` erases through an adapter (`StdlibClass::erases_through_adapter`).
- Union recovery tries each adapter-erased stdlib class arm by host origin
  before the ordered shape guards.

## Still failing (3, mount)

Each passes a value that violates its declared type:

- `undefined as unknown as Response` guarded by `if (res)` (2 tests).
- A `@ts-ignore` mock `{ waitUntil: 'waitUntil', ... }` as an
  `ExecutionContext` whose fields are methods (1 test).

Supporting them means loosening strict types; awaiting a decision.

## Ratchets

- Examples avoidable `SmeltUnknown`: 0. An earlier draft of example 184
  rejected with an `Error` and kept a `setTimeout` handle, which are erased
  (`reject`'s reason and the timer id, an `UntypedBuiltin` erasure). The
  example now builds its deadline from `sleep(ms).then(...)`.
- Examples typedness: no program less typed; 184 added to the baseline.
- es-toolkit: 1052 passed / 7 failed (unchanged); avoidable 22157 (unchanged);
  no program less typed.
