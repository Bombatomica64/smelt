# Hono phase 3, round 1 — the test closure, measured

Branch `worktree-agent-aabf2b588e2283140`, base `origin/lorenzo/great-rubin-vy0943`.
Clone: `honojs/hono` at `eebdf7be39abf0a872671835ccce0c4f03ea497a` + `.github/compat/hono/.`,
full-feature binary, `smelt build/check/probe --manifest-path <abs>` from the repo root.

## Summary

Items 1–3 of the round-1 brief landed in full. Item 4 measured the closure and found that the
"→ 0 blockers" target is a multi-round campaign, not a round: the honest inventory is **66 files
with blockers (30 of them test files), 140 diagnostics, 45 distinct blocker classes**. Two
findings dominate everything else, and both were invisible before this round:

1. **87 of Hono's 101 `*.test.ts` files never import from `vitest`** (`vitest.config.ts` sets
   `globals: true`). Smelt gated its whole test model on the import, so those files were not test
   modules: `describe(...)` was an ordinary call, and `expect(x).toBe(y)` lowered through the
   erased dynamic-call path — an assertion that *cannot fail*. Fixed (commit `9fac7035`): a
   test-tier module now has the vitest API names in scope unless it binds them itself. Generated
   `#[test]` count on the same buildable slice went **9 → 315**.
2. **A frontend panic** (`index out of bounds` in `Body::push_stmt_to_block`) killed the whole
   diagnostic pass at `src/jsx/dom/render.ts`, hiding every blocker in the modules after it.
   Fixed (commit `24f73c8f`): `current_statement_block` is a `BlockId` into the *enclosing* body
   and must not cross a closure-body boundary. Blocker inventory went 21 files → 62 files once the
   pass could finish.

A third finding is a hard blocker for round 2 and is NOT fixed: sequential `expect(...).toThrow()`
assertions duplicate the rest of the function per assertion, so twelve of them in one test produce
a **616 MB, 9.5-million-line** generated module (`src/utils/cookie.test.ts`), and `cargo check
--tests` is OOM-killed (SIGKILL at 15 GB) on any slice that contains it.

## Item 1 — the test glob and the dead-glob warning — LANDED (`53d7b567`)

