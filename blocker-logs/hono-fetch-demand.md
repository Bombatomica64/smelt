# Hono's demand on the standards stream

Owner of everything below: `blocker-logs/standards-tier-plan.md`. The Hono
stream does not model any of it (campaign plan §6); this file records what the
pinned checkout actually uses, with counts, so the demand is evidence rather
than a guess.

Source: `honojs/hono@eebdf7be39abf0a872671835ccce0c4f03ea497a`, `src/`,
289 `.ts` files (188 non-test, 101 test). Counts are `grep` over the checkout.
Two columns where they differ: **src** = non-test files only, **all** = with the
`*.test.ts` files, which is where the volume is (967 `app.request(` calls and
388 `new Request(`).

Method-call counts are receiver-name based, so a name that Hono also uses for
its own wrapper is marked. Where that mattered the count was re-derived from an
unambiguous spelling (e.g. native `Request` members via `HonoRequest#raw`).

---

## 1. Blocking a source-lowering blocker today

These five are the *only* reason `smelt check` on the in-scope set is not at
zero blockers; each is a fetch type Smelt does not resolve at all.

| # | needed | where it blocks | occurrences |
| --- | --- | --- | ---: |
| 1 | **`Response`** as a class | `src/http-exception.ts` (`new Response(...)`), `src/context.ts` | 2 |
| 2 | **`Headers`** as a class | `src/context.ts` | 1 |
| 3 | **`BodyInit`** as a type (the union `ReadableStream \| Blob \| BufferSource \| FormData \| URLSearchParams \| string`) | `src/request.ts` ×2 — `JSON.stringify(body)` where `body: BodyInit`; currently reported as `JSON.stringify() value must be JSON-serializable (got Class 'BodyInit')` | 2 |
| 4 | **`TextEncoder`** as a class | `src/utils/cookie.ts` ×2 | 2 |
| 5 | **`crypto`** as a namespace value | `src/utils/cookie.ts` ×2 | 2 |
| 6 | **`Request.url` typed `string`** | `src/utils/url.ts:108` — `request.url.indexOf(':')` is rejected as `string search methods require string receiver and argument` because `request.url` is not typed | 1 |
| 7 | **`FormData`** as a class | `src/client/client.ts` — excluded with `src/client/**` (see §5), so not blocking | 1 |

`BodyInit`, `RequestInit`, `ResponseInit` and `HeadersInit` are *type aliases*,
not classes; Smelt currently turns each into an opaque `Type::Class` with the
alias's own name, which is why the `JSON.stringify` diagnostic names a class
called `BodyInit`. Resolving them to their real unions is what item 3 needs.

## 2. `Response`

Constructed 28× in src, 138× overall, mentioned in 61 files — the most demanded
type in the corpus.

| member | src | all | note |
| --- | ---: | ---: | --- |
| `.headers` | 40 | 161 | |
| `.status` | 5 | 882 | test assertions (`res.status`) |
| `.text()` | 3 | 420 | |
| `.json()` | — | 311 | |
| `.body` | 4 | 24 | |
| `.ok` | 1 | 21 | |
| `.statusText` | — | 8 | |
| `.arrayBuffer()` | 3 | 7 | |
| `.clone()` | 2 | 3 | |
| `.formData()` | — | 1 | |
| `Response.json` (static) | — | 3 | |
| `Response.error` (static) | — | 1 | |

Constructor forms used: `new Response()`, `new Response(body)`,
`new Response(body, { status, statusText, headers })`. `status` appears as an
init key 167×, `headers` 348×, `statusText` 11×.

## 3. `Request`

Constructed 14× in src, **388×** overall (60 files); `app.request(...)` — which
builds one internally — is called **967×**. This is the type the test suite runs
on.

Native members, counted through the unambiguous `HonoRequest#raw` spelling
(`req.raw.<member>`), because Hono's own `HonoRequest` shares the receiver name
`req`:

| member | count |
| --- | ---: |
| `.headers` | 21 |
| `.body` | 6 |
| `.text()` | 4 |
| `.signal` | 3 |
| `.bodyUsed` | 3 |
| `.method` | 2 |
| `.clone()` | 2 |
| `.redirect` | 2 |
| `.referrerPolicy` | 2 |
| `.mode` | 2 |
| `.credentials` | 2 |
| `.cache` | 2 |
| `.url` | 1 (plus `request.url` in `utils/url.ts`, item 6 above) |
| `.json()` | 1 |
| `.referrer` | 1 |
| `.keepalive` | 1 |
| `.integrity` | 1 |
| `.cf` | 1 | *(Cloudflare extension; in an adapter file, excluded)* |

