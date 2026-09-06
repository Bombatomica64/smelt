# Hono campaign: transpile the framework and its tests

Owner: Hono implementer (Opus). Architecture: Fable. Date: 2026-09-06.
Runs in parallel with `blocker-logs/standards-tier-plan.md` (the "standards stream").

---

## PROGRESS LOG — rounds 6 and 7

Probe on this head: **258 files / 3 with blockers / 4 occurrences**, unchanged
across both rounds. Two of the four things fixed in these rounds produced NO
blocker at all — they were silent wrong values — which is why the probe count
does not move and why the count alone is the wrong measure of this phase.

### Landed

* **Panic route keeps the throw's class** (round 6 item (a); `hono-fallible-ops.md` §10).
* **Interface-literal keys match by source spelling.** `{ camelCase: "a" }`
  against `interface Shape { camelCase?: string }` emitted
  `Shape { camel_case: None }` — every camelCase field dropped, `undefined`
  where Node prints the value.
* **Nested optional-interface literals build directly**, which is what took the
  examples invariant back to 0 by construction after the fixture above.
* **Module-scope reassignments are kept.** `let n: number | undefined; n = 5`
  at module scope discarded the write and still read `undefined`; so did
  `let m: number = 0; m = 6`. Any type, no diagnostic.
* **A contextual type reaches an IIFE's return position** (round 7 item 1).
* **A computed method call over a known member set lowers as a choice**
  (round 7 item 2). `return req[key]()` emitted `return String::new()`.

### Open families, numbered

| # | shape | state |
| --- | --- | --- |
| H13 | `dynamic computed method names are not lowered yet` — `request.ts:385`, `get [GET_MATCH_RESULT]()`, a computed class GETTER keyed by an imported `unique symbol` const. Minimal repro: `const KEY: unique symbol = Symbol(); class C { get [KEY](): number { return 1 } }`. | open, mine. This is the request.ts blocker; it is NOT `req[cacheKey]()`, which never produced a blocker and is now fixed. |
| H14 | `type table does not contain literal operand type Unknown` (`emitter/types.rs:823`, reached from the index/field fallback `_ => self.type_id(Type::Unknown)`). The fallback asks for a type the crate may not have interned, because a crate with no erased value has no `Unknown` entry. | open, mine, not currently reachable from Hono. Not "small": the obvious fix — always interning `Unknown` — would flip `needs_unknown_type` for every crate and emit the whole erased prelude unconditionally. The fix has to degrade at the fallback site instead, and needs a reproduction to pin which degraded answer is right. |
| H15 | Generated-union generic member: `enum SmeltUnionN<T>` declaring an unused `T` and substituting `ResponseInit<SmeltUnknown>` where MIR has `ResponseInit<Float>` (round 7 item 3). | **blocked on the repro note.** `blocker-logs/generated-union-generic-member.md` does not exist in the repository (see below). The described symptom does not reproduce on a plain generic interface in a union: `number \| Init<number> \| Holder` emits `enum SmeltUnion3 { M0(f64), M1(Init<f64>), M2(Holder) }` — instantiated, no unused parameter. |

### The three repro notes never arrived

`blocker-logs/interface-literal-camel-case.md`, `blocker-logs/find-callback-repro.md`
and `blocker-logs/generated-union-generic-member.md` are not in the repository.
`.gitignore` ignores `blocker-logs/*.md` by default and allowlists each readable
note by name, so these three were never committed and reached neither merge.
They are allowlisted now.

Consequences: the camelCase defect was reproduced from the coordinator's
one-line description and fixed; the `find` callback (item 2 of the round-6
dispatch) could NOT be reproduced — a typed arrow parameter through `find`
lowers correctly today in every shape tried (`(row: Row): boolean`,
`(row: Row) => row.name` with truthiness, an `unknown[]` receiver with a typed
parameter, and an `unknown`-typed field as the predicate result), so it needs
the note's exact source; and H15 is blocked for the same reason.

### Two site attributions corrected

* `request.ts` — the `dynamic computed method names` blocker is the symbol-keyed
  getter at line 385, not `await req[cacheKey]()` at 485. The latter emitted no
  diagnostic; it emitted a wrong value.
