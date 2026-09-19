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
