# Hono phase 3, round 2 — candidate families (orchestrator brief)

Source data: `hono-phase3-round1.md`, `hono-phase3-round1-families.md` (whole-closure `smelt check`
table), and the phase-3 overlay pass (`hono-current.md` top section), which excluded 64 test files
under `phase 3 pending` comments in `.github/compat/hono/Smelt.toml`. Every family below is a
general lowering/codegen rule; none is a Hono spelling. **When a family lands, delete its overlay
lines in the same commit** and re-measure `smelt build` / `cargo check` / `cargo check --tests`.

"Files" = overlay test files the family keeps out (a file can sit behind several families; it is
counted under the one its overlay comment names). "n" = diagnostics in the round-1 closure table
or, for the generated-Rust families, rustc errors measured on the build that included them.

## Head of the queue (in this order)

| # | family | n | files | proposed general rule |
| ---: | --- | ---: | ---: | --- |
| 1 | Sequential `expect(..).toThrow()` duplicates the continuation per assertion (2^N): `src/utils/cookie.test.ts` → 616 MB / 9.5 M-line module, rustc OOM | 1 module | 1 (+ blocks every `cargo check --tests` that contains it) | Lower a `toThrow` assertion as a self-contained expression: run the thunk in its own `catch_unwind`/`Result` scope and assert on the outcome, then fall through to ONE shared continuation. A try/catch whose catch arm does not escape must join, not clone the rest of the body. Fixture: N sequential `toThrow`s → generated size linear in N. |
| 2 | `Context<E>` type-parameter defaults — `Context` written with fewer type arguments than `class Context<E, P, I>` declares | E0107 6473 of 7722 (84 %) on the round-1 221-test slice | all test modules that touch `Context` | Fill omitted type arguments from the declared defaults (`E = any`, `P = any`, `I = {}`) at every type-reference site before arity checking, and drop parameters the class does not carry (`generic_elision.rs`) consistently in the definition and the reference. Includes the collision seen this round: once a test does `new Context(req, ..)`, `compose.rs`/`hono_base.rs` spell the renamed class (`Context_1`, collides with `router/reg-exp-router/node.ts`'s `Context` interface) as the UNRENAMED `Context<SmeltUnknown>` → E0107 + E0308 `Context` vs `Context_1`. The type-argument path must resolve through the symbol, not the source name. |
| 3 | `unknown class method set` — calling a class FIELD whose declared type is a callable interface (`Context.set: Set<E>`) | 8 (7 modules) | 7 (`compose`, `hono`, `types`, `helper/factory`, `language`, `request-id`, `secure-headers`) | A property whose type has call signatures is callable: `obj.field(args)` lowers to a field read + call of its function type (the interface's call signature, overloads resolved as for a function type alias). Same path as a function-typed field; the callable interface only supplies the signature. |
| 4 | `expect(..).toContain(..)` on an erased actual | 7 (7 test files) | 3 primary (`context`, `basic-auth`, `bearer-auth`) + 4 behind other families | Give `toContain` the erased-receiver path the other matchers already have: when the actual is `SmeltUnknown`, dispatch at runtime on string / array / set / map in the prelude (`smelt_to_contain`). A static type still takes the typed path. |

## Remaining families (from `-families.md` and this round's cargo gate)

| family | n | files | proposed general rule |
| --- | ---: | ---: | --- |
| Host streams: `ReadableStream`/`TransformStream`/`(De)CompressionStream` | 7+1+3 | 9 | Scope decision first (`hono-fetch-demand.md`); if in scope, a WHATWG streams model in `smelt-stdlib` (bounded, `Uint8Array` chunks). |
| `callback method X is not lowered into closure bodies yet` (`safe_parse` 24, `header` 2, `param` 1, `has` 1) | 28 | 1 primary (`validator`) + 3 | Lower method calls on a captured receiver inside a closure through the same member-call path as outside it. |
| Dynamic `import()` | 6 | 3 (`preset/quick`, `preset/tiny`, `jsx/context-isolation`; via `utils/color.ts`) | Lower `import(spec)` of a static, in-closure specifier to a resolved `Promise` of the module namespace record. |
| Cross-kind `Context` collision with type arguments (see row 2) + `serve-static` E0308 `Option<f64>` vs `SmeltResponse` (28) | 53 E0107 + 28 E0308 | 10 (conninfo ×8, `bun/server`, `helper/route`, `cloudflare-workers/serve-static`) | Row 2's symbol-resolved type-argument spelling; then re-measure serve-static. |
| Library modules with generated-Rust errors reached only by tests: `utils/accept` (E0308 tuple `Option<Accept>`), `utils/basic-auth` (E0308 `SmeltMatch`), `utils/concurrent` (E0425 capture name, E0605, E0308 future nesting), `helper/cookie`+`utils/cookie` (E0425 temp, E0605, E0308 record value) | ~25 | 4 | Per-code families like rounds 30–33 (fixtures in `examples/typescript/end-to-end/`). |
| `middleware/trailing-slash/index.ts` emits 3 MB / 11k lines from 158 source lines; plain `cargo check` SIGKILLed > 11 GB in 40 s | 1 module | 1 | Same duplication class as row 1 until shown otherwise: measure which construct is cloned. |
| `Request`/`Response` init is an erased value | 4 | 3 | Type the init record at the construction site from the `RequestInit`/`ResponseInit` model instead of erasing it. |
| Build-only: `list unshift item must match`, `new Headers(init)` from erased init, emitter panic `type table does not contain literal operand type Optional(..)` (`emitter/form_data.rs:63`) | 3 | 3 (`combine`, `cors`, `buffer`) | Make `smelt check` run the whole-crate pass that finds these, then fix each; the panic must become a diagnostic. |
| Array/string builtins on erased/optional receivers (`string case`, `concat`, `splice`, index on `Optional`) | ~8 | 5 | Narrow `Optional` receivers before builtin dispatch; erased receivers take the runtime path. |
| `EventTarget`/`Event` + values imported from the excluded `helper/websocket` | 2+1+4 | 3 | Scope: follows the websocket scope decision. |
| Runtime globals `Deno`/`navigator`/`awslambda`, `node:async_hooks` | 4 | 2 (+ aws-lambda) | Scope: model as erased host globals (like other host globals) or keep excluded with a scope reason. |
| `LabeledStatement` | 3 | 1 (`linear-router`) | Lower labelled loops to Rust labelled loops (`'label: loop`), `break label`/`continue label` direct. |
| Test-shape gaps: `call expression is not lowered yet`, non-direct `describe` bodies, modifiers other than `.not`, `.resolves` on a non-Promise, `declare` methods | ~12 | 6 | Extend the vitest model generally (a `describe` body is an ordinary function body whose `it` calls register tests). |
| Tagged templates with a user tag | 2 | 1 (`helper/html`) | Lower `tag\`..\`` to `tag(strings, ...values)` with a frozen strings array. |
| `jsx/**` runtime closure (~15 classes in 10 modules) | ~30 | 1 (`jsx/children`) | After the rows above; re-run `-families.md` on it. |
| `Object.assign` with no source | 1 | 1 (`csrf`) | `Object.assign(t)` is `t`. |