`.github/compat/hono/Smelt.toml` now spells `test-prefix = ["**/*.test.ts"]` (remeda's spelling).
`discover_test_paths` tracks which glob matched at least one file and prints a warning naming every
glob that matched nothing under every source root, from both `smelt build` and `smelt probe`.
Three unit tests in `crates/smelt-transpiler/src/lowering.rs` cover the message, the multi-glob
message and the silent (all-matched) case.

## Item 2 — exclusions with reasons — LANDED (`53d7b567`)

Added to the overlay, each with a one-line reason:

| file | reason |
| --- | --- |
| `src/middleware/cache/index.test.ts` | `vi.stubGlobal('caches'/'crypto')` ×8 |
| `src/adapter/service-worker/handler.test.ts` | `vi.stubGlobal` + `vi.unstubAllGlobals` |
| `src/middleware/jwk/index.test.ts` | `vi.spyOn(global.crypto, 'subtle', 'get')` — accessor spy on a host global |
| `src/helper/dev/index.test.ts`, `src/middleware/logger/index.test.ts`, `src/utils/color.test.ts` | `vi.stubEnv` / `vi.unstubAllEnvs` — process-env mutation is not a modeled surface |
| `src/middleware/timing/index.test.ts` | `vi.spyOn(Date, 'now')` — the runtime models the clock as ambient state (`vi.setSystemTime`), not a replaceable member |

Decisions the brief left to this round:
- **`vi.spyOn(console.*)` is modeled, so those files stay in scope.** `vi.spyOn` already resolves a
  member's current value, wraps it in a forwarding mock and writes it back (`VitestSpyOn`), which
  covers host-module members generally; `console` needs nothing special. Files affected and kept:
  `src/jsx/context-isolation.test.ts`, `src/middleware/language/index.test.ts`.
- **`vi.spyOn(Date, 'now')` is excluded**, because the clock is not a member: the generated runtime
  keeps it as its own state, so the spy would be silently inert (a passing test asserting nothing).
- **`vi.stubEnv` is excluded** (3 files): no `vi.stubEnv` member exists in the model at all, and
  adding process-env stubbing is a host-model item, not a spy-model one.

Committed exclusions total 7 test files, leaving **84 in-scope `*.test.ts` files** of the 101 on
disk (the survey's 91 minus the 7 above, plus the 10 already excluded by directory).

## Item 3 — the matcher model — LANDED (`53d7b567`)

`TestMatcher` and the mock-call matchers grew, all through general rules:

| matcher | lowering |
| --- | --- |
| `toBeTruthy` / `toBeFalsy` | `lowered_condition_expression` — the same coercion `if (v)` uses |
| `toBeDefined` | the `toBeUndefined` comparison with the failure condition flipped |
| `toMatch` | RegExp argument → `RegexTest`; string argument → `contains_expr`, chosen from the lowered argument's TYPE |
| `toMatchObject` | a new `matchObject` asymmetric-matcher kind: a recursive-subset walk in the runtime prelude, so a nested `expect.any(..)` keeps working |
| `toHaveBeenCalled` | `!calledTimes(0)` |
| `toHaveBeenCalledOnce` | `calledTimes(1)` |
| `toHaveBeenNthCalledWith(n, …)` | `mock.mock.calls[n-1]` compared with the matcher-aware deep equality |
| `toBeLessThan(OrEqual)` / `toBeGreaterThan(OrEqual)` | the inverse comparison as the failure condition |
| `toBeTypeOf` | `TypeofValue` on the erased actual, compared to the expected string |
| `toThrowError`, `toBeCalled`, `toBeCalledWith`, `toBeCalledTimes`, `toBeCalledOnce`, `lastCalledWith`, `nthCalledWith` | one alias table (`canonical_matcher_name`) onto the canonical arms |

Tests: 14 new cases in `crates/smelt-codegen-rust/tests/vitest_async_matcher_runtime.rs` (each
matcher in both directions — a satisfied assertion must pass AND a false one must fail; the tier is
**19 passed / 0 failed**), plus four frontend lowering tests.

`toBeFunction` / `instanceOf`: Hono has **no `expect.extend`** anywhere in the repo
(`grep -rn "expect.extend" src` → empty). The two `toBeFunction` uses and the one bare `instanceOf`
sit in `src/types.test.ts` and `src/utils/types.test.ts`, which are type-level test files; they are
NOT vitest core and are not modeled. Reported, not special-cased.

## Item 4 — the test closure

### Test-closure numbers

| metric | value |
| --- | ---: |
| `*.test.ts` files on disk under `src/` | 101 |
| in scope after the committed excludes | 84 |
| files with blockers (whole closure, `smelt check --message-format json`) | 66 |
| …of which test files | 30 |
| diagnostics | 140 |
| distinct blocker classes | 45 |
| `#[test]` emitted on the buildable slice (measurement manifest, see below) | **315** (was 9 before the vitest-globals fix) |
| generated test modules on that slice | 22 |

`smelt build` on the committed manifest still fails: the closure that the test roots pull in
contains modules the non-test crate never reached (`src/adapter/**` handlers, `src/middleware/**`
that `src/index.ts` does not import, `src/jsx/dom/**`), and any one blocker aborts the build. The
315 number therefore comes from a **measurement-only** manifest (`Smelt-measure.toml` in the
scratchpad clone, not committed) that excludes the 84 files listed in
`blocker-logs/hono-phase3-round1-measure-excludes.txt` plus `src/utils/cookie.test.ts` — derived
mechanically by re-running `smelt check` and adding every still-blocked file until it was clean.
That slice is the part of the test closure that lowers today; everything excluded from it is the
round-2 work list.

### Blocker families (whole closure, after both fixes)

See `blocker-logs/hono-phase3-round1-families.md` for the full 45-row table. The head of it:

| n | files | test files | blocker |
| ---: | ---: | ---: | --- |
| 24 | 1 | 1 | callback method `safe_parse` is not lowered into closure bodies yet |
| 8 | 7 | 4 | unknown class method `set` (a class FIELD whose type is a callable interface — `Context.set`) |
| 7 | 7 | 5 | unresolved class `ReadableStream` |
| 7 | 7 | 7 | `expect(...).toContain(...)` requires a string, array, set or map |
| 6 | 2 | 1 | dynamic `import()` is not lowered |
| 6 | 3 | 0 | module-level function return type needs a supported default value |
| 4 | 3 | 3 | call expression is not lowered yet |
| 3 | 3 | 0 | imports a value from an excluded module (`helper/websocket`) |
| 3 | 2 | 0 | string case methods require a string receiver |
| 3 | 2 | 0 | assignment target must be a local, field or index expression |
| 3 | 2 | 0 | array push argument must match the array element type |
| 3 | 2 | 0 | `LabeledStatement` is not lowered |

Host surfaces in the list that are arguably scope decisions rather than families:
`ReadableStream`/`TransformStream`/`Compression`/`DecompressionStream`, `EventTarget`/`Event`/
`CustomEvent`, `SVGElement`/`requestAnimationFrame` (DOM), `Deno`/`navigator`/`awslambda` globals,
`node:async_hooks`.

### `cargo check --tests`

Run once on the measurement slice (282 `#[test]`, `cookie.test.ts` removed for the reason below):

Run once on a reduced slice: 20 generated test modules, **221 `#[test]`**, 43 MB of generated
Rust (`Smelt-small.toml`). The larger 282-test slice could not be checked — see the duplication
blocker below: rustc was still resident at 6 GB after 1h40m on it, and the full slice (with
`cookie.test.ts`) is OOM-killed outright.

`cargo check --tests` → **7722 errors**:

| code | n | what it is |
| --- | ---: | --- |
| E0107 | 6473 | wrong number of generic arguments — `Context<E>` written against `class Context<E, P, I>` with defaults; the type-parameter-defaults family phase 2 already names, but the test bodies write `Context` far more often than the library does |
| E0560 | 630 | struct has no such field |
| E0308 | 331 | mismatched types |
| E0609 | 225 | no field on this type |
| E0615 | 26 | method used as a field (a callable field read without a call) |
| E0277 | 10 | trait bound not satisfied |
| E0425 | 9 | unresolved name |
| E0618 | 7 | called something that is not a function |
| E0605 | 3 | invalid cast |
| E0599 | 2 | no method |
| E0369 | 1 | binary operation not supported |

461 warnings. Nothing here was fixed — round 2 owns this table, and E0107 alone is 84 % of it.


### The blocker that stops the test build: exponential `toThrow` duplication

`src/utils/cookie.test.ts` has one test with twelve sequential
`expect(() => …).toThrowError(…)` assertions. Each one lowers to a `try`/`catch` whose
continuation is the rest of the function, and the continuation is DUPLICATED per assertion, so the
twelve compose multiplicatively: the generated module is **616 MB / 9,567,616 lines**, of which one
function is 9,565,974 lines. rustc is OOM-killed (SIGKILL, 15 GB machine) on any build containing
it. This is a general codegen defect, not a Hono one — any suite with N sequential `toThrow`
assertions in one test pays 2^N. It was invisible until this round because `toThrowError` was not
a modeled matcher and the file was not a test module. **It is the first thing round 2 should fix**;
it is not touched here because the duplication is in the try/catch continuation lowering, which is
the area another stream is working in this round.

Measured consequence: `cargo check --tests` on the 282-test slice (95 MB of generated Rust, the
`cookie.test.ts` module removed) had not finished after **1h43m** with rustc resident at 6.3 GB and
was stopped; the slice WITH that module is SIGKILLed outright. The table above is therefore the
reduced 221-test slice.

## Gates

| gate | expected | measured | verdict |
| --- | --- | --- | --- |
| examples invariant (`--fail-on-regression`) | avoidable 0 | avoidable **0**, 68700 total, exit 0 | pass |
| es-toolkit ratchet (`--fail-on-regression`) | avoidable ≤ 31431 | avoidable **30809** (−622), exit 0 | pass, baseline re-snapshotted in `ac47e17c` |
| remeda advisory report | — | avoidable **24142** (−731 vs baseline) | pass (advisory) |
| remeda generated `cargo test` | 1789 / 0 | **1786 passed / 3 failed** (1789 total) | see below |
| radash generated `cargo test` | 84 / 84 | **358 passed / 29 failed** (387 total) | see below |
| `cargo test -p smelt-frontend-ts --no-default-features` | green | 1101 passed / 0 failed | pass |
| `cargo test -p smelt-codegen-rust` | green | 1079 passed / 0 failed (+ ignored tiers) | pass |
| `cargo test --bin smelt` | green | 60 passed / 0 failed | pass |
| end-to-end (`hir_cli_cross_language_tests`) | green | 18 passed / 0 failed | pass |
| vitest runtime tier (`--ignored`) | green | 19 passed / 0 failed (5 pre-existing + 14 new) | pass |
| `cargo clippy --all-targets` | no new findings in touched files | one new (unnecessary qualification), fixed in `ce7b410b` | pass |

### The two corpus test gates moved, and why

Both moves are the vitest-globals fix telling the truth, not a behaviour regression:

- **radash 84 → 387 tests.** Eight of radash's nine `src/tests/*.test.ts` files never import from
  `vitest`; only `typed.test.ts` did (and only because the overlay's `sed` adds the import). So the
  committed "84 / 84" was 84 tests out of 387, with 303 silently not emitted at all. All **29**
  failures are in the eight newly-emitted modules (array 5, async 9, curry 11, number 1, series 3);
  the 84 that used to run still pass. This needs a new committed baseline and a family pass.
- **remeda 1789 → 1786 / 3.** The total is unchanged (remeda imports vitest), and the three
  failures are assertions that previously lowered to NOTHING: `toBeLessThanOrEqual` /
  `toBeGreaterThanOrEqual` were not in the matcher model, so the call fell through to the generic
  dynamic-call path and asserted nothing. Now that they assert, they fail on
  `randomBigInt`/`sample` because a `bigint` wider than `i64`
  (`9_999_999_999_999_999_999_999n`) is typed as a float and the comparison emits
  `(v as f64) < huge_number` — lossy at 1e22. That is the bigint literal-width family, not the
  matcher: the matcher is what made it visible.


## Not fixed, found

- The exponential `toThrow` duplication above (highest priority).
- `unknown class method \`set\``: `Context.set` is a class FIELD whose declared type is a callable
  interface (`Set<E>`, a conditional type over the env). Calling a function-valued field whose type
  is a callable interface is not modeled; it blocks `src/hono.test.ts`, `src/compose.test.ts`,
  `src/types.test.ts` and four middleware modules, i.e. the largest test files in the repo.
- `expect(...).toContain(...)` on an erased actual (7 test files) — now that the globals fix makes
  those assertions real, `toContain` needs the erased-receiver path the other matchers have.
- Blockers behind `smelt build` that `smelt check` does not report (they need the whole-crate
  pass): `list unshift item must match the list element type` (`middleware/combine`), `new
  Headers(init)` from an erased/record initializer (`middleware/cors`, `adapter/aws-lambda`), and
  an emitter panic `type table does not contain literal operand type Optional(..)`
  (`src/utils/buffer.test.ts`).
