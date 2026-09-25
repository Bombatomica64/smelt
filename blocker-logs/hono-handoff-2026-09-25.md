# Hono campaign — handoff (2026-09-25)

Resume from here. Orchestrator rules: architecture and direction only; grunt work and code go to
Opus subagents (`model: opus`, own worktree via `isolation: worktree`); at most two cargo builders
at a time; verify every agent branch from a clean `dist-smelt` with a freshly built binary before
merging; measure Hono from the repo root with an absolute `--manifest-path`.

## Where things stand

- **main** = `187610a2` (PR #256, #257, #258 merged). PR #258 landed Hono round 34 (phase 2
  complete: the non-test crate at 0 `cargo check` errors and linking) and phase 3 round 1 (test
  glob fixed, vitest globals, ~15 matchers, 64 `phase 3 pending` overlay exclusions, CI `hono` job
  hard-gates `smelt build` + `cargo check`, advisory `cargo test` and erasure report).
- **Working branch** `lorenzo/great-rubin-vy0943` = `621ea502`, restarted from main after #258
  and carrying **phase 3 round 2** work from three agents, all merged and pushed, NOT yet in a PR:
  - R — try/catch **join** emission (`emitter/throwing_join.rs`): sequential `toThrow`/try-catch
    was 3^N (cookie.test 616 MB → 114 KB); `toContain`/`toMatch` on nullable and erased actuals.
    Fixture 121. Note `hono-phase3-round2-continuations.md`.
  - T — closure-body joins for `&&`/`||` chains and throwing terminators (trailing-slash 3 MB →
    60 KB); interface-typed receivers dispatch class methods
    (`class_method_implements_interface_field`, `emitter/core.rs`). Fixtures 124, 125. Note
    `hono-phase3-round2-closures.md`. Documented limitation: an interface view copies data
    properties at construction (exact for `readonly`; a later class write is not seen).
  - S — ambiguous-class resolution (`Context` vs a same-named interface) through import cycles,
    barrel re-exports and `new`; callable-interface fields (`Context.set`), incl. a silent runtime
    bug (closure stored as `Default::default()`) and per-call overload choice; shared
    `__smelt_from_record_<Type>` extraction helper (was a 230 KB inline rebuild per erased call
    site); getter satisfies an implemented interface property. 9 test files re-admitted.
    Fixtures 122, 123. Note `hono-phase3-round2-generics.md`. One classifier change in
    `unknown_report.rs` (`fn __smelt_from_record_` is a boundary; test
    `shared_record_extractor_is_a_boundary`).

### Hono on `621ea502` (clean clone `eebdf7be…` + `.github/compat/hono/.`, fresh binary)

| metric | value |
| --- | --- |
| modules / `#[test]` | 82 / 234 (19 test modules) |
| `cargo check` (no tests) | **0 errors**, links |
| `cargo check --tests` | **421 errors**: E0308 259, E0560 154, E0282 4, E0609 2, E0605 1, E0369 1 (94 s) |
| of which newly admitted `helper/route` | 138 (E0560: derived `Hono_1` passed where base `Hono` is declared, rebuilt as a struct literal) |
| overlay `phase 3 pending` test exclusions | ~55 (see `.github/compat/hono/Smelt.toml`, grouped by family) |

Gates on `621ea502`: examples avoidable 0; es-toolkit ratchet **26608** (from 31645 at the start of
round 34); remeda pinned `1787 passed; 2 failed`; radash pinned `384 passed; 3 failed`; remeda
advisory 23534; Hono advisory baseline `smelt-unknown-baseline-hono.json` (8278 avoidable, from
round 1 — re-snapshot when the round-2 PR is cut).

## Stopped mid-flight (user stopped them; work checkpointed, NOT merged, NOT verified)

- **Agent U** — branch `worktree-agent-a2f348f58993d6e95` (pushed). Items: (1) subclass passed
  where its base class is declared (the E0560 family, now 154 incl. `helper/route`); (2) library
  modules with generated-Rust errors reached only by tests (`utils/accept`, `utils/basic-auth`,
  `utils/concurrent`, `utils/cookie`+`helper/cookie`). Two real commits (`d7ea6d49` "Subclass-to-
  base views for reference classes; bind own methods into virtual slots", `d2d5d208` WIP item 2),
  then an orchestrator WIP checkpoint of uncommitted edits. Design intent given to U: prefer a
  generated base trait / `impl BaseTrait` over a copied view; overriding must dispatch to the
  subclass method. Resume by reading `d7ea6d49`'s diff first and deciding whether its "view"
  approach meets that bar.
- **Agent V** — branch `worktree-agent-aa8e2a786664e0a39` (pushed). Items: (1) E0308 bulk in the
  router tests (`Node<T>`/`Router<T>` instantiated with `T = string` but erased to `SmeltUnknown`
  — a concrete type argument lost at instantiation/elision); (2) E0428 named function expression
  (`const requestId = function requestId(..)`) also emitting a module-level `fn` (`request-id`,
  `secure-headers`). Only a diagnosis checkpoint with scratch files (`scratch/`); no fix. Clean the
  scratch files before anything lands. Fixture numbers reserved: 129, 130.

Next free fixture number after those: **131**. Fixture numbers 126–128 were reserved for U.

## Immediate next steps

1. Decide: resume U and V from their checkpoints, or cut the round-2 PR now with R+T+S
   (`lorenzo/great-rubin-vy0943` → main, `run-regressions` label, PR body per
   `.github/PULL_REQUEST_TEMPLATE.md`; re-snapshot the Hono advisory baseline in the PR).
   Either way, re-verify Hono from a clean clone with a fresh binary on the exact head first.
2. Round 3 queue (from `hono-phase3-round2-brief.md` remaining table + this round's findings):
   E0560 subclass-to-base (U); router `T` erasure (V); E0428 named function expr (V); E0277 serde
   `Deserialize` on a `SmeltRecord` field (`aws-lambda/conninfo`); dynamic `import()` (blocks
   `hono.test.ts`); `hc` client import (blocks `types.test.ts`, `helper/factory`); `serve-static`
   37 MB module (another duplication class — measure the construct); `new URL(path, base)`;
   `unshift` item type; string case on a non-string receiver; host streams (scope decision);
   `LabeledStatement`; tagged templates; `Object.assign(t)`.
3. Open design decisions for the user (none rejected): BigInt as a real type on `num-bigint`
   (2 remeda tests); `Proxy` traps (1 radash test); per-call overload return selection for
   overloaded METHODS (Agent Q found `header()` returns `Unknown`); the two radash `as any`
   tests treated as unfixable test behaviour; the interface-view data-property copy (T).

## Process notes that saved time this round

- An agent's overlay change verified on an older base can break on the merged head (Agent P's
  overlay + Agent O's frontend change broke `request.ts`); always re-verify the MERGED head.
- Agents share the scratchpad directory; tell them to keep scratch work inside their worktree.
- `git add -A` in the main checkout once committed `target-orch/` marker files; `target-*/` is
  now gitignored. Stale library clones left in stopped worktrees show up as untracked files.
- CI pins: radash `FAILED. 384 passed; 3 failed`, remeda `FAILED. 1787 passed; 2 failed`; any
  change to a total or a different failure turns the job red on purpose.
- Notes are gitignored by default: allowlist each `blocker-logs/<name>.md` in `.gitignore`.
