# Probe report: hono

- Transpile: **yes** — Rust crate emitted
- Generated `cargo test`: not run (pass `--run-tests`)
- Files scanned: 258 · with blockers: 0
- CI: `.github/workflows/ci.yml`'s `hono` job now hard-gates `smelt build` + `cargo check` on `dist-smelt` (mirroring radash); `cargo test` and the SmeltUnknown erasure report stay advisory until the phase-3 test baseline is stable.


## Phase 3 round 2, defaults / callable-interface fields / `Context` collision (Agent S, `worktree-agent-aa7111d6443760ca8`)

Merged head `80c6545c` + this branch. The E0107 family was the `Context`/`Context_1` symbol
problem, not missing defaults: an ambiguous imported name now resolves through its declaring
module in the predeclaration pass, through barrel re-exports and at `new`. `Context.set: Set<E>`
is callable (a module-declared interface/class/enum shadows the library `Set`), arrows stored into
callable-interface fields keep their body (was a silent no-op default), and erased → record-class
extractions are one shared `__smelt_from_record_<Type>` helper per class (the correctly typed
`Context` otherwise inlined ~230 KB per erased middleware site). Details:
`hono-phase3-round2-generics.md`.

| gate | result |
| --- | --- |
| overlay | 9 test files re-admitted (`compose`, six `conninfo`, `bun/server`, `helper/route`); 7 moved to their next family |
| `smelt build` | passes — **82 modules, 234 `#[test]`** in 19 test modules |
| `cargo check` (no tests) | **0 errors**, 376 warnings, 98 s |
| `cargo build` | links (`hono_probe`) |
| `cargo check --tests` (limit 30 min) | 94 s: **421 errors** — E0308 259, E0560 154, E0282 4, E0609 2, E0605 1, E0369 1; 141 of them in the re-admitted `helper/route` (138) and `compose` (3) tests; pre-existing modules 308 → **280** |

## Phase 3 round 2, closure joins + interface receivers (Agent T)

Closure-body forks (`&&`/`||`, `if`, throwing terminators) now rejoin once through
`forked_region_join`; a class instance viewed through an interface fills its method slots with
closures dispatching to the class's own methods. Details: `hono-phase3-round2-closures.md`.

| gate | result |
| --- | --- |
| `middleware/trailing-slash/index.ts` | 3.0 MB / 11 357 lines → **60 KB / 367 lines** (4 `redirect`, as in the source) |
| overlay lines removed | 0 — `trailing-slash/index.test.ts` still adds 48 E0308 (variadic handlers / literal union); comment updated |
| `cargo check` (no tests) | **0 errors** (also with the trailing-slash test admitted) |
| `cargo build` | links (`hono_probe`) |
| `cargo check --tests` | **308 errors** (was 402) — E0308 285, E0560 22, E0609 1, E0615 0 |

## Phase 3 round 2, rows 1 + 4 (`worktree-agent-a3a9afb6b46a223e1`)

Throwing-terminator arms now join one shared continuation (`emitter/throwing_join.rs`), and a
branch inside a forward region stays inside it; `toContain`/`toMatch` narrow a nullable actual
and dispatch an erased one at runtime (`SmeltUnknown::to_contain`). Details:
`hono-phase3-round2-continuations.md`.

| gate | result |
| --- | --- |
| `utils/cookie.test.ts` module | 616 MB → **114 KB / 1562 lines** (still excluded: `utils/cookie.ts` E0425 + E0605) |
| `middleware/trailing-slash/index.ts` | unchanged, 3.0 MB / 11 357 lines — a different construct (closure-body `&&`/`\|\|` joins), rule proposed |
| overlay lines removed | 0 — none of the 4 files' ONLY blocker was these families (next blockers named in the overlay) |
| `smelt build` | passes — 63 modules, **211 `#[test]`** in 10 test modules |
| `cargo check` (no tests) | **0 errors**, 438 warnings |
| `cargo build` | links (`hono_probe`) |
| `cargo check --tests` (limit 30 min) | 2 min 12 s: **402 errors** — E0308 285, E0609 69, E0615 26, E0560 22 (= baseline) |

## Phase 3 overlay pass — the gated build passes again (`worktree-agent-a027ada4a0cd01ce8`)

