# Round 30 — Agent D: emitter and standards tier

Owner: Agent D. Base: `2e60c96b` (round-30 brief on `claude/estoolkit-test-failures-4fuf9e`).

Every family below is stated as a **TypeScript-semantics rule** before it is fixed. Nothing here
keys off a Hono spelling.

## Measurement note: `[sources] exclude` is resolved against the PROCESS CWD

Before any of the work below: running `smelt build` from *inside* the Hono checkout aborts at
`src/client/utils.ts`, a file the manifest's `exclude` list names (`src/client/**`). Running the
same manifest from the Smelt repo root with `--manifest-path <checkout>/Smelt.toml` emits the
crate. So `[sources] exclude` globs are matched against paths relative to the process working
directory rather than the manifest directory. That is a real manifest bug, unrelated to the
families in this round; recorded here so the next measurement does not mistake it for a
"does not emit" regression (round 29 reported exactly that symptom). **Measure from the repo root
with `--manifest-path`.**

## Baseline (this worktree, freshly built full-feature `smelt`, clean clone)

`eebdf7be39abf0a872671835ccce0c4f03ea497a` + `.github/compat/hono/.`, `cargo check` on
`dist-smelt`: **329 errors** (the brief's 319 was measured the same way; the delta is not
attributable here and the per-code table below is what each family is measured against).

| code | baseline |
| --- | ---: |
| E0107 | 136 |
| E0308 | 134 |
| E0609 | 17 |
| E0282 | 10 |
| E0121 | 10 |
| E0382 | 8 |
| E0277 | 5 |
| E0063 | 5 |
| E0425 | 2 |
| E0599 | 1 |
| E0271 | 1 |

## Item 1 — `+` / `+=` with a union operand is string concatenation

**TypeScript rule.** ECMAScript's `ApplyStringOrNumericBinaryOperator` coerces both operands of
`+` with `ToPrimitive` and concatenates as soon as either result is a String; only when neither
is does it add. TypeScript decides that statically from the operand types, and a union with at
least one `string` arm counts — `tsc --strict` accepts `buffer[0] += str` on a
`(string | Promise<string>)[]` and types the `+` as `string`. The non-string side is stringified
with the same `ToString` a `${}` template uses.

**What was wrong.** The frontend already typed such an expression `string`
(`binary_result_type` / `has_static_string_type` in `lowering/ty/annotations.rs`). The emitter's
`Rvalue::Binary` dispatch, however, chose the concatenation path only when the STATEMENT's
destination type was `String`. A compound assignment writes the result back into the place it
read, so for `buffer[0] += str` the destination is the element's own union. The union operand
then fell through to `erased_arithmetic_text`, which emitted
`SmeltUnion305::from_smelt_unknown(SmeltUnknown::Number(ToNumber(buffer[0]) + ToNumber(str)))`
— a `ToNumber` `match` whose `SmeltUnknown::…` arms were matched against a `SmeltUnion305`
scrutinee. Ten arms × seven sites = the 70 `E0308`s in `html.rs`. Had it type-checked it would
have been `NaN` at runtime: Hono's HTML escaper would have produced numbers instead of markup.

**Fix.** `emitter/binary_ops.rs` gains `add_operand_is_string_like` (mirrors the frontend
predicate: `string`, an optional whose inner type is, or a union with any string arm; `unknown`
and type parameters deliberately excluded, they keep the erased runtime path) and
`string_addition_text`, which builds the concatenation at `String` and then hands it to the
ordinary coercion seam so it re-enters the destination through the union's string arm
(`SmeltUnion2::M0(..)`). An `Int`/`Float` destination keeps the numeric path.

**Result:** 70 → 0. `html.rs` E0308 eliminated; crate total 329 → 259.

## Item 1b — the eight `E0382`s: erasing a union must not consume it

**Rule.** Erasing a value to inspect it is a read, not a move. `into_smelt_unknown()` takes
`self` by value, so any erasure rendered from a bare place read moves out of the local.

`emitter/coercion.rs::erase`'s concrete-union arm was the one consuming arm that passed the raw
operand text instead of `smelt_owned_text`; its rendered-value twin
`emitter/union.rs::erase_concrete_union_text` has always cloned, and its doc comment already
states the reason. So the two halves of one decision disagreed.

**Result:** 8 → 5. The remaining five are a DIFFERENT family and are recorded, not fixed here:
`html.rs:362/372/480` are `_smelt_tmp_N = str;` — MIR emitting `Operand::Move` for a read of a
`SmeltUnknown` local that is read again later in the same block. That is a MIR last-use /
liveness question, not an emitter clone-discipline one; it belongs with whoever owns MIR operand
selection.

