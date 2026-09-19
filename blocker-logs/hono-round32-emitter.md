# Round 32 — Agent H: emitter singletons and the round-31 leftovers

Owner: Agent H. Base: `claude/estoolkit-test-failures-4fuf9e` merged into this worktree
(branch `worktree-agent-a057f1e7f1be5e260`). Measurement is the brief's: a clean Hono clone at
`eebdf7be39abf0a872671835ccce0c4f03ea497a` with `.github/compat/hono/.` copied over it, a freshly
built full-feature `smelt`, `smelt --manifest-path <abs>/Smelt.toml build` run from the repo root,
then `cargo check --message-format=short` on `dist-smelt`.

Every family is stated as a TypeScript- or WHATWG-semantics rule before it is fixed. Nothing here
keys off a Hono spelling; every gate below is the whole rule's, not the fixture's.

## Baseline and result

| code | baseline | after | delta |
| --- | ---: | ---: | ---: |
| E0308 | 35 | 24 | **-11** |
| E0631 | 5 | 5 | 0 |
| E0425 | 2 | 2 | 0 |
| E0609 | 1 | 1 | 0 |
| E0271 | 1 | 0 | **-1** |
| **total** | **44** | **32** | **-12** |

The baseline reproduced exactly (44 diagnostic lines, same per-code split as the brief's 45) before
any change. Agent G's generic-class families (`Context_1`/`Hono_1` pairs, `HonoRequest`, the E0631
closure signatures, the `Rc` triple and the `router` E0609) are untouched by design and account for
every remaining E0308 except none — all 24 remaining E0308s are in that family.

## Item 2 — a union of numeric literal types is `number`, and `Exclude` resolves over it

`2b79af20`. **TypeScript rule.** `Exclude<A, B>` distributes `A extends B ? never : A` over `A`'s
arms, so it is `A` minus every arm `B` covers. Smelt had no `Exclude` arm at all, so every reference
to one erased to `unknown`.

Two consequences follow from how Smelt interns types, and both are the general rule:

* a union of NUMERIC LITERAL types is already its arms' common base type (`200 | 204 | 404` interns
  as `Float`, because every arm's widened type is `number`). Such a base is not a `Type::Union`, so
  nothing can be subtracted from it and the answer is the base itself — which is correct: removing
  some literals from a literal union leaves a union of literals, still `number`. Hono's
  `ContentfulStatusCode = Exclude<StatusCode, ContentlessStatusCode>` is exactly this, and its
  `readonly status` field reached `SmeltResponse::from_parts(status: f64, ..)` as a tagged value;
* a union whose arms Smelt models distinctly really does lose the named arms, and the remainder
  stays a union that narrows by `typeof`.

Removing every arm leaves `never`.

**Result:** 44 → 42, both `expected f64, found SmeltUnknown` sites. Fixture
`examples/typescript/end-to-end/100_exclude_over_a_literal_union` (verified against Node 22) and a
frontend unit test pinning the lowered types directly.

## Item 3 — a callback result is erased when its class renders concretely

