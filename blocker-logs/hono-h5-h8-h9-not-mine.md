# Hono families H5, H8, H9 — probed, and each turned out not to be mine

The plan assigned these three to this stream with a guessed direction. Probing
showed a different owner for every one. Recorded here so the plan's progress log
has the evidence rather than three silent omissions.

---

## H5 — `JSON.stringify() value must be JSON-serializable` (2, `src/request.ts`)

**Plan's guess:** "value is a typed union/record with an `unknown` member;
serialization of `unknown` is a real dynamic boundary and already has a runtime
path, route unions through it."

**What it actually is.** `src/request.ts:228` and `:488` are both
`JSON.stringify(body)` where `body: BodyInit`. `BodyInit` is a fetch type alias
—
`ReadableStream | Blob | BufferSource | FormData | URLSearchParams | string` —
and Smelt does not resolve it: `grep -rn BodyInit crates/` finds nothing, so the
name becomes an opaque `Type::Class` carrying the alias's own name.

The union path the plan pointed at is **already correct**:
`is_json_serializable_type_inner`
(`crates/smelt-frontend-ts/src/lowering/stdlib.rs:1860`) recurses through
`Type::Union` with `all`, and through lists, tuples, dicts, optionals and class
fields. `Type::Unknown` is accepted. The rejection came from the one arm that
cannot answer — a class whose field list is unresolvable
(`json_class_fields` returns `None` only when the name is neither a class, an
interface, nor an alias with fields, i.e. only for an *unresolved* name).

Confirmed by construction: `declare class Opaque {}` + `JSON.stringify(value)`
lowers fine (a declared class with zero fields is serializable). Only an
unresolved name fails.

**Landed anyway:** the diagnostic now names the offending type, and for a class
its name. That single change is what identified the cause —

```text
JSON.stringify() value must be JSON-serializable
  (got Some(Class { name: Symbol(446), args: [] }), class `BodyInit`)
```

— where before it said only that *something* in a possibly deep type was not
serializable, which is the least actionable form of a true statement.

**Owner:** the standards stream. `BodyInit` (and its siblings `RequestInit`,
`ResponseInit`, `HeadersInit`) must resolve to their unions; recorded in
`hono-fetch-demand.md` §1 item 3.

**One thing worth flagging to that stream:** once `BodyInit` resolves, the arms
include `Blob`, `FormData`, `ReadableStream` and `URLSearchParams`. In JavaScript
`JSON.stringify` is *total* for a non-cyclic value — `JSON.stringify(new
FormData())` is `"{}"`, not an error — so the serializability check will need an
answer for a host class with no enumerable own properties rather than a
rejection. That is a decision for whoever models those classes.

---

## H8 — `rest parameter type must resolve to an array type` (1, `src/client/types.ts`)

**Plan's guess:** "`...args: Parameters<F>`-style; likely excluded with
`client/`, confirm."

**Confirmed, with a correction.** The site is `src/client/types.ts:34`:

```ts
webSocket?: (...args: ConstructorParameters<typeof WebSocket>) => WebSocket
```

so it is `ConstructorParameters<typeof WebSocket>`, not `Parameters<F>` — and it
depends on `WebSocket`, which is in no profile Smelt models and is not on the
standards stream's list either.

The plan's disposal ("excluded with `client/`") is **not achievable as
designed**: `[sources] exclude` prunes only root paths before the dependency
closure is built, and `src/client/**` is reached transitively from
`src/index.ts` and `src/helper/testing/index.ts`. See `hono-scope.md` §2 for the
mechanism and four alternatives. Not fixed, and left counted rather than hidden.

---

## H9 — string receiver blockers (2: `src/client/utils.ts`, `src/utils/url.ts`)

**Plan's guess:** "receiver is `string | undefined` or a template-literal type;
narrow/normalize before the string rule, never fall back to Unknown."

**Two different sites with two different owners, and neither is a narrowing
problem.**

* `src/utils/url.ts:108` — `url.indexOf(':')` where
  `const url = request.url` and `request: Request`. The receiver is not
  `string | undefined`; it has **no type at all**, because `Request.url` is not
  modeled. Reported as `string search methods require string receiver and
  argument`. **Owner: the standards stream** — `Request.url` typed `string`,
  recorded in `hono-fetch-demand.md` §1 item 6. This is the *last* blocker in
  `src/utils/url.ts`; the other four were the URI-globals family and are fixed.

* `src/client/utils.ts:21` — `urlString.replace(reg, () => …)` inside the
  `new Proxy` RPC client. Same disposal as H8: `src/client/**`.

So H9 needs no narrowing work. Had the receiver genuinely been
`string | undefined` the plan's direction would have been right; it is not.

---

## Summary

| family | plan's assignment | actual owner | landed |
| --- | --- | --- | --- |
| H5 | this stream (union/`unknown` routing) | standards stream (`BodyInit`) | a diagnostic that names the type |
| H8 | this stream, "confirm the exclusion" | `src/client/**`, and the exclusion mechanism cannot reach it | nothing; documented |
| H9 | this stream (narrowing) | standards stream (`Request.url`) + `src/client/**` | nothing needed |
