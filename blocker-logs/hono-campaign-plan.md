# Hono campaign: transpile the framework and its tests

Owner: Hono implementer (Opus). Architecture: Fable. Date: 2026-09-06.
Runs in parallel with `blocker-logs/standards-tier-plan.md` (the "standards stream").

## 1. Why Hono

Express is JavaScript and can only ever be a mapped host library. Hono is pure TypeScript with
zero runtime dependencies; its only external imports are Node builtins and web standards. It is
the corpus that proves Smelt can transpile a *framework*, not only utilities: routers (trie,
regexp, pattern), middleware composition, a context object, cookies, validators, and 2081
vitest cases that drive it with real `Request` objects (374 `new Request(`, 964
`app.request(`). It joins es-toolkit/remeda/radash as a gate.

Pinned ref: `honojs/hono` @ `eebdf7be39abf0a872671835ccce0c4f03ea497a` (v4.13.7).
Baseline probe on 2026-09-06 (core `src/`, entry `src/index.ts`): 288 files scanned, 14 with
blockers, 32 occurrences, 13 distinct shapes. Whole-crate build aborts at
`src/http-exception.ts`.

## 2. Setup

```bash
git clone --no-tags --filter=blob:none https://github.com/honojs/hono.git third_party/hono
git -C third_party/hono checkout eebdf7be39abf0a872671835ccce0c4f03ea497a
```
Create `.github/compat/hono/Smelt.toml` (copied over the checkout like the other gates):
roots `["src"]`, entry `src/index.ts`, `test-prefix = ["src/**/*.test.ts"]`, crate
`hono_probe`, output `./dist-smelt`. Add `third_party/hono/` to `.gitignore` and `hono` to
`.github/compat/libraries.json` (lang ts, roots `["src"]`) so the daily probe tracks it.

## 3. Scope: include by evidence, exclude with a reason

Start from the whole `src/` and exclude only what a probe or build shows cannot be in the
non-DOM Node profile, each with a one-line justification in `Smelt.toml` exactly like
`.github/compat/es-toolkit/Smelt.toml`. Expected excludes, to be confirmed by probing rather
than assumed:

- `src/jsx/**` and `src/helper/{html,css,ssg,jsx-renderer}` JSX/DOM surfaces (`.tsx`, DOM).
- `src/adapter/**` except none: Cloudflare/Deno/Bun/Lambda/Vercel/Netlify host globals
  (`Deno`, `Bun`, `caches`, `env`) are not in the profile. Keep `src/adapter` out unless a
  file is plain TS.
- `src/client/**`: RPC client built on `new Proxy` (a Smelt non-goal).
- `src/middleware/{jwt,jwk}` and `src/utils/jwt/**`: `crypto.subtle.importKey/sign/verify`,
  out of scope for the standards stream this round.
- `src/helper/websocket`, `src/helper/streaming` if they need `WebSocket`/unbounded streams.
- Tests using `vi.stubGlobal` (12) / `vi.useFakeTimers` (4) that cannot run under the Rust
  `vi` model: list them individually, do not exclude whole files for one case.

