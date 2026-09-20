// An `async` callee's contextual return type is the promise part of its context.
//
// A call's contextual type describes the call's RESULT, and the immediately
// invoked callee gets it on its return channel. An `async` function always
// returns a promise, so only the promise part of that context can describe what
// it returns: `Response | Promise<Response>` says `Promise<Response>` to an
// async callee, and a context with no promise part says nothing at all.
//
// Smelt handed the whole union through, so `return (async () => ...)()` inside
// a function declared `T | Promise<T>` made the arrow
// `async () => Promise<T | Promise<T>>` — a promise of the union where the
// union itself was expected. Hono's `hono-base.ts` HEAD branch is exactly that:
// `return (async () => new Response(null, await this.#dispatch(..)))()` inside
// `#dispatch(): Response | Promise<Response>`.

class Reply {
  readonly status: number;

  constructor(status: number) {
    this.status = status;
  }
}

function direct(): Reply | Promise<Reply> {
  return new Reply(200);
}

// The callee has no return annotation, so the contextual type is all it has to
// go on — and the union's promise arm is the only part of it an async callee
// can produce.
function head(isHead: boolean): Reply | Promise<Reply> {
  if (isHead) {
    return (async () => new Reply(204))();
  }
  return direct();
}

// A context with no promise arm leaves the callee's own inference alone.
function plain(): Promise<Reply> {
  return (async () => new Reply(201))();
}

console.log((await head(true)).status);
console.log((await head(false)).status);
console.log((await plain()).status);
