# M9: FastAPI & Pydantic Mapping (Python Demo)

**Milestone:** v1.0
**Estimated duration:** 5–7 weeks
**Depends on:** M7, M8

## Goal

The headline Python demo: a real FastAPI app with Pydantic models transpiles to a working `axum` server. The output should be **structurally similar** to the M7 Express demo's output, validating that the shared HIR was the right call.

## Why this matters

Two things need to be true at the end of v1.0: that smelt works for both languages, and that both languages converge on the same backend. M9 is where we prove the second one. If the Express demo and the FastAPI demo produce wildly different Rust code, the abstraction is leaking and we have a design problem.

## The Demo App

A FastAPI version of the M7 Express demo, with the same routes:

- `GET /health`
- `GET /users/{id}`
- `POST /users`
- `GET /users` with `?limit=` and `?offset=`
- An async route simulating a DB call

Constraints:
- Pydantic models for all request/response bodies.
- Type hints everywhere; passes `ty` strict.
- Logging configured via `logging` (not Python's `print`).
- Exception handlers registered for HTTP errors.

Source lives in `examples/fastapi-demo/`.

## Pydantic → Rust Struct Mapping

| Pydantic                                | Rust                                                          |
| --------------------------------------- | ------------------------------------------------------------- |
| `class User(BaseModel)`                 | `#[derive(Serialize, Deserialize)] struct User { ... }`       |
| `name: str`                             | `name: String`                                                |
| `age: int = 0`                          | `#[serde(default)] age: i64`                                  |
| `email: Optional[str] = None`           | `#[serde(default)] email: Option<String>`                     |
| `tags: list[str] = []`                  | `#[serde(default)] tags: Vec<String>`                         |
| `Field(..., min_length=1)`              | Custom validator function called in the handler               |
| `model_validator`                       | Custom validation in handler (no direct mapping)              |
| Nested Pydantic models                  | Nested structs with the same derive                           |

Pydantic's runtime validation features that have no direct Rust equivalent are emitted as validator functions called explicitly in handlers. Document the limits clearly.

## FastAPI → Axum Mapping

| FastAPI                                          | Axum                                                       |
| ------------------------------------------------ | ---------------------------------------------------------- |
| `app = FastAPI()`                                | `let app = Router::new()`                                  |
| `@app.get("/path")`                              | `.route("/path", get(handler))`                            |
| `@app.post("/path")`                             | `.route("/path", post(handler))`                           |
| `path parameter: int`                            | `Path(param): Path<i64>` extractor                         |
| Query parameters                                 | `Query(params): Query<...>` extractor                      |
| Request body with Pydantic model                 | `Json(body): Json<Model>` extractor                        |
| Return value                                     | `Json(value)` return                                       |
| `HTTPException(404, "...")`                      | Custom error type implementing `IntoResponse`              |
| `@app.exception_handler(...)`                    | Mapped to a handler in the custom error type               |
| `uvicorn.run(app, ...)`                          | `axum::serve(...).await`                                   |

The decorator recognizer pass extends the M8 frontend: when `@app.get(...)` etc. are seen on a function, smelt tags the HIR function with route metadata. The codegen layer (extending M7) reads the metadata and emits the route registration regardless of which frontend produced it.

## The Cross-Language Convergence Test

This is the key M9 test: take the Express demo and the FastAPI demo and dump their HIR. They should be **structurally equivalent** modulo identifier names. Differences indicate either bugs or genuinely needed HIR features. Either way, document them.

A second test: take the Rust output of both demos and run the same integration test suite (the same `reqwest`-based HTTP tests) against both. They should produce identical responses.

## Exit Criteria

- [ ] FastAPI demo compiles via smelt with no errors.
- [ ] Generated Rust crate `cargo build`s without warnings.
- [ ] Generated binary passes the same integration test suite as the Express demo.
- [ ] HIR convergence test: Express and FastAPI demos produce structurally equivalent HIR.
- [ ] FastAPI/Pydantic mapping documented in `specs/fastapi-mapping.md`.
- [ ] At least one route in each demo uses a feature unique to its language to verify graceful handling.

## Out of Scope

- FastAPI features beyond the listed mappings.
- Dependency injection (`Depends(...)`) — deferred to v1.1.
- WebSockets, streaming responses, file uploads.
- Pydantic v1 — v1.0 supports Pydantic v2 only.
- SQLAlchemy / database ORMs (huge scope; deferred).

## Notes

If the convergence test fails badly, **stop and fix the architecture before v1.0 ships**. The whole project rests on the shared-HIR claim. Better to slip the release than to ship with a foundation we know is wrong.
