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

---

# Reopened 2026-09-08: a FIFTH shape, and it was real

The coordinator reopened this with the site the four verified shapes missed:
`src/context.ts:617`, inside
`#newResponse(data, arg?: StatusCode | ResponseOrInit, headers?)`:

```ts
if (typeof arg === 'object' && arg.headers) {
  for (const [key, value] of new Headers(arg.headers)) { ... }
}
```

`arg` is `StatusCode | ResponseInit | Response` — a number, Hono's OWN
`ResponseInit` interface (`headers?: ResponseHeadersInit`, where
`ResponseHeadersInit = [string, string][] | Record<string, string> | Headers`),
and `Response`. So `arg.headers` is the join of `ResponseHeadersInit |
undefined` with `Response`'s `Headers`, and `new Headers(..)` must accept that
join.

The whole-crate build stopped, verbatim:

```
Error: EmitError { message: "`new Headers(init)` initializer type is not modeled: SmeltUnknown" }
```

## Half of it is fixed: the constructor now dispatches on the arms

Two initializer shapes were missing from `headers_conversion_text`, and both
are modeled now (`crates/smelt-codegen-rust/src/emitter/fetch_types.rs`):

* **A union initializer.** `HeadersInit` is a union in WHATWG's own IDL, so
  source that keeps it as one hands the constructor a value whose ARM the
  runtime picks while every arm's CONVERSION stays statically known. A
  generated union is a tagged enum, so the emitter matches it and runs each
  arm's own conversion:

  ```rust
  match init.clone() {
      SmeltUnion5::M0(v) => SmeltHeaders::from_pairs(v.to_vec().into_iter().map(..).collect::<Vec<(String, String)>>()),
      SmeltUnion5::M1(v) => SmeltHeaders::from_pairs(v.iter().map(..).collect::<Vec<(String, String)>>()),
      SmeltUnion5::M2(v) => SmeltHeaders::from_pairs(v.entries_sorted()),
  }
  ```

  Erasing the union to `SmeltUnknown` and re-reading its tag would answer the
  same question with the static type thrown away, and `SmeltHeaders` has no
  erased constructor to recover it with — which is why the blocker existed
  rather than a silent fallback.
* **An absent initializer.** WHATWG's constructor takes `HeadersInit?` and
  `new Headers(undefined)` is the empty header list, the same answer the
  no-argument spelling gives. An `Optional` init (what a `headers?:` key
  arrives as when the source narrowed it by truthiness and Smelt did not prove
  the narrowing) now emits
  `match init { Some(v) => <conversion>, None => SmeltHeaders::new() }`.

Fixture `examples/typescript/end-to-end/55_headers_init_union` covers both plus
copy independence and the two-values-per-name read, byte-identical to Node 22;
`headers_constructor_dispatches_on_a_union_initializer` and
`headers_constructor_accepts_an_absent_initializer` pin the emitted shape,
including the negative (no erased boundary).

## What Hono is actually waiting on: a field read on a union erases

With the constructor fixed, the Hono site stops at the SAME message, and the
`SmeltUnknown` in it is not the constructor's fault. It is `arg.headers`.

A field read whose base is a tagged union is emitted today by erasing the whole
union and looking the property up at runtime:

```rust
// arg: SmeltUnion2 (InitLike | Response)
smelt_get_unknown_field(&arg.clone().into_smelt_unknown(), "headers")
```

typed `SmeltUnknown` (`place_ty`'s `Place::Field` arm falls to `Unknown` for a
union base, and `place_text` handles a union base in the same arm as
`Type::Unknown`). Every consumer downstream then sees an erased value — which
is what hands `new Headers` a `SmeltUnknown` no arm-dispatch can help with.

So the demand's "dispatch on the static arms, never an erased fallback" needs
one more piece, and it is a general erasure fix rather than a fetch-types one:
**a field read on a tagged union should dispatch on the arms**, the way a
METHOD call on a union already does (`union_method_text`).

### Why it has to be a FRONTEND desugaring, not a codegen arm

The obvious placement — teach `place_text`/`place_ty` a union arm — cannot
work, and the reason is worth recording:

* **Codegen cannot render every arm's read.** `Response.headers` is not a
  struct field read at all: the frontend lowers it to a dedicated
  `response_headers` HIR op (`response_property_read`), and the same is true of
  the `Request`/`URL`/`RegExp`/text-codec/`Blob` members. Only the frontend
  knows which node an arm's member read becomes, so only the frontend can build
  the arms.
* **Codegen cannot intern the join.** `place_ty` must name the type of what it
  renders, and the emitter can only FIND interned types (`find_type_id`), not
  add them. The join of the arms' field types generally is not in the table.
  The frontend interns types freely, and `class_field_type` already computes
  exactly this join for its `Type::Union` arm.

### The shape the desugaring wants

In `static_member_with_absent_fallback`, when the receiver's type is a concrete
union and the member resolves on at least one arm:

1. Result type: the join `class_field_type` already computes, with `undefined`
   folded in for any arm that does not carry the member (reading `.headers` off
   a number is `undefined` in JavaScript, not an error — this is what keeps the
   `StatusCode` arm from forcing a narrowing proof).
2. One arm per union member: project the receiver to the arm type (a
   `TypeAssert` whose type is the arm — `project_union_value_text` already
   emits `match u { M{i}(v) => v, _ => unreachable!() }` for it, and round 10's
   `instanceof` narrowing rides the same path), then lower the member read on
   the projected value through the ordinary `static_member` machinery, so each
   arm gets its own correct node for free.
3. Chain the arms on tag checks the emitter already renders statically:
   `UnknownIs(kind)` becomes `concrete_union_tag_check` (a `matches!` over the
   arms of that JS kind) and a class check becomes
   `concrete_union_class_check`. The one arm with no runtime check available
   (a plain interface, e.g. `ResponseInit`) goes last as the `else`.

That is a self-contained item, it kills a whole erasure class rather than one
call site (every `u.field` on a union in every corpus), and Hono's
`context.ts:617` falls out of it: `arg.headers` becomes
`Optional<ResponseHeadersInit | Headers>`, which the constructor arm added
above already dispatches on.

## Adjacent gap found while writing the fixture

A nested array literal assigned into a union's TUPLE-list arm erases:

```ts
type Init = [string, string][] | Record<string, string> | Headers;
declare function f(init: Init): void;
f([["content-type", "text/html"]]);   // string[][] to TypeScript
```

emits `SmeltUnion5::from_smelt_unknown(SmeltUnknown::Array(vec![..]))` — the
literal is `List<List<String>>`, the arm is `List<(String, String)>`, and
`inject_union_value_text` has no element-wise list→tuple conversion for the
injection, so it round-trips through the erased boundary. Annotating the value
(`const pairs: [string, string][] = ..`) lowers it as a tuple list and injects
statically, which is what the fixture does. It matters for Hono: an inline
`{ headers: [['a','b']] }` init object hits the erasing path.
