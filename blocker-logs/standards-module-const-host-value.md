# A module-level `const` holding a modeled host value does not reach the functions that read it

Found 2026-09-07 while writing the `Blob`/`File` fixture. **Pre-existing** —
reproduced on the standards stream head (`e84cc248`, before the `Blob` work) —
and it is a HARD FAILURE, not a wrong value: the generated crate does not
compile.

## The repro

```ts
const headers = new Headers({ a: "b" });

function readSync(): string {
  return headers.get("a") ?? "null";
}

async function readAsync(): Promise<string> {
  return headers.get("a") ?? "null";
}
```

Both functions emit the same thing, and it is not the header list:

```rust
pub(crate) fn read_sync() -> String {
    let _smelt_tmp_0: SmeltRecord<String, SmeltUnknown> = SmeltRecord::from([]);
    let _smelt_tmp_1: SmeltHeaders = <SmeltHeaders as SmeltFromUnknown>::smelt_from_unknown(_smelt_tmp_0.clone());
    let _smelt_tmp_2: Option<String> = _smelt_tmp_1.get(&"a".to_owned());
    ...
```

```
error[E0308]: mismatched types
   --> src/source_main.rs:9
    | let _smelt_tmp_1: SmeltHeaders = <SmeltHeaders as SmeltFromUnknown>::smelt_from_unknown(_smelt_tmp_0.clone());
    |   expected `SmeltUnknown`, found `SmeltRecord<String, SmeltUnknown>`
```

Two things are wrong at once:

1. the module const's VALUE is gone — the reference lowered to an empty record
   default rather than to the `new Headers({ a: "b" })` the module evaluated, so
   even if it compiled the function would answer `"null"` where Node answers
   `"b"`;
2. the recovery coercion is emitted at the wrong type — `SmeltRecord<String,
   SmeltUnknown>` is handed to an adapter that takes `SmeltUnknown` — so the
   crate fails to build instead of failing quietly.

It reproduces for a plain function as well as an `async` one, so it is not an
async-capture problem. It affects every modeled host class the same way
(`Headers`, `URLSearchParams`, `Response`, `Request`, and — after standards
items 1 and 2 — `TextEncoder`, `TextDecoder`, `SmeltBlob`).

## Why it matters now

This is the single commonest shape in the Hono corpus for the types this stream
is building: `blocker-logs/hono-fetch-demand.md` records `TextEncoder` used
"36× as `new TextEncoder().encode(…)`, **5× via a bound `encoder`**", and a
bound encoder at module scope is exactly this. The inline spelling works; the
bound one does not compile. Any Hono file that hoists a codec, a header list or
a params object to module scope will stop the build.

## Where to look

The reference lowers through the module-globals path
(`lowering/module_init.rs` plus the const-item inlining described in
`blocker-logs/estk-const-item-inlining.md` and
`blocker-logs/module-const-construction-cost.md`): a module const is re-created
at each use site rather than read from a slot, and a modeled host construction
is not among the shapes that path can re-create, so it falls back to the erased
empty-record default. The fix is either to make that fallback re-run the
const's own initializer expression (it is available — the inlining path already
does this for the shapes it supports) or to give a module const of a concrete
host class a real module slot. Either way the erased-record default must stop
being a silent fallback for a value whose construction is known: it produced
both defects above.

A regression test belongs with the fixtures for each modeled type — reading the
value through a function, not only inline.
