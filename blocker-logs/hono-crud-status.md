# hono_crud: what breaks, why, and why the generated server is slower

Subject: `examples/typescript/hono_crud` (Hono 4.13.7 todo CRUD, strict TS, `node:sqlite`,
`@hono/node-server`, vitest). Branch `claude/hono-crud-perf`, 2026-10-16, base `origin/main`
`c9ce53fb` (the branch's original base `9aeedad8` did not compile: a semantic merge conflict in
`smelt-codegen-rust/src/emitter/coercion.rs` — `list_items_render_as_unknown` gained a second
argument on one side; `origin/main` already carries the fix, so the branch was fast-forwarded).

Evidence lives in `benchmarks/hono_crud/` (scripts, staging sources, probe outputs, results)
and in the regression fixtures: end-to-end `examples/typescript/end-to-end/{160,162,165}_*` and runtime `crates/smelt-transpiler/tests/fixtures/runtime/*` (the six whose programs cross an unrelated erasure live there, per that harness's rule, so the examples corpus keeps avoidable erasure at 0).

## TL;DR

| Question | Answer |
| --- | --- |
| Does the shipped example transpile? | **No.** Four blocker families (below); nothing builds, so 0/9 tests and 0/30 probe requests. |
| With the blockers routed around (staging tree: Hono TS source, in-memory store, `node:http` shim) — generated tests, before this branch | **1/9** pass. |
| Same, after this branch | **9/9** pass. |
| Generated server vs Node, 30-request route probe | before: no server could start; after: **identical status, headers and bodies on 29/30**; 1 differs only in the JSON parse-error message text; hyper omits Node's `connection`/`keep-alive` headers. |
| Throughput, identical source, one core | generated server is **0.43×–0.66×** Node (`node-staging`); see "Performance". |
| Memory | before §10: the generated server leaked **~16 KB per request** (10.7 GB RSS after ~650k requests); Node stays at ~160 MB. After §10: **flat at 12 MiB** through every read scenario (peak 127 MiB, all of it the todos POST inserts). |
| Throughput after §10 (vs new `main`, interleaved, medians of 3) | **+31 % to +51 %** on every scenario; see §10. |
| Python counterpart in the repo | **None exists** (searched `examples/`, `benchmarks/`, `specs/`; `specs/m9-fastapi-pydantic.md` is a spec, not an app). No Python numbers are reported. |

## 1. The shipped example does not transpile

`target/debug/smelt --manifest-path examples/typescript/hono_crud/Smelt.toml build` stops at the
first blocker. Routing each one around in turn (`smelt check --message-format json` on a staging
copy) gives the full list:

| Family | Blockers | Cause | Site |
| --- | ---: | --- | --- |
| R1 package source | 1 (fatal first) | npm `hono` ships only `.js` + `.d.ts`; a bare specifier resolves only through a workspace package whose `package.json` has a TS `source`/`types`/`main` entry. There is no manifest way to point `hono` at TypeScript sources. | `crates/smelt-transpiler/src/manifest.rs` `resolve_workspace_package_import`, `workspace_package_entry` |
| R2 `node:sqlite` | 1 | `DatabaseSync`/`StatementSync` are *declared* host surfaces, not modeled. | `crates/smelt-stdlib/src/host_modules.rs` (`SQLITE_REASON`) |
| R3 `@hono/node-server` | 10 | its TS source needs `node:http2` (unmodeled), `class extends GlobalRequest`, `ReadableStream`, the `Response` global as a value, `for...in` over a non-record, `.then` read off a `Future` field. | node-server `src/{request,response,listener,utils}.ts` |
| R4 Hono client via barrel | 3 | `hono/src/index.ts` re-exports the RPC client types; with Hono vendored, its `client/` (`new Proxy`) enters the crate unless excluded, as `.github/compat/hono/Smelt.toml` already does. | manifest `exclude` |
| R5 relative manifest path | — | `smelt --manifest-path Smelt.toml` run *inside* the project resolves workspace packages from the empty path `""`, so `read_dir("")` fails and `hono` is "unresolved"; an absolute manifest path works. | `manifest.rs` `workspace_search_root` (path not canonicalized) |

R2 and R3 are feature gaps (modeling `node:sqlite`; lowering or modeling the Node adapter), not
lowering bugs. They are listed in the plan at the end.

## 2. Staging tree (how everything below was measured)

`benchmarks/hono_crud/prepare-staging.sh` builds it: Hono 4.13.7 TypeScript source (the pinned
compat ref) vendored as `packages/hono`; `db.ts`/`repository.ts` replaced by an in-memory row array
with the same columns and AUTOINCREMENT rule; `main.ts` serving `app.fetch` through `serve.ts`, a
buffered `node:http` shim. `src/app.ts` changes only its `DatabaseSync` type import; routes, model,
validation and the vitest file are the shipped ones. On Node the staging tree passes the vitest
suite 9/9 and answers the probe identically to the shipped app except one `@hono/node-server`
quirk (it adds `content-type: text/plain` to a 204) —
`benchmarks/hono_crud/results/probe-node-original-vs-staging.normalized.diff`.

## 3. Runtime breakage, grouped by root cause

Before this branch the generated test binary passed 1/9 (only "404 for an unmatched route"). Each
family below was isolated to a minimal TypeScript repro, fixed by a general rule, and pinned by an
end-to-end fixture whose `expected.stdout` is Node's output.

### Fixed on this branch

| # | Symptom in hono_crud | Root cause | Fix site | Fixture |
| --- | --- | --- | --- | --- |
| A | every route returned 200 with an empty body: the handler never ran | a call whose callee is a computed member (`matchResult[0][0][0][0](c, next)`) of a **union-of-functions** type lowered to its *first argument*; an `unknown` callee lowered to `undefined`; an asserted erased callee too. The call was dropped silently. | `smelt-frontend-ts/src/lowering/stdlib/call_dispatch.rs` — `callable_through_erased_abi`, `erased_value_call` | runtime `union_function_element_callee` |
| B | every mounted route (`app.route('/todos', sub)`) 404'd | `Object.assign(this, rest)` on a class instance merged into a **copy** of the instance's erased view; Hono's `basePath()` clone kept a default router. Now lowered as guarded typed field writes over the class's declared own fields. Also: a generic record's `SmeltFromUnknown` dropped function-typed fields (`Router<T>.add/match`) — now restored from the erased function's registered typed origin. | `smelt-frontend-ts/src/lowering/stdlib.rs` `object_assign_class_target`; `smelt-codegen-rust/src/lib.rs` `emit_function_field_origin_restore` | runtime `object_assign_onto_instance` |
| F | mounted handlers wrapped in an error-composing adapter | a named function referenced as a **typed** value built a fresh `Rc` per reference, so `app.errorHandler === errorHandler` was false; now one cached `Rc` per item and rendered type. Programs without the `SmeltUnknown` runtime referenced a missing `smelt_same_function_identity` — now `Rc::ptr_eq` there. | `smelt-codegen-rust/src/emitter/core.rs` `function_item_typed_accessor`; `emitter/binary_ops.rs` | e2e `160_function_item_typed_identity` |
| J | every POST/PATCH body parsed as `null` | `raw[key]()` with a run-time key on a class/host value answered `undefined`; now read through the value's erased view, with a `text`/`json` body-reader resolver for erased `Request`/`Response`. | `emitter/place.rs`; `lib.rs` `emit_body_host_method`; `stdlib.rs` `needs_body_host_method` | runtime `dynamic_key_body_reader` |
| I | every `:id` route answered "id must be a positive integer" | `type IdContext = Context<Env, '/:id'>` — an alias **naming a class** recorded the class's fields as the alias's own, so references lowered to a nominal record named after the alias that nothing backs, and `c` erased. | `smelt-frontend-ts/src/lowering/decls/types_iface.rs` (`names_nominal_type`) | e2e `162_alias_names_generic_class` |
| C | `c.json(x)` answered status `NaN` | `a ?? b` typed by an enclosing ternary's other arm dropped `b`'s `undefined`. | `lowering/expr/operators.rs` `nullish_coalesce_expression` | runtime `nullish_fallback_and_nullable_union_slot` |
| K | `c.body(null, 204)` sent `content-type: application/x-www-form-urlencoded` | a union with a `null` arm passed to an optional slot was wrapped whole in `Some(..)`, so `null` picked the `URLSearchParams` arm. | `emitter/coercion.rs` | runtime `nullish_fallback_…` |
| O | generated server panicked "no reactor running" | `main` became async only if the module body *itself* awaited/served; `serve()` in another function did not count. Callees are now followed. | `lowering/module_init.rs` `body_reaches_event_loop` | runtime `event_loop_reached_through_callee` |
| S1 | server stopped accepting as soon as `serve()` returned | the accept loop treated a dropped shutdown sender (server object out of scope) as `close()`. | `smelt-codegen-rust/src/http_server_prelude.rs` | covered by `44_node_http_echo` + the staging server |
| S2 | requests accepted, never answered | the exit loop kept the process alive while a handle was open but never ran queued promise tasks; it now runs the event loop and is woken when a task is queued. | `lib.rs` `emit_exit_drain_support`, `smelt_spawn_promise_task` | staging server |
| V | `void handle(req, res)` did nothing | `void X` dropped `X` entirely. | `lowering/expr/operators.rs` `void_expression`; `decls/types_iface.rs` `expression_statement` | e2e `165_void_operand_runs` |
| X2 | remeda `prop(undefined, 'a')` panicked (CI remeda gate on this PR) | an optional erased value read at `unknown` asserted presence (`.expect`); latent on main, exposed once K started passing `undefined` as `None`. Now `smelt_optional_unknown_or_undefined` maps absence to `undefined`, a named adapter between two erased spellings classified as a boundary | `emitter/call_runtime.rs`; `lib.rs`; `unknown_report.rs` | runtime `optional_unknown_reads_as_undefined` |
| X | a `null` JSON body passed "is an object" validation | untyped `JSON.parse` was typed as a string-keyed record, so `JSON.parse('null')` became `{}`. Now `unknown` (JSON is the canonical dynamic boundary). | `lowering/stdlib.rs` `json_parse_fallback_type` | runtime `json_parse_any_value` |

### Found, not fixed (reproduced, documented)

| # | Defect | Minimal repro | Notes |
| --- | --- | --- | --- |
| D | a user interface method named `match` called with one argument lowers as `String.prototype.match` on `"[object Object]"` | `interface R { match(p: string): boolean }` | method-name dispatch claims a builtin name before checking the receiver type |
| E | generic interface field holding a generic list: `SmeltArray: From<SmeltList<String>>` E0277 | `Router<T>` returning `T[]` through a round trip | erased-generic list alias (#279) |
| G | `err.message` on an `Error`-typed parameter in a crate without the `unknown` runtime is an emitter error | `function f(err: Error) { return err.message }` | |
| H | user names that collide with Rust: a class `Box`, a function `main` | `class Box {}` / `async function main()` | generated code does not mangle them |
| L | `null` vs `undefined` erased in `Optional`: `headers.get('x')` prints `undefined`, `void f()` prints `null` | | the "absence vs null" model does not reach `console.log` |
| M | `Headers` iteration: `headers.forEach((value, name) => ..)` gets Array's `(value, index)` signature; `for (const [k, v] of headers)` erases and panics "unknown is not iterable" | `serve.ts` shim (worked around there) | |
| N | a callback parameter (`&dyn Fn`) stored or moved into a `'static` closure is a lifetime error | `serve(opts, onListen)` → `listen(port, onListen)` | parameter-by-reference ABI does not check escape |
| P | `process.env.X` is folded to `undefined` (deterministic profile) | `process.env.PORT ?? 3000` | a server cannot read its port; profile decision, not a bug |
| Q | `type A = Gen` + a lifted module arrow `(c: A) => c.m()` types the call result as the receiver | same-module generic class | alias pre-pass runs before class defaults are known; an attempted re-run of the alias pass after `collect_class_type_parameter_defaults` made the Hono build hang and was reverted |
| R | a type parameter constrained `P extends string` is matched as `SmeltUnknown` in `this.path + ':'` (13 rustc errors) | `class C<P extends string> { path: P; f() { return this.path + ':' } }` | |
| T | `for...of` over an array literal with nested ternaries leaves temporaries undeclared (E0425) | debug test in the staging tree | |
| W | a floating promise and a 0 ms timer settle in the opposite order from Node | `setTimeout(cb, 0); void asyncFn()` | virtual-time loop drains timers before queued promise tasks |
| Z | an object literal passed into a `T | ResponseInit` union is typed as a homogeneous record (`Dict<String, Float>` for `{ status: 204 }`), and the `ResponseInit` adapter then reads `headers` out of it as `f64` (E0308) | `make(null, { status: 204 })` with `arg?: StatusCode \| ResponseInit` | found while writing the `nullish_fallback_and_nullable_union_slot` fixture, which now passes a typed `ResponseInit` |
| Y | JSON parse errors carry serde's message, not V8's | `{bad json` → `key must be a string at line 1 column 2` vs `Expected property name or '}' …` | the one remaining probe difference |

## 4. Route conformance (generated server vs Node, same staging source)

`benchmarks/hono_crud/probe.sh` drives 30 requests (every route, 400 bad id/body, 404 missing,
404 unmatched route, 404 unknown method, bad JSON → 500). Raw outputs and diffs are in
`benchmarks/hono_crud/results/`.

* **Before this branch:** no generated server could be built from any form of the app.
* **After:** status line, `content-type`, `content-length` and body identical on 29/30 requests.
  The 30th (`POST {bad json` → 500) differs in the error message text (family Y). Header
  differences are HTTP-layer only: hyper does not echo `connection: keep-alive` /
  `keep-alive: timeout=5`, and the staging shim writes lower-case header names where
  `@hono/node-server` writes `Content-Type`.

## 5. Performance

Benchmark defined before measuring: `benchmarks/hono_crud/README.md` (server pinned to one core,
`oha` on six others, 5 s warm-up + 10 s at 32 keep-alive connections per scenario, 20 seeded rows,
writes last). Release build of the generated crate (`lto = "thin"`, `codegen-units = 1`); Node
v24.21.0.

### Results (req/s; p50 / p99 ms; peak RSS after the scenario)

| Scenario | rust-staging (median of 3) | node-staging | node-original | rust / node-staging |
| --- | ---: | ---: | ---: | ---: |
| `GET /todos` | 6,542 (4.308 / 9.758) | 15,191 (1.902 / 4.755) | 9,432 (2.987 / 8.473) | 0.43× |
| `GET /todos/1` | 8,960 (3.022 / 9.235) | 16,206 (1.767 / 4.773) | 13,029 (2.138 / 5.697) | 0.55× |
| `GET /todos/abc` | 9,002 (3.029 / 6.591) | 16,227 (1.75 / 4.754) | 34,187 (0.824 / 2.155) | 0.55× |
| `GET /nope` | 10,827 (2.763 / 6.031) | 16,382 (1.721 / 4.771) | 32,329 (0.87 / 2.182) | 0.66× |
| `PATCH /todos/1` | 7,023 (4.116 / 8.673) | 12,167 (2.343 / 6.192) | 4,728 (5.444 / 34.082) | 0.58× |
| `POST /todos` | 7,194 (3.702 / 10.715) | 11,858 (2.418 / 6.509) | 206 (148.04 / 455.185) | 0.61× |

Peak RSS (`VmHWM`, end of the six scenarios): rust-staging **10.2 GiB**, node-staging 157 MiB, node-original 257 MiB.

Cells are req/s with (p50 / p99 ms). Raw rows: `benchmarks/hono_crud/results/bench-baseline.tsv`.
`node-original` uses a file-backed SQLite (writes are fsync-bound, hence 206 POST/s) and
`@hono/node-server`; it is a reference, not a like-for-like comparison. The generated server's
later scenarios are also depressed by its own memory growth (see below).

**Before this branch there was no generated server to measure** (section 1), so there is no
"before" throughput. One optimisation was tried against the benchmark after the correctness
fixes: hoisting the per-key host-marker check out of `JSON.stringify` (8.6 % of samples).
Interleaved before/after runs under the same load (medians of 3,
`results/bench-interleaved-serializer-candidate.tsv`): `GET /todos` 6,626 → 7,192 (+8.5 %),
`GET /todos/1` 8,917 → 9,240 (+3.6 %), `GET /todos/abc` 9,295 → 8,749 (−5.9 %, a path the change
does not touch — load noise; the host load average was 11–20 during the runs). That misses the
acceptance criterion fixed in advance (≥ 10 % on scenario 1 or 2, no scenario below −5 %), so the
change was **reverted** and is listed in the plan instead of being claimed.

### Where the time goes

Profile: `pprof` (setitimer sampling; `perf` is not installed for this kernel and
`perf_event_paranoid=4`), 10 784 samples over `GET /todos/1` + `GET /todos`, release build with
line tables. Folded stacks were aggregated with `fold.py`-style self/inclusive counts.

| Share of CPU | Cause | Where in the generated crate | Lowering/runtime rule that produces it |
| ---: | --- | --- | --- |
| **~20 %** (13.4 % `HashMap::entry` + 4.3 % rehash + identity lookups) | **identity registries grow on every request** — `smelt_retain_callable_key`, `smelt_link_function_identity_key`, `smelt_register_function_origin`, `smelt_register_host_origin` insert per erased callable/host value and never remove | prelude `SMELT_CALLABLE_KEY_GUARDS`, `SMELT_FUNCTION_IDENTITIES`, `SMELT_FUNCTION_ORIGINS`, `SMELT_HOST_ORIGINS` | `smelt-codegen-rust/src/lib.rs` (registries "never remove an entry, because there is no drop hook"); every erasure adapter in `emitter/coercion.rs` registers |
| **16.4 %** | `JSON.stringify` of erased values | `impl Serialize for SmeltUnknown` | handler results are erased before `c.json` serializes them (`c.json` takes `unknown`) |
| of which **8.6 %** | per-key host-marker probing (`smelt_is_for_in_object_key` → `smelt_object_has_host_marker`: ~30 linear-scan lookups per key) | same | `lib.rs` serializer arm — a one-pass-per-object fast path measured +8.5 % on `GET /todos`, below the bar; reverted (see Results) |
| **14.3 % self** | linear `position` scans over erased objects' field stores (`smelt_key_fingerprint`, `smelt_get_object_field`) | `SmeltFieldStore` | every member read on an erased value (Context options record, `matchResult`, request records) |
| 8.8 % | `TodoRepository.list()` → `rowToTodo` per row | `repository.rs` | ordinary; each row is converted through erased `SmeltUnknown` element reads |
| — | per-request erasure of whole class instances into records (`Context` constructor options, `this` erased for `Object.assign`, `c` erased into a 30-field record for the composed-handler path) | `main.rs` `Hono_1::new`, `_dispatch` | `IntoSmeltUnknown` of a reference class builds every field and prototype member |

The memory growth is the same mechanism as the first row: per request the size-instrumented build
(`/proc` + per-registry counters) shows **+22 callable-key guards, +6 host origins, +5 function
identities, +2 function origins, +1 each** for array props, erased-function values, reference-object
identities and function lengths. `SMELT_FUNCTION_ORIGINS` holds a *strong* clone of each typed
origin, so the captured `Context`/`Request` graph stays alive. An amortized sweep of dead
callable-key entries was prototyped and measured: it reclaimed little, because most entries stay
reachable through the strong origin/host registries themselves; it was reverted (it did not meet
the acceptance criteria and touches the address-reuse invariant that fixed remeda's flaky lazy
`pipe`).

### benchmarks/FINDINGS.md, revisited for this workload

| FINDINGS cause | Applies here? |
| --- | --- |
| 1 whole-collection clones in loops | marginal: the hot loops are per-row conversions, not repeated whole-list clones |
| 2 self-recursive closure `Rc` cycles | not observed (fixed upstream); the leak here is the identity registries |
| 3 module `const` regex rebuilt per call | **no** — `SMELT_REGEX_CACHE` memoizes compiled patterns; Hono's router builds its matcher once |
| 4 erasure cost | **yes, dominant** — routing values through `SmeltUnknown` (rows 1, 2, 4 above) |

## 6. Ranked plan for the remaining performance work

1. **Done (§10).** ~~Bound the identity registries (memory + ~20 % CPU).~~ The registries were
   bounded, and the leak turned out to be two program-graph `Rc` cycles as well. Give erased values their metadata
   *inline* instead of in address-keyed global maps: an erased callable as a small struct
   (`call`, `origin: Option<Rc<dyn Any>>`, `identity`) and host values through `SmeltObject`'s
   existing `origin` slot (`smelt_with_origin`, already used by reference classes). The by-id
   lookups (`smelt_restore_host_origin_by_id`, typed-array write-through) need a weak id→store
   map. Acceptance: RSS flat under the benchmark; remeda/es-toolkit suites unchanged. Est. 4–7 days.
2. **Keep `Context` typed through dispatch (largest erasure).** `Hono#dispatch` erases `c` into a
   record per request (`new Context(request, { … })` options bag, `Object.assign`, composed
   handlers). Typed options records (`HonoOptions<E>` is erased only because it is generic) and a
   typed `ContextOptions` struct remove the per-request record builds. Est. 3–5 days.
3. **Done differently (§10).** The hot `position` scans were misses on internal marker keys; the
   store now counts its `__smelt*` keys and marker probes on a store with none return at once.
   ~~Index erased field stores.~~ `SmeltFieldStore` lookups are linear with a fingerprint; a small
   inline hash index (or sorted keys) for stores above ~8 entries removes the 14 % `position` cost.
   Est. 1–2 days.
4. **Serialize typed values directly.** `c.json(todo)` erases `Todo` first; generic `json<T>`
   with a `Serialize` bound on concrete records (the `interface_is_json_serializable` path already
   derives `serde::Serialize`) would skip the erased walk. Est. 2–4 days.
5. **Done (§10).** ~~Serializer fast path~~ (measured, +8.5 % on `GET /todos`, below the 10 % bar alone): decide
   "plain record" once per object in `impl Serialize for SmeltUnknown`; land it together with 3. Est. 0.5 day.
6. **Partly done (§10):** `res.end()` now signals the connection task (it was a 1 ms poll), and
   the promise-task waker notifies the serving loop. Remaining: **Event loop.** The deterministic virtual-time loop polls promise tasks with a no-op waker and
   ticks every 5 ms while serving; real wakers (`spawn_local`) for I/O-driven tasks would cut
   latency and fix family W. Est. 3–5 days (semantics-sensitive: Vitest timer emulation).

## 7. Corpus gates touched by this branch

| Gate | Before | After |
| --- | ---: | ---: |
| examples SmeltUnknown invariant (avoidable) | 0 | **0** — six of the nine new fixtures cross an unrelated erasure (a union of function types, source `unknown`, `JSON.parse`), so they live under `crates/smelt-transpiler/tests/fixtures/runtime/` per that harness's documented rule; placed in `examples/` they would have added 46 avoidable occurrences |
| es-toolkit SmeltUnknown ratchet (avoidable) | 25 424 | **25 346 (−78)**; baseline re-snapshotted in the same commit. A first cut of the typed function-item accessor was **+6** (each accessor restated its callable type three times); spelling it once through a type alias turned it into the decrease |
| es-toolkit probe gate (files with blockers) | baseline 9 | 0 (PASS) |
| remeda SmeltUnknown (advisory, not re-snapshotted) | 24 167 | 23 563 (−604) |
| Hono compat (`smelt check`, pinned 4.13.7 + overlay) | — | 0 blockers, `smelt build` succeeds |
| examples HIR goldens | — | one moved: `41_symbol_keyed_class_members` now evaluates a computed-member callee before its arguments, as JavaScript does |

## 8. What was verified vs assumed

Verified: every "before/after" claim above by running the generated crate (tests, probe, `oha`);
each fixed family by a minimal repro run on Node and on the generated binary; the nine new fixtures
compare against Node's stdout; the staging tree passes 9/9 on Node. The profile shares are
from one pprof run (sampling noise ±1 %).

Assumed / not checked: behavior of the shipped app with real `node:sqlite` and
`@hono/node-server` under Smelt (they do not lower); performance under multiple cores (both
runtimes were pinned to one); that the `serve.ts` shim's cost is representative of
`@hono/node-server` on the Rust side (on Node, `@hono/node-server` is ~2× faster than the shim on
the 400/404 routes, so `node-staging` understates what Node achieves with its native adapter).

## 9. Decisions (user, 2026-10-17)

| Question | Decision |
| --- | --- |
| Order of work | **Track 1 first**: make the generated server fit to run — bound the identity registries (plan item 1, the leak), index erased field stores (item 3), then land the serializer fast path with them. Track 2 (shipped example builds as written) next; track 3 (typed Hono context) after. |
| `@hono/node-server` | **Model it** as a host module (`serve({ fetch, port })` on the existing hyper-backed `node:http` runtime), the way `@date-fns/tz` is modeled — not lower its source. |
| `process.env` in programs | **Real reads are allowed**: a server program must see its environment (`process.env.PORT`). The deterministic profile's "every variable is absent" fold (family P) is to be replaced by a real read, keeping determinism where tests need it. |
| Python-app showcase | **Deferred**; revisit with the open questions in `hono-crud-python-app.md`. |

## 10. Track 1: the generated server is fit to run (branch `claude/smelt-registry-memory`, 2026-10-18)

Base: `origin/main` `96047fdd` (PR #281 merged, including the Hono v5 merge #280). Every "before"
below is a release build of the staging server generated by `smelt` at that commit, rebuilt for
this comparison; "after" is the same staging source generated by this branch.

### What was actually leaking

The plan (§6.1) assumed the identity registries were the leak. They were one leak, but bounding
them alone reclaimed nothing: measured per-request growth stayed the same, because every entry was
still reachable. Three things kept each request's whole object graph alive, found by tagging the
live table entries with their Rust types:

| # | Cycle / pin | Shape in the source | What it kept alive |
| --- | --- | --- | --- |
| 1 | registries never removed an entry, and two of them held strong values reaching their own keys | erasing a callable, a host value, a match | every erased adapter, typed origin, `Request`/`Response`/`Headers`, match array |
| 2 | a class-field arrow captures the instance it is stored on (`Rc` cycle) | `json = (b) => this.render(b)`; on the new base Hono's `HonoRequest`, `Hono`/`HonoBase` | the instance and everything it reaches |
| 3 | a listener captures the emitter it is registered on (`Rc` cycle) | `req.on('end', () => handle(req, res, …))` (`serve.ts`; the everyday Node idiom) | `req`, `res`, the body, the handler's closures |

Investigation evidence (instrumented fast builds, 60k mixed requests): with (1) only, peak HWM was
unchanged; with (1)+(2) it halved; with (1)+(2)+(3) it is flat. On the new base `Context`'s arrow
fields are already lowered to methods (from #280), so (2) shows up in `HonoRequest` and the app
objects rather than in `Context`.

### What changed

* **Bounded identity tables** (`smelt-codegen-rust/src/callable_registry.rs`). The nine
  address-keyed maps (`SMELT_CALLABLE_KEY_GUARDS`, `SMELT_FUNCTION_ORIGINS`, `…_IDENTITIES`,
  `…_LENGTHS`, `SMELT_CALLABLE_OBJECTS`, `SMELT_ERASED_FUNCTION_VALUES`, `SMELT_FUNCTION_PROPERTIES`,
  `SMELT_FUNCTION_CLASSES`, `SMELT_RECEIVER_METHODS`) are one `SmeltAddressTable` whose entries own
  the `Weak` that reserves their key (so the remeda address-reuse guarantee holds by construction:
  guard and metadata are removed together) and are swept, amortized, when the table reaches 4× its
  live size. The sweep cascades until a pass frees nothing — a probe test caught that a single pass
  let chains of adapters inflate the threshold geometrically. Function identity is a shared token
  (`SmeltFunctionIdentity`) that also carries `length` and the own-property bag, so those die with
  the last representation of the function. `SMELT_REFERENCE_OBJECT_IDENTITIES` uses the same table
  (which also removes the documented stale-id-on-address-reuse hazard).
  *Why not the inline design of §6.1:* carrying metadata on the erased callable would bound only the
  erased side — typed `Rc<dyn Fn>` callbacks (the shape a hand port uses) still need identity and
  callable-object metadata — and would change the ABI of ~200 `SmeltUnknown::Function` sites. Tying
  each entry's life to its allocation bounds both sides with one mechanism.
* **Host values inline.** `SMELT_HOST_ORIGINS` (id-keyed, never removed) is gone; host erasures
  stamp the erased record's existing `origin` slot (`smelt_with_host_origin`), and the typed-array
  write-through reads the record's own slot instead of looking up an id.
* **Array named properties** (`SMELT_ARRAY_PROPS`) record the buffers that own them (erased arrays
  that wrote them, typed copies that adopt them) and are swept when all are dead; a property the
  program writes through a detached copy is pinned (the old lifetime). Known gap, documented at the
  site: a typed-to-typed element-type conversion (`unknown[]` → `string[]`) does not adopt.
* **Closures stored on what they capture** (`emitter/weak_receiver.rs`). A constructor closure
  whose only use is to be stored into a field of the instance it captures, and a listener whose
  only use is to be registered (`on`/`once`) on an emitter it captures, capture that object weakly.
  Every escape restores a strong path: the handle's `Drop` pins the instance when its last handle
  goes while such a field's closure is still shared; the erased view's field values hold the
  instance; a nested closure takes a strong capture; an erased-view write over a shared closure
  pins. Not qualified (kept strong, still leaking): closures also used elsewhere, fields written
  outside the constructor, listeners whose body builds closures.
* **Field-store internal-key index** (plan item 3, done differently): the profile's `position`
  cost was misses — ~45 `__smelt*` marker probes per object, each scanning the store. The store now
  counts its internal-namespace keys; every marker probe on a store with none returns immediately.
  A unit test pins the invariant that every host marker is in that namespace.
* **Serializer fast path** (plan item 5): `for...in`'s object-dependent checks are read once per
  object (`smelt_for_in_object_shape`) by the serializer and the own-key walks; the serializer also
  walks the store in place instead of cloning every entry first.
* **`res.end()` signals the connection** (`http_server_prelude.rs`): `wait_until_ended` polled
  every 1 ms, which added up to 1 ms of idle time to every asynchronously answered request and capped
  the server near connections ÷ 1 ms regardless of CPU (the after build measured 66 µs CPU/request
  vs 74 µs before, yet ran at 75 % CPU). The promise-task waker now also notifies the serving loop.

### Results

Benchmark exactly as `benchmarks/hono_crud/README.md`, 3 interleaved rounds, load average 2.7–3.6
throughout (`results/bench-interleaved-registry-memory*.{tsv,txt}`):

| Scenario | before (`main` 96047fdd) | after | Δ |
| --- | ---: | ---: | ---: |
| `GET /todos` | 7,293 | 11,041 | **+51.4 %** |
| `GET /todos/1` | 11,566 | 15,649 | **+35.3 %** |
| `GET /todos/abc` | 12,158 | 16,524 | **+35.9 %** |
| `GET /nope` | 13,391 | 18,697 | **+39.6 %** |
| `PATCH /todos/1` | 8,572 | 11,399 | **+33.0 %** |
| `POST /todos` | 9,679 | 12,705 | **+31.3 %** |
| peak `VmHWM` per run | 9,717–9,769 MiB | 123–127 MiB | |

After, `VmRSS` is 12 MiB after each of the five read/update scenarios and 123–126 MiB after `POST`,
which inserts ~165k todos into the in-memory store (program data; Node keeps them too).

Registry growth, instrumented fast builds, 60k mixed requests:

| Table | before, per request | after, total at 60k |
| --- | ---: | ---: |
| callable key guards / callable table | 20.7 | 318–593 (bounded) |
| function identities | 13.8 | (in the callable table) |
| typed function origins | 7.0 | (in the callable table) |
| function lengths | 2.8 | (on identity tokens) |
| host origins | 5.7 | 0 (inline) |
| array named properties | 1.5 | 12–194 (below the 256 sweep floor) |
| reference-cell identities | ~1 | 188–252 |
| peak `VmHWM` at 60k | 672 MiB | 19.5 MiB |

Earlier intermediate measurement (same harness, old base `d53c25bf`, before the cycle fixes): the
bounded tables alone were 11–38 % *slower* than the old leaking server, because with the cycles in
place nothing they held could die and their per-entry layout made rehashing costlier; the tables
were then compacted to one boxed slot per entry. This is why §10 is one change, not three.

### Acceptance (README) and gates

| Criterion | Result |
| --- | --- |
| RSS flat under the benchmark | yes (12 MiB across the read scenarios) |
| ≥ 10 % on scenario 1 or 2, none below −5 % | yes: +51 % / +35 %, smallest +31 % |
| staging vitest suite as generated Rust | 9/9 |
| `probe.sh` vs the generated server on `main` | byte-identical (30/30 requests) |
| examples SmeltUnknown avoidable | 0 (see the PR) |
| es-toolkit SmeltUnknown ratchet | 22,447 → 22,447 (+0); probe gate PASS (0 files with blockers) |
| remeda / es-toolkit / Hono suites | CI (`run-regressions`) |
| `cargo clippy --all-targets` | 0 errors; no warnings in the new modules |
| `cargo test --no-fail-fast` | all pass except the 11 bwrap/specializer tests, which fail on this machine with `bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted` |

Regression tests: `examples/typescript/end-to-end/166_closure_stored_on_its_receiver` (Node's
stdout; escapes through a returned field closure, a nested closure, a subclass, a self-emitting
listener); `crates/smelt-codegen-rust/tests/callable_object_identity_runtime.rs` (a swept address
is reused without reviving the dead entry; 100k erasures keep the table bounded and release every
captured closure; instances and emitters are freed while escaped closures keep theirs alive);
`src/tests/weak_receiver_tests.rs` (which closures qualify, and three that must not). Mutation
checks, each restored afterwards: disabling the weak captures fails both leak probes; disabling the
drop-time pin fails the escape probe; ignoring the guards fails "a sweep removed a live entry";
a single-pass sweep fails the bounded-table probe (205k entries).

### Verified vs assumed

Verified: the numbers above, by running the generated binaries; behavior against Node for the new
fixture; the probe and vitest results. Assumed / not checked: behavior of the weak-capture rule on
the remeda/radash/Hono suites until CI runs them; multi-core behavior (one pinned core, as before);
that the `SMELT_ARRAY_PROPS` known gap is unreachable in practice (it needs an element-type-changing
list conversion of an array the program wrote named properties to).