* `hono-base.ts` — the `Response init is an erased value` blocker is
  `new Response(null, await this.#dispatch(..))` at line 417, a `Response`
  passed as a `ResponseInit`. It is not the `replaceRequest` IIFE at 365-371,
  which now lowers with a typed `SmeltRequest` parameter.

---

## DEFERRED

Work that is designed, justified and deliberately not landed. Each entry names
what it would fix that the landed floor does not.

### D1. Callback `may_throw` inference (`hono-fallible-ops.md` §9.4, §10.5)

A callback parameter's fallibility is not spellable in TypeScript, so it must be
inferred whole-crate: the join of the fallibility of everything passed for that
parameter, recorded in a side table keyed by `(ItemId, param index)` and never in
the interned `Type::Function`. Design is complete in §9.4.

**Round 6 landed the floor instead** (§10): the panic route now carries a `Send`
class-plus-message payload, so a panic-routed throw keeps its class.

**Justification for still doing D1 — identity, abort strategy, and noise, not
reachability.** Round 5's stated reason was that Hono's `tryDecode` catch was
unreachable; §9.1 proved that false and it is withdrawn. What remains:

1. *Identity beyond a class and a message.* `panic_any` needs `Send` and a
   `SmeltUnknown` holds `Rc`, so a thrown class instance's custom fields,
   `cause`, and prototype identity still do not cross the unwind.
2. *Abort strategy.* `panic = "abort"` silently converts every catchable
   JavaScript exception into a process abort. Round 6 can only pin the manifest
   against it; only D1 removes the dependency where the callee is known.
3. *Noise and cost.* The hook silences the report, but a silenced panic is still
   an unwind per caught error, on ordinary input, in a request path.

D1 helps only where the argument is statically resolvable, so the round-6 floor
stays: an erased callback out of a data structure will always take the panic
route and must keep its identity there.

---

## PROGRESS LOG — round 3

Probe: **258 files / 6 with blockers / 8 occurrences / 6 shapes**
    -> **258 files / 5 with blockers / 7 occurrences / 4 shapes**,
and **every one of the 7 is standards-stream**. Nothing in Hono is left for
this stream until `Request`/`Response` land.

| item | status |
| --- | --- |
| 1. H6 via `Place::Global` | **landed.** One new `Place` variant naming the cell as the assignment root, so a write through a module-level mutable global mutates inside the cell with no copy. The compiler enumerated 17 sites in `smelt-mir` and 15 in `smelt-codegen-rust`; each got a decision, not a `_` arm. 7 runtime fixtures including the `RefCell` double-borrow shape. `blocker-logs/hono-h6-place-global.md`. |
| 2. fallible decoders | **not started.** Design still stands in `hono-fallible-ops.md`. |
| 3. masked `hono-base.ts` blockers | **landed**, and it was two, not one: a stub default for a `never`/union/`Set`/tuple return type (with the fallthrough now naming the type), and `let x;` inside an inlined callback defaulting to `undefined` as JavaScript and MIR's own `HirStmt::Let` lowering already say. `hono-base.ts` is clean. |
| 4. two pre-existing bugs | **(b) reproduced exactly, not fixed; (a) does not reproduce as described.** `blocker-logs/hono-round3-item4-findings.md` carries the source I ran and the emitted Rust for both. (a) needs the original repro before anything is changed. |
| 5. re-probe per merge | done above. |

### Method note carried from the round-2 correction

The radash "regression" I reported in round 2 did not reproduce; it came from a
gate run observed while the disk was full and the generated crate was stale. The
rule adopted for this round, and followed: **regenerate from clean and rebuild
before attributing a gate failure to anyone, or say "unverified" and stop.**
Item 4(a) above is that rule being applied — the reproduction attempt is
recorded, the conclusion is "not reproduced with this source", and no fix was
made on the strength of a description.

---

## PROGRESS LOG — round 2

Probe on the merged head: **286 files / 10 with blockers / 18 occurrences / 7
shapes** -> **258 files / 6 with blockers / 8 occurrences / 6 shapes**.

The file count drops because closure pruning genuinely removes the excluded
modules from the crate (286 -> 258), which is the point of item 1.

