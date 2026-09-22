# Hono phase 3 survey — vitest test closure inventory

Clone: `honojs/hono` at `eebdf7be39abf0a872671835ccce0c4f03ea497a` (pinned in
`.github/compat/libraries.json`), overlaid with `.github/compat/hono/.` (Smelt.toml +
excludes), in the scratchpad at `<scratchpad>/hono/`.

## 1. Test closure

`Smelt.toml`: `test-prefix = ["src/**/*.test.ts"]`, `roots = ["src"]`.
`exclude` (6 entries): `src/client/**`, `src/helper/testing/**`, `src/helper/streaming/**`,
`src/helper/websocket/**`, `src/utils/jwt/**`, `src/middleware/jwt/**`, plus two named leaf
files `src/client/client.test.ts` and `src/client/utils.test.ts` (redundant with the
`src/client/**` prune, listed explicitly for the reason comment).

- Total `*.test.ts` under `src/`: **101**
- Outside the overlay's excluded globs: **91** (not ~44 — see note below)

Note on the brief's "44": the brief's number appears to be a stale/rough estimate written
before the actual exclude list was finalized (the exclude list only removes 10 files: 8 under
`client/`, `helper/testing`, `helper/streaming`, `helper/websocket`, `utils/jwt`,
`middleware/jwt`, plus the client test leaves already counted). Applying the committed
`Smelt.toml` excludes to the 101 files on disk gives 91 in-scope files, confirmed by direct
`grep -v` against the exclude glob prefixes. This is worth flagging back to whoever owns the
brief — either the exclude list undercounts what the brief intended to exclude (e.g. the many
adapter tests for `bun`/`deno`/`cloudflare-workers`/`lambda-edge`/`vercel`/`netlify`/`aws-lambda`
runtimes, none of which are in the exclude list today) or the "44" figure is simply wrong.

Case counts across the 91 in-scope files:
- `describe(` : 585, `describe.each(` : 4 → 589 describe blocks
- `it(` : 1626, `test(` : 158, `it.each(` : 12, `test.each(` : 10, `it.skip(` : 3 → 1809 it/test cases

(Table-driven `.each` rows expand to more than one `#[test]` each at lowering time per
`suites.rs`, so the eventual `#[test]` count in `dist-smelt` will exceed 1809.)

## 2. Vitest API surface used in the 91 in-scope files

### `vi.*` members

| member | files | occurrences |
| --- | ---: | ---: |
| `vi.fn` | (subset of 25) | 107 |
| `vi.spyOn` | (subset of 25) | 16 |
| `vi.stubGlobal` | (subset of 25) | 9 |
| `vi.unstubAllGlobals` | (subset of 25) | 4 |
| `vi.unstubAllEnvs` | (subset of 25) | 3 |
| `vi.stubEnv` | (subset of 25) | 3 |
| `vi.resetModules` | (subset of 25) | 2 |
| `vi.restoreAllMocks` | (subset of 25) | 1 |

25 distinct in-scope files use some `vi.*` member. No `vi.mock(` call appears anywhere in the
in-scope set (`grep -n "vi\.mock(" → empty`).

### `expect.*` static (asymmetric matcher) members

| member | occurrences |
| --- | ---: |
| `expect.any` | 7 |
| `expect.stringContaining` | 1 |
| `expect.arrayContaining` | 1 |
| `expect.anything` | 1 |

### Matcher names after `expect(...)` (chain tail), with occurrence counts

