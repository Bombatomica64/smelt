# Open questions from deleted reports

These problems were recorded as open in investigation reports that have since been deleted. Each
entry names the report and the last commit that touched it, so the full write-up comes back with
`git show <commit>:blocker-logs/<report>`. None was re-verified against `main` when this list was
written (2026-11-14). Check that an entry still holds before working on it. Remove the entry when
the problem is fixed or gets a home in `docs/` or `specs/`.

Problems that a CI baseline already names (for example the `runtime_known_failures` in
`es-toolkit-ci-baseline.json` and `hono-ci-baseline.json`) or that a spec already covers (for
example `null` vs `undefined` in `specs/distinct-undefined.md`) are not repeated here.

## Needs a decision

- **Out-of-range element read** (`hono-h47-out-of-range-element-read.md`, `7899b1947`;
  `estk-final45-plan.md` D2, `5f94ce011`). Reading past the end of a concrete `T[]` or tuple yields
  `Default::default()` (`""`, `0`, a defaulted tuple) where JavaScript gives `undefined`. The fix
  needs a ruling: type unproven index reads as `Optional<T>`, or throw. es-toolkit's `at` known
  failure is one symptom.
- **Spreading an optional tuple** (`hono-h46-spread-arguments.md`, `7899b1947`). `f(...maybeTuple)`
  with `maybeTuple: T | undefined` is rejected as a named blocker. Spreading `undefined` throws in
  JavaScript, so the report argues for an unwrap that throws over per-element defaults. The same
  report records that a spread into a *named module function* callee (`record(...routes[i])`) still
  fails to compile.
- **Callback iteration borrow** (`smeltlist-shared-buffer.md`, `aa590f337`, follow-up 4).
  Iterating a list while calling a callback holds a read borrow: fast, but it panics if the callback
  writes the same list. The alternative is a snapshot per call (safe, one copy each). The report
  suggests snapshotting only when the callback's captures can reach the list.

## Known unfixed defects

- **List and promise identity tables can alias** (`remeda-pipe-identity-flake.md`, `e0853e75a`).
  `SMELT_LIST_IDENTITIES` and `SMELT_PROMISE_IDENTITIES` key on a local's storage address and
  never remove entries, so a reused address can alias two values.
- **`ListAliasOrigin` and typed-array `===`** (`smeltlist-shared-buffer.md`, `aa590f337`,
  follow-ups 2 and 3). The erased-base write-back arms were judged redundant: retire
  `list_alias_origin*` and the two special-cased push paths in `list_mutation.rs`. Separately,
  `BinOp::JsStrictEq` on typed arrays should go through `strict_identity_text`; this changes `toBe`
  answers, so rerun every library gate.
- **Write-back through nested optional projections** (`hono-h31-projected-receiver-writeback.md`,
  `7899b1947`). A write through a chain of optional projections whose intermediate values are
  value classes copies once per level and commits only the levels that are places. Fixing it
  needs a nested `Place`.
- **A throw through an awaited erased call escapes `try`/`catch`** (`hono-h66-promise-continuations.md`,
  `7899b1947`, numbered H70). Reproduced with radash's `guard` across two modules: the rethrow
  leaves through the awaited erased call and the program exits 101.
- **A closure passed for a function-typed alias parameter** (`hono-h71-cross-kind-type-name-collision.md`,
  `7899b1947`). It emits `Rc<{closure}>` against a `&dyn Fn(..)` parameter (E0308).
- **Object-shaped type aliases are erased** (`ts7-parity-phase2-round3-follow-ups.md`, `344b6de92`).
  `type R = { a: string }` becomes an erased record, because only classes and interfaces become
  structs. In a crate with no other erasure, a read of `r.b` then aborts the build. es-toolkit's
  `TruncateOptions` is one case.
- **`AbortSignal`** (`abort-signal-reason-and-statics.md`, `7899b1947`). `AbortSignal.any([..])` is
  not wired, although the follow list it needs exists. The signal record is still erased (about 30
  avoidable-erasure lines per signal), which is defensible for `reason` but not for `aborted` or
  the listener list.

## Not audited

- **Map and record iteration order** (`hono-h52-set-iteration-order.md`, `7899b1947`). The fix
  audited `Set` only. `SmeltJsMap` and `SmeltRecord` look insertion-ordered, but nobody has checked
  them against JavaScript, including its rule that integer keys come first in object key order.
- **Test-suite gaps** (`test-coverage-analysis.md`, `3105238ad`). The 2026-08 coverage analysis
  recommended three changes with no recorded outcome: snapshot-by-default for new emitter tests
  (gap 1), golden files for the report renderers (gap 6), and a real Python compat target (gap 5;
  the cachetools probe is a start).
