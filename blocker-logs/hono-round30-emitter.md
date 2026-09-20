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

## Item 3 — diagnose then fix: `String` vs `SmeltHeaders` (14), `SmeltUnknown` vs `SmeltRecord` (10), `SmeltUnion289` vs `String` (4)

Each family is named as a TypeScript rule first, as the brief asks.

### 3a. `String` vs `SmeltHeaders` (14) and `SmeltUnion289` vs `String` (4) — one family

**Diagnosis.** Hono's `context.ts` writes

```ts
const headers = this.#res ? this.#res.headers : (this.#preparedHeaders ??= new Headers())
```

Two general rules decide its type, and Smelt had neither:

1. **The VALUE of `x ??= v` is never nullish.** Either the store happened and the value is `v`, or
   it did not and `x` was already non-nullish, so the expression's type is
   `NonNullable<typeof x> | typeof v` — which is what TypeScript gives it. Smelt typed the result
   temporary at the TARGET's type, keeping the optional surface. (`||=`/`&&=` keep the target's
   type, because their value CAN be the original falsy one; only `??=` changes.)
2. **A conditional whose arms are `T` and `T | undefined` is `T | undefined`.** That is plain
   `typeof a | typeof b`. `conditional_branch_type` has carried this arm
   (`unify_optional_conditional_branches`) since the flow-typed numeric case, but the ternary's
   OWN inline type chain in `expression_with_hint` — the one that actually runs — never got it,
   so the two chains had drifted.

With neither rule, `Headers` joined with `Headers | undefined` fell all the way through to the
string-compatibility test, which accepts ANY `Type::Class` (the variant also spells an opaque
unresolved name), and unified to `String`. The code comment beside that test already records the
same failure for DECLARED classes and fixes it with a `declared_class_type` arm; a MODELED host
class such as `Headers` is not a declared class, so it was not covered.

The four `SmeltUnion289` vs `String` errors were the same values reaching
`ResponseInit.headers`, whose type is the `HeadersInit` union: they fell out with the family.

**Result:** 14 + 4 → 0. Four new `E0615`s appeared behind them (`.append`/`.set` taken as a FIELD
on a `&SmeltHeaders`), which is the third rule in the same area:

3. **A modeled method call on an OPTIONAL receiver is a call on the inner value.** `tsc` only
   accepts `maybe.set(k, v)` where it has already narrowed `maybe` to non-nullish, so the optional
   surface is one Smelt's own flow typing did not drop. The modeled PROPERTY reads already assert
   presence (`present_receiver`); the `Headers` and `URLSearchParams` METHOD dispatches did not,
   so the call fell through to a generic optional member read that took a method as a field.
   Fixed for both, which is one rule about modeled receivers rather than two.

### 3b. `SmeltUnknown` vs `SmeltRecord<String, SmeltUnknown>` (10)

**Diagnosis.** `extract_value_text`'s contract is "the text IS an already-erased `SmeltUnknown`",
and its fetch-runtime arm hands that text straight to
`SmeltFromUnknown::smelt_from_unknown`, which takes a `SmeltUnknown`. An object literal flowing
into a `Response`-typed slot arrives as a `SmeltRecord`, so the call was
`SmeltResponse::smelt_from_unknown(SmeltRecord::from([]))` — an `E0308` naming neither the source
line nor the reason. `extract`'s FUNCTION arm already normalizes exactly this way (an
`Rc<dyn Fn ..>` is erased first), so the fix is the same normalization for a record source rather
than teaching one arm of `extract_value_text` to re-derive a source type it was never given.

**Result:** 10 → 0.

### What this unmasked, recorded not fixed

`responseHeaders ?? headers` in `#newResponse` joins `Headers | undefined` with
`HeaderRecord | undefined`; TypeScript types it `Headers | HeaderRecord` and the destination is
`HeadersInit`. The emitter renders `??` on two optionals as `Option::or`, which requires both
arms at ONE Rust type, so it now reports 4 × `expected Option<SmeltHeaders>, found
Option<SmeltRecord<String, String>>`. These were previously hidden behind the `String` unification
above. The rule to implement is that `a ?? b` with differently-typed arms takes their union (or
coerces the right arm at the `Option::or` seam), which belongs with whoever owns the nullish-join
emission.

Fixtures: `examples/typescript/end-to-end/92_nullish_assign_and_optional_receiver/` (verified
against Node 22) covers rules 1, 2 and 3. Sub-fix 3b has no end-to-end fixture: the record reaches
that slot from a SYNTHESIZED default rather than from anything a source file can spell, so its
evidence is the corpus measurement (10 → 0) plus the unchanged emitter suite.

## Item 4 — `?` inside a closure whose generated return type is not `Result`

**Rule.** A getter whose body can throw is emitted returning `Result`, so reading the property is
a fallible operation. `?` is only one of the two ways to be fallible, and it travels ONLY where
the enclosing function returns `Result`. A closure whose contextual callback type is a plain
`Fn(..) -> T` is not such a function, so a `?` in its body is
`error[E0277]: the ? operator can only be used in a closure that returns Result`. Hono hits it
three times, reading `c.executionCtx` — a getter that throws when the context has none — inside a
callback the framework types as infallible.

**Fix.** `emitter/place.rs`'s descriptor-getter read chose `?` from the GETTER's fallibility
alone. It now also asks the enclosing function: fallible reader → `?`; non-fallible reader → the
panic path (`unwrap_or_else(|error| smelt_panic_throw(error))`), which is the round-6 floor every
other non-fallible callback seam already uses. The throw is not dropped: the program stops at it
rather than continuing with a fabricated value.

