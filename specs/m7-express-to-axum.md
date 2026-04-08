# M7: Express → Axum Mapping (TypeScript Demo)

**Milestone:** v1.0
**Estimated duration:** 6–8 weeks
**Depends on:** M6

## Goal

The headline TypeScript demo: a real Express app transpiles into a working `axum` server that compiles, runs, and passes integration tests.

## Why this matters

This is the v1.0 release-blocker for the TypeScript side. Everything before this milestone has been infrastructure; this is the first time someone could plausibly use smelt for something they actually want to build. It also stress-tests every previous milestone in combination — async, stdlib mapping, classes, generics, error handling — under one realistic workload.

## The Demo App

A small Express app with at least the following routes:

- `GET /health` — returns `{ status: "ok" }`
- `GET /users/:id` — returns a user by ID, 404 if not found
- `POST /users` — accepts a JSON body, validates it, returns the created user
- `GET /users` — returns all users, supports `?limit=` and `?offset=` query params
- One async route that simulates a DB call with `await`

It must use:
- Strict TypeScript with no `any`
- Typed request/response bodies via interfaces
- A logging middleware
- An error-handling middleware that catches thrown errors and returns 500

The full source for the demo lives in `examples/express-demo/`.

## Express → Axum Mapping

| Express                              | Axum                                                       |
| ------------------------------------ | ---------------------------------------------------------- |
| `const app = express()`              | `let app = Router::new()`                                  |
| `app.get(path, handler)`             | `.route(path, get(handler))`                               |
| `app.post(path, handler)`            | `.route(path, post(handler))`                              |
| `app.use(middleware)`                | `.layer(middleware_fn)`                                    |
| `req.params.id`                      | `Path(id): Path<...>` extractor                            |
| `req.query`                          | `Query(params): Query<...>` extractor                      |
| `req.body` (JSON)                    | `Json(body): Json<...>` extractor                          |
| `res.json(obj)`                      | `Json(obj)` return                                         |
| `res.status(404).json(...)`          | `(StatusCode::NOT_FOUND, Json(...))` return                |
| `app.listen(port)`                   | `axum::serve(listener, app).await`                         |
| Thrown error → 500                   | `Result<impl IntoResponse, AppError>` with custom `AppError` |

The mapping is implemented as a recognizer pass that runs on HIR: when smelt sees a call to a known Express API, it tags the node so codegen knows to emit the axum equivalent. The recognizer requires that `express` is imported from a known shape (a `.d.ts`-style declaration shipped with smelt).

## Integration Tests

The integration test suite spins up the compiled binary, hits it with HTTP requests via `reqwest`, and asserts on responses:

- `GET /health` returns 200 with the expected JSON
- `GET /users/1` returns 200 for a known ID
- `GET /users/9999` returns 404
- `POST /users` with valid body returns 201 and the created user
- `POST /users` with invalid body returns 400
- `GET /users?limit=2&offset=1` returns the right slice
- The async route completes and returns the expected payload

## Exit Criteria

- [ ] The demo app's TypeScript source compiles via smelt with no errors.
- [ ] The generated Rust crate `cargo build`s with no warnings.
- [ ] The generated binary runs and passes the full integration test suite.
- [ ] The generated Rust is human-readable enough that a Rust developer can navigate it. ("Readable" is judged by an actual code review, not metrics.)
- [ ] The Express → Axum mapping is documented in `specs/express-mapping.md`.
- [ ] CI runs the integration suite on every push touching `examples/express-demo/`.

## Out of Scope

- Express features beyond the listed mappings.
- Templates / view engines.
- Sessions, cookies, auth (unless trivial to support).
- WebSockets.
- File uploads.

## Notes

This milestone is where unanticipated problems will surface. Budget time generously and expect at least one previous milestone to need fixes. That's normal — it's the whole point of having an integration milestone.
