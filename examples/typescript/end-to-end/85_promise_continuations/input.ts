// A promise's continuations, through both spellings that reach them.
//
// `catch` RECOVERS: the promise it answers settles with the handler's value.
// The typed path called the handler and threw its value away, answering the
// output type's default, so `await p.catch(() => 'fallback')` evaluated to the
// empty string. It also handed the handler the rejection's message TEXT rather
// than the thrown value, so `error instanceof Error` was false.
//
// A promise that arrives ERASED — the result of calling a value whose type the
// crate lost, which is what a generic `TFunction extends () => any` parameter
// is — read its members through the dynamic field path, which answered
// `undefined`: the continuation silently vanished and the chain's value became
// a default (H66, radash's `guard`).
async function work(kind: string): Promise<string> {
  if (kind === 'bad') {
    throw new Error('failed:' + kind);
  }
  return 'ok:' + kind;
}

function erase(value: unknown): unknown {
  return value;
}

async function run(): Promise<string> {
  // Typed `catch` over a rejection: the handler's value is the answer, and the
  // handler sees the thrown value itself.
  const recovered = await work('bad').catch((error: unknown) =>
    error instanceof Error ? 'typed:' + error.message : 'typed:other'
  );

  // Typed `catch` over a resolved promise: the handler does not run.
  const untouched = await work('kept').catch(() => 'not reached');

  // Typed `then`, for contrast: it already answered its handler's value.
  const mapped = await work('mapped').then((value: string) => value + '/then');

  // The same three through an ERASED receiver.
  const erasedThen = await (erase(work('good')) as any).then(
    (value: unknown) => String(value) + '/erased'
  );
  const erasedCaught = await (erase(work('bad')) as any).catch((error: unknown) =>
    error instanceof Error ? 'erased:' + error.message : 'erased:other'
  );
  const erasedKept = await (erase(work('spared')) as any).catch(() => 'not reached');

  return [recovered, untouched, mapped, erasedThen, String(erasedCaught), String(erasedKept)].join(
    ' | '
  );
}

console.log(await run());
