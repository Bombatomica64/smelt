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

## Batch 5 (2026-06-29) — partial (interrupted by a host reboot)

A 3-agent batch was launched (constructable-values, missing-stdlib, unsupported-lowering tail). A host reboot mid-run killed all three. Only the work that had been **committed** before the reboot survived; the rest was uncommitted and unvalidated, so it was discarded. Salvaged & landed on main (Remeda gate 1789/0, smelt suite 1316/0):
- **`9b393b93` — general closure free-variable capture for for-loops & updates**: capture-name collectors now traverse `for`/`do-while`/`switch`/update/sequence AST nodes, so a closure that refers to a variable mutated in those forms captures it correctly. Unblocks the `overEvery`/`overSome` family (and is a prerequisite for the curry/partial/bind family).
- **`4e573747` — frontend panic fix**: truthy guard over a parameter with a dependent default no longer panics during lowering.

Result: **files-with-blockers 200 → 192** (−8). unresolved-reference 22 → 17, unsupported-lowering 163 → 161, missing-stdlib unchanged at 21.
Cumulative this session: **419 → 192** (~54%).

**Lost to the reboot, needs a clean re-run from current main** (prerequisites now landed, so they redo cleanly):
- *missing-stdlib residual* — typed-array runtime identity, Buffer value model, `setInterval`/`clearInterval`.
- *unsupported-lowering tail* — string methods / control-flow / exported-const long tail + call-site generic instantiation (thread a callee's param function-type as a hint so "callback method not lowered" resolves).

## Batch 6 (2026-06-30) — 14-agent fan-out, landed on main

Largest batch so far: **10 feature agents + 4 improvement agents** in isolated worktrees, integrated serially with conflict resolution and gated once. All agents ran only targeted tests (the coordinator gated the full suite + clippy + Remeda + probe).

Feature work landed:
- **callback/closure-body lowering** (44→17 occ): compact-method failures retry through the full closure-body path (reaching the general method table), lodash two-arg `_.map(coll, fn)` form, runtime-selected/opaque callbacks, item-less forEach, param reassignment.
- **expr/operators**: IIFEs (`call expression` 7→0), `Number.toFixed`, bitwise `&`/`|`/`^` (JS ToInt32), `in`/`instanceof` in the no-hint binary path, conditional list-branch unification, Bool index access.
- **structural tail**: function-expression-valued exported consts (8→2), constant-foldable + imported switch-case labels, `String.replaceAll` (2→0).
- **class/module**: private fields `this.#x`, `this`-param types, bare `asserts`, interface-extends-non-interface, numeric property keys.
- **whole-crate build**: structural function-arity assignability (optional-param callback into a shorter slot) — advances the strict build past the entire `promise/` directory.
- **missing-stdlib**: `setInterval`/`clearInterval` on the virtual-time timer queue; `instanceof Boolean/String/Symbol` via boxed-wrapper markers (mirrors boxed `Number`).
- **globals**: fold the UMD `globalThis` detection chain, short-circuiting absent-alias clauses (Phase-2 `SmeltGlobalObject` machinery deliberately *not* built — checkpoint showed no es-toolkit case needs it).
- **vitest harness**: suite bodies may declare class/interface/type helpers; broadened title-folding; erased-callable `toThrow`/`toContain` adapters.
- **Python frontend** (separate crate): `del dict[key]`, unary + conditional lambda callbacks.

Improvement work: CLI probe-report readability + determinism (`smelt-cli`); `specs/codegen-quality-assessment.md` (idiomaticness roadmap — 23k clones / 54k temps); `specs/lowering-architecture-refactor-plan.md` (the `include!`→`mod` plan); additive regression tests. Consolidated into GitHub issue #38.

Two superseded commits dropped at integration (boxed-primitive `instanceof` fold → replaced by the marker version; a duplicate bitwise impl). Constructable function *values* (`new par()`) deferred again — genuinely cross-cutting closure-ABI work, unsafe to run concurrently with 9 other agents on the same files.

Result: **files-with-blockers 192 → 144** (−48). missing-stdlib 21→18, unresolved-reference 17→18, unsupported-lowering 161→114. Whole-crate build advanced `semaphore.ts` → `globalThis.ts` → **`_internal/DOMException.ts`**. Remeda gate 1789/0 (0 compile errors); smelt suite **1383/0**; clippy clean.
Cumulative this session: **419 → 144** (~66%).

## Batch 7 (2026-06-30) — agent fan-out interrupted; salvaged 4 of 6

A 6-agent fan-out was launched. The parent Claude Code process exited mid-run and **killed all six before any committed** — same failure mode as the batch-5 reboot. Each worktree held ~900 lines of uncommitted partial work. Recovery: WIP-committed all six to preserve them, then triaged (all compiled clean — the agents died right before their validate/commit step) and integrated on a staging branch with the full gate.

Landed (4 areas, squashed to one commit `3770cc7e`):
- **missing-stdlib** — `DOMException` modeled as a `__smelt_domexception` marker-record class + further missing builtin identifiers. The whole-crate build now advances **past DOMException** (`_internal/DOMException.ts` → `array/uniq.ts`).
- **member-access** — broaden field/member-access resolution beyond Record/class/interface receivers.
- **nested-function rest parameters** lowering.
- **control-flow / exported-const / timer tail** (switch case labels, `defer`, etc.).

Dropped, not landed:
- **callback-cluster** — its conditional-branch unification regressed `preserves_erased_date_values_when_retyping_unknown_callback_fields` (the same test a batch-6 agent had to revert). The agent died before its own self-check would have caught it; dropped at integration. Re-run cleanly later. (Including it would have reached 129; without it, 135.)
- **collections-arity** — user-killed mid-run while chasing a cross-module `empties`-type issue that is a probe-isolation artifact (see [[probe-lowers-files-in-isolation]]); discarded.

Golden `27_optional_chains/expected.rs` regenerated for the new `__smelt_domexception` marker (only diff). Remeda gate 1789/0 (0 compile errors); smelt suite green; clippy clean.

Result: **files-with-blockers 144 → 135** (−9). missing-stdlib 18→19, unresolved-reference 18→20, unsupported-lowering 114→102 (category upticks are downstream blockers surfacing as files advance).
Cumulative this session: **419 → 135** (~68%).

Note: agents this round ran targeted `cargo clippy -p <crate>` on their own crates, which eliminated the integration clippy debt that batch-6 incurred.

## Batch 8+ roadmap (current breakdown: 135 files = unsupported-lowering 102 · missing-stdlib 19 · unresolved-reference 20)

Priority order — biggest *general* semantic families first, never per-function special cases (per CLAUDE.md). Each item: fix generally in frontend/IR/emitter, add a focused compiler regression test, re-probe, keep the Remeda gate (1789/0) and smelt suite green, commit.

1. **Constructable function *values* (`new par()`) + prototype-chain runtime ABI** — the deferred deep one; unblocks curry/partial/bind/flow + `this instanceof <named-fn>` (lodash called-with-`new` detection). A function value needs a constructable identity + `.prototype` slot through the erased ABI. Best done as a *dedicated single-agent effort*, not in a wide fan-out (it touches the shared closure type/codegen/`new`/`instanceof`). NOTE: this collides with PR #37's HIR/MIR core — do it after #37 merges.
2. **Re-run the dropped callback-cluster** (unmodeled stdlib methods in callbacks — `localeCompare`/`apply` etc.; fn-expr array elements; if/else blocks) but WITHOUT the conditional-branch unification that regressed `preserves_erased_date_values` — that needs branch-casting in the conditional materializer first.
3. **typed-array runtime identity** (`isView`/`isTypedArray`, needs re-modeling typed arrays off bare `List<Float>`) + **Buffer value model** — concrete missing-stdlib models (marker pattern, never `SmeltUnknown` erasure). (DOMException — DONE in batch 7; whole-crate abort now at `array/uniq.ts`.)
4. **Conditional-branch type unification across List/Union** (needs `isArrayLike` guard narrowing) + **switch fallthrough/labels** + **negative-index** (intentionally rejected — leave) + the **unsupported-lowering long tail**. Triage by recurrence.
5. **Globals Phase 2/3** — runtime `SmeltGlobalObject` only if a real dynamic-global case appears (none yet); a **DOM profile** (`window`/`document`) for browser-targeting specs.
6. **Codegen-quality phase** — execute `specs/codegen-quality-assessment.md` (temp-inlining MIR pass → `.clone()`/`unused_mut` reduction → paren/cast printing) once feature churn settles.

When es-toolkit reaches a fully-green generated suite, add it as a **second CI regression gate** beside `remeda-regression` (the original goal at the top of this file).

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

## Compile campaign (2026-07-11) — branch `claude/estoolkit-compilation-29quxk`

Goal shifted from probe blockers (9 residual, all `compat/**`) to making the
emitted crate **compile**. Whole-crate `cargo check` errors: **184 → 37** over
six integration rounds (2 Opus agents per round, isolated worktrees, every
round gated on the full smelt suite + clippy + cross-language goldens).

Landed (all general rules, no special cases):
- **Emitter syntax family (26→0)**: shared-capture rewrites in binding
  positions (`let` patterns, struct field keys, closure params — now hygienic
  wrt closure shadowing via `closure_shadow_intervals`), cast parenthesization.
- **Async ABI**: return-hint unwraps one `Future` layer; non-throwing async fns
  `Ok(..)`-wrap returns.
- **Generics**: array-callback closures inherit enclosing type params (fixes
  `difference<T>` cascade erasure); `SmeltJsKeyEq` bound inference for map-key
  class generics; nested-union flattening for composed type aliases.
- **Flow typing**: optional-local narrowing after `x = x ?? d`; destructuring
  defaults unify to unions; RegExp members typed concretely.
- **Coercion seam**: concrete unions erased via `into_smelt_unknown()` at
  equality/field/dispatch boundaries; erased-call results adapted through the
  checked nullish boundary; optional/mixed relational ToNumber coercion.
- **Classes**: collection-field mutation lifts classes to reference
  representation; callable interfaces get a synthetic `__smelt_call` field with
  `.apply`/`.call`/`.bind` routed through it (construction dataflow for
  populating method fields deferred — runtime fidelity, not compile).

Remaining 37 (all root-caused in `blocker-logs/estk-compile-round8.md` and
agent handoff notes): E0308 20 (Math.max-spread reduction lowering, includes
branch-join narrowing, delay/trim/xorBy/template while-cond, union injection),
E0277 5 (future Default, serde bound), E0057 2 (Parameters<F>-over-union
arity), E0599 2, singletons E0107/E0282/E0283/E0381/E0425/E0609/E0689 +
cloneDeepWith borrowck lifetime.
