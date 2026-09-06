# Standards tier: what landed, and the mechanism the rest needs

Date: 2026-09-06. Plan: `blocker-logs/standards-tier-plan.md`. Evidence for
Milestone 0: `blocker-logs/express-v1-baseline.md`.

## 1. State after this pass

| plan item | state |
| --- | --- |
| M0.1 unresolved package used as a value is a blocker | landed, both halves (section 4) |
| M0.2 NodeNext `.js`/`.mjs`/`.cjs` specifiers | landed |
| M0.3 dropped free functions | landed (it was a symptom of M0.2; invariant now has a regression test) |
| M0.4 `??` type join | landed |
| 3 host-module registry | landed (`smelt_stdlib::host_modules`) |
| 4 `Headers` | landed, concrete Rust, runtime tier green |
| 4 `URLSearchParams` | landed, concrete Rust, runtime tier green |
| 4 `Response` + `SmeltBody` | landed, concrete Rust, runtime tier green (section 3) |
| 4 `Request` | landed, concrete Rust, runtime tier green (section 3) |
| 4 `fetch` upgrade to return a `Response` | landed, runtime tier against a real socket (section 3) |
| 4 `TextEncoder`/`TextDecoder`, `FormData`, `ReadableStream`, `AbortController`, `crypto` | not landed |
| 4 `Blob`/`File` upgrade (`text()`, `arrayBuffer()`, `slice`) | not landed |
| 5 `node:http` on hyper | **not landed** — declared as a blocker; the runtime-flavor and body-model questions are now decided (section 5) |

`smelt probe` on `examples/typescript/express_crud` reports **3 blockers** in 3
of 6 files — two `unresolved package \`express\`` (`app.ts`, `todos/routes.ts`)
and one declared `node:sqlite` `DatabaseSync` — where before this stream it
reported 0 blockers and emitted a no-op crate whose only item was `main`. With
the `node:sqlite` blocker removed by hand it lowers all six modules and all
seven free functions, which is what M0.3 was.

## 2. The mechanism, now proven twice

`Headers` and `URLSearchParams` establish the route for a **concrete host
class** — a host type modeled as a real Rust value with typed methods, not as a
marker-bearing `SmeltUnknown` record (`host_object.rs`) and not as an inline
expression rule (`new URL(x).pathname`). Nine sites, in order:

1. `smelt-stdlib/src/classes.rs` — a `StdlibClass` variant and its name.
2. `smelt-stdlib/src/recognition.rs` — a `TypeScriptReceiverKind` variant plus
   one `method(kind, member, rule)` entry per modeled member. Recognition is
   **receiver-typed**: `get`/`set`/`has`/`entries` are also `Map` members and
   ordinary user method names, so keying on the member alone is wrong.
3. `smelt-stdlib/src/rules.rs` — one `RuleId` per member *group* (read /
   mutation / projection), its `backend_dependency`, and its `source_api`.
4. `smelt-hir` — an op enum in `expr/ops.rs` and two `ExprKind`s in
   `expr/kinds.rs` (`XNew { init }`, `XOp { op, receiver, args }`), plus arms in
   `expr/map.rs` and `format/call.rs`. Four sites, all compiler-enforced.
5. `smelt-mir` — the mirroring `Rvalue`s in `types.rs`, lowering in
   `lower/expr.rs`, and arms in `format.rs`, `opt/mod.rs`, `validate/operands.rs`
   (both the read and the mut visitor), and the exhaustive `lower/place.rs` list.
6. `smelt-codegen-rust/src/fetch_types_prelude.rs` — the struct, its inherent
   methods, and the `IntoSmeltUnknown`/`SmeltFromUnknown` boundary adapters
   (gated on `needs_unknown`).
7. `smelt-codegen-rust/src/stdlib.rs` — a `needs_*_runtime(mir)` gate (rvalues
   **plus** the type table, so a value that is only *named* still emits its
   type) and any Cargo dependency the type itself needs.
8. `smelt-codegen-rust/src/emitter/fetch_types.rs` — the construction and
   operation emitters, plus `emitter/types.rs` (`type_text`, `default_value`,
   field-read types), `emitter/core.rs` (`is_erased_class_type` must answer
   `false`), and the field-read sites in `emitter/place.rs` and
   `emitter/call_runtime.rs` when the type has data properties.