Fixtures: `examples/typescript/end-to-end/90_union_string_concat/` (verified against Node 22) and
`union_operand_makes_addition_a_string_concatenation` /
`erasing_a_union_local_does_not_move_it` in `crates/smelt-codegen-rust/src/tests/part_1_tests.rs`.

## Item 2 — the stored `RequestInit` members on `SmeltRequest`

**TypeScript/WHATWG rule.** `RequestInit` has eight members that are neither part of the
request's transport identity (`method`, `headers`, `body`) nor a live object (`signal`):
`cache`, `credentials`, `integrity`, `keepalive`, `mode`, `redirect`, `referrer` and
`referrerPolicy`. Each is a scalar the constructor stores and a read-only getter answers
unchanged. Reading one is an ordinary typed property read, not a struct field poke — and that is
what the generated Rust was doing (`req.raw.cache`), hence 16 `E0609`s in `request.rs`.

**Shape.** One enum, `smelt_hir::RequestInitMember`, carries the whole table (JS spelling, Rust
field name, `string` vs `bool`, the spec default, and whether a non-empty init resets it), and
every stage reads it from there:

* `RequestOp::Init(member)` is one variant for the whole family — the same shape `DataView`'s
  accessor uses — so no dispatch site grew eight arms;
* `ExprKind::RequestNew` / `Rvalue::RequestNew` gained ONE `init_members` list rather than eight
  more `Option` fields, because these keys differ in nothing the constructor does with them;
* the ambient `RequestInit` dictionary (`ambient_init_interface_type`) and the utility-type field
  table (`ambient_fetch_init_fields`) both append the same eight names from that enum, so the
  init surface and the getter surface cannot drift;
* the prelude emits `SmeltRequestInit` — eight concrete fields, `Default` = the spec's set — held
  by `SmeltRequest`, copied by `tee()`, and read by eight getters. No `SmeltUnknown` anywhere in
  it.

**Two rules that are not "copy or default", both measured against Node 22:**

1. a `Request` INPUT copies the whole group, but a NON-EMPTY init puts `referrer` and
   `referrerPolicy` back to their defaults (`new Request(src, { method: 'POST' }).referrer` is
   `"about:client"`); an EMPTY init (`new Request(src, {})`) still copies. "Non-empty" is the
   presence of any init key, which the emitter can see;
2. a spelled `referrer` is stored as its URL SERIALIZATION, the same rule the request URL goes
   through: `"https://c.test"` reads back as `"https://c.test/"`.

An init key read off a TYPED `RequestInit` is `Optional<T>`, so an absent one must leave the slot
holding whatever the input put there; it is emitted as `if let Some(v) = .. { slot = v; }` rather
than an `unwrap_or_default` that would overwrite a copied value with `""`.

**Result:** E0609 17 → 1 (the survivor is `router` on `Hono<E, S, BasePath>`, Agent C's family).
Crate total 259 → 240.

### A pre-existing runtime-tier failure fixed on the way

`fetch_init_runtime::a_request_at_the_init_position_copies_the_source` was RED on the round-30
integration head (verified by re-running it on the base). `d6cffdc6` changed a `Request` at the
body position to `SmeltBody::take_from_source`, which is right for the INPUT position and wrong
for the INIT position. Node 22:

* `new Request(src)` — INPUT — leaves `src.bodyUsed` TRUE at once, the copy's false;
* `new Request(url, src)` — INIT — leaves `src.bodyUsed` FALSE, and reading the COPY is what
  makes it true: one shared handle, used flag included.

The distinction is the POSITION, which one conversion function cannot see, so `init_body_text`
now takes it as a flag; a `Response`'s first argument is a `BodyInit`, not an init key, and keeps
the extraction path.

### Found, not fixed

`request_runtime::an_erased_request_info_input_takes_every_arm` is also RED on the base (verified
the same way). `new Request(input)` on an ERASED `string | Request` input recovers the request
through `SmeltFromUnknown`, which REBUILDS a request from the erased record, so
`take_from_source` disturbs the rebuilt copy and the original's `bodyUsed` stays false where Node
says true. Fixing it means the boundary adapter restoring the ORIGIN request (a
`smelt_restore_host_origin`-shaped identity question) rather than rebuilding one; that is an
erasure-identity change, not an init change, so it is recorded here rather than folded in.

Fixtures: `examples/typescript/end-to-end/91_request_init_members/` (verified against Node 22)
and `the_stored_request_init_members_default_and_override` in
`crates/smelt-codegen-rust/tests/fetch_init_runtime.rs` (an already-registered runtime tier).
