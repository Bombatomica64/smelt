# Hono v5 — phase 4 (`claude/hono-v5-phase4`)

Probe bumped from `honojs/hono@eebdf7be39ab` (v4 line) to **`v5.0.0-rc.1` =
`0f5ad7b6bf0c5973a1bcad2b954ede5cc7e224a2`** (2026-10-10). Every number below
was measured locally on this branch: clean clone at the pin, `cp -R
.github/compat/hono/. <clone>`, `smelt build`, then `cargo test --no-fail-fast
-- --test-threads=4` on `dist-smelt`. Test names are compared modulo the
`_NNN` de-dup suffix and with module names mapped back to their source file
(the module numbering shifts between pins), by `/tmp`-local tooling described
in "Method".

## Step 0 — the v5 bump and its baseline

### What moved out of the denominator

v5 moved the runtime adapters out of core: `src/adapter/**` no longer exists
(they are the workspace packages `adapters/{aws-lambda,bun,cloudflare-workers,
deno,lambda-edge,netlify,service-worker,vercel}`), and `cloudflare-pages` is
gone entirely. `src/jsx/dom` is still in core; v5 adds `src/context/`
(`constants.ts`), `src/request/constants.ts` and `middleware/mount`.

| | old pin `eebdf7be` | v5 `0f5ad7b6` |
| --- | ---: | ---: |
| `*.test.ts` under `src/` | 101 | 84 |
| excluded by the overlay (globs expanded) | 73 | 64 |
| in scope | 28 | 20 |

The 19 `src/adapter/**` test files **leave the denominator; none of them was
fixed**: 10 were excluded (`aws-lambda/{conninfo,handler}`,
`bun/websocket`, `cloudflare-pages/handler`, `cloudflare-workers/{serve-static,
websocket}`, `deno/websocket`, `lambda-edge/{conninfo,handler}`,
`service-worker/handler`) and 9 were in scope and ran 18 tests
(`bun/{conninfo,server}`, `cloudflare-pages/conninfo`,
`cloudflare-workers/{conninfo,utils}`, `deno/conninfo`, `netlify/conninfo`,
`vercel/{conninfo,handler}`). New in v5: `helper/factory/define-handler.test.ts`
(excluded, `phase 3 pending`: a user-defined tagged template in the test, the
`asserted call callee must be a function` gap in `define-handler.ts`, and the
`jsx/**` runtime closure it reaches) and `middleware/mount/index.test.ts` (in
scope, 14 tests). The 12 scope-decision excludes were re-checked against v5
and all still apply (`vi.stubGlobal`/`vi.stubEnv`/`vi.spyOn(Date|crypto)`, the
`new Proxy` client, streaming/websocket/jwt).

Probing `adapters/*` as separate packages later is possible (they import
`hono` as a workspace package), but each is a thin host binding over a runtime
Smelt does not model (Bun/Deno/Workers/Lambda event shapes), so their tests
are mostly host-shape fixtures; low value until the host profiles exist.

### Baseline

| run | result |
| --- | --- |
| old pin, `main` binary (`c9ce53fb`), old overlay | **318 passed / 9 failed** of 327 (matches the CI probe report) |
| v5, `main` binary, overlay with the vanished entries dropped | **does not build**: `smelt build` stops in `src/hono-base.ts` — "Response init is an erased value" (0 tests) |
| v5, after the core fixes of family A (needed to get any number) | **290 passed / 42 failed** of 332 |
| v5, end of this phase (all families below, 5 files back in scope) | **364 passed / 90 failed** of 454 |

The core regression is v5's `Context` reshaping (below): `hono-base.ts` lowers
before `context.ts` in an import cycle, and v5 reads the response through
`c[GET_RESPONSE]`, a getter keyed by an imported `unique symbol`.

## Families fixed (general rules)

Each rule is stated with its behavioural and its type-preservation
requirement; the regression check named for it is the one that fails when the
rule is removed (see "Defect injection").

### A. Import cycles: a module lowered before the class it reads sees its declared surface