9. `smelt-frontend-ts/src/lowering/stdlib/fetch_types.rs` — the constructor
   entry (called from `new_expr.rs`, guarded by `!self.classes.contains(name)`
   so a user class of the same name wins) and the `dispatch_*_method` entry
   registered in the `call_dispatch.rs` handler chain.

Two traps this pass hit, both worth knowing before the next type:

- **The generic `.toString()` handler accepts any class-typed receiver** and
  turns it into a string cast. A modeled type with its own serialization has to
  be declined there (`type_defines_its_own_to_string`), *not* by hoisting the
  modeled dispatch above it: a dispatch probe lowers its receiver, so hoisting
  duplicated unrelated receiver expressions (a `new URL(..).toString()` grew a
  second `UrlField` read).
- **A dependency must be tied to the type, not only to a rule.** The
  dependency collector scans rvalues; a type whose *runtime* needs a crate
  (`SmeltUrlSearchParams` needs `url`) has to be added to that scan or the
  emitted crate references an unlinked crate.

## 3. `SmeltBody`, `Response` and `Request`: landed

`Response` is a concrete generated Rust type, not a tagged record:

```rust
struct SmeltResponse { id: usize, status: f64, status_text: String, headers: SmeltHeaders, body: SmeltBody }
```

The status line and headers are plain fields because the spec makes them
immutable on a response — there is nothing for a shared cell to coordinate. The
**body** is the mutable part, and `SmeltBody` owns that sharing
(`Rc<RefCell<payload>>` beside an `Rc<Cell<bool>>` `bodyUsed`), so the response
does not wrap itself in a second `Rc<RefCell<..>>`.

What that buys, per the north star: `response.status` is an `f64`, `ok` a
`bool`, `statusText` a `String`, `headers` a `SmeltHeaders`, `text()` a
`SmeltFuture<String>`. No caller re-narrows anything, and no `SmeltUnknown`
appears anywhere in the surface — the examples invariant stays at 0 avoidable
erasure with this landing.

### Members, against the Hono demand file

`blocker-logs/hono-fetch-demand.md` §2 ranks the corpus's usage. Landed:
`.headers` (161), `.status` (882), `.text()` (420), `.ok` (21), `.statusText`
(8), `.clone()` (3), `.bodyUsed`, and the three constructor forms
(`new Response()`, `new Response(body)`, `new Response(body, init)`). Not yet:
`.json()` (311), `.arrayBuffer()` (7), `.body` (24), `.formData()` (1), and the
statics `Response.json`/`Response.error` — each a named blocker meanwhile.
`.json()` needs the JSON-parse plumbing and the erased carrier's gate, so it
goes with `arrayBuffer` and the statics rather than doubling this commit.

### Four decisions worth naming

1. **The init literal's keys become their own typed fields**
   (`ResponseNew { body, status, status_text, headers }`), not a record. Each
   key has an exact source type; keeping them as one erased object would mean
   codegen re-deriving `status`'s type from a tagged value at run time. A
   non-literal init (`new Response(b, init)`) is therefore a named blocker:
   honest, and it is not what the demand file shows Hono writing.

2. **`ok` is derived, never stored.** The spec derives it from the status, so
   storing it would let the two drift. No compile step would notice.

