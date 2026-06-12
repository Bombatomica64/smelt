# Compile-the-output corpus findings

This log records real compile failures found by the codegen compile tier in
`crates/smelt-codegen-rust/tests/compile_corpus.rs`. Each entry corresponds to a
case excluded from the green corpus via `KNOWN_COMPILE_FAILURES`. The tier is
additive only — these are emitter bugs to fix later, not in the tier change.

How the tier was run:

```sh
cargo test -p smelt-codegen-rust --test compile_corpus -- --ignored
```

---

## 1. async/Promise return value is not wrapped in `Ok(...)` (case: `async_await`)

Area: `async`

Source lowered (from `examples/typescript/hir/09_async_function.ts`):

```ts
async function lift(value: number): Promise<number> {
  return value;
}

async function run(): Promise<number> {
  return await lift(5);
}
```

The async emitter gives async functions a fallible Rust signature
`async fn lift(value: f64) -> Result<f64, Box<dyn std::error::Error>>` (so that
`await` / `?` can propagate errors), but the `return` statement still emits the
bare value rather than wrapping it in `Ok(...)`. The generated body is therefore
ill-typed.

`cargo check` output:

```
error[E0308]: mismatched types
 --> src/main.rs:5:12
  |
4 | async fn lift(value: f64) -> Result<f64, Box<dyn std::error::Error>> {
  |                              --------------------------------------- expected `Result<f64, Box<(dyn std::error::Error + 'static)>>` because of return type
5 |     return value.clone();
  |            ^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
help: try wrapping the expression in `Ok`

error[E0308]: mismatched types
  --> src/main.rs:12:12
   |
 8 | async fn run() -> Result<f64, Box<dyn std::error::Error>> {
   |                   --------------------------------------- expected ... because of return type
...
12 |     return _smelt_tmp_1.clone();
   |            ^^^^^^^^^^^^^^^^^^^^ expected `Result<f64, Box<dyn Error>>`, found `f64`
```

Root cause: the async function return-type lowering (fallible `Result<...>`
signature) and the `return` statement emission are out of sync — `return <expr>`
in an async function body must emit `return Ok(<expr>);` to match the synthesized
`Result` return type. This is exactly the kind of async-lowering ABI mismatch the
compile tier was added to surface.

Status: excluded from the green corpus via `KNOWN_COMPILE_FAILURES` until the
async emitter wraps returns in `Ok(...)` (or stops synthesizing a `Result`
return type for infallible async bodies).
