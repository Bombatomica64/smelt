# `new Headers(init)` in Hono's `src/context.ts`

**STATUS: resolved twice.** The four shapes checked on 2026-09-07 were already
fine (below); the FIFTH shape the coordinator reopened on 2026-09-08 was real
and is fixed in round 15 — see "Reopened 2026-09-08" at the bottom, which is
the current record. `context.ts:617` now transpiles and the whole-crate build
has moved on to an unrelated family.

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

## Part one: the constructor now dispatches on the arms

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

## The other half: a field read on a union no longer erases

The constructor fix alone did not move the Hono build, and the `SmeltUnknown`
in its message was not the constructor's fault — it was `arg.headers`.

A field read whose base was a tagged union used to erase the whole union and
look the property up at runtime:

```rust
// arg: SmeltUnion2 (ResponseInit | Response)
smelt_get_unknown_field(&arg.clone().into_smelt_unknown(), "headers")
```

typed `SmeltUnknown`, which then infected every consumer downstream. A union is
a generated Rust enum, so at `u.f` the ARM is a runtime question and each arm's
READ is a static one; both halves were being thrown away. Two changes fix it.

### `typeof x === 'object'` keeps a union's object arms

`typeof_matched_type` answered `Type::Unknown` for the `"object"` kind —
unconditionally, whatever the local held. So Hono's own guard,
`typeof arg === 'object' && arg.headers` on
`StatusCode | ResponseInit | Response`, was what erased `arg`: inside the guard
that proves the value is one of the two OBJECT arms, the value had no type left
at all.

It now keeps the object-kinded arms
(`crates/smelt-frontend-ts/src/lowering/testing/matchers.rs`), which is what
the guard actually proves, and the emitted check is a static tag test over the
enum (`matches!(arg, Some(M1(_) | M2(_)))`). A union with no object arm, or a
non-union receiver, still answers `Unknown` — there is nothing more precise to
say. An `Optional` wrapper is dropped (an `x?: T` parameter's absent value is
`undefined`, whose `typeof` is `"undefined"`), while a `T | null` spelling
keeps its `None` arm, because `typeof null === "object"`.

### The read dispatches on the arms

`crates/smelt-frontend-ts/src/lowering/union_member_read.rs` desugars `u.f` on
a tagged union into one conditional per arm: project the arm (a `TypeAssert`,
the node round 10's `instanceof` narrowing already uses, which the emitter
renders as `match u { M{i}(v) => v, .. }`), read the member the way that arm's
type reads it, and join the results. Hono's site becomes:

```rust
if matches!(arg.clone(), Some(SmeltUnion16::M2(_))) {
    let response = arg.clone().map_or(..., |value| match value { SmeltUnion16::M2(value) => value, _ => unreachable!(..) });
    Some(SmeltUnion6::M2(response.headers()))     // the Response arm's ACCESSOR
} else {
    let init = arg.clone().map_or(..., |value| match value { SmeltUnion16::M1(value) => value, _ => unreachable!(..) });
    init.headers.clone()                          // the interface arm's FIELD
}
```

typed `Option<SmeltUnion6>` — which the constructor arm above then dispatches
on. No erasure anywhere in the chain.

The desugaring lives in the frontend, and both reasons are worth recording
because they rule out the emitter placement that looks obvious:

* **Only the frontend can pick each arm's read.** `res.headers` on a
  `Response` is not a struct field access — it is a `ResponseOp::Headers` node,
  and the same holds for the other modeled host classes. Which HIR node a
  member read becomes is decided during lowering, so only there can one arm
  read a struct field while its sibling runs a runtime accessor. An emitter
  looking at a `Place::Field` could only ever render one of the two.
* **Only the frontend can name the joined type.** `place_ty` must name the type
  of what it renders, and the emitter can only FIND interned types
  (`find_type_id`), never add them; the join of the arms' member types
  generally is not in the table. Interning is free during lowering.

Guards come from nodes the emitter already renders as static tag tests:
`UnknownIs(kind)` becomes `concrete_union_tag_check`, an `instanceof` becomes
`concrete_union_class_check`. An arm with no available runtime check — a plain
interface, which is not a runtime value — goes last as the `else`, and at most
one such arm is allowed; a second one makes the desugaring decline. An arm that
does not carry the member reads `undefined` (`(204).headers` in JavaScript), so
a union still holding a primitive arm needs no narrowing proof.

The desugaring is all-or-nothing on purpose: any arm whose read is not one of
the modeled shapes makes the whole read fall back to the erased path, so it can
only remove erasure, never leave a half-typed value behind. Measured effect on
the corpora, from narrowing and dispatch together: es-toolkit avoidable erasure
32493 → 32491, remeda 25028 → 25017.

## Where the Hono build stops now

`context.ts:617` is past. The whole-crate build's next stop, verbatim:

```
Error: EmitError { message: "`new Headers(init)` initializer type is not modeled: SmeltUnknown (initializer `smelt_get_unknown_field(&closure_arg_1.clone().unwrap_or(SmeltUnknown::Undefined), \"headers\").clone()` in `<unnamed>`)" }
```

(The blocker now names the initializer expression and the enclosing function,
because a bare type name gave a reader no way to find the site in a corpus with
seventeen `new Headers(..)` calls.)

This is a different family: `closure_arg_1` is an erased CLOSURE PARAMETER, so
`.headers` on it is a runtime property read for the ordinary reason — the
callback's parameter type was not carried into the closure. The candidates are
the arrow-bodied middleware sites, `src/middleware/method-override/index.ts:80`
and `:111` (`new Headers(clonedRequest.headers)`, `new Headers(c.req.raw.headers)`)
and `src/helper/proxy/index.ts:58`/`:175`. Nothing about `Headers` or unions is
missing there; it wants the callback-parameter typing work, which is the
`callback-generics` stream rather than this one.

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