| item | status |
| --- | --- |
| 1. `exclude` prunes the closure | **landed.** The dependency collector filters every resolved edge, records the specifier when a module is wholly excluded, and the frontend reports `` `hc` is imported from `../../client`, which the manifest excludes `` at the first *value* use. Type-only imports and `export type` re-exports stay free. 2 integration tests + 2 collector unit tests. This alone took 18 -> 8 occurrences. |
| 2. absent globals throw | **half landed.** A call to (or read of) a global the profile declares absent lowers to a thrown `ReferenceError`, catchable by the enclosing `try`. `NON_DOM_ABSENT_GLOBALS` gains the three `EventTarget` names. The fallible-rvalue rewire (`decodeURI`, `decodeURIComponent`, `atob`) is **designed, not landed** — `blocker-logs/hono-fallible-ops.md`. |
| 3. H6 write-through | **not landed.** Probing the backend first changed the design; see `hono-h6-module-mutable-globals.md` §8. The blocker stays specific, so there is still no silent lost write. |
| 4. ratchet classifier | left alone as instructed; es-toolkit stays byte-identical. |
| 5. streams/websocket excludes | **landed**, each with its reason, now that item 1 makes closure pruning real. Recorded as future standards work in `hono-fetch-demand.md`. |

### The one correction to carry forward

Round 1 claimed `decodeURI`'s `.expect(...)` shared a root cause with
`JSON.parse`. **That was wrong.** `JSON.parse` already lowers through
`Terminator::Call { Callee::Builtin(BuiltinFn::JsonParse), unwind }` and is the
*reference implementation* for fallible operations, not a fellow victim. The
stale claim was also written into a code comment in
`crates/smelt-codegen-rust/src/emitter/strings.rs`, which would have sent the
next reader to fix something already correct; the correction is in
`hono-fallible-ops.md` §1 and the comment is fixed.

### Remaining 8 occurrences

| # | shape | file | owner |
| ---: | --- | --- | --- |
| 2 | unresolved class | `context.ts` | standards |
| 2 | `JSON.stringify` of `BodyInit` | `request.ts` | standards |
| 1 | field access on `Float` receiver (`.status`) | `context.ts` | standards |
| 1 | module-level function return default | `hono-base.ts` | newly revealed, unattributed |
| 1 | H6 write-through | `router/reg-exp-router/router.ts` | item 3 |
| 1 | string search needs string receiver | `utils/url.ts` | standards |

6 of the 8 are standards-stream. The `hono-base.ts` one appeared when the
`addEventListener` blocker in the same file stopped firing — it was masked
behind it, and an isolated fixture of the absent-global shape lowers with zero
diagnostics, so it is not caused by that change.

---

## PROGRESS LOG — round 1

Probe: **288 files / 14 with blockers / 32 occurrences / 13 shapes**
    -> **286 files / 10 with blockers / 18 occurrences / 7 shapes**
(current state in `blocker-logs/hono-current.md`).

| family | status | note |
| --- | --- | --- |
| H1 call expression not lowered | **done**, 4 -> 0 | ES private-name method calls, plus private reads in argument position. `hono-h1-private-method-calls.md` |
| H2 regex replacement callback | **done**, 4 -> 0 | the full ECMA-262 replacer argument list. `hono-h2-regex-replacer-arguments.md` |
| H3 tuple element intersection | **done**, 2 -> 0 | every `TSType` in tuple position, not just intersections. `hono-h3-tuple-element-types.md` |
| H4 condition over a union | **done**, 1 -> 0 | a union of objects is constantly truthy. `hono-h4-union-truthiness.md` |
| H5 `JSON.stringify` | **not this stream** | `BodyInit`, an unresolved fetch alias. `hono-h5-h8-h9-not-mine.md` |
| H6 module-level mutable global | **partial**, 1 -> 1 | non-literal initializers and non-primitive types landed; a write THROUGH the binding is a new named blocker rather than a silent data loss. `hono-h6-module-mutable-globals.md` |
| H7 exported const unresolved | **done**, 1 -> 0 | folded into H10; an exported const may alias a global. |
| H8 rest parameter type | **not fixable as designed** | `src/client/**` cannot be excluded by the current mechanism. `hono-scope.md` §2 |
| H9 string receiver | **not this stream** | `Request.url` must be `string`; the other site is `client/**`. |
| H10 unresolved identifier/class | **mostly done**, 9 -> 5 | the URI transcoding globals landed; `btoa`/`atob` and `addEventListener` remain, both needing a throwing mechanism. `hono-h10-uri-and-base64-globals.md` |
| H11 (new) `String` field reads | **done** | text and type were decided separately, so `${s.length}` in a callback did not compile. `hono-h11-string-field-read.md` |
| H12 (new) const alias callee | **done** | `const alias = fn; alias(x)` compiled and answered a default. `hono-h12-const-alias-callee.md` |

