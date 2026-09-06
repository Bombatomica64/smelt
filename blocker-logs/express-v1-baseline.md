# Express v1 baseline: what transpiles today

Date: 2026-09-06. Head: `31f4866` (main after PR #248).

## Question

Before starting the Express -> axum campaign: how much of "express" does
Smelt already transpile and compile?

## Two different things called "express"

1. **The express library** (`expressjs/express`, `lib/*.js`, ~2.8k lines).
   Plain CommonJS JavaScript: `require`, `module.exports`, `mixin(app,
   EventEmitter.prototype)`, `Object.create(req, ...)`. `smelt probe` stops at
   `unsupported source extension: index.js` before parsing anything. Even
   renamed, it is not strict TypeScript and would not pass `tsc --strict`.
   **Transpiled: 0%, and that is the right answer.** Express is a host library
   to be *mapped* to axum, like `vitest` is mapped to `#[test]`; it is not a
   source project to lower. The README's v1.0 goal says the same ("a real
   Express app ... compiles to a working axum server").

2. **An express app** (strict TS importing `express`). Probed with a 45-line
   in-memory todo app (`express()`, `express.json()`, `app.get/post/use`,
   `req.params`, `req.body`, `res.status().json()`, error middleware,
   `app.listen`, `process.env.PORT`).

## Result for the app probe

| step | result |
| --- | --- |
| `smelt probe` | "Transpile: yes, 0 blockers" |
| `cargo check` of the emitted crate | compiles, 5 warnings |
| SmeltUnknown report | 1283 occurrences, **47 avoidable** in 45 source lines |
| runtime behaviour | program does nothing |

The clean probe is a false green. `import express from "express"` is an
unresolved bare package, and `module_init.rs::import_declaration` falls back
to inserting the binding as a module global of `Type::Unknown`. From there:

```rust
let _smelt_tmp_5: SmeltRecord<String, SmeltUnknown> = SmeltRecord::from([]);   // `express`
let _smelt_tmp_6: SmeltUnknown = SmeltUnknown::Object(SmeltObject::from_unknown_record(..));
let _smelt_tmp_7: SmeltUnknown = { /* look up "__smelt_call" on it */ .. else { SmeltUnknown::Null } };
let app: SmeltUnknown = _smelt_tmp_7;                                             // Null
let _smelt_tmp_8: SmeltUnknown = smelt_get_unknown_field(&app.clone(), "use");   // Null
```

Every handler parameter (`req`, `res`, `next`, `err`) is `SmeltUnknown`;
`res.status(404)` is `smelt_get_unknown_field(&closure_arg_1, "status")` on
`Null`. The generated `main` builds the todo list and then performs a chain of
no-op dynamic lookups. Nothing listens on a port.

This is exactly the erasure CLAUDE.md forbids ("never use SmeltUnknown merely
to make generated Rust compile"), sitting at the import boundary rather than in
user code, which is why the examples/es-toolkit ratchets never saw it.

## Result for the reference app (`examples/typescript/express_crud/`)

Six files, 356 lines: `express()` app factory, `Router`, typed
`Request<Params, ResBody, ReqBody>`, hand validation of `unknown` bodies into
a `Validated<T>` union, a `TodoRepository` class over `node:sqlite`
`DatabaseSync`, vitest + supertest tests. `tsc --strict` clean, 8/8 tests pass
under Node.

| variant | probe | `cargo check` | avoidable erasure | emitted user code |
| --- | --- | --- | --- | --- |
| as written (NodeNext `./app.js` specifiers) | yes, 0 blockers | 1 error | 9 | `main` only, 2679 lines |
| same source with extensionless specifiers | yes, 0 blockers | 2 errors | 407 | structs, `TodoRepository`, `main`; no free functions |

Three more findings, each a silent false green rather than a blocker:

3. **NodeNext `.js` import specifiers drop modules.** `import { createApp }
   from './app.js'` (the standard ESM-in-TS spelling under
   `moduleResolution: NodeNext`) does not resolve to `app.ts`, so the probe
   scans 6 files but the crate contains only `main`, with `createApp` and
   `openDatabase` degraded to `Type::Unknown` callables. The manifest
   resolver must map `.js`/`.mjs`/`.cjs` specifiers to `.ts`/`.mts`/`.cts`
   the way `tsc` does.
4. **Free functions are dropped, call sites are kept.** With extensionless
   specifiers, `open_database(..)`, `create_app(..)` and `row_to_todo(..)` are
   called from `main` and `TodoRepository`, but no definition is emitted for
   any of the app's free functions (`createApp`, `openDatabase`,
   `createTodosRouter`, `rowToTodo`, `parseId`, the two validators). Only the
   class and `main` survive. Whatever suppresses them must become a named
   blocker.
5. **`process.env.PORT ?? 3000`** emits `let _smelt_tmp_3: String = 3000.0;`
   (E0308): the `??` join of `string | undefined` with a numeric literal picks
   the left arm's type for the right arm's value instead of the
   `string | number` union that `Number(..)` accepts.

## What exists that a campaign can build on

- `Smelt.toml` scaffolds already declare `axum`, `tokio`, `serde`,
  `serde_json` under `[rust.dependencies]`, but **no crate in the workspace
  mentions axum**: the emitter never consumes those dependencies.
- Host-library precedent: `test_support::is_vitest_compatible_module` +
  `is_vitest_builtin_name` recognise the `vitest` module by name and route
  its imports to `#[test]` lowering. That is the shape a host-library mapping
  takes today. It is spelled per module name, and there is one more of those
  in `import_declaration`: a `@date-fns/tz` + `tz` rule, which is a
  library-spelling special case by the "Type lowering" rule and should be
  folded into whatever general host-module mechanism express gets.
- `node:` builtins used by the app (`process.env`) already lower.
- Async/Promise lowering, closures with typed params, interfaces, records,
  JSON (`JsonParse`), and the identity-keyed object model from the es-toolkit
  campaign are all in place, so the *handler bodies* are not the problem; the
  framework surface is.

## Where the campaign starts

1. **Stop the false green.** An unresolved bare package import that is used as
   a value must be a named blocker ("unresolved package `express`"), not
   `Type::Unknown`. This alone turns the 0-blocker probe into an honest report.
2. **Host module declarations.** A general mechanism that gives a bare package
   a typed surface (types for `Express`, `Router`, `Request<P,ResB,ReqB>`,
   `Response`, `NextFunction`; values `express()`, `express.json()`,
   `express.Router()`, `app.get/post/patch/delete/use/listen`,
   `router.*`, `req.params/body/query`, `res.status/json/send/end`,
   `next(err)`), sourced from the package's own `.d.ts` where possible
   rather than hand-listed per method.
3. **axum emission** for that surface: `Router::new().route(..)` per method,
   `Json<T>` extractor for typed bodies, `Path<P>` for params,
   `StatusCode` + `Json` for responses, error middleware as a fallible
   handler result, `app.listen` as `tokio::main` + `axum::serve`.
4. **Reference app**: `examples/typescript/express_crud/` (todos CRUD over
   `node:sqlite`, vitest + supertest tests). Its probe is the first gate; its
   generated crate serving the same routes is the v1 showcase. `node:sqlite`
   is a second host module (another agent owns the database mapping), so the
   host-module mechanism in item 2 has two consumers from day one.

Items 1, 3 and 4 are honesty fixes and should land first, so every later
probe number is real. Item 5 is an ordinary lowering bug.