`RequestInit` keys Hono passes: `method` (71), `headers` (68), `body` (26),
`signal`. `src/request.ts` also declares a `RequiredRequestInit` covering the
full init surface (`cache`, `credentials`, `integrity`, `keepalive`, `mode`,
`redirect`, `referrer`, `referrerPolicy`), used by `cloneRawRequest`.

## 4. The rest

| type | constructed (src / all) | members used |
| --- | --- | --- |
| **`Headers`** | 17 / 35 | `.get` 679, `.set` 148, `.has` 37, `.append` 32, `.delete` 16, `.getSetCookie` 8, `.forEach` 7, `.entries` 3, `.keys` 1 — all counts over any `headers.` receiver |
| **`URL`** | 17 / 46 | `.pathname` 15, `.searchParams` 14, `.href` 7, `.search` 2, `.host` 1 |
| **`URLSearchParams`** | 3 / 22 | `.append` 26, `.get` 13, `.toString` 11, `.set` 2, `.keys` 2, `.getAll` 2, `.forEach` 1, `.delete` 1 |
| **`TextEncoder`** | 11 / 44 | `.encode` only (36× as `new TextEncoder().encode(…)`, 5× via a bound `encoder`) |
| **`TextDecoder`** | 3 / 7 | `.decode` only |
| **`FormData`** | 4 / 50 | `.append` 26, `.get` 4, `.forEach` 4 |
| **`Blob`** | 0 / 8 | constructed only, in tests |
| **`File`** | 0 / 7 | constructed only, in tests |
| **`ReadableStream`** | 2 / 20 | `.getReader` 1, `.pipeTo` 1 |
| **`AbortController`** | 0 / 3 | `.signal`, `.abort` |
| **`AbortSignal`** | — | `.addEventListener('abort', …)`, `.aborted` |
| **`crypto`** | namespace | `crypto.subtle.importKey` 19, `.generateKey` 13, `.exportKey` 11, `.digest` 4, `.verify` 2, `.sign` 2; `crypto.randomUUID` 3, `crypto.getRandomValues` 1, `crypto.webcrypto` 2 |
| **`CryptoKey`** | type only | `.type` 2, `.extractable` 1 |
| **`BufferSource`** / **`ArrayBufferView`** | types only | 1 file each (`utils/cookie.ts`, `utils/buffer.ts`) |

`crypto.subtle` is concentrated in `src/utils/jwt/**` and
`src/middleware/{jwt,jwk}`, which the campaign plan excludes this round; the
`utils/cookie.ts` uses (`importKey`, `sign`, `verify`) are the ones in scope.

## 5. Not demanded

* **`node:http`** — not used anywhere in `src/`. The only `node:` import in the
  corpus is `node:crypto` in `src/adapter/lambda-edge/handler.ts`, an adapter
  that is out of scope.
* **`WritableStream`** (`.getWriter` 2) and **`TransformStream`**
  (constructed 17×) are used by `src/helper/streaming/**` and
  `src/utils/stream.ts`. They are NOT on the standards stream's list; the Hono
  stream will exclude those files with that reason rather than model them.
* **`WebSocket`**, **`MessageEvent`**, **`CloseEvent`** — `src/helper/websocket`,
  likewise excluded.
* **`Proxy`** — `src/client/**` builds its RPC client on `new Proxy(fn, { get })`
  for dynamic member dispatch, a Smelt non-goal; that directory is excluded, and
  with it the `FormData` use in item 7, the `rest parameter type must resolve to
  an array type` blocker (`ConstructorParameters<typeof WebSocket>`), and
  `unresolved identifier proxyCallback`.

## 6. What the Hono stream will do with each

Nothing, except keep lowering against whatever the current (marker/erased)
behaviour is and re-probe. When a member above lands as a real typed surface,
the corresponding entry in §1 stops blocking and any new mismatch appears at
`cargo check` of the generated crate — which is where the next round of this
file's counts will come from.

---

## Round 2: streaming and WebSocket are now excluded, and are future standards work

Item 5 of the round-2 decisions. Now that `[sources] exclude` prunes the
dependency closure (`hono-scope.md`), these surfaces are genuinely out of the
crate rather than dragged in transitively, each with its reason in
`.github/compat/hono/Smelt.toml`:

| excluded | needs | Hono usage |
| --- | --- | --- |
| `src/helper/streaming/**` | `ReadableStream`, `WritableStream`, `TransformStream` | `stream.ts`, `sse.ts` — SSE and streaming responses |
| `src/helper/websocket/**` | WHATWG `WebSocket`, and `WebSocketPair` on the Cloudflare adapter | the `upgradeWebSocket` helper |

Neither is on the standards stream's list for this round, and neither was on it
before: they surfaced from probing rather than from the plan, which is why they
are recorded here rather than assumed. **These are demand, not blockers** — the
excludes make the count honest, and the surfaces come back into scope the day
the standards stream models them.