With the test glob fixed the committed overlay selected 84 test files and `smelt build` aborted
on the first blocker of the test closure (`unresolved class ReadableStream` in
`middleware/body-limit`). The overlay now excludes **64 test files** (79 exclude entries total),
each under a grouped `phase 3 pending` comment naming its family; only test files, so no library
module is pruned. 49 hit a source-lowering blocker (incl. `utils/cookie.test.ts`, the 616 MB
`toThrow` duplication), 14 lower but pulled library modules with generated-Rust errors into the
phase-2 gate (a first cut at 99 modules gave `cargo check` **125 errors**: E0308 59, E0107 53,
E0425 9, E0605 3, E0609 1 — the `Context`/`Context_1` cross-kind collision with type arguments,
`accept`, `basic-auth`, `concurrent`, `cookie`, `serve-static`; that check never finished, stopped
after ~2 h), and 1 (`trailing-slash`) pulls a 3 MB module that OOMs plain `cargo check`.

| gate | result |
| --- | --- |
| `smelt build` | passes — 63 modules, **211 `#[test]`** in 10 test modules |
| `cargo check` (no tests) | **0 errors**, 438 warnings, 40 s |
| `cargo build` | links (`hono_probe`) |
| `cargo check --tests` (limit 30 min) | finished in 3 min 38 s: **402 errors** — E0308 285, E0609 69, E0615 26, E0560 22 |
| `cargo test --no-fail-fast` | did not compile |
| phase-3 baseline | **0 passed / 0 failed / 211 total (test binary does not compile)** |
| SmeltUnknown (advisory, `smelt-unknown-baseline-hono.json`) | 101488 total, avoidable **8278** |

Details: `hono-tests.md`; round-2 family queue: `hono-phase3-round2-brief.md`.

## Phase 3 round 1 (the test closure) — `worktree-agent-aabf2b588e2283140`

The test glob was dead (`src/**/*.test.ts` under `roots = ["src"]` matches paths relative to the
root, so it could never match); `smelt build`/`probe` now warn about a glob that matches nothing.
With tests actually selected, the closure reports **66 files with blockers (30 of them test
files), 140 diagnostics, 45 classes** — the phase-3 "→ 0" target is a multi-round campaign.

Two defects were fixed and are why that number is trustworthy now: a frontend panic
(`current_statement_block` carried into a closure body) used to abort the whole diagnostic pass at
`src/jsx/dom/render.ts`, and 87 of the 101 test files never import from `vitest` (`globals: true`),
so they were not test modules at all and their `expect(...)` lowered to an assertion that could not
fail. Generated `#[test]` count on the buildable slice: **9 → 315**.

Blocking round 2: sequential `expect(..).toThrow()` assertions duplicate the rest of the function
each, so 12 of them in `src/utils/cookie.test.ts` emit a 616 MB / 9.5 M-line module and rustc is
OOM-killed. Full inventory and the family table: `hono-phase3-round1.md` (+
`hono-phase3-round1-families.md`).
## Whole-crate `cargo check` (round 34 merged, head `76bda3df`) — PHASE 2 COMPLETE

