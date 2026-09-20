// `response.body` is the body HANDLE, and handing it back shares it.
//
// The spec's `body` is a `ReadableStream | null`. Smelt models the stream as
// the handle itself — a class with no readable members — and what that answers
// is exactly what the idiom needs: whether there IS a body, and being passed
// back to a constructor, which shares the same payload and the same `bodyUsed`
// cell rather than copying bytes.
//
// The read used to fall through to the erased field path, whose emitted text
// read the generated struct's own `body` field: the type claimed an erased
// value while the Rust was an `Option<SmeltBody>`, so the crate failed to
// compile with an E0308 that named neither the source line nor the reason.
//
// Typing it as the body's TEXT was considered and rejected: `if (res.body)`
// would then answer `false` for an empty-STRING body, where JavaScript answers
// `true` because a stream object exists either way. Presence is what a handle
// can answer honestly, and the last two lines are that distinction.

interface BoomOptions {
  res?: Response;
  message?: string;
}

// The shape Hono's `HTTPException.getResponse` has: re-wrap a carried
// response's body under a new status, keeping its headers.
//
// (A plain class rather than an `Error` subclass, which the original is. An
// error carries `cause?: unknown` — a genuine dynamic boundary the corpus
// classifier counts as avoidable erasure, and this corpus holds zero — so the
// subclass shape is covered by the probe corpora instead. See the note for
// that and for the `super(options?.message)` gap it sits next to.)
class Boom {
  readonly res?: Response;
  readonly status: number;
  readonly message: string;

  constructor(status: number = 500, options?: BoomOptions) {
    this.message = options?.message ?? "";
    this.res = options?.res;
    this.status = status;
  }

  getResponse(): Response {
    if (this.res) {
      const newResponse = new Response(this.res.body, {
        status: this.status,
        headers: this.res.headers,
      });
      return newResponse;
    }
    return new Response(this.message, { status: this.status });
  }
}

async function run(): Promise<void> {
  const withRes = new Boom(418, {
    message: "teapot",
    res: new Response("teapot", { headers: { "x-a": "1" } }),
  });
  const rewrapped = withRes.getResponse();
  console.log(rewrapped.status);
  console.log(rewrapped.headers.get("x-a") ?? "none");
  // The handle was SHARED, so the bytes are the carried response's own.
  console.log(await rewrapped.text());

  const bare = new Boom(404, { message: "missing" });
  const fromMessage = bare.getResponse();
  console.log(fromMessage.status);
  console.log(await fromMessage.text());

  // Presence, not content: a body built from nothing has none, and one built
  // from the empty string has one.
  console.log(new Response(null).body === null);
  console.log(new Response("").body === null);
}

await run();
