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
