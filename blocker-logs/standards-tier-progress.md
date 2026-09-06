# Standards tier: what landed, and the mechanism the rest needs

Date: 2026-09-06. Plan: `blocker-logs/standards-tier-plan.md`. Evidence for
Milestone 0: `blocker-logs/express-v1-baseline.md`.

## 1. State after this pass

| plan item | state |
| --- | --- |
| M0.1 unresolved package used as a value is a blocker | **partly landed** — mechanism in place, policy for *unmodeled* packages deliberately off (section 4) |
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

`smelt probe` on `examples/typescript/express_crud` now reports 3 blockers
(2x unresolved `express`, 1x `node:sqlite` `DatabaseSync`) and emits all six
modules with all seven free functions, where before it reported 0 blockers and
emitted `main` only.

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

## 4. Why the unmodeled-package half of M0.1 is off

The blocker fires today for a **modeled** host module whose export is declared
but unimplemented (`node:http`, `node:sqlite`, `node:crypto`, `node:events`).
For an **unmodeled** bare package (`express`, `lodash`, `yup`) it is gated off
behind one documented constant,
`smelt_stdlib::host_modules::unmodeled_package_use_blocks()`, currently `false`.

The reason is measured, not a preference. Erased-library interop is a
deliberate, tested capability that program code depends on today:

- 13 tests in `crates/smelt-frontend-ts/src/tests/part04_tests.rs` lower real
  Strapi / lodash / yup / zod / cuid2 **program** modules (not test modules)
  through erased imported values;
- the radash compatibility gate lowers `import { assert } from 'chai'`.

Turning it on therefore needs one of two decisions, which belong to the
architect rather than to this stream:

1. **Model the packages.** `chai`'s `assert` is an assertion surface like
   `vitest`'s `expect`, and would fit the host-module registry as a test-tier
   entry; `lodash`/`yup`/`zod` are much larger and would each be their own
   campaign.
2. **Re-baseline the corpora.** Accept the blockers, exclude the affected files
   from those corpora, and record the new numbers. This makes every probe honest
   immediately at the cost of coverage the campaigns rely on.

The carve-outs already in the classification are independent of that decision
and stay either way: a relative specifier never blocks (it names a source file
the manifest owns), and the test tier keeps erased interop with unmodeled
assertion/fixture libraries (`CLAUDE.md`'s test-function exception).

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
