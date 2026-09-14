# A modeled member read off an erased host record

Round 18 item 3. The gap the round-16 sweep measured: eight of nine probes wrong
against Node 22.

## What was wrong

A modeled host value that crosses the dynamic boundary — `as any`, an
`any`-typed field, a JSON-shaped bag — becomes a marker-bearing record. Its DATA
properties were readable from the record. Its METHODS were not: the member read
answered `undefined`, the optional-call adapter substituted a no-op default
callback, and the call produced `null`.

| read | Node 22 | Smelt before |
| --- | --- | --- |
| `(headers as any).get("a")` | `b` | `null` |
| `(headers as any).set(..)` then `.get` | `d` | `null` |
| `(headers as any).has("a")` | `true` | `null` |
| `(params as any).get("y")` | `2` | `null` |
| `String(params as any)` | `x=1&y=2` | `[object Object]` |
| `(form as any).get("k")` | `v` | `null` |
| `(encoder as any).encode("ab").length` | `2` | `null` |
| `(blob as any).size` | `2` | `2` — a data property, already right |
| `(blob as any).type` | `` (empty) | `` (empty) |

All nine agree now.

## The three things it took

**1. The member has to resolve.** `smelt_host_method` — the one place that
decides "own member, else synthesized host method" — knew only the abort
surface. Each modeled class with an erasure adapter now contributes its own
resolver (`smelt_headers_host_method`,
`smelt_url_search_params_host_method`, `smelt_form_data_host_method`,
`smelt_text_encoder_host_method`, `smelt_text_decoder_host_method`), composed
into `smelt_host_method` only for the classes whose prelude is emitted at all —
the fetch types are pay-for-use.

**2. The erased READ has to ask.** There are two erased-read spellings, and only
one asked: `place.rs`'s erased field path emitted
`smelt_host_method(..).unwrap_or_else(|| smelt_get_object_field(..))`, while
`smelt_get_unknown_field` — the helper every other erased read goes through —
did not. The same source read answered differently depending on which spelling
reached it. It asks now. An own key still wins, because `smelt_host_method`
answers `None` for a record that has one, which is JavaScript's own-property
lookup order.

**3. The value has to be the SAME one.** This is the half that would have been a
silent wrong value in the other direction: `Headers`, `URLSearchParams` and
`FormData` recover STRUCTURALLY (their record carries the pairs), so a resolver
built on that would make `set` write to a rebuilt copy and the next `get` would
read the stale record. All three now retain their live value in
`SMELT_HOST_ORIGINS` on erasure and restore it first on recovery — the shape
`host_value_erasure::emit_adapters` already used for the codecs, the
`node:events` emitter and the `node:http` types. So an `append` through an
erased view is visible on the concrete value the program still holds, which is
what the runtime tier's mutation cases prove.

The origin registry's pay-for-use gate grew the three classes with it.

## Adjacent fix: `URLSearchParams.toString`

`String(params)` and `${params}` are the QUERY STRING in the spec — the class
overrides `toString` — and the erased coercion answered `[object Object]`, the
answer for a record with no override. The coercion now narrows the record back
to the concrete list and asks `to_text()`, so the encoding rule stays in one
place. Gated on the params prelude being emitted, since the coercion match
itself is not pay-for-use. (`String(headers)` is `[object Headers]` in Node and
`[object Object]` here; that one is cosmetic and unchanged.)

## What is deliberately NOT resolved

Only the SYNCHRONOUS members. The async body readers — `response.text()`,
`request.json()`, `blob.arrayBuffer()` — are not resolved and keep the erased
read's `undefined`: each would have to build a `SmeltUnknown::Promise` around a
future spawned from inside a resolver, and the erased call site has no `await`
to drive it. `Blob.slice` is sync and could join; it is left out only because
nothing measured wanted it.

`Response` and `Request` themselves are not in the list either. Their modeled
members are mostly the async readers above, and their data properties
(`status`, `headers`, `url`) are already on the erased record.

## Coverage

`crates/smelt-codegen-rust/tests/erased_host_member_runtime.rs`, three
`#[ignore]`d cases run with `-- --ignored`: the header list (including the
mutation seen on the concrete value), the parameter list (including
`String(erased)`), and the form plus both codecs. The tier exists rather than an
examples fixture for the usual reason — an erased host record costs
avoidable-erasure lines and that corpus holds zero.
