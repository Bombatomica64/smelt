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