v5's `hono-base.ts` lowers before `context.ts` (`context.ts -> types.ts
(type-only) -> hono-base.ts -> context.ts`; the manifest order is a
depth-first walk, so whichever side of a cycle is entered first lowers first).
A cycle-early module only sees a class through its *predeclared* surface, which
held plain methods only. v5 moved `Context`'s API onto accessors —
`get text(): TextRespond`, `get set(): Set<E>`, `get req()`, and
`get [GET_RESPONSE](): Response | undefined` keyed by a `unique symbol`
imported from `context/constants.ts` — so every such read in `hono-base.ts`
erased to `SmeltUnknown`, and `new Response(res.body, res)` hit the
"Response init is an erased value" blocker.

* **A1 — unique-symbol identity is its module and offset.** A `Symbol()`
  spelling was `Symbol(desc)@<offset>`; two modules declaring
  `export const K: unique symbol = Symbol()` at the same offset were one member
  name (`get [KEY_A]` and `get [KEY_B]` collided). The site is now
  `file << 32 | offset` (module 0 keeps its bare offset, so single-file goldens
  do not move). Behaviour: Node prints `1 2 false`; so does Smelt.
* **A2 — module-level unique-symbol consts are predeclared crate-wide**
  (`lowering/symbol_consts.rs`), before any module body lowers, and a computed
  key spelled as an identifier that is a const or an *imported* const resolves
  through them (`identifier_computed_key_name`).
* **A3 — the predeclared class surface includes annotated instance getters and
  data properties** (`predeclared_class_member_fields`), keyed by the resolved
  member name and typed by the annotation. Callable members are left out of
  that table on purpose (a predeclared function-typed entry is how a
  cycle-early consumer recognises a *method*).
* **A4 — callable interfaces' call signatures are predeclared**
  (`predeclare_interface_call_signatures`, heritage-free interfaces only, after
  the module's own aliases), so `c.text('..')` through `get text():
  TextRespond` is a read-then-call instead of a method the class does not
  have ("class method `text` is not resolvable"). Because a predeclared
  signature can be *less precise* than the interface's own lowering (`headers?:
  HeaderRecord` names `BaseMime`, computed from a const object in
  `utils/mime.ts`), a call through an interface this module knows only by
  predeclaration calls the RECORD (`closure_call` on it), and the emitter
  invokes its `__smelt_call` slot with the slot's final signature, padding
  omitted optional arguments and honouring the borrowed-callback ABI.
* **A5 — an absent argument to a closure value is a bare `None`**
  (`closure_value_argument_text`): a `Rc<dyn Fn ..>` callee is never generic,
  so its own signature types the `Option`, as a hand port writes it.

Type preservation, measured on the 5-file regression
(`build_types_cycle_early_reads_of_a_pending_class`): `let label: String`,
`let done: bool`, `let res: Option<SmeltResponse>` — all three were
`SmeltUnknown` before — and the callable getter is read then called; stdout
`205 417` = Node.

### B. Branches and `try`s rejoin: their continuation is emitted once (Step 2's root cause)

The closure emitter rendered control flow as a tree, so a join block was
emitted once per incoming path, and the function emitter's throwing-call /
throwing-`await` terminators re-emitted the whole continuation in every arm of
their `match` (twice for the `catch` side: the `Ok(Err)` and the panic arm).
`n` sequential branches cost `2^n` copies, `n` sequential `try`s `3^n`. The
same duplication also produced v5 `compose`'s
`panic!("recursive closure control flow is not structured yet")`: a join
reached on a second path inside an already-duplicated subtree looked like a
cycle.

* **B1** — `emitter/post_dominance.rs`: immediate post-dominators of the MIR
  function (dataflow over the reverse CFG with a virtual exit; a call's unwind
  edge counts), cached per function.
* **B2** — closure `Switch`: when the branch's immediate post-dominator `J`
  lies inside the region being emitted, emit `if c { then..J } else { else..J }`
  and `J` once (`closure_branch_join`); arms use explicit `return`.
* **B3** — throwing call/`await`: the arms stop at the nearest common
  post-dominator of the normal and `catch` successors and the tail is emitted
  once after the `match` (`emitter/throwing_join.rs`), for top-level, region
  and closure continuations (loop continuations keep per-arm emission).

Size, measured: `utils/cookie.test.ts` (460 lines) — 616 MB / 9.5 M lines
before → **114 KB / 1,652 lines**; its transpile took >30 min (stopped) → 11.6 s.
The whole v5 crate went 19 MB → 9.8 MB of Rust. `compose.rs` 1,377 → 327 lines.
Regression: `tests/rejoin_tests.rs` asserts the shared tail is emitted once for
six sequential closure branches and five sequential `try`s, and that doubling
the branch count grows the output by less than 2.5x.

### C. `Promise.prototype.finally`

`promise.finally(cb)` had no lowering: `member_call` answered the unresolvable
method on a `Future` with a `None` literal, so `return dispatch(i + 1).finally(..)`
(v5 `compose`'s `next`) silently returned `undefined` and no downstream
middleware ran. `AsyncOp::Finally` runs `cb` with no argument on either
settlement and keeps the original outcome unless `cb` throws or returns a
rejected promise (ECMA-262). Node-checked: value kept (`42`), callback ran,
original rejection kept, `finally`'s own rejection wins.

### D. A `void` value erased to `SmeltUnknown` still evaluates

`value_at_type_text` / `erase_value` rendered a `None`-typed value as the
constant `SmeltUnknown::Null` and dropped the expression, so
`let r: unknown = setter(k, v)` deleted the call (`ctx.set(..)` inside a
middleware arrow never ran; the round-6 `compose` "stuck on next" test).

### E. `Promise.race` during an async function's eager prefix poll

`SmeltFuture::from_future_primed` polls an async body once at call time; a
`Promise.race` reached in that poll saw every racer `Pending` (they had only
yielded for priming) and advanced the virtual clock to the other racer's
timer. A race inside a function called from a `.then` callback therefore
always lost to its timeout. The race now yields while priming.

### F. Joins and absence

* A `null` consequent, or a `never`-returning function consequent, does not
  hint the alternate (`c ? null : d ? ['a'] : ['b']` lowered the arrays against
  `None`).
* `never` is the bottom of a function join — in the frontend ternary and in
  MIR's throwing-closure widening, which replaced a slot's return type with the
  stored throwing closure's (`rethrow: (e) => never`).
* `x || y` over an optional primitive is a truthiness test (`0 || 80` is `80`;
  presence-coalescing answered `0`). A frontend unit test that required the old
  `OptionalCoalesce` was corrected, with the Node evidence in the test.

### G. Modeled host values and records

* `smelt_stdlib::host_data_property` — one table for primitive data properties
  of modeled host classes (`Response.status/ok/statusText/bodyUsed`,
  `Request.url/method/bodyUsed`, `ReadableStream.locked`), used by the
  frontend's member typing and the emitter's accessor rendering, so optional
  chains keep concrete types (`res?.status: Option<f64>`). `locked` is the
  body's consumed state: the model has no reader API, so a stream is locked
  exactly when consumed (Node agrees for a consumed body).
* `SmeltRecord` implements `serde::Deserialize` (a struct deriving it with a
  record field did not compile).
* `push`/`unshift`/`splice` items share one element-typed lowering
  (`list_element_argument`).
* An object literal with spreads and no contextual type is the join of all its
  pieces; a source whose value type differs (including an `Optional` record)
  makes it the erased record instead of being REPLACED BY AN EMPTY RECORD —
  `{ path: '/', ...opt, secure: true }` used to drop `opt`.
* A tuple at a different tuple type converts element-wise in the operand
  ladder (`[n, accept] as [number, Accept]` returned as
  `[number, Accept | undefined]`).
* Bitwise/shift operands apply `ToNumber` to an erased operand (`as f64` on a
  `SmeltUnknown` was E0605).
* A record value whose type is none of a concrete union field's arms leaves the
  field absent in record→struct projection (`{ method: 'HEAD' }` into
  `RequestInit.headers`).
* `expect(record[key]).toBeUndefined()` / `toBeDefined()` test the key's
  presence, like `record[key] === undefined` already did.
* A `catch` parameter predeclared at function scope is assigned, not shadowed,
  in the throwing terminator's arms (needed once B3 moved the tail after the
  `match`).

### H. Tooling

`smelt check --message-format json` reports `line`/`column` (it reported only
the file, which is how `hono-base.ts:336` took an hour to find).

## Step 1 / Step 2 — the excludes, re-derived on v5

Derived mechanically on the v5 tree: `smelt check --message-format json` with
only the 12 scope-decision excludes, a relative-import closure map to attribute
each blocked module to the test files that reach it, then `smelt build`
(build-only blockers), `cargo check` and `cargo test --no-run` per candidate.

**Back in scope (5 files):** `middleware/cors`, `middleware/pretty-json`,
`middleware/trailing-slash` (was the 3 MB / OOM module), `preset/tiny`,
`utils/cookie.test.ts` (was the 616 MB module). Their tests run; failures among
them are real behavioural gaps, listed below.

**Still excluded with a named reason** (each comment in the overlay updated):

| file(s) | stage | blocker |
| --- | --- | --- |
| `helper/factory/define-handler` (new) | frontend | user-defined tagged template; `jsx/**` closure |
| `helper/proxy`, `middleware/method-override` | frontend | `Request` init erased |
| `middleware/basic-auth` | cargo | passes the frontend since the shared element-typed `unshift`; reaches `utils/basic-auth.ts` (evolving `let`, below) |
| `middleware/ip-restriction` | frontend | `Set.add` item at the set element type (the `splice` in `utils/ipaddr` now lowers) |
| `middleware/combine` | frontend | `unshift` inside a CALLBACK body has its own element check (`callbacks/body_lowering`), not the shared rule |
| `middleware/bearer-auth`, `middleware/method-not-allowed` | frontend | index on an `Optional` receiver: a captured `let` assigned in `if (!r) r = new X()` is not narrowed after it |
| `middleware/context-storage` | frontend | `node:async_hooks` host module |
| `middleware/secure-headers` | frontend | tuple vs list ternary arm in a `.map` callback |
| `utils/buffer` | emitter | `form_data.rs` literal-operand panic |
| `helper/route` | test compile | `toEqual` on `RouterRoute` (holds a function: no `PartialEq`) |
| `helper/cookie` (+ `middleware/language`) | test compile | `getCookie(c)` must select the `(c): Cookie` signature of an overloaded callable-interface `const`; the lifted item is typed by the first signature |
| `utils/accept` | runtime | an 800 KB header parsed with `charCodeAt(i)`: O(i) on the UTF-8 `String` runtime, quadratic, does not finish |
| `utils/basic-auth`, `utils/concurrent` | cargo | evolving `let x = undefined`; closure capture name + future nesting |
| `middleware/request-id` | cargo | module-body fn `request_id` vs exported `requestId` (E0428) |
| the rest of the round-1 list | frontend | unchanged families (`ReadableStream`, dynamic `import()`, `LabeledStatement`, `new URL(path, base)`, `jsx/**`, `Object.assign` with no source, test-shape gaps) |

`hono.test.ts` and `context.test.ts` were the stated priority; on v5 both
import a `new Proxy(..)` helper at module level, and `hono.test.ts` imports `hc`
from the excluded RPC client — both scope decisions — so they cannot come back
without a per-test isolation mechanism (not built this round).

## Result

Final run (binary at the tip of this branch, overlay as committed, clean
clone at `0f5ad7b6`): **364 passed / 90 failed of 454**, identical to the
previous run on the second-to-last binary.

Compared by name against the old pin (318 / 9 of 327 on `main`):

* **Previously passing, now failing: 1.**
  `middleware/timeout::no timeout should pass`: the error handler prints
  `null` where Node prints the error. The value is lost converting the
  rejection to `Error | HTTPResponseError`. I have not found the root cause.
* **Old tests absent in v5: 20.** 18 are `src/adapter/**` tests, which moved
  out of core. The other two are one `router/pattern-router` test and one
  `utils/url` test, both removed or renamed upstream.
* **New tests (146)**, passed / failed by file: `compose` 2/1, `middleware/cors`
  4/21, `middleware/mount` 0/14, `middleware/pretty-json` 1/11,
  `middleware/trailing-slash` 15/21, `preset/tiny` 2/0,
  `router/reg-exp-router` 8/0, `utils/cookie` 31/15.

Failures by file: cors 21, trailing-slash 21, cookie 15, mount 14,
pretty-json 11, timeout 4, powered-by 3, compose 1.

## SmeltUnknown

| corpus | before | after | |
| --- | ---: | ---: | --- |
| examples, avoidable | 0 | **0** | invariant holds (`--fail-on-regression` exit 0) |
| es-toolkit, avoidable | 25,424 (`main`) | **22,526** | ratchet; re-snapshotted, and `--fail-on-regression` on the final binary exits 0 (legitimate-boundary 35,592 → 32,167; runtime-prelude 2,806 → 2,811) |
| hono v5, avoidable (advisory) | 8,278 (`main` baseline, old pin, 63 files) | **19,578** (77 files) | re-snapshotted |

The rise in Hono is a change of SCOPE, not of lowering:

* **10,498** of the 19,578 come from the five files brought back into scope:
  `cors` test 7,412, `trailing-slash` 2,554, `pretty-json` 395, `cookie` 97,
  `tiny` 40. Most of the `cors` count comes from the 18 KB adapter setup that
  the test file replays per case.
* The files that were in scope before account for **9,080**. That includes v5's
  new `mount` test (766). The old pin's 8,278 is a different source tree, so
  the two numbers cannot be compared exactly.
* For a like-for-like check, I rebuilt the OLD pin with both `main` and this
  branch: avoidable went 7,648 → **7,521** (total 153,504 → 132,752; crate size
  27 MB → 17 MB).

## Defect injection

Each check below was confirmed to FAIL with the named defect in place, and
the implementation was restored afterwards:

* `rejoin_tests`: emitting the tail inside every arm (dropping B2/B3).
* `build_keeps_same_offset_unique_symbols_of_two_modules_distinct`:
  reverting the site to the bare offset (A1).
* `build_types_cycle_early_reads_of_a_pending_class`: removing the getter
  predeclaration (A3). The reads go back to `SmeltUnknown`.
* `never_returning_arm_does_not_type_a_function_join`: removing the `never`
  bottom from the join.
* `unhinted_spread_with_heterogeneous_values_keeps_its_sources`: restoring
  the empty-record replacement.

## Remaining, and why

* **mount (14)**: new in v5; every test fails. Not yet triaged.
* **cors / pretty-json / trailing-slash / cookie**: behavioural gaps, now
  visible because these files run at all. Not yet triaged per test.
* **timeout `null`** (the one lost test): see Result.
* **Excluded with a named reason** (table above): `charCodeAt` is O(i) on
  the UTF-8 string runtime (`utils/accept`); evolving `let x = undefined`
  (`utils/basic-auth`, `utils/concurrent`); an overloaded callable-interface
  const typed by its first signature (`helper/cookie`); `toEqual` on a record
  that holds a function (`helper/route`); a module-function name clash
  (`request-id`); `Set.add` element typing; an `unshift` in a callback body;
  narrowing of a captured `let` after `if (!r) r = ..`.
* `hono.test.ts` / `context.test.ts`: blocked by scope decisions (`new Proxy`
  client, `hc`), not by lowering.
* Also noted, not fixed: `Promise.race` over heterogeneous racer types;
  `String(undefined)` prints `null` in one erased path; the `cors` test
  is 6.6 MB because adapter setup is replayed per case (linear, not
  exponential).

## Verified vs assumed

Verified locally: `cargo clippy --all-targets` is clean. `cargo test`: every
failure is one of 11 specialization tests whose sandbox cannot start here
(`bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted`; Python
parity 7, `smelt-specialize` 3, one transpiler build test). The end-to-end
goldens match. The examples invariant holds. The Hono numbers above come
from a clean clone. The es-toolkit SmeltUnknown ratchet was run on the final
binary.

Not verified locally: es-toolkit's generated-crate `cargo check` and tests,
and the other compat probes (radash, remeda). CI runs those.

## Method

The scratch tooling lives in `/tmp` and is not committed:
* `closure.py` maps each test file to its relative-import closure.
* `derive.py` re-excludes the candidates whose closure reaches a module
  with a build blocker.
* `compare.py` normalizes test names to `<source file>::<name without
  _NNN>` (it reads each generated module's `// source:` header), so runs
  from different pins can be diffed.
