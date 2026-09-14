// A promise's continuations, through the TYPED spelling.
//
// `catch` RECOVERS: the promise it answers settles with the handler's value.
// The typed path called the handler and threw its value away, answering the
// output type's default, so `await p.catch(() => 'fallback')` evaluated to the
// empty string.
//
// The rest of the same family is asserted in the executing tier rather than
// here, because both halves of it are erased BY CONSTRUCTION and the examples
// corpus holds a zero-avoidable-erasure invariant:
//
// * a `catch` handler that takes a PARAMETER receives the thrown value, and a
//   thrown value is a real dynamic boundary — JavaScript rejects with any
//   value, so the reason reaches the handler through the tagged runtime value
//   whatever the handler's own annotation says. The rule (the handler sees the
//   thrown value itself, not the rejection's message TEXT, so
//   `error instanceof Error` holds) is a VALUE assertion, which is what the
//   tier is for: `a_typed_catch_handler_receives_the_thrown_value`.
// * a promise that arrives ERASED — the result of calling a value whose type
//   the crate lost, which is what a generic `TFunction extends () => any`
//   parameter is — read its members through the dynamic field path, which
//   answered `undefined`: the continuation silently vanished and the chain's
//   value became a default (H66, radash's `guard`). See
//   `continuations_on_an_erased_promise_run_and_answer_their_handler`.
//
// Both live in `crates/smelt-codegen-rust/tests/promise_value_fidelity_runtime.rs`.
// Fixtures 66 and 74 were split the same way.
async function work(kind: string): Promise<string> {
  if (kind === 'bad') {
    throw new Error('failed:' + kind);
  }
  return 'ok:' + kind;
}

async function run(): Promise<string> {
  // Typed `catch` over a rejection: the handler's value is the answer.
  const recovered = await work('bad').catch(() => 'typed:recovered');

  // Typed `catch` over a resolved promise: the handler does not run.
  const untouched = await work('kept').catch(() => 'not reached');

  // Typed `then`, for contrast: it already answered its handler's value.
  const mapped = await work('mapped').then((value: string) => value + '/then');

  // A typed `then` chained onto a typed `catch`, so the recovery's value is
  // what the next continuation receives.
  const chained = await work('bad')
    .catch(() => 'recovered')
    .then((value: string) => value + '/chained');

  return [recovered, untouched, mapped, chained].join(' | ');
}

console.log(await run());
