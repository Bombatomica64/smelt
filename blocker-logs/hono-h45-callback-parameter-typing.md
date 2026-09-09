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

---

## RESOLVED (round 17): the stop is standards demand, not this stream's

Round 17 landed `Mir::file_paths` and `current_function_site` (item 2), and the
blocker immediately named itself:

```
… (initializer `smelt_get_unknown_field(&closure_arg_1…, "headers")`
   in `<unnamed>` at third_party/hono/src/context.ts:8797..8909)
```

Bytes 8797..8909 are `context.ts:287-291`:

```ts
const createResponseInstance = (
  body?: BodyInit | null | undefined,
  init?: globalThis.ResponseInit
): Response => new Response(body, init)
```

`init` is `closure_arg_1`, optional, and `new Response(body, init)` reads
`.headers` off it inside the `Response` lowering.

### Three conclusions, and they close this item

1. **It is not a callback-parameter typing bug.** `init` carries an EXPLICIT
   annotation, `init?: globalThis.ResponseInit`. There is no contextual typing
   involved: the type is written at the parameter, and it is honoured. The
   erasure happens because `ResponseInit` has no model, not because a type
   failed to reach the body.

2. **It is standards-owned.** `ResponseInit` is part of the `Response` family,
   which the campaign plan's §6 contract puts in `standards-tier-plan.md`
   ("You never model `Headers`, `Request`, `Response`, …"). So this stop is
   **recorded fetch demand**, and belongs in `hono-fetch-demand.md` rather than
   being implemented here.

3. **It is not the rest-parameter spread either.** `context.ts:658`'s
   `(...args) => this.#newResponse(...(args as Parameters<NewResponse>))` is a
   genuine, separate bug (documented above, and fixture-reproducible: the rest
   packs to `SmeltList<SmeltUnknown>`, the spread passes the pack as argument 0,
   and the remaining parameters default). It is worth fixing on its own merits —
   the closure's own emitted signature does not even match its declared field
   type, so the field assignment cannot type-check — but it is **not** what the
   full crate is stopping on, and fixing it will not move this stop.

### What H45 is now

Closed as a diagnosis. Nothing in it is this stream's work:

- the `Headers`/`ResponseInit` stop → standards demand, hand off;
- contextual callback-parameter typing → already implemented, no work needed;
- the rest-parameter tuple spread → real, separate, renumber it if it is to be
  scheduled (it needs the callback classifier, see below).

### Sizing the rest-parameter spread, for whoever picks it up

Not a small fix. The wrong type is made in
`arrow_callback_param_types_with_hint`
(`crates/smelt-frontend-ts/src/lowering/callbacks/body_lowering.rs`, around the
`if let Some(rest) = &arrow.params.rest` block): when the contextual function
has no rest at that index, the remaining parameter types are folded into a
`Type::Union` and wrapped in a `Type::List`. TypeScript's answer is a TUPLE —
`Parameters<F>` is a tuple — so `(String, Optional<Init>)` becomes
`List<String | Optional<Init>>`, which then renders `SmeltList<SmeltUnknown>`.

Changing that type alone is not enough. A single tuple-typed parameter still
gives the closure arity 1, so its Rust signature stays
`Fn((String, Option<Init>)) -> String` against a declared
`Fn(String, Option<Init>) -> String`. The fix has to EXPAND the rest into the
contextual arity — N real parameters, with the rest name bound to a tuple local
built from them — and then let the spread distribute positionally. That reaches
into `arrow_callback_from_params`
(`callbacks/classify.rs`) and the `CallbackExpr`/param-index machinery, which is
why it wants its own round rather than a corner of one.

---

## The call-argument half, found in round 17 — and it silently miscompiles

Round 16 sized the REST-PARAMETER half of this rule (above). Round 17 found the
other half while working the slice's remaining rows, and it is worse: a spread
of a tuple-typed value into a call **drops the arguments entirely and calls with
defaults**. That compiles, so nothing catches it.

### The mechanism

`crates/smelt-frontend-ts/src/lowering/guards.rs`, in the call-argument
lowering:

```rust
Argument::SpreadElement(spread) => self.expression(&spread.argument, body),
```

The spread-ness is discarded: `f(...triple)` lowers as if it were `f(triple)`,
one argument where the callee declares three. The arity mismatch is then filled
with defaults.

### Reproduction

```ts
type Route = [string, string, number];
export function replay(routes: Route[], sink: (a: string, b: string, c: number) => void): void {
  for (let i = 0; i < routes.length; i++) {
    sink(...routes[i]);
  }
}
```

emits

```rust
_smelt_tmp_5 = sink(String::new(), String::new(), 0.0);
```

All three arguments are defaults. The tuple is never read. A program built this
way runs and produces wrong answers rather than failing.

### In the slice

hono's `smart-router/router.ts:36` is the same shape —
`#routes?: [string, string, T][]`, then `router.add(...routes[i])` — and there
it surfaces as **1 slice error** (`expected String, found (String, String, T)`,
`main.rs:7219`) rather than silently, because `add` is reached through an
`Rc<dyn Fn(String, String, &T)>` field whose types do not admit the fudge. The
compile error is luck, not detection: the same rule that produced it produced
the silent version in the fixture above.

### The rule

A spread argument whose operand is a TUPLE of known arity distributes its
elements positionally, each at the callee's corresponding parameter type. HIR
already has what this needs — `ExprKind::Index` handles a tuple receiver, and
the emitter's tuple index path renders `.0` / `.1` — so the work is at the
argument-COLLECTION site rather than in `argument()`: one `Argument` currently
maps to exactly one `ExprId`, and a spread has to be able to yield several. The
operand should be lowered once into a local first, so a spread of a call result
is not re-evaluated per element.

Where the operand is a LIST rather than a tuple the arity is not statically
known and this rule does not apply; that case needs the packed-vector call path
and should stay on it.

### Why it is not in this round

It is a correctness bug rather than a blocker, it wants its own commit with a
runtime fixture proving the arguments actually arrive (a compile-only assertion
would have passed on the silent version), and the round it was found in had
already spent its budget. It is also the same rule as the rest-parameter half,
so doing them together is cheaper than doing either alone — the rest-parameter
case needs the closure arity expansion described above, and both need the
argument-collection site to yield several arguments from one `Argument`.
