# Round 31 — Agent F: emitter coercions and MIR moves

Owner: Agent F. Base: `claude/estoolkit-test-failures-4fuf9e` merged into this worktree
(`3d66d389` + the round-30 merge). Measurement method is the brief's: a clean Hono clone at
`eebdf7be39abf0a872671835ccce0c4f03ea497a` with `.github/compat/hono/.` copied over it, a freshly
built full-feature `smelt`, `smelt --manifest-path <abs>/Smelt.toml build` run **from the repo
root**, then `cargo check --message-format=short` on `dist-smelt`.

Every family is stated as a TypeScript-semantics rule before it is fixed. Nothing here keys off a
Hono spelling.

## Baseline and result

| code | baseline (90) | after | delta |
| --- | ---: | ---: | ---: |
| E0308 | 51 | 36 | **-15** |
| E0277 | 24 | 24 | 0 |
| E0631 | 5 | 5 | 0 |
| E0382 | 5 | 0 | **-5** |
| E0425 | 2 | 2 | 0 |
| E0609 | 1 | 1 | 0 |
| E0599 | 1 | 1 | 0 |
| E0271 | 1 | 1 | 0 (fixed, then reverted — see item 3a) |
| **total** | **90** | **70** | **-20** |

The baseline reproduced exactly (90, same per-code split as the brief) before any change.

## Item 1 — a generated union value is dispatched through its own enum arms

**TypeScript rule.** A union with concrete arms lowers to a tagged `SmeltUnion…` enum, so the tag
already says which arm is live. A value whose static type is such a union is dispatched through its
OWN arms; the `SmeltUnknown` runtime-shape `match` is only for a value that is statically erased.
Matching a `SmeltUnion…` scrutinee with `SmeltUnknown::…` patterns does not type-check at all, and
erasing first would throw away an arm the compiler already knows.

Three emitter paths ignored it.

1. `fetch_types::body_conversion_text` sent a `BodyInit` union to the erased match (5 × E0308 in
   `context.rs`: `expected SmeltUnion1307, found SmeltUnknown`). Every arm of `BodyInit` names a
   concrete WHATWG type, so each arm now takes that type's own extraction rule — a string arm is
   text, a buffer-source arm is its bytes, a blob arm contributes bytes AND its MIME type. The
   sibling `headers_conversion_text` has had exactly this shape for `HeadersInit` since round 29;
   this is the same rule on the body channel.
