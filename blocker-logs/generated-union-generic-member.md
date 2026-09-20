# A generic member in a generated union does not compile

**Severity: compile break in the generated crate.** `smelt build` succeeds and
`cargo build` on the emitted crate fails with two errors. Found in standards
round 5 while closing the `ResponseOrInit` demand (item 3): the *lowering*
blocker there is fixed, and this is the next layer down.

Not fixed by the standards stream: it is in the generated-union emitter, not in
the fetch types. The equivalent union with a **non-generic** member compiles and
runs correctly, which is what narrows it.

## Repro

`src/main.ts`:

```ts
type StatusCode = 200 | 201 | 404 | 500;

interface ResponseInit<T extends StatusCode = StatusCode> {
  status?: T;
  statusText?: string;
  headers?: Headers;
}

type ResponseOrInit<T extends StatusCode = StatusCode> = ResponseInit<T> | Response;

export function pickStatus(arg?: StatusCode | ResponseOrInit): number {
  return typeof arg === "number" ? arg : (arg?.status ?? 200);
}

console.log(pickStatus(404));
console.log(pickStatus({ status: 201 }));
console.log(pickStatus(undefined));
console.log(pickStatus(new Response("x", { status: 500 })));
```

```
$ smelt --manifest-path Smelt.toml build   # succeeds
$ cd dist && cargo build --message-format short
src/main.rs:2979:22: error[E0392]: type parameter `T` is never used: unused type parameter
src/main.rs:2978:10: error[E0282]: type annotations needed: cannot infer type
```

## The wrong lines

```rust
#[derive(Clone)]
pub enum SmeltUnion4<T> {
    M0(ResponseInit<SmeltUnknown>),
    M1(SmeltResponse),
}
```

Two defects in three lines, and they point at the same place:

1. **`<T>` is declared but no member mentions it** — E0392. The enum inherited a
   type parameter from the alias (`ResponseOrInit<T>`) that its own members do
   not use.
2. **`M0` is `ResponseInit<SmeltUnknown>`, not `ResponseInit<f64>`** — the arm's
   generic argument was replaced with the erased carrier. The MIR type is
   `ResponseInit<Float>` (visible as `t13 = ResponseInit<Float>` in
   `smelt dump-hir` for the same source, because the interface's default
   `T = StatusCode` resolves to a numeric literal union and so to `Float`), so
   the concrete argument was available and was discarded.

E0282 then follows from (1): nothing can infer `T` at a construction site.

## What narrows it

The same union with a **non-generic** member compiles and runs correctly:

```ts
interface PlainInit {
  status?: number;
  statusText?: string;
}

export function pickStatus(arg?: number | PlainInit | Response): number {
  return typeof arg === "number" ? arg : (arg?.status ?? 200);
}
```

Output `404 / 201 / 200 / 500`, byte-identical to Node 22 — including the
`typeof` narrowing reaching the interface arm and `?.status` resolving on both
the interface and the `Response` arm. So union lowering, `typeof` narrowing,
optional-chain field access and the `Response` member read are all fine; only a
**generic** member is mishandled.

## Suggested shape of the fix

Two independent halves, and the first alone removes the compile break:

* declare on the generated enum only the type parameters its members actually
  reference, so an alias's unused parameter cannot leak in;
* substitute a member's generic arguments with the concrete arguments the MIR
  type carries, instead of falling back to `SmeltUnknown`. That is also the
  `SmeltUnknown`-boundary rule's answer: `ResponseInit<Float>` is knowable here,
  so the arm should not be tagged.