| matcher | occurrences |
| --- | ---: |
| `toBe` | 2457 |
| `toEqual` | 664 |
| `not.toBeNull` | 269 |
| `toBeTruthy` | 94 |
| `toBeNull` | 83 |
| `toBeFalsy` | 81 |
| `toMatch` | 78 |
| `toBeUndefined` | 61 |
| `not.toHaveBeenCalled` | 61 |
| `toThrowError` | 42 |
| `not.toThrow` | 28 |
| `toThrow` | 27 |
| `not.toBeCalled` | 26 |
| `toContain` | 24 |
| `toBeCalled` | 19 |
| `toStrictEqual` | 16 |
| `toHaveBeenCalledWith` | 16 |
| `toHaveBeenCalledOnce` | 15 |
| `toBeInstanceOf` | 14 |
| `not.toBe` | 14 |
| `toHaveBeenCalled` | 13 |
| `toBeCalledWith` | 12 |
| `toMatchObject` | 11 |
| `not.toBeFalsy` | 11 |
| `toHaveBeenCalledTimes` | 10 |
| `not.toContain` | 10 |
| `mockImplementation` (chained off a mock, not `expect`) | 10 |
| `rejects.toThrow` | 9 |
| `mockResolvedValue` | 8 |
| `toBeCalledTimes` | 6 |
| `toBeDefined` | 5 |
| `rejects.toThrowError` | 5 |
| `not.toEqual` | 5 |
| `not.toBeUndefined` | 4 |
| `toHaveLength` | 3 |
| `toHaveBeenNthCalledWith` | 3 |
| `mockReturnValue` | 3 |
| `mockRejectedValue` | 3 |
| `toHaveProperty` | 2 |
| `toBeLessThan` | 2 |
| `toBeFunction` | 2 |
| `not.toThrowError` | 2 |
| `toBeTypeOf` | 1 |
| `toBeLessThanOrEqual` | 1 |
| `toBeGreaterThanOrEqual` | 1 |
| `not.toHaveProperty` | 1 |
| `not.toHaveBeenCalledWith` | 1 |
| `not.instanceOf` | 1 |
| `instanceOf` | 1 |

(Rows for plain method names that the naive grep also matched, e.g. `get(`, `post(`, `push(`,
`map(`, `then(`, `text(`, `use(`, `route(`, `next(`, `join(`, `slice(`, `reduce(`, `replace(`,
`update(`, `fill(`, `at(`, `on(`, `digest(`, `decode(`, `encode(`, `basePath(`, `createApp(`,
`stream(`, `pipeThrough(`, `arrayBuffer(`, `uuid(`, `match(` — are ordinary app/collection calls
that happen to sit right after some `)` in the source, not vitest matchers; they are omitted
above as noise from the "any name after a closing paren" heuristic.)

### `describe.each` / `it.each` / `test.each`
`describe.each` 4, `it.each` 12, `test.each` 10 (already counted in case totals above).
`it.skip` 3 occurrences. No `describe.skip`, `describe.only`, `it.only`, `test.only`,
`it.todo`/`test.todo`, or `it.concurrent`/`test.concurrent` found in-scope.

### Lifecycle hooks

| hook | files | occurrences |
| --- | ---: | ---: |
| `beforeEach` | 21 | 75 |
| `afterEach` | 3 | 3 |
| `beforeAll` | 6 | 7 |
| `afterAll` | 7 | 8 |

