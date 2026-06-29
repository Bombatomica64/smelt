# es-toolkit compatibility — plan & baseline

Goal: take es-toolkit (toss/es-toolkit) to a fully-green generated Rust test
suite, like Remeda (1789/0), then add it as a second CI regression gate.

## Setup

- Source: `toss/es-toolkit` pinned at `e008a2818cd8d07469a5cc12ee0c02405d523e07`
  (also the daily-probe ref in `.github/compat/libraries.json`).
- Fixture: `.github/compat/es-toolkit/Smelt.toml` (roots `["src"]`, test-prefix
  over array/function/math/object/predicate/promise/string/util/error specs;
  the lodash-compat `src/compat/**` layer is intentionally out of scope).
- Local checkout: `third_party/es-toolkit` (untracked, like `third_party/remeda`).
- Probe: `smelt probe --manifest-path third_party/es-toolkit/Smelt.toml` (scans
  all files, does not abort). Baseline report: `blocker-logs/estk-baseline.md`.

## Baseline (2026-06-29)

- Whole-crate `smelt build` **aborts** at `src/array/at.spec.ts` (strict mode
  stops at the first blocker — `data.at(i)` called inside a `.map()` callback
  body: "callback method `at` is not lowered into closure bodies yet").
- Probe: **1219 files scanned, 419 with blockers.**
- Categories: unsupported-lowering 317 · missing-stdlib 79 ·
  unresolved-reference 35 · internal 1.

## Batch 1 (2026-06-29) — landed on main (6 parallel agents)

Files with blockers **419 → 333** (−86). unsupported-lowering 317 → 226; missing-stdlib 79 → 67. (unresolved-reference rose 35 → 51 — newly-surfaced downstream `compat/**` function-as-constructor cases, out of scope.) Whole-crate `smelt build` abort moved `array/at.spec.ts` → `array/chunk.spec.ts`. Remeda gate stayed 1789/0; smelt suite 1246/0. Fresh report: `blocker-logs/estk-after-batch1.md`.

Landed: named-local array callbacks 54→0 · object-value rest params 31→0 · `arguments` in non-arrow fn expressions 28→0 · array methods in callback bodies 12→9 · bare `Array(n)` call 57→41 · `new Object()`→concrete record (struct-backed builtins ArrayBuffer/AbortController/Number/Blob/Proxy left as honest `missing-stdlib` blockers, NOT erased).

## Batch 2 (2026-06-29) — stdlib + globals, landed on main

Concrete stdlib models (mirroring the Date/Error marker pattern — distinct
identity markers, never generic `SmeltUnknown` erasure):
- **ArrayBuffer** — `new ArrayBuffer(n)` + `instanceof` via `__smelt_arraybuffer`.
- **Blob, boxed Number, Proxy** — Blob/Number markers (`isNumber(new Number())`
  stays false); Proxy lowered to its transparent `target` (not faked into `instanceof`).
- **AbortController/AbortSignal** — full model (shared `aborted` flag, `abort()`,
  `addEventListener` listeners fire) for debounce/throttle.
- **builtins-as-values** — `Number`/`String`/`Boolean`/`parseInt`/`parseFloat`/
  `isNaN`/`isFinite` referenced as bare values lower to concrete `Rc<dyn Fn>` closures.
- **globals Phase 1** — registry-derived presence (can't drift from the stdlib
  registry), `ambient_globals` module, compile-time erasure of `typeof`/`in`
  global probes + namespace normalization (`globalThis.Object.keys` → `Object.keys`),
  conservative shadowing denylist. Phase 2/3 (runtime `SmeltGlobalObject`)
  deliberately deferred per the checkpoint — es-toolkit's residual global use
  (escaping global identity, `globalThis.Buffer`, dynamic computed access) needs
  it, but those depend on other unmodeled features; build it against real blockers.

Result: **missing-stdlib 63 → 49**, files-with-blockers 331 → 324 (the
unsupported-lowering uptick is downstream blockers surfacing as files advance).
Remeda gate 1789/0; smelt suite 1278/0. Still-unmodeled honest blockers:
SharedArrayBuffer, DOMException, Buffer-as-value, `Math.PI`/`Reflect.ownKeys`
member-access, and the deferred ambient globals (globalThis runtime object).

## Batch 3 (2026-06-29) — callbacks + vitest, landed on main

- **Callbacks/closures**: numeric-index truthy conditions 27→0; `try/catch`+loop
  callback bodies 10→0 (routed through the closure-body fallback); erased
  named-local callbacks 7→1. Left as documented blockers: "callback method not
  lowered" (needs call-site generic instantiation to thread a callee's param
  function-type as a hint), lodash two-arg `_.map(coll, cb)` utilities (separate
  lowering), arrow-of-callables / divergent-list-type unification (low-value).
- **Vitest harness**: `.rejects.toThrow`/`.resolves` Promise matchers 11→0
  (reuse `smelt_await_flatten`), `toHaveProperty` 4→0, `mockReturnValue` args 2→0,
  `describe` loop-unrolling + template-literal test names. Residual describe/
  computed-name cases are all in out-of-scope `compat/**`.