2. A dotted property WRITE through a union receiver matched `&mut SmeltUnion…` with
   `SmeltUnknown::Object(..)` arms (4 × E0308 in `body.rs`). When EVERY arm declares the property,
   the write is dispatched on the tag and each arm performs its member's typed struct-field
   assignment, so nothing is erased. When one arm does not — the shape a source-level narrowing
   selects, which is what `request.bodyCache.formData = ..` under `!isRawRequest(request)` is — the
   write crosses the same boundary adapter the INDEXED write on a union receiver already uses (the
   `Place::Index` arm of the same function, H26's rule on the write path): erase out, mutate the
   erased view, convert back in and commit. `x.k = v` and `x["k"] = v` are one operation in
   TypeScript, so they take one rule here.
3. The matching READ, with `place_ty` resolving the field's type by the same rule, so the emitted
   text and the reported type are decided together. This one was not in the Hono count: it surfaced
   from the fixture, in a crate that interns no `unknown` and therefore has no erased carrier to
   answer a union field read with at all.

**Result:** 90 → 81. Fixture `examples/typescript/end-to-end/95_union_value_dispatch` (verified
against Node 22), runtime tier cases `union_receiver_runtime::
a_generated_union_dispatches_a_property_through_its_own_arms` and
`fetch_types_runtime::a_body_init_union_is_extracted_per_arm`.

`fetch_types_tests::an_erased_body_init_dispatches_on_the_runtime_tag` became
`a_body_init_union_dispatches_through_its_own_arms`: its premise — that `BodyInit`'s arms are
unmodeled host classes, so the whole union erases — stopped holding once the standards tier modeled
them, and its negative assertions now pin the new rule instead.

## Item 2 — the E0382s are not a MIR liveness family

**The brief's diagnosis was wrong, and the evidence is in the dumps.** `html.rs:357/362` and
friends are `resolve_callback(mut str: SmeltUnknown, ..)` reading its parameter four times, with
the first read moving out of it. But:

* `smelt dump-mir` prints OPTIMIZED MIR (`pipeline::lower_to_optimized_mir`), and every read of
  `%0` in `resolve_callback` is `copy %0`;
* `opt::move_on_last_use::convertible_locals` EXCLUDES parameters from conversion by construction,
  so that pass cannot have produced these moves and there is no liveness rule to fix.

The move came from the emitter. `core::operand_text` drops the `.clone()` on a `Copy` operand whose
type `type_contains_noncloneable` reports, and that predicate walked a union's ARMS.
`string | Promise<string> | HtmlEscapedString` has a `Future` arm, so the whole value looked
unclonable — while the Rust local it names is a plain `SmeltUnknown`.

**Rule.** A union's cloneability is its RUST representation's, not its arms'. A union that erases
stores a `SmeltUnknown`; a union with concrete storage stores a generated `SmeltUnion…` that
derives `Clone` (Hono's own `SmeltUnion66` holds a `SmeltFuture<SmeltResponse>` and derives it
today, which is the proof that the `Future` arm was never the obstacle). Erasing a value to inspect
it is a read, not a move — the same rule the concrete-union arm of `coercion::erase` already
states, and the one round-30 item 1b applied on the rendered-value side.

**Result:** 81 → 76, E0382 5 → 0. Fixture
`examples/typescript/end-to-end/96_union_read_is_not_a_move` (verified against Node 22): without
the fix its generated crate does not compile, because the guard's read of the parameter moves it
away from both branches.

## Item 3 — typed promise continuations

Both halves were diagnosed against the source line first, as the brief asks, and neither matched
the brief's guess.

### 3a. An arrow constant's return type is inferred from its body — WRITTEN, THEN REVERTED

**Landed as `9dd4afea`, reverted in the same branch because it turns the radash gate RED.** The
rule below is right and the fix is right; what it unmasks is a defect in generic instantiation that
belongs to another stream. Evidence and a reproduction are at the end of this section.

**TypeScript rule.** A `const` with no type annotation takes its initializer's type, and an arrow
with neither a return annotation nor a contextual signature infers its return type from what its
body evaluates to.

`lowering::local_arrow_callback_declaration` interns the binding's signature BEFORE lowering the
body, and its closure-body branch left the return as the erased placeholder. So Hono's

```ts
const res = (html: string) => this.#newResponse(html, arg, setDefaultContentType(..))
```

bound `(string) => unknown` while the closure it holds is `(string) => Response`. The binding's
type is what every CONSUMER reads, which is where the loss showed: `promise.then(cb)` is
`Promise<U>` for the callback's own `U` — `promise_continuation_call` already reads it off the
argument's function type — so the erased binding made a continuation that resolves a `Response`
claim to resolve `unknown`. That is the E0271.

`inferred_return_function_ty` is the return-type sibling of the existing
`throwing_widened_function_ty` beside it: it takes the lowered closure's return type, and only when
the declaration left it erased, so an annotated or contextually typed arrow keeps exactly what the
declaration resolved.

**Result while it was in:** 76 → 75, E0271 1 → 0, with fixture
`examples/typescript/end-to-end/97_arrow_const_infers_its_return` (verified against Node 22) and
every other gate green — `smelt-frontend-ts` 1092/0, `smelt-codegen-rust` 1060/0, the end-to-end
suite 17/0, the examples invariant at 0 and the es-toolkit ratchet unmoved.

**Why it was reverted.** `radash` stops EMITTING:

```
Error: callback adapter would discard the wrapped callback: in `<unnamed>`, adapting
`_smelt_tmp_5` (source return Some(Function(fn(f64) -> f64)) -> target return Some(String))
emitted body `String::new()`, which never invokes it (in `curry.test` at src/tests/curry.test.ts)
```

A coercion from a FUNCTION value to `String` has no conversion, so it rendered the target's default
and the adapter's backstop (which exists exactly to refuse a silently discarded callback) failed the
build. The types being paired are wrong, not the coercion.

**Reproduction, bisected to two whole tests** — `src/tests/curry.test.ts:206-213` and `:215-224`.
Each of them ALONE emits fine; only together do they fail:

```ts
test('calls add(1), then addFive, then twoX functions by 1', () => {
  const add = (y: number) => (x: number) => x + y
  const addFive = add(5)
  const twoX = (num: number) => num * 2
  const func = _.chain(add(1), addFive, twoX)      // chain's 3-arg overload, T4 = number
  ...
})
test('calls add(2), then addFive, then twoX, then repeatX functions by 1', () => {
  const add = (y: number) => (x: number) => x + y
  const twoX = (num: number) => num * 2
  const repeatX = (num: number) => 'X'.repeat(num)
  const func = _.chain(add(2), addFive, twoX, repeatX)   // 4-arg overload, T5 = string
  ...
})
```

Two separate closures each declare their own `const add`, at the same source text and therefore at
the same interned type. `chain` is an overload set generic in every step's type. With `add` typed
`unknown` at the binding, both instantiations coerce through the erased carrier and nothing is
observable; with `add` correctly typed `(number) => (number) => number`, the two instantiations of
the same overloaded generic collide and ONE call site's adapter is handed the OTHER's final return
type (`String`, which only the four-argument chain produces).

**So the defect this unmasks is a generic-instantiation one: two instantiations of the same
overloaded generic function, at the same argument type but different type arguments, share a
result.** That is Agent E's area ("generics across instantiation boundaries"), not an emitter
coercion. The frontend change is a five-line addition plus its helper
(`inferred_return_function_ty`, the return-type sibling of `throwing_widened_function_ty`) and
should be re-landed as soon as the instantiation defect is fixed; `git show 9dd4afea` in this branch
has it with its fixture.

### 3b. An async IIFE's contextual return type is its context's promise

**TypeScript rule.** A call's contextual type describes the call's RESULT, and round 29 already
routes it to an immediately invoked callee's return channel (`const f: T = (() => ..)()`). An
`async` function always returns a promise, so only the promise PART of that context can describe
what it returns: `Response | Promise<Response>` says `Promise<Response>` to an async callee, and a
context with no promise part says nothing at all.

Smelt handed the whole contextual type through, so Hono's `hono-base.ts` HEAD branch

```ts
return (async () => new Response(null, await this.#dispatch(request, executionCtx, env, 'GET')))()
```

inside `#dispatch(): Response | Promise<Response>` lowered the arrow as
`async fn() -> Future<Response | Future<Response>>` — a promise of the union where the union itself
was expected. `future_contextual_arm` reduces the context: a `Promise<T>` answers itself, a union
answers its single promise arm, anything else answers nothing (which drops the hint and leaves the
callee's own inference in charge), and a union with more than one promise arm is ambiguous and is
left to inference for the same reason.

**Result:** 76 → 70, the 4 `Result<SmeltUnknown>` vs `Result<SmeltUnion66>` and the 2
`SmeltUnion66` vs `SmeltFuture<SmeltUnion66>`. Fixture
`examples/typescript/end-to-end/98_async_iife_return_channel` covers the union context, the
non-union context, and the non-async sibling that must keep round 29's behaviour.

## Diagnosed, not fixed

Each of these has a named rule and the source line it comes from. None of them gets an erasure.

### `Option<globalThis_ResponseInit>` vs `Option<SmeltResponse>` / `SmeltResponse` (2, `main.rs:9276`, `:10154`)

**Source.** `context.ts:521`,
`this.#res = createResponseInstance((this.#res as Response).body, this.#res)`, against
`const createResponseInstance = (body?: BodyInit | null, init?: globalThis.ResponseInit): Response`.

**Rule — structural assignability.** A `Response` IS assignable to `ResponseInit`: it has `status:
number`, `statusText: string` and `headers: Headers`, and `HeadersInit` admits a `Headers`. Nothing
here is an overload or a union — the brief's guess (`ResponseInit | Response`) is not what the
source declares — it is plain structural typing, which Smelt requires nominal identity for.

**What the fix needs.** A conversion at the coercion seam from a value whose type exposes every
member of a target record type to that record: read each target field off the source by the same
member-read rule `x.member` uses, and build the struct. The member read has to work from TEXT plus
a type (a `SmeltResponse`'s `status` is the `status()` method on the prelude struct, not a field),
and `emitter::place` currently only answers that from a `Place`. That text-level member read is the
missing piece; the rule itself is one line.

### `dispatch` not in scope (E0425, `compose.rs:12`)

**Source.** `compose.ts`: `return dispatch(0)` textually BEFORE
`async function dispatch(i: number): Promise<Context> { .. }`, both inside the closure `compose`
returns. `dispatch` closes over `index`, `middleware`, `context`, `next` and `onError`.

**Rule.** A function DECLARATION is hoisted to the top of its enclosing function scope and is bound
for that whole scope, including uses that precede it textually. Smelt lifted `dispatch` to a free
item, so the name is bound nowhere at the call site. A nested declaration that captures enclosing
locals has to stay a closure bound in that scope (or become an item that takes its captures as
parameters and be referenced through that form everywhere), and the binding must exist before the
first use, not at the declaration's textual position.

### `__smelt_fn_value_627` not in scope (E0425, `main.rs:11041`)

Same shape one level out: a synthesized function value is referenced by name from a scope that does
not declare it. The generated name is minted at the definition site and used at a site the
definition was not lifted into; whoever owns `__smelt_fn_value_*` interning should make the name's
scope and the reference's scope one decision, as H42's render-scope rule does for types.

### `f64` vs `SmeltUnknown` (2, `main.rs:8483`, `:8487`)

**Source.** `http-exception.ts:48`, `readonly status: ContentfulStatusCode`, where
`ContentfulStatusCode = Exclude<StatusCode, ContentlessStatusCode>` and `StatusCode` is a union of
NUMERIC LITERAL types (`200 | 201 | … | -1`).

**Rule.** A union of numeric literal types is `number`: every arm's widened type is `number`, so
the union's is. `Exclude<A, B>` over such a union removes arms and leaves a union of numeric
literals, so it is `number` too. Smelt erased the whole thing to `unknown`, and the field then
reached `SmeltResponse::from_parts(status: f64, ..)` as a tagged value. Two rules, both general:
widen a literal union to its arms' common base type, and resolve `Exclude` over a literal union
rather than erasing it.

### `SmeltRequest` / `SmeltTypedArray` vs `SmeltUnknown` (2, `main.rs:7471`, `:8934`)

Both are inside a `_smelt_adapted_callback` wrapper: the adapter converts the erased incoming
argument to the CONTEXTUAL signature's parameter type and hands it to a function whose own
parameter is `SmeltUnknown`.

**Rule.** A callback adapter converts each argument to the ADAPTED function's own parameter type,
and its result to the contextual signature's return type. The contextual signature decides the
adapter's OUTER shape, never what the inner function is handed. This sits next to Agent E's item 3
(a substituted type parameter keeping the declaration's ABI), which is the same "the declaration
decides, not the instantiation" principle on the passing-mode channel.

### `router` on `Hono<E, S, BasePath>` (E0609, `main.rs:49690`)

Unchanged from round 30, where it is recorded as the survivor of item 2 and attributed to the
generic-class family (Agent E item 1): the field exists on the inner struct the generic class holds,
and the reference resolves against the outer one.

### `instanceof Promise` on a generated union folds to a constant `false`

Found while building fixture 96, not in the Hono count because the fixture's first shape was
replaced by a `typeof` probe. For

```ts
async function label(value: string | Promise<string>): Promise<string> {
  if (value instanceof Promise) { return `deferred:${await value}`; }
  return `direct:${value}`;
}
```

the emitted guard is `let _smelt_tmp_1: bool = false;`, so the promise branch is dead and the
function answers `direct:[object Promise]`-shaped output for a promise argument. A `typeof value
=== "string"` guard on the same union emits the correct `matches!(value, SmeltUnion2::M0(_))`, so
the tag test exists — `instanceof` against a modeled host class simply does not reach it. The rule
is the one item 1 states: a generated union is tested through its own arms, and an `instanceof`
whose right-hand side is the class of an arm is that arm's discriminant test.

### `union_optional_element_read_narrows_at_runtime` is RED on the base

`cargo test -p smelt-codegen-rust --test union_receiver_runtime -- --ignored` fails with
`type table does not contain literal operand type Unknown at emitter/optional_access.rs:563`.
Verified against a pristine emitter (every file of `crates/smelt-codegen-rust/src/emitter/`
checked out from the merge base) — it fails there too, so it is not from this round. The tier is
not in the brief's gate list, which is presumably why nobody has seen it.

## Gates

| gate | result |
| --- | --- |
| examples invariant (`--fail-on-regression`) | avoidable erasure **0**, unchanged |
| es-toolkit ratchet | avoidable **31716**, exactly the committed baseline (+0) |
| remeda generated `cargo test` | **1789 passed / 0 failed** |
| radash generated `cargo test` | **84 passed / 0 failed** |
| `cargo test -p smelt-frontend-ts --no-default-features` | 1092 passed / 0 failed |
| `cargo test -p smelt-codegen-rust` | 1060 passed / 0 failed (tiers `#[ignore]`d) |
| `cargo test --bin smelt` | 57 passed / 0 failed |
| `cargo test -p smelt-transpiler --test hir_cli_cross_language_tests` | 17 passed / 0 failed |
| new runtime tier cases | `fetch_types_runtime` 9/0, `union_receiver_runtime` 2 new + 1 pre-existing RED (below) |
| `cargo clippy --all-targets` | 0 errors; the three new findings on lines this round added were cleared, every other finding in the touched files is pre-existing |

While the reverted item 3a was in, the es-toolkit ratchet read **31714** — a fall of 2. Rebuilt
from a clean `dist-smelt` after the revert it is back to the baseline's 31716, so the baseline file
is left unchanged.

## SmeltUnknown delta

Net **negative**. Item 1 replaces three erased runtime-shape matches with typed per-arm dispatch,
item 2 removes a false "unclonable" claim (no representation change), and item 3b recovers the
concrete promise type the async IIFE was widening away. Nothing here adds a `SmeltUnknown`. The
examples invariant stays at 0 and the es-toolkit ratchet is unmoved at 31716 (it fell to 31714 with
the reverted item 3a in, which is the erasure that change was removing).

Final Hono measurement, clean clone, `dist-smelt` regenerated with the binary at the branch head:
**70 errors** — E0308 36, E0277 24, E0631 5, E0425 2, E0609 1, E0599 1, E0271 1.