Phases: **1 not reachable from this stream** (8 of the 18 remaining occurrences
are standards-stream names), **2 blocked** on phase 1 (the whole-crate build
aborts at the first file needing one), **3 not started** (waits for the
standards stream), **4 blocked** on phase 2 (no generated crate to measure),
**5 done** (advisory `hono-advisory` job in `ci.yml`).

Scope: `hono-scope.md`. Demand on the standards stream: `hono-fetch-demand.md`.

---

## 1. Why Hono

Express is JavaScript and can only ever be a mapped host library. Hono is pure TypeScript with
zero runtime dependencies; its only external imports are Node builtins and web standards. It is
the corpus that proves Smelt can transpile a *framework*, not only utilities: routers (trie,
regexp, pattern), middleware composition, a context object, cookies, validators, and 2081
vitest cases that drive it with real `Request` objects (374 `new Request(`, 964
`app.request(`). It joins es-toolkit/remeda/radash as a gate.

Pinned ref: `honojs/hono` @ `eebdf7be39abf0a872671835ccce0c4f03ea497a` (v4.13.7).
Baseline probe on 2026-09-06 (core `src/`, entry `src/index.ts`): 288 files scanned, 14 with
blockers, 32 occurrences, 13 distinct shapes. Whole-crate build aborts at
`src/http-exception.ts`.

## 2. Setup

```bash
git clone --no-tags --filter=blob:none https://github.com/honojs/hono.git third_party/hono
git -C third_party/hono checkout eebdf7be39abf0a872671835ccce0c4f03ea497a
```
Create `.github/compat/hono/Smelt.toml` (copied over the checkout like the other gates):
roots `["src"]`, entry `src/index.ts`, `test-prefix = ["src/**/*.test.ts"]`, crate
`hono_probe`, output `./dist-smelt`. Add `third_party/hono/` to `.gitignore` and `hono` to
`.github/compat/libraries.json` (lang ts, roots `["src"]`) so the daily probe tracks it.

## 3. Scope: include by evidence, exclude with a reason

Start from the whole `src/` and exclude only what a probe or build shows cannot be in the
non-DOM Node profile, each with a one-line justification in `Smelt.toml` exactly like
`.github/compat/es-toolkit/Smelt.toml`. Expected excludes, to be confirmed by probing rather
than assumed:

- `src/jsx/**` and `src/helper/{html,css,ssg,jsx-renderer}` JSX/DOM surfaces (`.tsx`, DOM).
- `src/adapter/**` except none: Cloudflare/Deno/Bun/Lambda/Vercel/Netlify host globals
  (`Deno`, `Bun`, `caches`, `env`) are not in the profile. Keep `src/adapter` out unless a
  file is plain TS.
- `src/client/**`: RPC client built on `new Proxy` (a Smelt non-goal).
- `src/middleware/{jwt,jwk}` and `src/utils/jwt/**`: `crypto.subtle.importKey/sign/verify`,
  out of scope for the standards stream this round.
- `src/helper/websocket`, `src/helper/streaming` if they need `WebSocket`/unbounded streams.
- Tests using `vi.stubGlobal` (12) / `vi.useFakeTimers` (4) that cannot run under the Rust
  `vi` model: list them individually, do not exclude whole files for one case.