Result: **files-with-blockers 324 → 281** (−43), unsupported-lowering 235 → 187.
Remeda gate 1789/0; smelt suite 1289/0. Cumulative this session: **419 → 281**.

## Batch 4 (2026-06-29) — full-category sweep, landed on main

Three category-owning agents (compat now in scope, no shortcuts):
- **unresolved-reference 52 → 22**: implemented the general **function-as-constructor + `.prototype`** feature (synthesize `Item::Class` from `function Foo(){}` + `Foo.prototype.x=…`) + a default-param scoping fix. Residual 20-ish are deeper (constructable function *values* `new par()`, closure free-variable capture, dynamic `new Ctor()`, wholesale prototype reassignment).
- **missing-stdlib 53 → 21**: modeled WeakMap/WeakSet/DataView/SharedArrayBuffer/File markers, `typeof` presence guards, `Reflect.ownKeys`, `Math.*` constant folds, bare namespace objects + bare `globalThis`/`global`/`self` (per-read marker host-record, dynamic global access kept as honest blockers). Residual: typed-array runtime identity, Buffer value model, `setInterval`, constructor-as-value + vitest-matcher, DOM `window`/`document`.
- **unsupported-lowering 235 → 163**: multi-arg concat/push, relaxed `Array(n)` length, `new Set(iterable)`, `typeof` in generic unary, `Object.hasOwn` receivers, string-method receiver/arg coercion, `Array.from` optional length, foldable `Number`/`Math` const members. (Agent was killed mid-stream; its 5 commits + the salvaged in-progress work were integrated.)

Integration fix: host-builtin marker objects now report **no enumerable own keys** (`smelt_is_for_in_*`), matching JS — caught a Remeda `isEmptyish(new WeakMap())` regression at the gate.

Result: **files-with-blockers 281 → 200** (−81). Remeda gate 1789/0; smelt suite 1312/0.
Cumulative this session: **419 → 200** (−219, ~52%).

## Batch 5+ roadmap (the deeper remaining features)
- Constructable function *values* (`new par()`) + closure free-variable capture (unblocks the curry/partial/bind family).
- Typed-array runtime identity (`isView`/`isTypedArray`), Buffer value model, `setInterval`/`clearInterval` async runtime.
- Globals Phase 2/3 runtime object (`SmeltGlobalObject`) for dynamic global access; a DOM profile for `window`/`document`.
- The remaining ~163 unsupported-lowering long tail (string methods, control flow, exported-const, misc).

## Batch 4 roadmap (remaining non-stdlib lowering)

1. **46 — unresolved identifier** (missing-stdlib): builtins-used-as-*values* (`Number`, `Math`, `parseInt`, `globalThis`…) — needs a general bare-builtin-value lowering path.
2. **38 — unresolved class** (unresolved-reference): `compat/**` function-as-constructor + `X.prototype.y=…` idiom (see `blocker-logs/plan-class-prototype-*.md`); out of the test-prefix scope.
3. **26 — callback conditions must be boolean/optional/supported-truthy**.
4. **15 — exported const values support only primitive literals / foldable**.
5. **12 — unresolved class** (missing-stdlib): the struct-backed builtins (ArrayBuffer/AbortController/boxed Number/Blob) — each a self-contained RegExp-style runtime model.
6. **10 — callback block statements** (non-const/if/return/throw) · **10 — callback method not lowered** (remaining erased-callback-param family) · **9 — typeof in callbacks** · **8 — instanceof on non-lowered class**.

## Prioritized roadmap (biggest general blocker classes first)

Work the smelt-debug-workflow loop: pick a repeated *semantic family* (never a
per-function special case), fix it generally in frontend/IR/emitter, add a
focused compiler regression test, re-probe, confirm no regression in the Remeda
gate (1789) or the smelt unit suite, commit, repeat.

1. **57 — unresolved identifier `X`** (missing-stdlib): many distinct missing
   stdlib functions; triage which recur and map them. Not one fix.
2. **54 — array callback local callback `X` is not in scope**: array methods
   (map/filter/...) whose callback is a *named local* (not an inline arrow); the
   local isn't captured into the lowered closure body.
3. **31 — function expression rest parameters not lowered in object values**:
   `{ key: function (...args) {} }` / rest params in object-valued functions.
4. **29 — unresolved class `X`** + **13 missing-stdlib unresolved class**.
5. **28 — `X` is only available inside function bodies**.
6. **17 — callback conditions must be boolean/optional/supported-truthy**.
7. **15 — exported const values support only primitive literals / foldable**.
8. **12 — callback method `X` not lowered into closure bodies** (the abort
   class: array methods like `.at()` used inside a callback body).
9. Long tail: typeof unary in callbacks (9), instanceof on non-lowered class (8),
   `rejects.toThrow` actual must be Promise (8), concat single-arg (6), switch
   fallthrough/labels, replaceAll, new Map/Set shapes, etc.

## Notes

- Regenerate/measure with `smelt probe`; track progress against this baseline.
- Every codegen-affecting change must keep the Remeda gate green
  (`ci.yml` → `remeda-regression`, currently 1789/0) and the smelt suite (1230).
