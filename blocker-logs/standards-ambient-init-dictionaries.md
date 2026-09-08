# An ambient init dictionary is a struct, not an erased record

Round 17 item 0, ahead of everything else because it was the full Hono crate's
stop. The site the Hono agent traced (`src/context.ts:287-291`):

```ts
const createResponseInstance = (
  body?: BodyInit | null | undefined,
  init?: globalThis.ResponseInit
): Response => new Response(body, init)
```

## What round 4 decided, and why it could not hold

`ResponseInit` and `RequestInit` were modeled as opaque classes, on the
reasoning that an ambient interface "has no runtime representation, so its keys
are read through the checked cast". Two consequences, and the second is worse
than the first:

* **A `SmeltUnknown` the constructor refuses.** An OPTIONAL ambient init
  parameter is `Optional<Class(ResponseInit)>`, which the cast path did not
  recognize (it matched a bare `Class`), so `init.headers` fell through to the
  erased member read and `new Headers(init)` reported an unmodeled initializer
  type. That is the message the whole-crate build stopped on.
* **A silent wrong value where it did build.** With a REQUIRED ambient init the
  cast fired — and the key's declared type was `Optional<Headers>`, so a record
  literal (`{ headers: { "x-a": "1" } }`) was cast to `Headers`, recovered from
  a record carrying no `__smelt_headers` marker, and answered an EMPTY header
  list. Measured against Node 22: `headers.get("x-a")` was `"1"` in Node and
  `"none"` in Smelt, with no diagnostic anywhere.

The second is the reason the design had to change rather than be patched: the
cast cannot convert between `HeadersInit`'s arms, because the arm is exactly
what it threw away.

## What it is now

An ambient init dictionary is DECLARED, as an interface with the spec's
optional typed keys, the first time a program references it
(`ambient_init_interface_type` in `lowering/ty/annotations.rs`):

| type | keys |
| --- | --- |
| `ResponseInit` | `status?: number`, `statusText?: string`, `headers?: HeadersInit` |
| `RequestInit` | `method?: string`, `headers?: HeadersInit`, `body?: string`, `signal?: AbortSignal` |

`HeadersInit` stays the UNION WHATWG spells (`[string, string][] |
Record<string, string> | Headers`) rather than being narrowed to the `Headers`
arm, which is what lets all three spellings reach the constructor's per-arm
conversion (round 15).

So the emitted code is a struct:

```rust
struct globalThis_ResponseInit { status: Option<f64>, status_text: Option<String>, headers: Option<SmeltUnion17> }
```

a parameter typed by one is `Option<globalThis_ResponseInit>`, and a key read is
`init.as_ref().and_then(|value| value.status.clone())` — a real field, no
erased record and no cast.

**A source declaration of the same name wins.** Hono declares its own
`interface ResponseInit<T extends StatusCode>`; the bare spelling keeps it. The
`globalThis.`-qualified spelling is the source asking for the ambient one by
name — which is precisely why Hono writes it at this site — so a local
declaration cannot shadow that. The probe is textual, the same one
`source_contains_class` uses for the sibling shadowing question, because a
declaration can appear after the reference that needs to know about it.

The two spellings intern as distinct types (`ResponseInit` and
`globalThis.ResponseInit`), which is what keeps them apart in a crate that has
both, at the cost of a second emitted struct in such a crate.

## Two more stops on the same line, both fixed

* **An optional/erased `BodyInit`.** `body?: BodyInit | null | undefined`
  erases (the union's unmodeled arms are host classes), and the constructor
  refused the whole call at BUILD time — including for the string every real
  caller passes. An `Optional` body now delegates to its inner conversion with
  the absent arm as the empty body (WHATWG's own answer for a missing body),
  and an ERASED body dispatches on the runtime tag: a string is text, a nullish
  value is empty, and an unmodeled arm throws naming itself rather than putting
  wrong bytes in the body. Documented as a dynamic boundary at the emit site —
  the arms are distinguishable only at run time, and the erasure is the
  parameter's own declared type.
* **A modeled member read on a receiver the source narrowed.** Hono's
  `HTTPException.getResponse` does `if (this.res) { .. this.res.headers .. }`
  on a field typed `Response | undefined`. Narrowing tracks locals, so the
  field's declared type arrives at the read, and every modeled-host member read
  was gated on the receiver being exactly that class — the guarded read fell
  through to the erased path. `present_receiver` asserts the receiver present
  (the same `TypeAssert` the `length` path and the union-arm dispatch use) for
  the `Response`/`Request` property reads and method dispatches. A source `?.`
  is never routed there: that spelling asks for `undefined` when the receiver
  is absent, which is the optional-chain lowering's job.

## Where the Hono build stops now, verbatim

```
Error: Custom { kind: InvalidData, error: "/home/user/smelt/third_party/hono/src/http-exception.ts:\n[\n    ManifestDiagnostic {\n        file: \"/home/user/smelt/third_party/hono/src/http-exception.ts\",\n        category: UnsupportedLowering,\n        code: \"smelt::unsupported-ts\",\n        message: \"`Response.body` is a ReadableStream, which is not modeled; use `await response.text()`, or pass the response itself as the body\",\n    },\n]" }
```

`src/http-exception.ts:68` is `new Response(this.res.body, { status, headers })`.
`Response.body` is a `ReadableStream | null`, which Smelt does not model — the
same surface the probe manifest already excludes by name
(`src/helper/streaming/**`).

It used to fall through to the erased field read, whose emitted text read the
generated struct's OWN `body` field: the HIR claimed an erased value while the
Rust was an `Option<SmeltBody>`, so the crate failed `rustc` with an E0308 that
named neither the source line nor the reason. It is a named blocker now, with a
file and a message, which is why the stop moved from the emitter into the check
phase.

**This is a design question for the next round, and it is small.** Smelt already
models a response's body internally as a `SmeltBody` handle, and
`body_conversion_text` already accepts "a `Request` at the body position" by
taking its handle. The same treatment for `Response.body` needs one modeled
class with no members — a body HANDLE whose only capability is being passed
back to a constructor — plus `Optional` for the spec's `null` when there is no
body. That covers `new Response(res.body, ..)`, the whole idiom Hono uses, and
it is the last thing on this line. The alternative considered and rejected:
typing `res.body` as the body's TEXT, which would make `if (res.body)` answer
`false` for an empty-string body where JavaScript answers `true` (a stream
object always exists).

## Adjacent gap found while writing the fixture

An object literal passed straight to a CONST-BOUND ARROW is not reached by the
parameter's type hint:

```ts
const show = (opts: Opts): string => `${opts.status}`;
show({ status: 8 });     // builds a SmeltRecord<String, SmeltUnknown>, then converts
```

while the same literal passed to a plain `function`, or to an arrow with an
OPTIONAL parameter, is built directly as `Opts { status: Some(8.0) }`. The
record intermediate erases every value on the way in, which is why the fixture
binds its init to an annotated const at that one call site. It is a
hint-propagation gap in the closure-call path rather than anything about init
dictionaries, and it is worth its own item: it costs erasure at every
struct-typed argument written inline against an arrow.