Both branches of the rule existed only by accident before: the previous emission put `?` in a
plain non-throwing METHOD too (`fn spread(&self) -> SmeltUnknown` with `self.__smelt_get_active()?`
in its body), which is the same `E0277` — the codegen test that pinned it asserted text, never
compilability, so it passed. That test now asserts both forms.

**Result:** E0277 5 → 2; the two survivors are the `(SmeltUnknown, RouterRoute): SmeltFromUnknown`
tuple family, Agent C's. Crate total 216 → 213.

**Follow-up, not done here.** The better answer for a plain method is to make the READER fallible
so the `?` form applies, rather than to panic. That is a MIR change:
`lower/passes/throwing.rs` keys off CALLS (`Callee::Builtin`, a call to a throwing function), and
a descriptor-getter read is a `Place::Field` read, not a call — so the pass never sees it. Adding
it would mark `spread`-shaped methods throwing and let their throws stay catchable. It would also
mark the Hono closures, whose contextual callback type is NOT fallible, so that change has to come
with the decision about what a throwing closure does against an infallible `dyn Fn` signature —
which is why it is recorded rather than folded into this item.

Fixture: `examples/typescript/end-to-end/93_throwing_getter_in_callback/` (verified against
Node 22; nothing in it makes the getter actually throw from the infallible side, because a throw
that escapes an infallible callback is a process stop rather than a catchable error — that is the
floor being recorded, not a behaviour to diff). The codegen test
`a_throwing_getter_read_is_fallible_in_both_enclosures` pins both forms.

## Final measurement

Clean clone of `eebdf7be…` + `.github/compat/hono/.`, `dist-smelt` deleted and regenerated with a
freshly built full-feature `smelt`, `cargo check --message-format=short` from the repo root with
`--manifest-path`:

| code | baseline | after | delta | owner |
| --- | ---: | ---: | ---: | --- |
| E0107 | 136 | 136 | 0 | Agent C (type-parameter defaults) |
| E0308 | 134 | 40 | **-94** | Agent D (items 1, 3) + the remainder split across owners |
| E0609 | 17 | 1 | **-16** | Agent D (item 2); the survivor is `router` on `Hono<E, S, BasePath>` |
| E0282 | 10 | 10 | 0 | unowned |
| E0121 | 10 | 10 | 0 | Agent C |
| E0382 | 8 | 5 | **-3** | Agent D (item 1b); the survivors are a MIR last-use family |
| E0277 | 5 | 2 | **-3** | Agent D (item 4); the survivors are the tuple `SmeltFromUnknown` family |
| E0063 | 5 | 5 | 0 | Agent C |
| E0425 | 2 | 2 | 0 | unowned |
| E0599 | 1 | 1 | 0 | unowned |
| E0271 | 1 | 1 | 0 | unowned |
| **total** | **329** | **213** | **-116** | |

The 40 remaining `E0308`s, by shape: 7 `&(SmeltUnknown, RouterRoute)` vs `&SmeltUnknown`
(Agent C), 5 `SmeltUnion1313` vs `SmeltUnknown`, 4 `SmeltUnion778` vs `SmeltUnknown`, 4
`Result<SmeltUnknown, _>` vs `Result<SmeltUnion66, _>`, 4 `Option<SmeltHeaders>` vs
`Option<SmeltRecord<String, String>>` (the `??` join unmasked by item 3, written up above), 3
`Rc<dyn Fn(.., &(..))>` vs `Rc<dyn Fn(.., (..))>`, and eleven singletons.

## Gates

| gate | result |
| --- | --- |
| examples invariant (`--fail-on-regression`) | avoidable erasure **0**, unchanged |
| es-toolkit ratchet vs `smelt-unknown-baseline-es-toolkit.json` | avoidable **31716**, exactly the baseline (did not increase) |
| remeda generated `cargo test` | **1789 passed / 0 failed** |
| radash generated `cargo test` | **84 passed / 0 failed** |
| `cargo test -p smelt-frontend-ts --no-default-features` | 1085 passed / 0 failed |
| `cargo test -p smelt-codegen-rust` | 1060 passed / 0 failed (tiers `#[ignore]`d) |
| `cargo test -p smelt-codegen-rust --test fetch_init_runtime -- --ignored` | 8 passed / 0 failed (was 7/1 on the base) |
| `cargo test -p smelt-codegen-rust --test request_runtime --test fetch_response_runtime --test fetch_types_runtime -- --ignored` | 12 passed / 1 failed — `an_erased_request_info_input_takes_every_arm`, RED on the base too (see item 2) |
| `cargo test --bin smelt` | 57 passed / 0 failed |
| `cargo test -p smelt-transpiler --test hir_cli_cross_language_tests` | 17 passed / 0 failed |
| `cargo clippy --all-targets` | no findings |

## SmeltUnknown delta

Net **zero**. Nothing in these four items introduces a `SmeltUnknown`: item 1 replaces an erased
`ToNumber` match with a typed concatenation, item 2 adds eight concrete `String`/`bool` fields,
item 3 replaces a `String` unification with the concrete `SmeltHeaders`/`Optional<SmeltHeaders>`
and removes one record-to-unknown mismatch, and item 4 changes only which fallible form a read
takes. The examples invariant stays at 0 and the es-toolkit ratchet is unmoved.