`84ac0b95`. Erasing a function value builds a forwarding closure that answers `SmeltUnknown`, so the
wrapped call's RESULT is erased on the way out. Four seams skipped that step when
`class_has_no_known_fields(return_ty)` held — but that predicate answers a DIFFERENT question ("this
class declares no fields I could build an erased object from"), and it is also true of every MODELED
host class, because a modeled class is not in `mir.classes` at all.

**Rule.** A value skips erasure only when its RUST REPRESENTATION already is `SmeltUnknown`, which is
`is_erased_class_type` — asked of the stdlib registry, so it stays right as more host classes become
concrete. Every other case falls through to `erase_value_text`, which already knows how each class
erases and returns an erased class's text unchanged, so the shapes that took the old branch are
emitted exactly as before. `class_has_no_known_fields` had no other caller and is gone.

**Result:** 42 → 39. The `expected SmeltUnknown, found SmeltRequest` and `… found SmeltTypedArray`
sites, and — not predicted — the E0271, whose async continuation is typed from the same erased
return channel. Fixture `examples/typescript/end-to-end/101_erased_callback_returns_a_host_class`.

## Item 1 — structural assignability of a value to a record type

`1f466938`. **TypeScript rule.** Assignability is structural: a `Response` IS assignable to a
`ResponseInit`, because it has `status: number`, `statusText: string` and `headers: Headers`.
Nothing in `createResponseInstance((this.#res as Response).body, this.#res)` (`context.ts:521`) is
an overload or a union — the round-31 note's guess (`ResponseInit | Response`) is not what the
source declares. The rule is one line: a value whose type exposes every member a target record type
declares converts by reading each target field off it and building the struct.

The missing piece was the READ, exactly as round 31 diagnosed. `emitter::place` answers `x.member`
from a MIR place; a coercion seam has no place. A modeled host class needs that doubly: a
`SmeltResponse`'s `status` is the prelude struct's `status()` accessor, so the existing field-pairing
adapter could not even name it (`structural_record_fields` answers `None` for a class Smelt models).

New module `emitter::host_member_read` is that text-level member read. Its table is the same member
model the frontend lowers `response.status` through (`ResponseOp`), asked by NAME, so the conversion
and the ordinary read cannot disagree; `response_op_text` is split into an operand wrapper and
`response_op_on_text` so both share one statement of what each member means. Class identity comes
from the stdlib registry, never from a name comparison, so a crate that declares its own
`class Response` shadows the host one.

The conversion declines — leaving every existing coercion byte-identical — unless the source is a
host class WITH a member table and every non-optional target field is one of its members. An
optional field the source does not expose is absent, which is what an optional property means. The
"the source exposes members at all" gate is load-bearing: without it a dictionary source reported
"no member" per field and an all-optional record was built entirely out of absent fields, which
COMPILES and silently drops the value. `part_7_tests::a_record_target_is_not_built_from_a_source_with_no_members`
pins that.

**Result:** 39 → 37, both `Option<globalThis_ResponseInit>` sites. Fixture
`examples/typescript/end-to-end/103_host_value_at_a_record_type` plus two emitter snapshot tests.

## Item 5 — a value asked for at `string` is its string conversion

`152da6b1`. The sibling of the truthiness rule this seam already states ("a value asked for at
`bool` is a TRUTHINESS test, not a cast: that is what JavaScript does wherever a boolean is
expected"), and stated in the same place — last, so every structural rule above answers first.

**Source.** `utils/buffer.ts`:

```ts
export const bufferToString = (buffer: ArrayBuffer): string => {
  if (buffer instanceof ArrayBuffer) {
    const enc = new TextDecoder('utf-8')
    return enc.decode(buffer)
  }
  return buffer
}
```

The brief's guess (a `TextDecoder`/`String.fromCharCode` shape) is not what this is. It type-checks
because `buffer` is `never` on the fall-through — `instanceof` on a value whose declared type IS
that class leaves nothing in the negative branch — and `never` is assignable to `string`. Smelt had
no rule for a concrete class at a string target and handed the value back at its own type.

Nothing that works is displaced: every site this now catches emitted text that did not compile. It
also settles a disagreement Smelt had with itself — the same function written with a CLOSURE body
already emitted exactly this conversion, because its return channel erases first.

**Attempt recorded, not kept.** The more faithful reading is the narrowing one: implement
`instanceof_inverse_guard` so the negative branch types the local `never`. It was written and it
works at the HIR level (the probe interned `Type::Never`), but it does NOT fix the site: MIR reads
the value through its declaring local, so the return operand is still `%0: ArrayBuffer` and the
coercion seam still needs a rule. Landing the narrowing alone would add risk with no payoff, so it
was reverted; the coercion rule is the whole fix. If a later round wants source-faithful `never`
narrowing, the guard belongs beside `instanceof_local_guard` in `matchers.rs` and the three shapes
are: a union loses the arms that are the class, an `Optional<C>` keeps only its absent side (because
`null instanceof C` is `false`), and a value whose type IS `C` becomes `never`.

**Result:** 37 → 36. Fixture
`examples/typescript/end-to-end/104_never_branch_at_a_string_return` covers both spellings.

## Item 6c-adjacent — a `HeadersInit` value at a `Headers` slot

`99db263c`. The four `Option<SmeltHeaders>` vs `Option<SmeltRecord<String, String>>` errors the
brief filed under item 1 are their own family, and not an object literal either.

**Source.** `context.ts:654`, `headers: responseHeaders ?? (headers as Record<string, string> | undefined)`.

**Rule.** WHATWG builds a header list from a `Headers`, a `Record<string, string>` or a sequence of
name/value pairs, and `headers_conversion_text` is already that conversion — `new Headers(init)` and
every init-dictionary `headers` key go through it. A coercion seam reaches the same pairing whenever
an init arm flows into a slot Smelt has typed `Headers`, which `a ?? b` does because it takes its
non-nullish LEFT type. `is_headers_init_type` is the predicate half, with the same arms, so a seam
can ask whether the conversion exists before committing to it.

**Also tried, and reverted.** The *source-faithful* rule is that TypeScript types `a ?? b` as
`NonNullable<typeof a> | typeof b`, so this expression is `Headers | Record<string, string>` — two
arms of the `HeadersInit` the property declares. `nullish_coalesce_expression` never reaches its own
union branch here, because `erased_or_union_surface` counts every `Type::Class` as an erased surface
and the last-resort `TypeAssert` arm fires first. Tightening
`is_structural_object_surface` so a MODELED HOST class is not a structural surface (its data is not
a set of declared fields) moved the error one level inward but did not change the count, and
changing `erased_or_union_surface` is far too wide a blast radius for this. Measured both ways: the
emitter rule alone gives 32, the two together also give 32, so the frontend change was reverted as
risk with no payoff. It is the right next step if someone revisits `??`.

**Result:** 36 → 32, all four sites. Fixture
`examples/typescript/end-to-end/105_headers_init_at_a_headers_slot`.

## Item 6a — `instanceof` on a generated union folds to a constant `false`

`5784aceb`. Round 31 found this and did not fix it. It is not in the Hono count and it is the most
serious thing in this round: **nothing fails to compile, the output is simply wrong.**

**Rule.** `x instanceof C` on a value whose static type is a generated union is the discriminant test
for the arms that ARE that class — the same rule the `typeof` probe on a union already follows. The
arm filter asked only whether the arm was a `Type::Class` with that name, but several JavaScript
classes lower to their own representation precisely because Smelt models their data: a `Promise<T>`
is `Type::Future`, an array is `Type::List`, a `Map` is `Type::JsMap`, a `Set` is `Type::Set`. Every
one of them answered "no arm matches", and no matching arm is emitted as a constant `false` (which
is the right answer for a union that genuinely cannot hold the class). So

```ts
async function label(value: string | Promise<string>): Promise<string> {
  if (value instanceof Promise) { return `deferred:${await value}`; }
  return `direct:${value}`;
}
```

answered `direct:[object Promise]` for a promise. The new predicate is stated over the
representation, so any class Smelt models as its own `Type` variant is answered without naming
spellings.

Fixture `examples/typescript/end-to-end/102_union_instanceof_through_arm_tags` and runtime tier case
`union_receiver_runtime::union_instanceof_selects_the_arm_that_is_that_class` cover promise, array
and map.

## Item 6b — the RED base tier

`fb8cd01e`. `union_receiver_runtime::union_optional_element_read_narrows_at_runtime` has been RED on
the base (round 31 verified it against a pristine emitter): emission ABORTED with
`type table does not contain literal operand type Unknown at optional_access.rs:563`.

**Rule.** The element an optional index read produces comes out of the runtime-narrowing `match`,
not out of a MIR local — it is a `SmeltUnknown` by construction. Converting it into the read's
declared result type is an EXTRACTION, which is target-only. The seam spelled it as
`value_at_type_text(.., source = <Unknown TypeId>, ..)` and so required the crate to have interned
`Type::Unknown`, which a crate that never writes `unknown` in source has not. `coercion`'s module doc
already states this rule for the erasing direction ("it must not require a `Type::Unknown` to be
interned (it often is not)"); extraction is its mirror.

The whole `union_receiver_runtime` tier (in the `runtime-tiers.yml` matrix) is now **4/4 green**. No
golden moved: every existing site that reached this seam had `unknown` interned.

## Item 4 — NOT LANDED: a nested function declaration hoists within its enclosing closure

This is the one item of the six that did not land. It is not rejected — it is a real rule and it
should be built — but it is two features, not one, and the second is a new emission shape that
wants its own agent and its own context. The evidence and the design are below so the next round can
start from them rather than re-derive them.

**Source.** `compose.ts`: `return dispatch(0)` textually BEFORE
`async function dispatch(i: number): Promise<Context> { .. }`, both inside the closure `compose`
returns, with `dispatch` closing over `index`, `middleware`, `context`, `next`, `onError` and
`onNotFound`, and recursing through `() => dispatch(i + 1)`.

**Rule.** A function DECLARATION is hoisted to the top of its enclosing function scope and is bound
for that whole scope, including uses that precede it textually — and, like any declaration, its own
name is in scope inside its body.

**What Smelt does today**, reduced to two probes (both are three lines; neither involves Hono):

```ts
function hoisted(base: number): number {
  const scale = 3
  return step(base)                                  // (a)
  function step(value: number): number { return value * scale }
}

function recursive(base: number): number {
  const limit = 3
  function walk(i: number): number {
    if (i >= limit) { return i }
    return walk(i + 1)                               // (b)
  }
  return walk(base)
}
```

(a) emits `let _smelt_tmp_3: f64 = (step)(base);` and **no `step` at all** — the declaration is
dropped and the name is bound nowhere. (b) emits the closure correctly, captures `limit` correctly,
and then references `(walk)` from INSIDE its own body, one statement before `let walk = …`.

Both have one cause: `lowering::local_function_declaration` lowers the declaration to a local closure
bound at its TEXTUAL position, and `defining_local_functions` deliberately keeps the function's own
name out of scope while its body is lowered. An unresolved name then falls back to "a free item with
that name", which is what leaves the dangling reference in each case.

**The design.**

1. *Hoisting.* A function declaration is bound for its whole block, so Smelt should lower it before
   the FIRST statement in that block that mentions its name (leaving it in place when nothing does).
   Hoisting unconditionally to the top of the block is wrong for Smelt specifically: it captures by
   value at creation, so a declaration moved above a `const` it closes over would lose the binding —
   `hoisted`'s `scale` is exactly that shape. "Before the first use" keeps every existing capture and
   is still the general rule, because hoisting is only observable when a use precedes the
   declaration.
2. *Self-reference.* Bind the declaration's name to its own local before lowering its body (drop the
   `defining_local_functions` exclusion for the self-name), which makes the closure capture the local
   it is assigned to. The emitter then needs the knot-tying shape a hand-writing Rust team would use
   for a recursive `Rc<dyn Fn>`: a `Rc<RefCell<Weak<dyn Fn(..)>>>` cell captured by the closure,
   upgraded at each recursive call, and filled in immediately after the closure is built. `Weak`
   rather than `Rc` so the closure does not keep itself alive. Detecting the shape is the emitter
   work: `closures.rs` must see that the closure being emitted captures the local the enclosing
   statement assigns it to, which `capture_analysis.rs` has the information for but does not
   currently report.

Hono's `dispatch` needs BOTH, so neither half alone moves `compose.rs:12`. Step 1 is self-contained
and fixes a dangling-name bug on its own (case (a) above does not compile today); step 2 is the new
emission shape.

## Also diagnosed, not fixed

### `__smelt_fn_value_627` not in scope (E0425, `main.rs:11000`)

Unchanged from round 31 and NOT the same family as `dispatch`, despite both being E0425. The
generated name is minted at the definition site and referenced from a scope the definition was not
lifted into; whoever owns `__smelt_fn_value_*` interning should make the name's scope and the
reference's scope one decision, as H42's render-scope rule does for types.

## Gates

| gate | result |
| --- | --- |
| Hono, clean clone, regenerated `dist-smelt` | **32 errors** (E0308 24, E0631 5, E0425 2, E0609 1) |
| examples invariant (`--fail-on-regression`) | avoidable erasure **0**, unchanged |
| es-toolkit ratchet | avoidable **31696**, down 20 from the committed 31716 — **re-snapshotted** |
| remeda generated `cargo test` | **1789 passed / 0 failed**; advisory report avoidable 24132 (no rise) |
| radash generated `cargo test` | **84 passed / 0 failed** |
| `cargo test -p smelt-frontend-ts --no-default-features` | 1093 passed / 0 failed |
| `cargo test -p smelt-codegen-rust` | 1065 passed / 0 failed (tiers `#[ignore]`d) |
| `cargo test --bin smelt` | 57 passed / 0 failed |
| `cargo test -p smelt-transpiler --test hir_cli_cross_language_tests` | 17 passed / 0 failed |
| `union_receiver_runtime -- --ignored` | **4 passed / 0 failed** (was 3/1 on the base) |
| `cargo clippy --all-targets` | 0 errors; no new finding in any file this round touched |

## SmeltUnknown delta

Net **negative**, in every corpus that measures it.

* Item 2 replaces a whole erased type (`ContentfulStatusCode` was `unknown`) with `f64`.
* Item 3 removes no erasure and adds none — it adds the erasure ADAPTER that was missing at a
  boundary that was already erased by construction.
* Item 1 replaces an erasure: the alternative to the structural conversion was routing the value
  through the runtime carrier, which is what CLAUDE.md's "preserve or recover the shape" forbids.
* Items 5, 6a, 6b and the `HeadersInit` rule all recover a concrete type where the emitter had
  none.

The examples invariant stays at 0 and the es-toolkit ratchet FELL by 20 (31716 → 31696), which is
re-snapshotted in the same commit as this note. Nothing in this round introduces a `SmeltUnknown`.

## Fixtures added

`100_exclude_over_a_literal_union`, `101_erased_callback_returns_a_host_class`,
`102_union_instanceof_through_arm_tags`, `103_host_value_at_a_record_type`,
`104_never_branch_at_a_string_return`, `105_headers_init_at_a_headers_slot` — each registered in
`END_TO_END_EXAMPLES` in the same commit as its rule, each `expected.stdout` diffed against Node 22.
