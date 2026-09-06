# Standards tier: WHATWG fetch types and `node:http` on hyper

Owner: standards-tier implementer (Opus). Architecture: Fable. Date: 2026-09-06.
Runs in parallel with `blocker-logs/hono-campaign-plan.md`; the contract between the two is in
section 6.

## 1. Why

`blocker-logs/express-v1-baseline.md` shows the current state: an Express app "transpiles with
0 blockers" into a crate that does nothing, because every host surface it touches is erased at
the import boundary. Smelt's model for host surfaces is the Bun model: `node:*` builtins and
web standards are reimplemented in Rust (as `vitest`, `Blob`, `Intl`, `setTimeout` already
are). This plan builds the two pieces every server needs and that Hono, Express and raw
`node:http` apps all sit on:

- the WHATWG fetch types (`Headers`, `URL`, `URLSearchParams`, `Request`, `Response`,
  `FormData`, `Blob`/`File`, `TextEncoder`/`TextDecoder`, `ReadableStream`,
  `AbortController`/`AbortSignal`) as **concrete Rust runtime types**, not markers and not
  `SmeltUnknown`;
- `node:http` (`createServer`, `IncomingMessage`, `ServerResponse`, `listen`) on **hyper 1.x**
  over the existing Tokio runtime. No axum here: `createServer((req, res) => ..)` is exactly
  hyper's `service_fn`, and axum is reserved for the mapped-framework tier (Express) later.

## 2. Milestone 0: stop the false greens (land first, small)

All three are recorded with evidence in `express-v1-baseline.md`. Acceptance for each is that
`smelt probe` on `examples/typescript/express_crud/Smelt.toml` reports it as a named blocker
instead of emitting a no-op crate.

1. **Unresolved bare package used as a value is a blocker.**
   `module_init.rs::import_declaration` currently inserts an unresolved import into
   `module_globals` as `Type::Unknown`. Replace that fallback: a value import from a module that
   neither resolves to a source file nor to a registered host module produces
   `SmeltError::unsupported("unresolved package `express`: not a source file and not a modeled host module")`
   at first *use* (type-only imports stay free). The probe's `missing-stdlib` category is the
   right bucket. Keep the `@date-fns/tz` rule working but move it behind the same host-module
   registry shape you build in section 3, so it stops being a spelling in `import_declaration`.
2. **NodeNext specifiers.** `./app.js`, `./x.mjs`, `./x.cjs` must resolve to `.ts`/`.mts`/`.cts`
   (and `/index.ts`) in the manifest dependency collector (`smelt-transpiler/src/manifest.rs`,
   `typescript_resolver()`), exactly as `tsc` does under `moduleResolution: NodeNext`. Test: a
   two-file fixture with `.js` specifiers produces the same HIR as extensionless specifiers.
3. **Dropped free functions.** With extensionless specifiers, `createApp`, `openDatabase`,
   `rowToTodo`, `createTodosRouter`, `parseId` and the validators in `express_crud` are called
   but never emitted. Find the suppression (likely a lowering error swallowed during item
   emission, or an unresolved-type function silently skipped), make it a named blocker, and
   add a test that a function whose body fails to lower is reported, never omitted.
4. `process.env.PORT ?? 3000` emits `let _: String = 3000.0;`. The `??` join must type as
   `string | number` (the union `Number(..)` accepts), not the left arm's type. Ordinary bug;
   end-to-end fixture.

## 3. Host-module registry (the general mechanism)

One registry in `smelt-stdlib` (new module `host_modules.rs`, documented) declares, per module
specifier, the exported values and types Smelt models: `node:http`, `node:sqlite` (surface
only; the database stream owns its Rust side later, so declare the shape and leave the
implementation a named blocker), `node:crypto`/`crypto`, `node:events` (EventEmitter, needed
by `IncomingMessage`). Globals (`Request`, `Response`, ...) reuse the same declaration shape
through `globals.rs` so a name has ONE modeled surface whether it is imported or ambient.
Each entry carries: TypeScript-visible member names and their HIR types, the Rust runtime
type it lowers to, and the Cargo dependencies it pulls in (pay-for-use: a crate that never
uses `node:http` gets no hyper in its `Cargo.toml`, like `fancy-regex` today).

This replaces the ad-hoc `host("Request", "__smelt_request")` marker in `host_object.rs`:
`instanceof Request` must keep working through the new concrete type. Keep the marker registry
for the genuinely shapeless objects (`WeakMap`, `DOMException`, boxed primitives).

## 4. Fetch types as concrete Rust

Demand comes from the Hono core (`src/**/*.ts`, non-test), measured on 2026-09-06; build the
surface Hono and Express actually use, in this order, and keep every method a real
implementation:

| type | members used (count) | Rust shape |
| --- | --- | --- |
| `Headers` | `get` 679, `set` 148, `has` 37, `append` 32, `delete` 16, `forEach` 7, `getSetCookie` 7, `entries` 3, `keys` 1, constructor from record/array/Headers | wrapper over `http::HeaderMap` with WHATWG case-folding and comma-joining semantics; `Set-Cookie` kept separate |
| `Request` | `headers` 21, `body` 6, `text()` 4+502 shared, `json()` 353 shared, `signal` 3, `bodyUsed` 3, `method`, `url`, `clone()`, `arrayBuffer()`, `formData()`, `blob()`, `bytes()`, and the RequestInit fields `mode`/`credentials`/`cache`/`redirect`/`referrer`/`referrerPolicy`/`integrity`/`keepalive` (read-only, stored) | struct with `http::Method`, `url::Url`, `Headers`, body as `SmeltBody` (bytes or stream, single-use with `bodyUsed`) |
| `Response` | constructor `(body?, init?)`, `status`, `statusText`, `ok`, `headers`, `body`, `text()/json()/arrayBuffer()/blob()/bytes()/formData()`, `clone()`, statics `Response.json` 3, `Response.error` 1, `Response.redirect` | struct mirroring `Request`; `status` is `u16` validated 200..599 like the spec |
| `URL` / `URLSearchParams` | existing `TsUrlField` rule covers field reads; add `searchParams`, `URLSearchParams` `get/getAll/has/set/append/delete/toString/entries/forEach` | `url::Url`, `form_urlencoded` |
| `TextEncoder` / `TextDecoder` | `encode`, `decode` | thin functions over `String`/`Vec<u8>` |
| `Blob` / `File` | already byte-backed markers; add `text()`, `arrayBuffer()`, `size`, `type`, `slice` | upgrade in place |
| `FormData` | `get/getAll/has/set/append/delete/entries/forEach`, multipart parse in `formData()` | `multer`-free: use the `multer`-less `multipart` crate or hand-parse boundaries with `httparse`-level simplicity; justify the pick |
| `ReadableStream` | constructor with `start(controller)`, `controller.enqueue/close`, `getReader().read()`, async iteration | bounded model over `tokio::sync::mpsc` of `Vec<u8>`; document what is not modeled |
| `AbortController` / `AbortSignal` | `abort()`, `signal.aborted`, `addEventListener('abort')` | `tokio_util::sync::CancellationToken` or a small `Rc<Cell>`; pick the simplest that works in the single-threaded `Rc` runtime |
| `crypto` | `randomUUID` 3, `getRandomValues` 1, `subtle.digest` (SHA-1/256/384/512) | `uuid`, `getrandom`, `sha1`/`sha2`. `subtle.importKey/sign/verify` (JWT/JWK middleware, 58 `crypto.subtle` uses in total) is **out of scope** this stream; leave it a named blocker so Hono's jwt/jwk middleware stays excluded honestly |

`fetch()` today is `AsyncOp::HttpGetText` returning `string`. Upgrade it to return `Response`
(method/headers/body from `RequestInit`) over `hyper` client + `hyper-util`; keep the existing
GET-text tests passing through the new type.

Rules: types are exact (`Headers.get` returns `Option<String>` for `string | null`; body
readers are `Future<...>`), no `SmeltUnknown` in any of these surfaces, and the runtime
prelude for them lives in a NEW focused prelude module (`fetch_types_prelude.rs`), emitted
pay-for-use like the other preludes.

## 5. `node:http` on hyper

Surface: `createServer(handler)`, `server.listen(port[, host][, cb])`, `server.close()`,
`IncomingMessage` (`method`, `url`, `headers` as the Node lower-cased record, `on('data')`,
`on('end')`, async iteration, and a convenience body collector), `ServerResponse`
(`statusCode`, `setHeader`, `getHeader`, `writeHead`, `write`, `end`). Lowering: the handler
becomes a hyper `service_fn` closure; `listen` becomes the awaited server future inside the
already-emitted `#[tokio::main]`; `listen(0)` must work so tests can bind an ephemeral port.
`IncomingMessage` and `ServerResponse` are concrete structs; the request body is the same
`SmeltBody` as in section 4 so `fetch` and `node:http` share one body model.

Dependencies added only when used: `hyper` 1 (`server`, `http1`), `hyper-util`,
`http-body-util`, `http`, `bytes`, `url`. Justify any other crate in a comment at the
`Cargo.toml` emit site.

## 6. Contract with the Hono stream

- This stream OWNS every name in sections 4 and 5. The Hono stream never models them; where
  Hono touches a member not listed above it reports the demand and leaves the blocker.
- Deliver section 4's `Headers`, `Request`, `Response`, `URL` first (they unblock Hono's core
  `context.ts`/`request.ts`), then `TextEncoder`/`FormData`/streams, then section 5.
- Both streams add runtime tiers and touch `runtime-tiers.yml` and `tests/mod.rs`; keep those
  edits additive and expect a trivial merge.

## 7. Acceptance

1. `smelt probe` on `examples/typescript/express_crud/Smelt.toml` reports real blockers
   (`unresolved package express`, `node:sqlite` surface declared but unimplemented) and no
   longer emits a no-op crate. Its module graph shows all 6 files with their functions.
2. New end-to-end fixtures in `examples/typescript/end-to-end/` for `Headers`/`Request`/
   `Response`/`URLSearchParams` construction and body reading (deterministic, no sockets), and
   a new runtime tier `fetch_types_runtime.rs`.
3. A `node:http` echo server fixture (`examples/typescript/node_http_echo/`: `createServer`,
   reads method/url/headers/body, answers JSON) transpiles, compiles, and a runtime tier
   `node_http_runtime.rs` starts it on port 0, round-trips a request through the upgraded
   `fetch`, and asserts the JSON.
4. All gates in the implementer brief green; SmeltUnknown examples invariant at 0; es-toolkit
   ratchet equal or lower.