### Other vitest imports
```
import { describe } from 'vitest'
import { expectTypeOf } from 'vitest'
import { describe, expect, it } from 'vitest'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { vi } from 'vitest'
import type { RunnerTestSuite } from 'vitest'
```
`expectTypeOf` is imported in 5 files but *called* 580 times across 14 in-scope files (many
files use it as an implicit global without importing it, matching vitest's `globals` config) —
`src/types.test.ts`, `src/utils/types.test.ts`, `src/request.test.ts`, `src/hono.test.ts`,
`src/preset/{quick,tiny}.test.ts`, `src/validator/{utils,validator}.test.ts`,
`src/helper/{adapter,factory}/index.test.ts`, `src/middleware/bearer-auth/index.test.ts`,
`src/adapter/aws-lambda/handler.test.ts`, `src/adapter/cloudflare-workers/serve-static.test.ts`.
`import type { RunnerTestSuite } from 'vitest'` is type-only plumbing in one internal test
harness file, not a runtime dependency.
No `toMatchInlineSnapshot` / `toMatchSnapshot` usage anywhere in-scope (`grep` → empty).

## 3. Smelt's vitest host model — what exists vs. missing

Model lives in `crates/smelt-frontend-ts/src/lowering/testing/matchers.rs` (assertions),
`.../testing/suites.rs` (`describe`/`it`/`test`/`.each`/lifecycle hooks), and
`crates/smelt-frontend-ts/src/lowering/stdlib.rs` (`vi.fn`, `vi.spyOn`, `vi.setSystemTime`,
`vi.restoreAllMocks`, `vi.useRealTimers`, `vi.useFakeTimers`, mock chain methods
`mockImplementation`/`mockReturnValue`/`mockRestore`/`mockClear`/`mockReset`).

**Already modeled** (confirmed by grep of the frontend lowering source):
- `describe`, `it`, `test`, `describe.each`, `it.each`/`test.each`, `beforeEach`, `afterEach`,
  `beforeAll`, `afterAll` (`testing/suites.rs`, `decls/types_iface.rs`)
- `vi.fn`, `vi.spyOn`, `vi.setSystemTime`, `vi.restoreAllMocks`, `vi.useRealTimers`,
  `vi.useFakeTimers`, and mock methods `mockImplementation`, `mockReturnValue`, `mockRestore`,
  `mockClear`, `mockReset` (`lowering/stdlib.rs`)
- Matchers: `toBe`, `toEqual`, `toStrictEqual`, `toContain`, `toHaveLength`, `toHaveProperty`,
  `toBeInstanceOf` (the closed `TestMatcher` enum, `lowering.rs` lines 42-57/302-313), plus
  `toBeUndefined`, `toBeNull`, `toThrow`/`toThrowErrorMatchingInlineSnapshot`,
  `toHaveBeenCalledTimes`, `toHaveBeenCalledWith`, `toHaveBeenLastCalledWith`,
  `toHaveLastResolvedWith`, and `.not` inversion (special-cased in `testing/matchers.rs`)
- `expect.any`/`.anything`/`.arrayContaining`/`.objectContaining`/`.stringContaining`/
  `.stringMatching`/`.closeTo` asymmetric matchers (`ASYMMETRIC_MATCHER_NAMES` in
  `testing/matchers.rs`)
- `expectTypeOf`-style type-test chains (`call_dispatch.rs`, lines ~5760/~6057)

**Missing** (no hit anywhere in `crates/smelt-frontend-ts/src` or `crates/smelt-stdlib` for
these exact matcher/API spellings):
- Matchers Hono's suite actually calls that are absent from the model: `toBeTruthy` (94×),
  `toBeFalsy` (81×), `toMatch` (78×), `toThrowError` (42×, distinct from plain `toThrow`),
  `toBeDefined` (5×), `toMatchObject` (11×), `toBeCalled`/`toBeCalledWith`/`toBeCalledTimes`
  (jest-style aliases for the `toHaveBeenCalled*` family — 19/12/6×),
  `toHaveBeenCalled` (bare, no args — 13×) — as opposed to the already-modeled
  `toHaveBeenCalledTimes`/`toHaveBeenCalledWith`, `toHaveBeenCalledOnce` (15×),
  `toHaveBeenNthCalledWith` (3×), `toBeLessThan`/`toBeLessThanOrEqual`/`toBeGreaterThanOrEqual`
  (4× combined), `toBeTypeOf` (1×), `toBeFunction` (2×, an expect-type helper, not vitest core),
  `instanceOf`/`not.instanceOf` as bare asymmetric-matcher calls used as statements (1× each) —
  `.resolves`/`.rejects` themselves ARE modeled (`lowering/stdlib.rs:770`, matches
  `"resolves" | "rejects"` and builds the `LoweredActual` the matchers.rs doc comment
  describes) — so `rejects.toThrow`/`rejects.toThrowError` (14× combined) only fail today
  because the underlying `toThrow`/`toThrowError` half of the chain hits the same missing-
  matcher gaps listed above (`toThrow` itself is modeled; `toThrowError` is not), not because
  of the `.rejects` modifier.
- `vi.stubGlobal`, `vi.stubEnv`, `vi.unstubAllGlobals`, `vi.unstubAllEnvs`, `vi.resetModules`,
  `vi.mock` — none of these vi.* members appear anywhere in `crates/smelt-frontend-ts/src` or
  `crates/smelt-stdlib`. This matches the brief's §exclude rationale: `vi.stubGlobal`/`vi.spyOn`
  on a global is the monkey-patching model Smelt deliberately does not have.

This is a **rise from a closed 7-matcher enum to double digits of missing matcher spellings**
once the real test closure (not just the non-test crate) is exercised — i.e., step 2 of the
brief ("compile the test build") should expect a large first wave of "unsupported matcher"
blockers purely from matcher-surface gaps, before any generic-language-feature blocker shows up.

## 4. Exclusion candidates: files needing global monkey-patching (`vi.stubGlobal`/`vi.spyOn` on
a global, or `vi.mock`)

No in-scope file calls `vi.mock(...)`. Files using `vi.stubGlobal` (monkey-patches a global
directly — clear exclusion candidates per the brief):

- `src/adapter/service-worker/handler.test.ts:8` — `vi.stubGlobal(...)`
- `src/middleware/cache/index.test.ts:33,386,400,417,446,465,1035,1087` — `vi.stubGlobal('caches', ...)` / `vi.stubGlobal('crypto', ...)`

Files using `vi.spyOn` on a genuine global (as opposed to spying on an app/local object, which
is not a global-monkey-patch problem):

