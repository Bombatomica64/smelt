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
| 4 `Request` / `Response` / `fetch` upgrade | **not landed** — needs the body model in section 3 below |
| 4 `TextEncoder`/`TextDecoder`, `FormData`, `ReadableStream`, `AbortController`, `crypto` | not landed |
| 4 `Blob`/`File` upgrade (`text()`, `arrayBuffer()`, `slice`) | not landed |
| 5 `node:http` on hyper | **not landed** — declared as a blocker instead (honest today, section 5 below) |

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

## 3. What `Request`/`Response` need next: the body model

`Headers` and `URLSearchParams` were reachable because they are **synchronous
value types**. `Request`/`Response` are not: `text()`, `json()`,
`arrayBuffer()`, `formData()` and `blob()` all return promises, and the body is
**single-use** (`bodyUsed`). That is the one genuinely new piece, and it should
land before either type:

- a `SmeltBody` in the fetch prelude: `enum { Empty, Bytes(Vec<u8>), Stream(..) }`
  behind the same `Rc<RefCell<..>>` identity, with `used: Cell<bool>` so a
  second read is the spec's `TypeError`;
- the readers are `Future<T>` in HIR (`Type::Future(String)` for `text()`), which
  the existing async lowering already carries — `AsyncOp` and `SmeltFuture` are
  in place, so a body reader is an ordinary awaited call, not new machinery;
- `json()` is `Future<Unknown>` and that erasure is genuine (a JSON boundary),
  so it is the one place in these types where a tagged value is correct; it must
  be spelled as such at the emit site with the comment `CLAUDE.md` requires.

With `SmeltBody` in place, `Request` and `Response` are the same nine-site
recipe as above, with `status`/`ok`/`statusText`/`method`/`url` as data
properties (the `URLSearchParams.size` field path shows how), `headers` as a
`Headers`-typed field read, and the statics (`Response.json`,
`Response.error`, `Response.redirect`) as namespace-call rules.

`fetch()` is `AsyncOp::HttpGetText` today (a GET returning `string`, over
`reqwest`). Upgrading it to return `Response` is a change of that op's result
type plus a request builder that reads `RequestInit`; the existing GET-text
tests must keep passing through the new type, so the upgrade should keep
`HttpGetText` as a *derived* path (`fetch(url).then(r => r.text())`) rather than
deleting it.

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

## 5. Why `node:http` is a declared blocker rather than hyper


Section 5 needs three things that do not exist yet:

1. **The body model** (section 3) — `IncomingMessage` shares it with `Request`,
   and the plan is explicit that they must be one model.
2. **`node:events`** — `IncomingMessage.on('data')` / `on('end')` is an
   `EventEmitter`, which is declared in the registry and unimplemented. Async
   iteration over the request body is the modern spelling and needs the stream
   half of the body model.
3. **A server lifetime in the emitted `main`** — `createServer(handler)` is
   `service_fn` and `listen(port)` is an awaited server future inside the
   already-emitted `#[tokio::main]`; the handler closure has to become a
   `'static` service, which is a new shape for closure emission (today closures
   are `Rc<dyn Fn>` in a single-threaded runtime, and hyper's `service_fn` wants
   a future per call).

Item 3 is the real unknown and deserves its own investigation before code: the
generated runtime is deliberately single-threaded `Rc`-based, so the server must
run on a current-thread runtime (`#[tokio::main(flavor = "current_thread")]`)
for a handler closure to be usable at all. That is a decision about the emitted
runtime, not a detail of `node:http`.

Until then the surface is declared in the registry and using it is a named
blocker, which is the honest state: `blocker-logs/express-v1-baseline.md`
recorded a Koa-style `http.createServer` module that lowered silently to
nothing, and that module is now a reported diagnostic (see
`qualified_node_http_server_factory_reports_the_unimplemented_surface`).

## 6. Fidelity gap found while testing (not fixed here)

`console.log` of an `Option<T>` prints `Some("a")` / `None` where Node prints
`a` / `undefined`. It is pre-existing (reproduced with a plain
`function pick(values: string[]): string | undefined` fixture, no fetch types
involved) and it is a *runtime output* difference, so it is invisible to every
compile gate. Both end-to-end fixtures added here spell `?? "null"` to avoid
depending on it. Worth its own fix: an optional in an erased `console.log`
argument should print its inner value or `undefined`.
