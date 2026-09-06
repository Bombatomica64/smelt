# Hono campaign — scope: what is in, what is out, and why

Campaign plan §3 says "include by evidence, exclude with a reason". This file
records the evidence. Fixture: `.github/compat/hono/Smelt.toml`, copied over a
checkout of `honojs/hono@eebdf7be39abf0a872671835ccce0c4f03ea497a`.

Authority for every claim below is
`smelt --manifest-path third_party/hono/Smelt.toml check --message-format json`
— the whole-crate recoverable lowering, which resolves imports. A per-file
`smelt dump-hir` sweep reports far more (cross-module names are unresolved in
single-file mode) and was used only to locate spans within a file.

Baseline: 288 files scanned, 14 with blockers, 32 occurrences, 13 distinct
shapes.

## 1. The plan's expected excludes, tested rather than assumed

Four of the five candidates the plan expected to exclude **do not need
excluding**. Host globals are modeled as erased value closures and namespaces,
so files that mention `Deno`, `Bun`, JSX factories or DOM helpers still lower.

| candidate | expected | actual | in scope? |
| --- | --- | --- | --- |
| `src/jsx/**` | exclude (JSX/DOM) | 0 manifest blockers. The `.tsx` files are not `.ts` and are never discovered; the `.ts` files lower. | **in** |
| `src/adapter/**` | exclude (Deno/Bun/Lambda globals) | 0 manifest blockers, even though a per-file sweep shows `Deno`, `Bun`, `awslambda`, `WebSocketPair`, `EventTarget`. | **in** |
| `src/helper/{html,css,ssg,jsx-renderer}` | exclude | 0 manifest blockers. | **in** |
| `src/middleware/{jwt,jwk}`, `src/utils/jwt/**` | exclude (`crypto.subtle`) | 0 manifest blockers — `crypto` resolves as an erased namespace here; the `crypto` blockers land in `utils/cookie.ts` instead. | **in** |
| `src/client/**` | exclude (`new Proxy`) | 4 manifest blockers. Cannot be excluded — see §2. | **in, unfixed** |

`src/helper/websocket` and `src/helper/streaming` also produce no manifest
blockers, so they stay in scope too, but they will need `WebSocket`,
`TransformStream` and `WritableStream` at `cargo check` time and none of those
is on the standards stream's list (see `hono-fetch-demand.md` §5). Expect them
to move to the exclude list on phase-2 evidence.

## 2. `src/client/**` cannot be excluded — a plan item that is not possible as designed

The plan assigns `src/client/**` an exclude with the reason "RPC client built on
`new Proxy` (a Smelt non-goal)". That reason is correct —
`src/client/client.ts:16` is `new Proxy(() => {}, { get })`, resolving
`client.posts.$get` by intercepting property reads — but the mechanism does not
reach it.

`[sources] exclude` is applied in exactly two places
(`crates/smelt-transpiler/src/lowering.rs:416` and `:522`), both over **root**
paths: the `entries` plus the `test-prefix` globs. It runs *before*
`dependency_closure`, so a module reached transitively is never filtered. And
`src/client/**` is reached transitively twice:

* `src/index.ts:48` — `export type { InferRequestType, InferResponseType, ClientRequestOptions } from './client'`
* `src/helper/testing/index.ts:6` — `import { hc } from '../../client'` (a **value** import, not type-only)

So excluding it would require excluding the framework entry point and the
testing helper as well.

**What I did:** excluded only the two leaf test files
(`src/client/{client,utils}.test.ts`), which the mechanism can drop and which
additionally use `vi.stubGlobal`. The three non-leaf modules stay in scope, and
their 4 blockers stay counted:

| file | blocker |
| --- | --- |
| `src/client/types.ts` | `rest parameter type must resolve to an array type` (`ConstructorParameters<typeof WebSocket>`) |
| `src/client/utils.ts` | `string replace requires string-compatible receiver, pattern, and replacement` |
| `src/client/client.ts` | `unresolved class FormData` |
| `src/client/client.ts` | `unresolved identifier proxyCallback` |

**Alternatives, for the orchestrator to choose between — I did not pick one:**

1. **Make `exclude` prune the dependency closure.** An excluded module's
   importers would then see unresolved imports, which is a new blocker class
   rather than a fix, unless type-only re-exports are tolerated. Needs design.
2. **Ship a fixture patch.** `.github/compat/<lib>/` is copied *over* the
   checkout, so the fixture could carry a `src/index.ts` without the type
   re-export and a `src/helper/testing/index.ts` without `hc`. There is
   precedent for a small fixture patch (the radash job `sed`s a test file), but
   patching library *source* to remove a public export misrepresents the corpus
   more than an exclude does.
3. **Implement the `Proxy` get-trap.** The plan calls it a Smelt non-goal.
4. **Leave it in scope and report 4 permanent blockers**, stating phase 1's
   metric as "0 outside `src/client/**`". This is what the current fixture
   does.

## 3. Test-level exclusions

The plan asks for `vi.stubGlobal` (12 uses) and `vi.useFakeTimers` (4) to be
listed individually rather than excluding whole files. Found in 5 files:

| file | reason | status |
| --- | --- | --- |
| `src/client/client.test.ts` | `vi.stubGlobal` | excluded (also the `Proxy` client) |
| `src/middleware/cache/index.test.ts` | `vi.stubGlobal` | in scope; produces no lowering blocker, so left for phase 3 evidence |
| `src/adapter/service-worker/handler.test.ts` | `vi.stubGlobal` | in scope, same |
| `src/helper/testing/index.test.ts` | `vi.stubGlobal` | in scope, same |
| `src/utils/jwt/jwt.test.ts` | `vi.useFakeTimers` | in scope, same |

None of these blocks lowering, so none is excluded yet: a `vi.stubGlobal` case
that cannot run is a phase-3 (runtime) fact, and excluding a whole file now
would hide the cases in it that do work.

## 4. Ownership of the remaining blockers

After families H1–H4, H7, H10, H11 and H12, 32 occurrences are down to 12, and
every one is either standards-stream demand or listed above:

| owner | count | detail |
| --- | ---: | --- |
| standards stream | 8 | `Response` ×2, `Headers`, `BodyInit` ×2, `TextEncoder` ×2, `crypto` ×2, and `Request.url` needing to be `string` — see `hono-fetch-demand.md` §1 |
| `src/client/**` | 4 | §2 above |
| mine, remaining | 3 | `atob` ×2 (`utils/cookie.ts`, blocked behind `crypto`/`TextEncoder` in the same functions), and the module-mutable-global initializer in `router/reg-exp-router/router.ts` |
| needs a decision | 1 | `addEventListener` in `hono-base.ts` — see `hono-h10-uri-and-base64-globals.md` §8 |

(The counts overlap: `atob` ×2 and the standards items sit in the same two
`utils/cookie.ts` functions.)

**Phase 1's metric cannot reach 0 from this stream alone.** Eight of the twelve
remaining occurrences are names the campaign plan §6 forbids me to model.