- `src/jsx/context-isolation.test.ts:28` — `vi.spyOn(console, 'warn').mockImplementation(...)`
- `src/middleware/cache/index.test.ts:418` — `vi.spyOn(console, 'log').mockImplementation(...)`
- `src/middleware/jwk/index.test.ts:230` — `vi.spyOn(global.crypto, 'subtle', 'get').mockReturnValue(...)`
- `src/middleware/language/index.test.ts:428,453,471,490` — `vi.spyOn(console, 'error'/'log')`
- `src/middleware/timing/index.test.ts:97,253,273,294` — `vi.spyOn(console, 'warn')`, `vi.spyOn(Date, 'now').mockReturnValue(...)`

Non-global `vi.spyOn` calls (spy on an app/instance/module object — NOT a monkey-patch
candidate by the brief's own distinction, since the receiver is not a global):
- `src/adapter/cloudflare-pages/handler.test.ts:51` — `vi.spyOn(app, 'fetch')`
- `src/middleware/body-limit/index.test.ts:92` — `vi.spyOn(stream, 'getReader')`

Files using `vi.stubEnv`/`vi.unstubAllEnvs` (env var stubbing, arguably a narrower/safer global
than `stubGlobal` but still process-global mutation):
- `src/helper/dev/index.test.ts:134,139`
- `src/middleware/logger/index.test.ts:134,159`
- `src/utils/color.test.ts:13,17`

Files using `vi.unstubAllGlobals`/`vi.resetModules` (paired with the stubGlobal calls above,
already counted there): `src/adapter/service-worker/handler.test.ts:26`,
`src/jsx/context-isolation.test.ts:34`, `src/middleware/cache/index.test.ts:413,430`.

**Recommended exclusion-candidate set** (files whose vitest usage is a genuine global
monkey-patch, beyond the two client tests already excluded):
1. `src/middleware/cache/index.test.ts` — `vi.stubGlobal('caches'/'crypto', ...)` (heaviest user, 8 call sites)
2. `src/adapter/service-worker/handler.test.ts` — `vi.stubGlobal(...)` + `unstubAllGlobals`
3. `src/middleware/jwk/index.test.ts` — `vi.spyOn(global.crypto, 'subtle', 'get')` (also excluded jwt dir is separate; jwk is not currently excluded)

`console.*` spies (`context-isolation.test.ts`, `cache/index.test.ts`, `language/index.test.ts`,
`timing/index.test.ts`) and `Date.now` spy (`timing/index.test.ts`) are lower-priority
candidates — worth flagging to the brief owner as borderline: spying on `console`/`Date` is
common test hygiene rather than testing dynamic-dispatch semantics, so these may be worth a
scoped model (e.g. an erased-console/erased-Date host object with mock semantics) rather than
blanket exclusion. That decision is for whoever runs step 1(b)/1(c) of the brief, not this
survey.

## 5. Glob bug confirmation

`crates/smelt-transpiler/src/lowering.rs`, `discover_test_paths` (line ~586) calls
`collect_matching_test_paths(&absolute_root, &absolute_root, config.test_globs(), &mut paths)`
once per source root. Inside `collect_matching_test_paths` (line ~612), each candidate file's
path is stripped of the **root** prefix (`path.strip_prefix(root)`, where `root` is the
absolute source root, e.g. `.../hono/src`), and the glob patterns are matched against that
root-relative path. So for Hono's `roots = ["src"]` and `test-prefix = ["src/**/*.test.ts"]`,
a file at `src/hono.test.ts` becomes the root-relative path `hono.test.ts`, and the pattern
`src/**/*.test.ts` requires a literal leading `src/` segment that the root-relative path no
longer has — the pattern can never match anything under this root. This is exactly what
`hono-current.md`'s probe report shows (0 files with blockers, and separately, no tests are
emitted) and what the brief describes.

remeda's overlay (`.github/compat/remeda/Smelt.toml`) sidesteps this by spelling its glob with
no literal root segment: `roots = ["packages/remeda/src"]`, `test-prefix = ["**/*.test.ts"]` —
a pattern with no hardcoded root name, which matches correctly against the root-relative path
regardless of where the root sits. Hono's overlay would need the same fix, i.e.
`test-prefix = ["**/*.test.ts"]` (or equivalently, `src/**/*.test.ts` matched against paths
relative to the *manifest directory* rather than the root — but the current code matches
relative to the root, so the fix is on the glob side, not the code side, unless the intended
design is changed).
