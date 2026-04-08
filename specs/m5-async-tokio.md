# M5: Async & Tokio Lowering

**Milestone:** v1.0
**Estimated duration:** 4–5 weeks
**Depends on:** M4

## Goal

Translate `async`/`await` and `Promise<T>` into Rust async code that runs on Tokio.

## Why this matters

The v1.0 demos (Express and FastAPI apps) are inherently async. Without this milestone, neither headline demo can work. This is also the first place we touch a real Rust runtime, which surfaces decisions that affect every later milestone.

## Scope

- `Type::Future(T)` lowers to `impl Future<Output = T>` for return positions.
- For locals and fields holding futures, use `Pin<Box<dyn Future<Output = T> + Send>>` as a default. v1.0 prefers correctness over zero-cost.
- `await` in MIR lowers to `.await` in Rust.
- `is_async: true` functions become `async fn`.
- The generated `main` function is wrapped in `#[tokio::main]` if any async is reachable from it.
- Map `Promise.all([a, b, c])` (TS) and `asyncio.gather(a, b, c)` (Python, in M8) to `tokio::join!` or `futures::future::join_all` depending on whether the call is fixed-arity or variadic.
- Error propagation through async boundaries: `Result` returns work the same way; `?` operator emitted where appropriate.

## Tests

- 15+ golden tests with async TS programs that compile and run.
- At least one test for sequential await chains.
- At least one test for parallel awaits via `Promise.all`.
- At least one test for error propagation across async boundaries.
- At least one test where an async function calls a sync function.
- At least one test where a sync function calls an async function (must error gracefully — this is illegal).

## Tokio Version Pinning

The generated `Cargo.toml` pins a specific Tokio minor version. Document the choice in the milestone PR. v1.0 should default to `tokio = { version = "1", features = ["full"] }` unless there's a strong reason otherwise.

## Exit Criteria

- [ ] All async tests in the golden suite compile and run.
- [ ] `Promise<T>` and `Promise.all` lower correctly.
- [ ] Generated async code passes `cargo clippy`.
- [ ] Calling async from sync (forbidden) produces a clear error at the smelt level, not a confusing Rust compile error.
- [ ] Documentation update in `specs/architecture.md` describing the async lowering strategy.

## Out of Scope

- Custom executors (we only support Tokio in v1.0).
- `Send`/`Sync` analysis (we just slap `Send` on everything and let `cargo build` complain if it's wrong).
- Streams / async iterators (deferred).
- Cancellation safety analysis (deferred).