3. **`clone()` is not Rust's `Clone`.** The spec's `clone()` gives the copy its
   own unread body (`SmeltBody::tee`, a payload copy with a fresh flag), while
   assigning a response to another variable shares one body and one used flag
   (Rust's `Clone`, the handle copy). Both spellings exist in real code and they
   are observably different; the runtime tier pins both.

4. **A body reader takes a handle clone into its async block.** The first
   emission moved the receiver into `async move`, so `response.bodyUsed` after
   `response.text()` did not compile. A handle clone is also the semantically
   right copy: it shares the payload and the flag, so consuming the body through
   the future is observable on the original, which is what the spec says.

### A shadowing bug this surfaced

`!self.classes.contains(name)` was the guard that lets a *user* class named
`Response`/`Headers`/`URLSearchParams` win over the modeled host class. It is
not enough: while a class's own members are being lowered the class is only
**pending**, so a `this.status` read inside a user `class Response` saw no
registered class and was claimed by the modeled fetch type. Both states answer
"does the source own this name" the same way, so they now sit in one predicate
(`user_class_shadows`) that all three modeled fetch types read. `Headers` and
`URLSearchParams` carried the same latent bug and are fixed by the same change;
only `Response` had a property read to expose it.


### `Request`, and what it shares

`SmeltRequest` is the same shape with the spec's differences: a serialized url
and a method where a response has a status line. It holds the **same**
`SmeltBody`, so single-use reading, `tee()` on `clone()`, and the implied
`Content-Type` all come from one place rather than being written twice.

Landed members, against `blocker-logs/hono-fetch-demand.md` §3: `.headers`
(21), `.text()` (4), `.method` (2), `.url` (1, plus demand item 6), `.clone()`
(2), `.bodyUsed` (3), and `new Request(input, { method, headers, body })` —
which is `method` (71), `headers` (68) and `body` (26) of the init keys Hono
passes. Not yet: `.json()`, `.body`, `.signal` (3), and the `RequestInit` keys
`cache`/`credentials`/`integrity`/`keepalive`/`mode`/`redirect`/`referrer`/
`referrerPolicy` — each a named blocker, because accepting and ignoring one
would change what the program does with no diagnostic.

Two behaviours that only a runtime tier catches, both diffed against Node:

* **`url` is the serialization, not the input.** `new Request('https://a.test')`
  reads back `https://a.test/`. Storing the input verbatim gives a plausible url
  missing its path, so the constructor parses through `url::Url` — which is why
  `Request` declares the `url` backend dependency.
* **`method` is normalized for exactly the spec's list**
  (`DELETE GET HEAD OPTIONS POST PUT`) and left alone otherwise: `post` becomes
  `POST` while **`patch` stays `patch`**. Upper-casing everything is the easy
  wrong answer and Node keeps `patch` lower-case.

Demand item 6 is closed: `request.url` is typed `String`, so
`request.url.indexOf(':')` lowers — it was `string search methods require
string receiver and argument` before, because the read had no type.

### Host identity moved from construction to the boundary

`Request` was a **marker-only** host object: `new Request('http://localhost')`
built `{ __smelt_request: true }` because es-toolkit's `isPlainObject` spec
constructs one only to probe identity. A concrete type cannot also be a marker
record, so the marker moved to `IntoSmeltUnknown` — stamped when the value
crosses into an `unknown` position, which is exactly where `isPlainObject`
reads it. The guarantee is unchanged; the place that carries it moved, and the
two es-toolkit gate tests moved with it (one asserts construction is typed, one
asserts the adapter stamps the marker).

`Response` gained the marker it never had, so `Object.prototype.toString.call`
answers `[object Response]` and `instanceof Response` resolves. `Request` lost
its entry in `smelt_builtin_construct_kind`, which is correct: a dynamic
`new Request(..)` must not build a record when the type is real.

**The es-toolkit ratchet fell by 4** (32912 → 32908 avoidable erasures), because
the `isPlainObject` spec's `new Request(...)` is now a typed value rather than
an erased record. Baseline re-snapshotted in the same commit, as the
`SmeltUnknown` rule requires.

Both types' erasure adapters are documented dynamic boundaries: the receiving
position's type is `unknown`, so no concrete type, union, or generic can stand
in for the record. The body crosses as its **text** rather than as a handle,
because an erased record cannot hold a single-use cell — a body that
round-tripped would otherwise share a used flag with a value that no longer
exists. Erasing peeks rather than consumes, so a response can be logged and
still read.

### Runtime tier

`crates/smelt-codegen-rust/tests/response_runtime.rs` (6 tests) and
`tests/request_runtime.rs` (3 tests), every
expectation diffed against Node 22 line by line — including the thrown
`TypeError`'s exact message. It covers what compiles either way and is only
wrong when it runs: the empty default reason phrase (**not** `"OK"`), `ok`
derived across 200/299/300/404/500/599, single-use bodies and the second-read
throw, tee-vs-share, and a `Headers` reached through `.headers` being the same
list.

**Known gap, recorded in the test module.** The spec requires the init status in
200-599 and Node throws a `RangeError` outside it; Smelt accepts it, because a
constructor is a stdlib *rvalue* and a fallible rvalue has no throwing edge in
MIR to reach an enclosing `try`. That is the same shape as `JSON.parse` and the
URI decoders (`blocker-logs/hono-h10-uri-and-base64-globals.md`), so it is one
known gap rather than a new one.

**Pre-existing gap found, not fixed:** a floating top-level promise is never
driven. `run();` at module scope emits `smelt_spawn_promise_task(..)` and
`main` returns without draining the queue, so an async top-level program prints
nothing. Node runs the microtask queue at exit. Top-level `await` is separately
not lowered (`await expressions are only lowered inside async functions`), which
is why the runtime tier uses generated vitest tests, whose callbacks are `async`.

### `fetch` answers a `Response`

`fetch(url)` lowered to `AsyncOp::HttpGetText` and typed `Promise<string>` —
the fused "GET and give me the body text". That is not what `fetch` returns in
any runtime, and `tsc` rejects the signature the old tests used
(`async function load(): Promise<string> { return await fetch(url); }`).
Collapsing it threw away the status, the reason phrase and the header list, and
no compile step could notice, because the program had no way to ask.

`AsyncOp::HttpFetch` now answers `Future<Response>`, assembled from what the
transport actually reports: the status, its canonical reason phrase, every
response header in order, and the body as **raw bytes**. Bytes rather than
text is deliberate — `SmeltBody::from_text` stamps an implied
`text/plain;charset=UTF-8`, and a fetched response's content type belongs to
the server.

`HttpGetText` stays in the op set. Python's `requests.get(url).text` really is
the fused operation, so it keeps it, and a codegen test now pins that the
Python path builds no `Response`.

`crates/smelt-codegen-rust/tests/fetch_response_runtime.rs` proves the round
trip against a **real HTTP server** — a `TcpListener` on port 0 speaking
enough HTTP/1.1 to answer one request — because the parts being asserted are
exactly the ones that come from the transport. A mocked transport would only be
asserting Smelt's own construction, which the `Response` tier already covers.
The generated crate fetches it and reads `status` 201, `statusText` `Created`,
`ok`, two headers, the body once through `text()`, and a clone that reads
independently.

## 4. M0.1's second half, now on

The blocker fires for a **modeled** host module whose export is declared but
unimplemented (`node:http`, `node:sqlite`, `node:crypto`, `node:events`,
`node:path`) *and* for an **unmodeled** bare package (`express`, `lodash`,
`yup`). `smelt_stdlib::host_modules::unmodeled_package_use_blocks()` is `true`.

`express_crud` is the acceptance case:

| | before the flip | after |
| --- | ---: | ---: |
| files with blockers | 1 | 3 |
| `unresolved package \`express\`` | 0 | 2 (`app.ts`, `todos/routes.ts`) |
| `node:sqlite` `DatabaseSync` declared | 1 | 1 |

The two carve-outs are what make the flip safe, and they are load-bearing
rather than incidental:

- a **relative specifier** never blocks — it names a source file the manifest
  resolver owns, and a module lowered on its own legitimately sees it
  unresolved;
- a **test module** never blocks — `CLAUDE.md`'s test-function exception. The
  radash gate lowers `import { assert } from 'chai'` and still runs 84/84.

### What the flip cost, and what it bought

The 13 `part04_tests.rs` tests that had been the argument against the flip were
not, on reading them, tests *of* erased-library interop. Each one covers a real
lowering rule — array `concat` with mixed erased/concrete arms, `new` from a
destructured namespace member, a curried `_.map(_.prop(..))` factory, aliasing
an imported value as a const, top-level destructuring of a module global — and
used a library import only as a convenient source of an erased value. Rewriting
them to assert the blocker instead would have deleted coverage of eleven rules
to gain one repeated assertion.

So each was re-pointed at the erasure it actually needs, and the blocker got
its own focused tests:

- where the subject is a rule over an **erased value**, the value now comes
  from `declare const x: any` — a source-level dynamic boundary, no import;
- where the subject is a rule over an **erased namespace** whose members
  dispatch as static helpers (`_.join(items, sep)`, `_.forEach`, `_.has`,
  `async.map`), the import became **relative** (`'./lodash-compat'`). That is
  also the honest shape: this is how the compat corpora import their own
  helpers, and a relative specifier is exactly the carve-out above;
- six new tests in `host_module_tests.rs` pin the policy itself: default import
  blocks, named import blocks the same way, type-only import stays free, and
  `node:path` blocks in both spellings.

Two things the flip exposed that were not in the plan:

1. **`node:path` was faking it.** `path.join`/`path.resolve` had a lowering rule
   that returned an **empty string literal**, so `resolve(__dirname,
   '../key.pub')` became `""` and the program went on to open it. That is worse
   than erasure — a wrong value with no diagnostic. The rule is deleted,
   `node:path` is a registry entry with its surface `Declared`, and the test
   that asserted those calls "lower" now asserts they block. Implementing it for
   real is `std::path` plus the semantics to get right (`..` collapsing,
   absolute-segment reset, separators), so it is declared rather than rushed.

2. **A member rule could outrank the blocker.** `path.join('/tmp', 'x.json')`
   was claimed by the static array-join helper form, which reads its *first
   argument* as the receiver, and reported "array join requires an array
   receiver" — a diagnostic true of nothing the source wrote. The receiver's
   import was never lowered, so the named blocker never fired. Fixed with one
   general check at the head of `call_expression`
   (`blocked_import_member_call`): if a member call's receiver chain roots in a
   binding marked unresolved, that import's blocker is the diagnostic. It is one
   place rather than a guard in each rule, and it made four more tests honest.

### Corpus effect

No corpus regressed, because the compat corpora lower their **own** source
through relative specifiers:

| corpus | measure | result |
| --- | --- | --- |
| es-toolkit | files with blockers | 0 (baseline high-water 9) |
| es-toolkit | avoidable erasure | 32912, +0 |
| remeda | runtime tier | 1789 passed / 0 failed |
| remeda | avoidable erasure | 25191, +0 |
| radash | runtime tier | 84 passed / 0 failed (the `chai` carve-out) |
| examples | avoidable erasure | 0, +0 (hard invariant) |

There is no probe fixture for a framework-heavy corpus to show the flip's
intended cost: `third_party/strapi` has no `Smelt.toml`, and `third_party/nest`
is a checkout of Smelt itself rather than NestJS. `express_crud` is the only
framework program with a fixture, and its count is the table above.

## 5. `node:http`: still declared, but the two open questions are now closed

The surface is still a named blocker (`blocker-logs/express-v1-baseline.md`
recorded a Koa-style `http.createServer` module that lowered silently to
nothing; it is now a reported diagnostic — see
`qualified_node_http_server_factory_reports_the_unimplemented_surface`). What
changed is that the two things that had to be decided before code are decided.

### Decided: the server runs on a current-thread runtime

The generated runtime is deliberately single-threaded and `Rc`-based, and Node
is single-threaded too, so a program that uses `node:http` emits
`#[tokio::main(flavor = "current_thread")]`, runs the accept loop inside a
`tokio::task::LocalSet`, and `spawn_local`s each connection's
`hyper::server::conn::http1::Builder::serve_connection(io, service_fn(..))`.
Handler closures stay `Rc<dyn Fn>`; the service closure clones the `Rc` per call
and returns a `Pin<Box<dyn Future<Output = Result<Response<..>, Infallible>>>>`,
so no `Send` bound is needed under `spawn_local`. A program with no server keeps
today's `#[tokio::main]`. This belongs in a comment at the emit site.

### Decided: the body model, and one thing it forces

`SmeltBody` is the piece `Request`, `Response` and `IncomingMessage` share:

```
enum SmeltBodyPayload { Empty, Bytes(Vec<u8>), Stream(Vec<Vec<u8>>) }
struct SmeltBody { id: usize, payload: Rc<RefCell<SmeltBodyPayload>>, used: Rc<Cell<bool>> }
```

`Rc<Cell<bool>>` beside the payload rather than a moved-out value, because the
spec's `bodyUsed` is observable through *every* handle: two variables holding
the same response see one another's consumption. `take_bytes` sets it and a
second call is the spec's `TypeError`; `peek_bytes` is the non-reader path for
equality, `Debug` and `Response.clone()` (which the spec gives its own unread
body, so it is `tee()` — a payload copy with a fresh flag — not Rust's `Clone`,
which is the handle copy). Readers are `Future<T>` in HIR, which the existing
`AsyncOp`/`SmeltFuture` machinery already carries, so a body reader is an
ordinary awaited call rather than new machinery. `json()` is `Future<Unknown>`
and that erasure is genuine (a JSON boundary) — the one place in these types
where a tagged value is correct, to be spelled as such at the emit site.

The thing it forces, found while drafting the prelude: **the double-read
`TypeError` cannot be unconditionally branded.** The error channel is
`Box<dyn std::error::Error>` and a branded JS error is
`smelt_throw(error_payload_record_expr("TypeError", ..))`, which is a
`SmeltUnknown::Object` — but `SmeltUnknown` is gated on `needs_unknown`, and a
crate doing `new Response("hi").text()` need not carry the erased carrier at
all. So the body emitter takes `needs_unknown`: with the carrier, the throw is
the branded record and a source `catch` sees `error.name === "TypeError"`;
without it, the same failure is a message-only error on the same channel, which
is consistent because such a crate has no erased values to inspect.

## 6. `console.log` of an optional: fixed, and what it cost


`console.log` of an `Optional<T>` printed Rust's `Some("ada")` / `None`. Node
prints `ada` / `undefined`. Two committed end-to-end fixtures had that Rust
shape baked into their `expected.stdout`, and a CLI test asserted `Some("a")`
as the output of a **Python** program, so the bug was pinned three times over
rather than caught.

The present arm now renders the inner value the way `console.log` renders that
type alone, so the wrapper is invisible, and nested optionals recurse.

The absent arm needed a decision. TypeScript's `null` and `undefined` both
intern to `Type::None`, so `T | null` and `T | undefined` are the *same*
`Optional(T)` by the time the emitter sees one; and both frontends lower to the
same `CONSOLE_LOG_SYMBOL` builtin, while Python prints `None` where JavaScript
prints `undefined`. Codegen therefore cannot tell either pair apart, and
guessing was not acceptable.

This is exactly the problem `NegativeIndex` already solves in this codebase
(`xs[-1]` is the last element in Python and `undefined` in JavaScript), so the
fix takes the same shape: an `AbsentSpelling` enum decided during MIR lowering
from the call site's span — the span names the file, the file names the frontend
— and carried on `BuiltinFn::ConsoleLog`. Verified in both directions:

| program | source | Smelt output | reference |
| --- | --- | --- | --- |
| optional param, `Map.get` hit and miss | TypeScript | `ada undefined 1 undefined` | Node 22, identical |
| `obj.id or None`, then a `None` | Python | `a` then `None` | CPython, identical |

Within TypeScript, `undefined` is the word for an absent optional because it is
what nearly every operation that *produces* one returns (`find`, `pop`,
`Map.get`, an optional property or parameter, `?.`, `process.env.X`); a value
annotated as plain `null` still prints `null` through the `Type::None` branch.
Printing the right word for `T | null` too needs a distinct `Type::Undefined`
carried down from the annotation, which is a type-table change and is not done
here.

Three fixtures now pin real program output where they used to pin Rust's
`Option` Debug: `27_optional_chains` (`Ada undefined 3 user`),
`28_regex_match_result` (29 lines, including two `undefined` non-participating
capture groups), and the new `33_console_optional_value`. All three were diffed
against Node 22 line by line.

### Two pre-existing bugs this uncovered

Neither is fixed here; both are recorded because they are invisible to the
compile gates:

1. **`Array.prototype.find` with a typed arrow does not compile.** The first
   draft of `33_console_optional_value` used
   `names.find((name: string) => name.startsWith("a"))`, and the generated crate
   fails with E0308: the predicate's `bool` is assigned to a `SmeltUnknown`
   temporary without being wrapped (`_smelt_tmp_3: SmeltUnknown =
   closure_arg_0.clone().starts_with(&"z".to_owned())`). The fixture uses other
   optional producers instead.

2. **An empty object literal against an interface erases.** `const withoutLabel:
   Config = {}` for `interface Config { label?: string }` emits
   `SmeltRecord<String, SmeltUnknown>` rather than the `Config` struct — one
   avoidable erasure, caught by the examples invariant when the second draft of
   the fixture used that shape.