Everything else (hono.ts, hono-base.ts, context.ts, request.ts, compose.ts, router/**,
utils/**, validator/**, the remaining middleware, helper/{cookie,factory,route,testing,
accepts,conninfo,proxy,dev}) is in scope.

## 4. Blocker families (all general rules; no Hono spelling anywhere)

Investigate each, write the finding into `blocker-logs/hono-<family>.md` (wrong output,
responsible function, design) before fixing, in this order:

| # | shape (occurrences, example file) | direction |
| --- | --- | --- |
| H1 | `call expression is not lowered yet` (4, `context.ts`) | identify the callee shapes; likely optional-call `fn?.()`, calls on getters, or `super.method()` chains. General lowering. |
| H2 | `regex replacement callback must accept a match string and return a string` (4, `router/reg-exp-router/prepared-router.ts`) | replacement callbacks `(match, p1, p2, ...) => string` with capture-group params and `offset`/`string` trailing params, per ECMA-262 `GetSubstitution` callback signature. Extend the existing `js_regex.rs` substitution path; the callback receives captures as `Option<String>` for unmatched groups. |
| H3 | `tuple element type is not lowered yet: TSIntersectionType` (2, `types.ts`) | tuple elements are ordinary types; route through the same intersection lowering the es-toolkit campaign added (structural intersection). |
| H4 | `condition expression must be boolean or optional (Union)` (1, `matcher.ts`) | ToBoolean on unions of truthy-testable arms; the campaign's `ValueTruthy` covers primitives, extend to unions whose every arm has a truthiness rule. |
| H5 | `JSON.stringify() value must be JSON-serializable` (2, `request.ts`) | value is a typed union/record with an `unknown` member; serialization of `unknown` is a real dynamic boundary and already has a runtime path, route unions through it. |
| H6 | `module-level mutable binding initializer must be a literal for now` (1, `reg-exp-router/router.ts`) | lazily initialised module state (`let x: T = expr`): lower to `thread_local!` + `RefCell` initialised by the expression, the same shape module `const` non-literals use. |
| H7 | `exported const expression references unresolved const` (1, `utils/url.ts`) | cross-module const folding order; resolve through the crate item like the import-time literal folding added for es-toolkit (`alias_imported_item`). |
| H8 | `rest parameter type must resolve to an array type` (1, `client/types.ts`) | `...args: Parameters<F>`-style; likely excluded with `client/`, confirm. |
| H9 | `string replace/search requires string receiver` (2, `client/utils.ts`, `utils/url.ts`) | receiver is `string | undefined` or a template-literal type; narrow/normalize before the string rule, never fall back to Unknown. |
| H10 | `unresolved identifier`/`unresolved class` (9, `client/client.ts`, `utils/cookie.ts`) | in `cookie.ts` it is likely `decodeURIComponent_`-style helpers or `Date`; in `client/` it is `Proxy`/`fetch` typing. Cookie must lower; client is excluded. |

New blockers will surface as excludes are removed and as the standards stream lands concrete
`Request`/`Response`/`Headers` types (members Hono uses that are not implemented become
compile errors in the generated crate). Treat each as a new numbered family in the plan's
progress log; report members you need from the standards stream instead of modelling them.

## 5. Phases and metrics

1. **Source lowering**: `smelt probe` on the fixture reports 0 files with blockers for the
   in-scope set. Metric: files with blockers 14 -> 0.
2. **Generated crate compiles**: `cargo check` of `dist-smelt` is clean for the non-test build
   (`test-prefix` off). Anything here that is a missing fetch-type member is demand for the
   standards stream: list it in `blocker-logs/hono-fetch-demand.md` with counts; do not
   implement it.
3. **Tests**: after the standards stream merges (the orchestrator will tell you), turn
   `test-prefix` on and run `smelt rust-test-report --full` against the crate. Metric: passed /
   failed / newly failing, baseline into `blocker-logs/hono-current.md`. 2081 cases is the
   ceiling; report the honest in-scope number.
4. **Erasure**: `smelt smelt-unknown-report dist-smelt/src --format json --output blocker-logs/smelt-unknown-baseline-hono.json`
   as an advisory baseline (remeda-style) and a paragraph in the report about the top avoidable
   shapes.
5. **CI**: add a `hono` regression job to `.github/workflows/ci.yml` mirroring the radash job
   (clone at the pin, copy `.github/compat/hono/.`, build, `cargo test --no-fail-fast` the
   generated crate) under the `run-regressions` label, marked advisory (`continue-on-error`)
   until phase 3 has a stable number.

## 6. Contract with the standards stream

You never model `Headers`, `Request`, `Response`, `URL`, `URLSearchParams`, `FormData`,
`Blob`, `File`, `TextEncoder`, `TextDecoder`, `ReadableStream`, `AbortController`,
`AbortSignal`, `crypto`, or anything from `node:http`. Those names are owned by
`standards-tier-plan.md`. You may read their current (marker/erased) behaviour to keep
lowering, and you record demand. Everything in section 4 is yours and must be a general rule
that would also fire for es-toolkit/remeda/radash source of the same shape; those three gates
stay green.