`src/adapter/**` deliberately stays IN scope even though the adapters reference
`WebSocketPair` and `Deno`/`Bun` globals: host globals lower as erased value
closures, so those files transpile today. Only the two helpers above actually
block, which is the distinction the exclude list is meant to record.

---

## Round 5: `ResponseInit` as a modeled type — one demand entry, same root cause as the init blockers

Recorded per the round-5 instruction ("if it is a fetch-type receiver whose
property the standards stream has not modeled, record it in the demand file
instead"). This is that case, and the diagnosis is that it is **not a new gap**.

### The site

`src/context.ts:651`, in `#newResponse`:

```ts
const status = typeof arg === 'number' ? arg : (arg?.status ?? this.#status)
```

with `arg?: StatusCode | ResponseOrInit` and, from `context.ts:277`:

```ts
type ResponseOrInit<T extends StatusCode = StatusCode> = ResponseInit<T> | Response
```

so `arg` is `StatusCode | ResponseInit | Response`. The blocker is

```
field access is only lowered for Record<string, T>, class, and interface values
for now (receiver: Float, field: status)
```

### Why it is demand and not a narrowing bug

I checked the narrowing machinery rather than assuming. It is all present and it
works: `inverse_guard_narrowing` -> `typeof_inverse_guard` ->
`typeof_excluded_type` / `remove_typeof_member` removes the members matching a
`typeof` kind from a union in the else branch. The equivalent shape with a plain
object type in the union does **not** produce this blocker:

```ts
type Init = { status?: number };
function pick(arg?: number | Init): number {
  return typeof arg === 'number' ? arg : (arg?.status ?? 7);
}
```

(That fixture surfaces a *different* and unrelated emitter issue — `type table
does not contain literal operand type Unknown` at
`emitter/call_runtime.rs:2115` — which is worth its own look but is not this
blocker.)

`ResponseOp::Status` is modeled too, so `.status` on a concrete `Response`
receiver lowers today.

What is missing is **`ResponseInit` as a type**. The standards stream models
`Response` *construction* from an object literal and deliberately blocks a
non-literal init — that is the existing
`Response init must be an object literal so its keys keep their types`
blocker. Because `ResponseInit` is not a modeled type, the three-member union
degenerates to the `StatusCode` half (`Float`), the `typeof` narrowing has no
non-numeric member left to narrow to, and the `.status` read lands on a `Float`.

### Ask

`ResponseInit<T>` (and `RequestInit`) as modeled types with typed keys —
`status?: number`, `statusText?: string`, `headers?: HeadersInit`. That is the
same thing the two "init must be an object literal" blockers need, so this
entry should fall out of the standards agent's round 4 rather than needing
separate work. **Worth confirming after that lands**: if the union survives, this
blocker goes with it and needs nothing from either stream.

Counts, for scale: `.status` reads on a `ResponseOrInit`-typed value appear at
this one site; `ResponseInit` appears in 5 signatures in `context.ts`.


---

## Update (Hono round 17): the full-crate build now stops on this same gap

The whole-crate emit stop is this entry, at a site that is now pinned exactly
rather than inferred. With `Mir::file_paths` landed, the blocker names itself:

```
EmitError: "`new Headers(init)` initializer type is not modeled: SmeltUnknown
  (initializer `smelt_get_unknown_field(&closure_arg_1.clone()
   .unwrap_or(SmeltUnknown::Undefined), \"headers\").clone()`
   in `<unnamed>` at third_party/hono/src/context.ts:8797..8909)"
```

Bytes 8797..8909 are `context.ts:287-291`:

```ts
const createResponseInstance = (
  body?: BodyInit | null | undefined,
  init?: globalThis.ResponseInit
): Response => new Response(body, init)
```

`init` is explicitly annotated `globalThis.ResponseInit`, so nothing about
inference or contextual typing is involved — the parameter erases because the
TYPE has no model, which is exactly the ask above. `new Response(body, init)`
then reads `.headers` off the erased value.

This raises the entry's priority from "one `.status` read" to **the single thing
blocking the whole-crate build**: phase 2's gate cannot be measured at all until
`ResponseInit` has typed keys. Two Hono-stream rounds were spent looking for a
lowering cause before the site was pinned, and there is none to find — the Hono
stream has nothing to implement here.

Re-probe after `ResponseInit` lands; the next stop is either the next standards
family or the Hono stream's, and this log gets whichever it is.

## Found by standards (round 18): an inline object literal against a const-bound arrow's parameter is not hinted

Handed to the Hono stream because it is general lowering, not fetch types: it
costs erasure at every struct-typed argument written inline against an arrow,
whatever the struct is.

An object literal passed straight to a parameter whose callee is a **const-bound
arrow with a required parameter** does not receive the parameter's type hint. It
is built as a `SmeltRecord<String, SmeltUnknown>` — erasing every value on the
way in — and then converted to the struct:

```ts
interface Opts { status?: number; label?: string }

// (1) plain function: hinted. Builds `Opts { status: Some(8.0), .. }`.
function showFn(opts: Opts): string { return `${opts.status}`; }
showFn({ status: 8 });

// (2) const-bound arrow, OPTIONAL parameter: hinted. Also builds the struct.
const showOptional = (opts?: Opts): string => `${opts?.status}`;
showOptional({ status: 7 });

// (3) const-bound arrow, REQUIRED parameter: NOT hinted.
const showRequired = (opts: Opts): string => `${opts.status}`;
showRequired({ status: 8 });
// let _t5: SmeltRecord<String, SmeltUnknown> = SmeltRecord::from([("status", SmeltUnknown::Number(8.0)), ..]);
// (_t4)({ let smelt_record_map = _t5.clone(); Opts { status: smelt_record_map.get("status")…cloned().map(…) } })
```

(1) and (2) are what (3) should look like. The closure-call path does ask for a
hint — `function.params.get(index)` in `stdlib/call_dispatch.rs`, feeding
`argument_with_hint` — so the hint is either not present in the callee's
resolved function type for that shape, or it is dropped before the object
literal is lowered; (2) working makes "no hint at all for closure calls" the
wrong explanation.

Found while writing `59_ambient_response_init`, whose forwarding helper is
exactly shape (3). The fixture binds its init to an annotated const at that one
call site to keep the examples corpus at zero avoidable erasure, with a comment
pointing here — so a fix can un-bind it and the golden will show the
improvement.

## Found by standards (round 27): a `T | null` return prints `undefined`

`console.log(headers.get("x"))` with no such header prints `undefined` where
Node prints `null`. The finding is WIDER than `Headers.get`, which is why it is
recorded here rather than fixed as a fetch-types detail. Measured against
Node 22:

| source | Node | Smelt |
| --- | --- | --- |
| `(): string \| null => null` | `null` | `undefined` |
| `(): string \| undefined => undefined` | `undefined` | `undefined` |
| `new Headers().get("x")` | `null` | `undefined` |
| `new Map().get("a")` | `undefined` | `undefined` (already right) |

So EVERY `T | null` annotation prints the wrong absent word, and `Headers.get`
is one instance of it. `Map.get` is right by luck: its absent value really is
`undefined`.

### Why it cannot be fixed at the declaration

The machinery is already there — `AbsentSpelling::null_text()` answers `"null"`,
and every site that still holds a runtime tag uses it
(`String(null)` is `"null"`). What is missing is the distinction reaching those
sites at all: in `ty::annotations`, the union arm lowers `TSNullKeyword` and
`TSUndefinedKeyword` to the SAME `Type::None`, and a union of one non-nullish
arm plus a nullish one collapses to `Type::Optional(inner)`. From that point on
`string | null` and `string | undefined` are the same interned type, so no
consumer can tell them apart — the information is gone before MIR, not lost in
the printer.

Typing `Headers.get` as something else does not fix it either, and would make
things worse: `Union([String, None])` is a two-arm CONCRETE union, so it emits a
generated tagged enum per nullable return instead of an `Option<String>`. That
is a real loss of concreteness against the north star, and it would move
es-toolkit and remeda output wholesale, to buy one printed word.

### The two designs, and their measured cost

1. **A nullish spelling on the optional** — `Type::Optional { inner, absent:
   AbsentSpelling-like }`, or a sibling `Type::Nullable(inner)`. The Rust
   representation stays `Option<T>`, which is what a hand-writing team would
   also choose; only the printed word and the erased tag (`SmeltUnknown::Null`
   vs `Undefined`) differ. This is the right answer.

   **Cost: 469 non-test sites pattern `Type::Optional`.** Adding a field or a
   variant makes every one of them a compile error, across
   `smelt-hir`/`smelt-mir`/`smelt-frontend-ts`/`smelt-frontend-py`/
   `smelt-codegen-rust`. That is a deliberate type-system refactor with its own
   corpus measurement, not a rider on a feature round — the same judgement
   `CLAUDE.md`'s "Refactoring timing" section asks for.

2. **Keep the collapse and thread the spelling beside the type** — a side table
   keyed by the declaring item, the way D1 proposes for callback fallibility.
   Cheaper to land, and wrong for the same reason it is wrong there when the
   value flows: an optional that crosses a function boundary, a field, or a
   collection loses its key, and the printer sees a bare `Type::Optional`
   again. It would fix the direct `console.log(headers.get(..))` and nothing
   reached through one hop.

**Recommendation:** design 1, as its own round. Until then every fixture that
would print an absent nullable compares against `null` instead, with the reason
at the line — see `78_request_input_forms` and `77_body_init_buffer_source`.
