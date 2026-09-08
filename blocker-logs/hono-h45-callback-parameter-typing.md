# H45 — the erased closure parameter behind the full crate's `new Headers` stop

Round 16, item 6. Diagnosis note, before building. The headline is that **all
four candidate sites in the brief are disproved**, and that the general rule as
stated is **already implemented** for the plain case — so the fix needs a
narrower trigger than "contextual typing of callback parameters".

## The stop

```
EmitError: "`new Headers(init)` initializer type is not modeled: SmeltUnknown
  (initializer `smelt_get_unknown_field(&closure_arg_1.clone()
   .unwrap_or(SmeltUnknown::Undefined), \"headers\").clone()` in `<unnamed>`)"
```

Three facts are readable straight off it: the receiver is a closure's parameter
at index **1** (numbering is 0-based — confirmed against
`44_node_http_echo/expected.rs`, which emits
`|closure_arg_0: SmeltIncomingMessage, closure_arg_1: SmeltServerResponse|`),
that parameter is **optional** (`unwrap_or(SmeltUnknown::Undefined)`), and
`.headers` is read **directly off it** rather than off a local.

## The four candidates are not the site

Bisected with the manifest's own `exclude` list. Adding
`"src/helper/proxy/**"` leaves the error **byte-identical**; adding
`"src/middleware/method-override/**"` as well leaves it byte-identical again.
So `proxy/index.ts:58`, `:175`, `method-override/index.ts:80` and `:111` are all
excluded as the site.

That is consistent with what those sites actually read: `method-override`'s two
read `clonedRequest.headers` and `c.req.raw.headers`, which are field chains on
**locals**, not direct parameter reads. `proxy`'s `:58` does read a parameter
(`request` in `buildRequestInitFromRequest`), but it is parameter **0**, and
proxy alone stops earlier anyway on a standards-owned diagnostic ("Request init
is an erased value, so its keys cannot be read with their types").

## Where it actually is

`new Headers(<non-empty>)` has only six sites in hono, and with proxy and
method-override excluded only `context.ts` remains:

| site | receiver | |
| --- | --- | --- |
| `context.ts:613` | `this.#res.headers` | a private field, not a closure arg |
| `context.ts:617` | `arg.headers` | **matches** |

`context.ts:608` declares `#newResponse(data, arg?: StatusCode \| ResponseOrInit, headers?)`
— `arg` is parameter index 1 and optional, and `:617` reads `arg.headers`
directly. That is the shape in the message, exactly.

The open question is why that body is emitted with **closure-arg** naming when
`#newResponse` is a private method. The likely reason is the two arrow-valued
class fields that wrap it:

```ts
// context.ts:658
newResponse: NewResponse = (...args) => this.#newResponse(...(args as Parameters<NewResponse>))
// context.ts:~685
… => this.#newResponse(data, arg, headers) as ReturnType<BodyRespond>
```

## The general rule is already there — for the plain case

This is the part that changes the plan. A class field with a declared function
type and an arrow initialiser **does** get contextual parameter typing today:

```ts
type Respond = (data: string, init?: Init) => string;
respond: Respond = (data, init) => data + (init ? init.label : '');
```

emits `|closure_arg_0: String, closure_arg_1: Option<Init>|` and a typed
`closure_arg_1.as_ref().map(|v| v.label.clone())`. So "the callback's parameter
type never reached the closure body" is not true in general, and a fix aimed at
that would be aimed at working code.

## The bug the probing did find, which is probably the root

The **rest-parameter** spelling — which is precisely `context.ts:658` — is
broken, independently of `Headers`:

```ts
respond: Respond = (...args) => this.inner(...(args as Parameters<Respond>));
```

emits

```rust
move |closure_arg_0: SmeltList<SmeltUnknown>| {
    let _smelt_tmp_2: String = this.inner(closure_arg_0.clone(), None::<Init>);
```

Two things are wrong. The rest parameter is packed into a
`SmeltList<SmeltUnknown>`, erasing every argument even though `Parameters<Respond>`
names their types; and the spread call does not spread — it passes the whole
packed **list** as the first parameter and defaults the second to `None`. The
arity is wrong and the types are gone.

That is a far better match for the observed failure than contextual typing: an
erased-and-mis-spread call into `#newResponse` is how an optional parameter at
index 1 ends up carrying `SmeltUnknown` for `arg`, which is then read for
`.headers`.

## Recommendation

Fix the rest-parameter spread first, as its own item: a spread of a rest
parameter into a call with declared parameters must distribute the packed
arguments positionally and recover each at the callee's declared parameter type,
rather than passing the pack as argument 0. It is general (nothing in it is
hono-specific), it is fixture-reproducible — the probe above is the fixture —
and it should be verified on its own terms (arity and types at the call) rather
than through the `Headers` stop.

Then re-probe the full crate. If the stop moves, the `Headers` blocker was
downstream of this all along and no contextual-typing work is needed. If it does
not, the remaining suspect is the union-typed optional parameter
`arg?: StatusCode | ResponseOrInit` failing to stay concrete — a different fix,
and one to confirm against the standards agent's union-member-read work rather
than duplicating it.

## Diagnostic gap worth recording

The emitter's message names the enclosing function, which is `<unnamed>` for
every closure, and MIR carries no spans (`Mir` has functions, classes,
interfaces, closures, globals, types, symbols — no source map). Pinning this
site took a manifest-level bisection. Threading a span into MIR so an
`EmitError` can name a file and line would have turned that into one build, and
it would pay off on every future emitter stop in a corpus this size.
