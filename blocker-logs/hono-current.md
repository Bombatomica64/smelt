# Probe report: hono

- Transpile: **yes** — Rust crate emitted
- Generated `cargo test`: not run (pass `--run-tests`)
- Files scanned: 258 · with blockers: 0
- CI: `.github/workflows/ci.yml`'s `hono` job now hard-gates `smelt build` + `cargo check` on `dist-smelt` (mirroring radash); `cargo test` and the SmeltUnknown erasure report stay advisory until the phase-3 test baseline is stable.


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