Orchestrator measurement, clean clone at `eebdf7be…` + `.github/compat/hono/.`, fresh full-feature
binary, repo-root absolute `--manifest-path`: 33 modules, **0 errors** (418 warnings), and
`cargo build` on `dist-smelt` **links** (`hono_probe` binary produced). Round 34 landed: nested
function declaration hoisting + self-reference (K, `hono-round34-hoisting.md`), synthesized
function-value name scope and symbol-keyed base-class chains incl. polymorphic-`this` inherited
copies (L, `hono-round34-scoping.md`), transitive closure captures through nested function
declarations + HIR frame-locality validation + distinct capture aliases (M,
`hono-round34-captures.md`). Erasure: examples avoidable 0; es-toolkit ratchet 31645 → **31431**;
remeda advisory 24873 → 24167. Phase 3 (Hono's own tests) is in progress: `hono-phase3-round1-brief.md`.

## Whole-crate `cargo check` (round 33 partial merge, head `39c9be73`)

Orchestrator measurement, clean clone, fresh full-feature binary, repo-root `--manifest-path`:
33 modules, **3 errors**: E0425 `dispatch` (nested function declaration hoisting), E0425
`__smelt_fn_value_627` (synthesized name scope), E0609 `router` on `Hono` (import-aliased base
class). Round 33 agent J landed both ABI/clone items (6 errors). Agent I's hoisting commit
`a6b308a2` is on `origin/worktree-agent-aefb17f85813045db` (plus a WIP checkpoint `afaf178e`) and
is NOT merged: it raises the es-toolkit ratchet by 4 (`curry.rs`: the self-recursive closure knot
`Rc<RefCell<SmeltErasedFunction>>` is erased where the closure type is concrete). Next session:
type the knot at the closure's own `Rc<dyn Fn..>` before merging, then items 2 and 3 of
`hono-round33-brief.md`, then `hono-phase3-brief.md`.

## Whole-crate `cargo check` (round 32 merged head `b291fc07`)

Orchestrator measurement, clean clone, fresh full-feature binary, repo-root `--manifest-path`:
33 modules, **9 errors** (E0308 3, E0507 2, E0425 2, E0609 1), from 45. Uncarried generic-class
type parameters are now elided (`generic_elision.rs`). Owners: `hono-round33-brief.md`.

## Whole-crate `cargo check` (round 31 merged head `bec88f30`)

Orchestrator measurement, clean clone, fresh full-feature binary, repo-root `--manifest-path`:
33 modules, **45 errors** (E0308 35, E0631 5, E0425 2, E0609 1, E0271 1), from 90. Two clusters
remain — uncarried generic-class type parameters (~29) and emitter singletons (~16); owners in
`hono-round32-brief.md`.

## Whole-crate `cargo check` (round 30 merged head `06c69924`)

Orchestrator measurement, clean clone, fresh full-feature binary, repo-root `--manifest-path`:
33 modules, **90 errors** (E0308 51, E0277 24, E0631 5, E0382 5, E0425 2, E0609 1, E0599 1,
E0271 1), from 329 at the start of round 30 (the 329 counts 10 E0282s the round-29 table
omitted). Families and owners for round 31: `hono-round31-brief.md`.

## Whole-crate `cargo check` (round 29 merged head `c2f22267`)

Measured by the orchestrator from a clean `dist-smelt` with a freshly built full-feature binary:
33 emitted modules, **319 errors** (E0107 136, E0308 134, E0609 17, E0121 10, E0382 8, E0277 5,
E0063 5, E0425 2, E0599 1, E0271 1). Round 29's "the crate does not emit" report did not
reproduce; the per-family breakdown and owners are in `hono-round30-brief.md`.

## Whole-crate `cargo check` (round 28, after the cross-kind type-name ruling)

Committed overlay, fresh clone at the pinned ref, full-feature `smelt`: the complete closure (258
files) transpiles and the crate is emitted. `cargo check` on `dist-smelt`: **326 errors**, down from
883 — 74% of the previous total was one wrongly resolved type name (H71,
`blocker-logs/hono-h71-cross-kind-type-name-collision.md`).

| code | round 27 | round 28 |
| --- | ---: | ---: |
| E0609 (no field) | 437 | 17 |
| E0107 (generic argument count) | 222 | 136 |
| E0308 (mismatched types) | 135 | 138 |
| E0599 (no method) | 33 | 1 |
| E0277 (trait bound) | 29 | 7 |
| E0560 (struct field does not exist) | 12 | 0 |
| E0382 (use of moved value) | 8 | 8 |
| E0121 (type placeholder) | 4 | 10 |
| E0425 (unresolved name) | 2 | 2 |
| E0063 (missing field) | 1 | 5 |
| E0271 (associated type mismatch) | 0 | 1 |
| other | 1 | 1 |

E0107's CAUSE changed: all 136 that remain are `struct takes 3 generic arguments but 1 generic
argument was supplied` — `Context<E>` written against
`class Context<E extends Env = any, P extends string = any, I extends Input = {}>`. That is the
**type-parameter-defaults** family (a type reference that omits trailing type arguments takes the
declaration's defaults) and it is now the largest single family in the crate.

This table, not the router slice, is phase 2's metric from here on.
