# `new Headers(init)` in Hono's `src/context.ts` is already resolved

Checked 2026-09-07 at the request of the coordinator, who relayed a Hono
phase-2 demand item: *"the full Hono crate's build stops at `new Headers(init)`
in `src/context.ts` because `Response.headers` arrives untyped there, so the
`HeadersInit` initializer is erased."*

**It does not, and no work is needed.** Round 10's `instanceof` narrowing is
what fixed it; the record is stale relative to that merge, not wrong about what
it saw earlier.

## What was asked, and what each shape does

The coordinator named four shapes to verify. All four are typed correctly today,
and each was hand-checked byte-identical against Node 22.

| shape | generated | |
| --- | --- | --- |
| `res.headers` on a `Response` parameter | `res.clone().headers()` typed `SmeltHeaders` | ✓ |
| `.headers` after `instanceof Response` on a union arm | union narrows to `SmeltResponse` through the checked cast, then `.headers()` | ✓ |
| `this.#res.headers` through a private field | `self._res.clone().headers()` typed `SmeltHeaders` | ✓ |
| `new Headers(x)` with `x: Headers` | `SmeltHeaders::from_pairs(x.entries_sorted())` — the copy constructor | ✓ |

The third column matters for the last row: `entries_sorted()` is correct rather
than incidental. WHATWG's `Headers` iterator is sorted by name, and
`new Headers(other)` runs the fill algorithm over that iteration order, so a
copy is name-sorted in the spec too.

## Why the record described a real failure that is now gone

The receiver in `context.ts` is not a plain `Response`. It is
`Response | ResponseInit`, and `ResponseInit` is a lib TYPE ALIAS that Smelt
turns into an opaque `Type::Class` — so the union degenerates and the parameter
arrives as `SmeltUnknown`. That is the same root cause the demand file already
recorded for the sibling `.status` read ("because `ResponseInit` is not a
modeled type, the three-member union degenerates").

What changed is what happens AFTER the degeneration. Before round 10,
`x instanceof Response` did not narrow an erased local, so the `.headers` read
stayed on the erased value and the `HeadersInit` initializer was erased with it.
Round 10 made an erased local narrow to the target class wherever that class has
a `SmeltFromUnknown` adapter, so the guard now recovers a real `SmeltResponse`
and `.headers` is `SmeltHeaders` again. The degenerate union is still there; it
is simply no longer load-bearing at this site.

Reproduced exactly:

```ts
type ResponseOrInit = Response | ResponseInit;

function headersOf(arg: ResponseOrInit): string {
  if (arg instanceof Response) {
    return arg.headers.get("content-type") ?? "none";   // "text/plain"
  }
  return "init";
}
function rebuild(res: Response): string {
  return new Headers(res.headers).get("content-type") ?? "none";  // "text/plain"
}
```

`headers_of` takes `SmeltUnknown` and the guard emits
`<SmeltResponse as SmeltFromUnknown>::smelt_from_unknown(arg.clone())`.

Modeling `ResponseInit`/`RequestInit` as real types with typed keys is still
worth doing — it is what the two "init must be an object literal" blockers need,
and it would stop the union degenerating in the first place — but it is no
longer what this site is waiting on.

## Where the Hono build actually stops now

`smelt check` over the whole Hono crate (pinned `eebdf7be`, with the committed
`.github/compat/hono` overlay) reports **zero** diagnostics. The build then
aborts one stage later, in MIR lowering:

```
field and index reads currently require a local receiver
  at src/router/trie-router/node.ts
```

**Verified the same at the round-10 head (`ced101ea`)**, so `context.ts` was
already clear before round 11 and this abort is not new either. It is a general
lowering family — a field or index read whose receiver is not a local — and it
belongs to the Hono stream, not to the fetch types.