Everything else (hono.ts, hono-base.ts, context.ts, request.ts, compose.ts, router/**,
utils/**, validator/**, the remaining middleware, helper/{cookie,factory,route,testing,
accepts,conninfo,proxy,dev}) is in scope.

## 4. Blocker families (all general rules; no Hono spelling anywhere)

Investigate each, write the finding into `blocker-logs/hono-<family>.md` (wrong output,
responsible function, design) before fixing, in this order:

| # | shape (occurrences, example file) | direction |
| --- | --- | --- |
| H1 | `call expression is not lowered yet` (4, `context.ts`) | identify the callee shapes; likely optional-call `fn?.()`, calls on getters, or `super.method()` chains. General lowering. |
| H2 | `regex replacement callback must accept a match string and return a string` (4, `router/reg-exp-router/prepared-router.ts`) | replacement callbacks `(match, p1, p2, ...) => string` with capture-group params and `offset`/`string` trailing params, per ECMA-262 `GetSubstitution` callback signature. Extend the existing `js_regex.rs` substitution path; the callback receives captures as `Option<String>` for unmatched groups. |
| H3 | `tuple element type is not lowered yet: TSIntersectionType` (2, `types.ts`) | tuple elements are ordinary types; route through the same intersection lowering the es-toolkit campaign added (structural intersection). |
| H4 | `condition expression must be boolean or optional (Union)` (1, `matcher.ts`) | ToBoolean on unions of truthy-testable arms; the campaign's `ValueTruthy` covers primitives, extend to unions whose every arm has a truthiness rule. |
| H5 | `JSON.stringify() value must be JSON-serializable` (2, `request.ts`) | value is a typed union/record with an `unknown` member; serialization of `unknown` is a real dynamic boundary and already has a runtime path, route unions through it. |
| H6 | `module-level mutable binding initializer must be a literal for now` (1, `reg-exp-router/router.ts`) | lazily initialised module state (`let x: T = expr`): lower to `thread_local!` + `RefCell` initialised by the expression, the same shape module `const` non-literals use. |
| H7 | `exported const expression references unresolved const` (1, `utils/url.ts`) | cross-module const folding order; resolve through the crate item like the import-time literal folding added for es-toolkit (`alias_imported_item`). |
| H8 | `rest parameter type must resolve to an array type` (1, `client/types.ts`) | `...args: Parameters<F>`-style; likely excluded with `client/`, confirm. |
| H9 | `string replace/search requires string receiver` (2, `client/utils.ts`, `utils/url.ts`) | receiver is `string | undefined` or a template-literal type; narrow/normalize before the string rule, never fall back to Unknown. |
| H10 | `unresolved identifier`/`unresolved class` (9, `client/client.ts`, `utils/cookie.ts`) | in `cookie.ts` it is likely `decodeURIComponent_`-style helpers or `Date`; in `client/` it is `Proxy`/`fetch` typing. Cookie must lower; client is excluded. |

New blockers will surface as excludes are removed and as the standards stream lands concrete
`Request`/`Response`/`Headers` types (members Hono uses that are not implemented become
compile errors in the generated crate). Treat each as a new numbered family in the plan's
progress log; report members you need from the standards stream instead of modelling them.

## 5. Phases and metrics

1. **Source lowering**: `smelt probe` on the fixture reports 0 files with blockers for the
   in-scope set. Metric: files with blockers 14 -> 0.
2. **Generated crate compiles**: `cargo check` of `dist-smelt` is clean for the non-test build
   (`test-prefix` off). Anything here that is a missing fetch-type member is demand for the
   standards stream: list it in `blocker-logs/hono-fetch-demand.md` with counts; do not
   implement it.
3. **Tests**: after the standards stream merges (the orchestrator will tell you), turn
   `test-prefix` on and run `smelt rust-test-report --full` against the crate. Metric: passed /
   failed / newly failing, baseline into `blocker-logs/hono-current.md`. 2081 cases is the
   ceiling; report the honest in-scope number.
4. **Erasure**: `smelt smelt-unknown-report dist-smelt/src --format json --output blocker-logs/smelt-unknown-baseline-hono.json`
   as an advisory baseline (remeda-style) and a paragraph in the report about the top avoidable
   shapes.
5. **CI**: add a `hono` regression job to `.github/workflows/ci.yml` mirroring the radash job
   (clone at the pin, copy `.github/compat/hono/.`, build, `cargo test --no-fail-fast` the
   generated crate) under the `run-regressions` label, marked advisory (`continue-on-error`)
   until phase 3 has a stable number.

## 6. Contract with the standards stream

You never model `Headers`, `Request`, `Response`, `URL`, `URLSearchParams`, `FormData`,
`Blob`, `File`, `TextEncoder`, `TextDecoder`, `ReadableStream`, `AbortController`,
`AbortSignal`, `crypto`, or anything from `node:http`. Those names are owned by
`standards-tier-plan.md`. You may read their current (marker/erased) behaviour to keep
lowering, and you record demand. Everything in section 4 is yours and must be a general rule
that would also fire for es-toolkit/remeda/radash source of the same shape; those three gates
stay green.
