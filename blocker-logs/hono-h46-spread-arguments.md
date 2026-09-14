# H46 — spread arguments: three paths fixed, two to go, and the real problem

Round 18, item 1. Partly landed. The slice row did **not** clear, and the
reason is worth more than the fix: spread-argument lowering is scattered across
at least **five** call paths, each with its own idea of what a spread is, and
three of them were silently wrong in different ways.

## The bug

A spread of a tuple into a call was not distributed onto the callee's
parameters. Depending on which path lowered the call, it either contributed ONE
argument (the whole tuple, in parameter 0) or NONE at all — and the emitter then
padded the remaining parameters with `default_value(..)`. The result compiles:

```ts
sink(...triple)
```

```rust
sink(String::new(), String::new(), 0.0)   // every argument a default
```

Every argument a default, the tuple never read, no diagnostic. A silent wrong
answer is the worst outcome available here, which is why this is a correctness
bug rather than a blocker.

## What landed

`tuple_spread_arguments` expands one spread into one argument per tuple element,
each at the element's own type, and is now used by three paths:

| path | was | site |
| --- | --- | --- |
| typed closure callee, spread with no rest slot | `args: Vec::new()` — every parameter padded | `call_dispatch.rs`, before the arity checks |
| typed under-application | spread as argument 0, rest typed `None` | `typed_under_applied_closure_arguments` |
| function-typed FIELD callee | spread as argument 0, rest padded | the `self.argument(..)` loop after the rest branch |

Two details the first attempt got wrong, both caught by measurement rather than
review:

1. **Placement.** The expansion has to run BEFORE the arity checks, because
   `supplied_arg_count` counts a spread as one argument: a three-parameter call
   spread from one tuple looked like an under-supplied call and took the
   shortfall branch, which returned the empty argument list. Patching after the
   check left the branch unreachable and the output unchanged.
2. **Single evaluation.** Indexing the spread operand directly re-materializes
   it per element — the emitted code read `routes[i]` three times, and
   `f(...g())` would have called `g()` once per tuple element. The operand is
   now bound to a local (`__smelt_spread`) first and indexed from there.

A tuple index must reach codegen as a constant `Literal::Int`; the emitter
renders it as a Rust field access (`.0`, `.1`) and rejects anything it cannot
read as a constant. A `Literal::Float` index fails with "tuple index must be a
non-negative constant integer".

Verified on a closure-callee fixture: `sink(...routes[i])` now emits
`sink(__smelt_spread.0.clone(), __smelt_spread.1.clone(), __smelt_spread.2)`
with the operand evaluated once.

## What did NOT land, and why the row is still there

**The hono row is unchanged.** `smart-router/router.ts:36` is
`router.add(...routes[i])` where `#routes?: [string, string, T][]` — so the
spread operand is an **`Option<(String, String, T)>`**, not a bare tuple, and it
reaches the call through a path none of the three patches cover. The emission is
still `(router.add.clone())(_smelt_tmp_21.clone().clone().map_or(String::new(),
|value| value), String::new(), &(Default::default()))`.

Two gaps remain, and they are separate:

1. **A named top-level function callee.** `record(...routes[i])` where `record`
   is a module function takes a fourth path, still unfixed — the runtime fixture
   written for this item exercised exactly that shape and did not compile
   (`expected String, found (String, String, f64)`), which is how the gap was
   found. The fixture was withdrawn rather than committed non-compiling.
2. **An optional tuple operand.** `Option<Tuple>` needs a decision before code:
   spreading `undefined` THROWS in JavaScript, so the faithful lowering is an
   unwrap that throws, not a per-element default. `tuple_spread_arguments`
   currently rejects a non-tuple operand as a named blocker, which is why
   generation still succeeds — nothing silently guesses.

## The real problem, and the recommendation

Five paths, five behaviours. Chasing them one at a time is what this round did,
and it cost most of the round to fix three; the remaining two are the same work
again. The paths were located by putting a `panic!` in
`argument()`'s spread arm and reading the backtrace, which is the only reliable
way to find which of them a given source shape takes — that alone says the
structure is wrong.

**Recommend a single choke point** before finishing: one helper that every
fixed-arity call-argument collection goes through, which expands tuple spreads,
rejects unknown-arity spreads with a named blocker, and is the ONLY place a
spread is interpreted. Then finish (1) and (2) against it, and add the runtime
fixture — which must be runtime, since the compile-only version of this
assertion passes on the default-padded output.

Until then the landed part is a strict improvement (three silently-wrong paths
now correct, nothing regressed, 2680 tests green) but the family is open.

## Not this item

`context.ts:658`'s `(...args) => this.#newResponse(...(args as Parameters<NewResponse>))`
is half (b) of the brief and is still untouched. It needs the closure's ARITY
expanded from the contextual type as well as the spread distributed — see
`hono-h45-callback-parameter-typing.md` for the sizing. It should follow the
choke point rather than precede it.

---

## Round 19: the choke point, six paths, and the row cleared

**Slice 8 → 7.** The hono `router.add(...routes[i])` row cleared, and every one
of the 7 remaining errors is already accounted for: 6 are H42 (deferred), 1 is
H44 (numbered).

### The choke point

`lowering::spread_arguments` is now the only place a spread is interpreted for a
fixed-arity call:

- `expanded_call_arguments` turns an argument list into `Vec<CallArg>`, where a
  tuple spread has become one `Lowered` entry per element and everything else
  stays `Source`. Positional pairing with parameters is therefore correct only
  after expansion, which is the invariant every path needed and none had.
- `lower_call_arg` lowers one entry, applying a contextual hint only where there
  is still source to apply it to, and carries the debug assertion.

### Six paths, not five

Round 18 counted five. There were six; the sixth was found by the runtime
fixture rather than by reading code:

| # | path | was |
| --- | --- | --- |
| 1 | typed closure callee, no rest slot | `args: Vec::new()` — every parameter padded |
| 2 | typed under-application | spread as argument 0, rest typed `None` |
| 3 | function-typed field callee | spread as argument 0, rest padded |
| 4 | erased callee | spread lowered and DISCARDED |
| 5 | named top-level function (hints + deferred callbacks) | spread in slot 0, rest padded |
| 6 | **method callee** | spread as argument 0, rest padded |

Path 5 was the one hono's slice row needed. Path 6 only surfaced when the
fixture called `sink.take(...routes[1])` and the generated crate failed to
compile — a compile-only inline assertion would not have covered it, and neither
would reading the five sites again.

### The debug assertion

It lives in `lower_call_arg`, not in `argument()`. The ruling asked for an
assertion that no other site sees a spread, and in `argument()` that is not
expressible: variadic callers legitimately hand it a spread and read the operand
as one value (a rest list, `Math.max(...xs)`, `new`, a super call — 30-odd sites
across the frontend). The precise invariant is the narrower one: a *fixed-arity*
call never lowers a spread as a single argument. `expanded_call_arguments` makes
every spread a `Lowered` entry, so a `Source` spread reaching `lower_call_arg`
means some path built its `CallArg`s another way — which is exactly the bug
class — and it trips in test builds.

### `Option<tuple>`: withdrawn in round 19, REINSTATED in round 20 (the withdrawal was wrong)

The ruling was that spreading `undefined`/`null` throws a branded `TypeError`,
and that is right about JavaScript. It is **not implementable as a distinct
path, because nothing reaches it.** Every route resolves the optional before the
spread sees it:

- a cast (`...x as Route`) lowers through the narrowing path to
  `.expect("optional value was absent after narrowing")`;
- a list element read (`...xs[i]` — hono's actual spelling) lowers through the
  array-hole path to `.unwrap_or_else(|| default)`.

Both were tried in the runtime fixture. The throw, the `== null` test and the
`error_object_from_message("TypeError", ..)` call were written and working, then
removed rather than shipped as a branch no test can reach. `Option<tuple>` is
now the same named blocker as a list spread, with the reason at the emit site.

**Correction (round 20).** The claim above -- that nothing reaches the optional
arm -- was wrong, and it cost the hono row. The two shapes I tested (a cast, and
a list element read in a small fixture) do resolve the optional first, but
hono's `router.add(...routes[i])` does NOT: `#routes?: [string, string, T][]`
makes the element read `Option<(String, String, T)>`, which reaches the arm
directly. Round 19's `UnexpandedSpread` fix therefore sent it down the
unexpanded path and the `String` vs `(String, String, T)` row came back --
which my round-19 report did not catch, because I asserted "slice 7, unchanged
by this commit" WITHOUT re-measuring after the un-regression. Same class of
error as the stale-build claim earlier in that round.

Round 20 reinstates the arm: an `Option<tuple>` has a statically known width, so
it expands, unwrapping first, with an absent operand throwing a branded
`TypeError` through the ordinary throw route (`error_object_from_message`, so
`error.name` answers `TypeError`). A LIST spread remains `UnexpandedSpread`,
which is what keeps es-toolkit on its previous path. Both are verified below
from a clean `dist-smelt` rather than asserted.

### A pre-existing gap this exposed, worth its own item

`oneRoute[1]` on a one-element array **prints `  @0` instead of throwing**. The
array-hole path defaults a missing element, so an out-of-range read yields
`("", "", 0.0)` and the spread distributes those defaults. JavaScript throws
`TypeError: undefined is not iterable`. That is a silent wrong answer of exactly
the kind this family is about, but it is upstream of the spread — the read is
already wrong before the spread sees it — so it is not fixed here and not
folded in. It is the reason the fixture's absent case was withdrawn.

### The fixture

`examples/typescript/end-to-end/61_tuple_spread_arguments`, listed in
`END_TO_END_EXAMPLES` in the same commit. It prints what each callee actually
received, across a named top-level function, a leading fixed argument before a
spread, a closure, a method, a spread of a call result (asserting `evaluations:
1`, since indexing the operand directly used to re-evaluate it per element), and
a list element read. Runtime, because the broken lowering compiled.

---

## Half (b), round 20: the chain is four sites deep, not one

The ruling was to find the third site and land the tuple-typed rest with the
arity it implies. I found the third **and a fourth**, got the spread expanding,
and stopped short of landing because the closure signature it produces is wrong.
Recording the map so the next attempt starts where this one ended.

The target shape (`context.ts:658`, reduced):

```ts
type Respond = (data: string, init?: Init) => string;
respond: Respond = (...args) => this.inner(...(args as Parameters<Respond>));
```

### The chain

| # | site | what it decides | state |
| --- | --- | --- | --- |
| 1 | `arrow_callback_param_types_with_hint` (`callbacks/body_lowering.rs`) | the closure's parameter TYPES: pushes ONE `List<union>` for a contextual rest | changed and reverted |
| 2 | `arrow_function_expression_with_hint`, the `rest` binding | whether the closure is variadic: `arrow.params.rest.map(\|_\| items.len())` marks it a rest even after expansion, which repacks the tail | changed and reverted |
| 3 | `arrow_callback_from_params`' non-list rest branch (`callbacks/classify.rs`) | the rest BINDING: `ListLit` of the individual `Param(i)`, typed `List<item>` where it should be the tuple | changed and reverted |
| 4 | the **non-callback fallback** closure lowering | the closure's actual parameter LOCALS for this shape | not reached |

Sites 1-3 were changed together and the spread did start expanding —
`this.inner(__smelt_spread.0.clone(), …)` instead of binding the whole `args` to
parameter 0. But the emitted signature was

```rust
move |closure_arg_0: Option<Init>, _arg0: Option<Init>| {
```

Both parameters typed `Option<Init>`, the first of which should be `String`, and
the second named `_arg0` — the emitter's PADDING name, not a real parameter.

### Why site 4 is the one that matters

`callback_expr_to_closure_with_return_ty` does build one local per parameter
type, so had this shape gone through it the arity would have been right. The
`_arg0` padding says it did not: the arrow's body is `this.inner(..)`, a
statically-resolvable method call that the compact callback IR does not model,
so classification fails and `arrow_function_expression_with_hint` falls through
to the full closure-body lowering — which builds the closure's params from the
ARROW's own AST, where there is exactly one binding (the rest). Widening the
type list without widening that param construction is what produced a
one-parameter closure wearing two parameters' types.

The comment in `arrow_function_expression_with_hint` describes that fallback and
why it exists; what it does not do is carry the expansion. That is the fix.

### Why it was reverted rather than shipped

The intermediate state emits a closure whose signature does not match its
declared field type, on the same corpus this family already broke once. Round 19
regressed the es-toolkit gate by generalising from an incomplete picture of
these paths, so shipping a fourth guess in the same area was not worth the
option value — particularly when the diagnosis is the durable part.

es-toolkit is unaffected either way: its customizer carries an explicit rest
annotation and takes the annotated-list branch, so none of sites 1-3 fire for
it. Verified by the full gate below rather than by reading.

---

## Round 21: site 4 implemented, arity fixed, one element still dropped

Site 4 is the right site and the fix works as far as the closure signature. It
is **not committed**, because the call it produces drops an argument, and a
dropped argument is the same silent-wrong-answer class this family exists to
remove. Recording the exact state so the next attempt starts from it rather than
from the diagnosis.

### What landed in the working tree (then reverted)

A shared predicate, `contextual_rest_expansion(arrow, contextual_function)`,
answers one question — "does this rest binding cover a statically known set of
contextual parameters?" — and all four sites ask it, because they have to agree:

| site | change |
| --- | --- |
| 1 `arrow_callback_param_types_with_hint` | push one type per covered parameter instead of one `List<union>` |
| 2 `arrow_function_expression_with_hint` | `rest = None` when expanded, so the tail is not repacked |
| 3 `arrow_callback_from_params` (compact IR) | rest binding typed as the tuple over the covered parameters |
| 4 `closure_body_expr_from_parts` (fallback) | one closure parameter local per covered type, then `let args = (p0, p1, …)` binding the rest name to the tuple |

`Pattern` had to be added to the module's `smelt_hir` imports for the `Stmt::Let`
that binds the tuple.

### What it produced

Before (one parameter wearing the wrong type, whole binding to parameter 0):

```rust
move |closure_arg_0: Option<Init>, _arg0: Option<Init>| {
    let _smelt_tmp_2: String = this.inner(closure_arg_0.clone(), None::<Init>);
```

After — the contextual arity, the right types, the spread expanded:

```rust
move |closure_arg_0: String, closure_arg_1: Option<Init>| {
    let __smelt_spread: (String, Option<Init>) = args.clone();
    let _smelt_tmp_7: String = this.inner(__smelt_spread.0.clone(), _smelt_tmp_6.clone());
```

### The remaining defect, located

`_smelt_tmp_6` is `let _smelt_tmp_6: Option<Init> = None;` — a synthesized
default, not `__smelt_spread.1`. So the second tuple element never reaches the
callee and `init` is dropped.

It is **not** the expansion: an `eprintln!` in `spread_argument_elements`
reports `items=2 optional=false`, so the choke point emits both elements. Nor is
it the `Parameters<Respond>` cast — removing the cast (`this.inner(...args)`)
gives byte-identical output. So a CONSUMER between the choke point and the call
takes the first expanded argument and synthesizes a typed `None` for the rest.

The `Literal::None`-at-the-parameter-type shape is the signature of the
under-application synthesis, and `inner(data: string, init?: Init)` has
`required_params = 1`, which is exactly the condition that path tests. The
candidate not yet ruled out is that `this.inner(..)` inside a closure body is
collected by a **seventh** argument site —
`CallbackExprKind::MethodCall`'s own collection in `callbacks/closures.rs` —
rather than by the method path routed in round 19. That is where to look first.

### Why it was not shipped

The arity half is a strict improvement and the diff is clean, but shipped
together with a dropped argument it would trade one silent wrong answer for
another. Rounds 18-20 shipped spread work three times on an incomplete picture
of these paths -- once breaking the es-toolkit gate, once giving back the hono
row -- and the cost of a fourth was not worth the partial credit. The gates were
not run against it either, since it was never a candidate to commit.

---

## Round 22: half (b) works, and it breaks remeda. Not shipped.

The acceptance criterion was met and the cause of the dropped element was found.
Then the corpus gate said no. Recording both, because the second is the reason
this family should stop being attempted as a patch.

### It was not the seventh site

The ruling named `CallbackExprKind::MethodCall`'s collection in
`callbacks/closures.rs`. It is not that, and it is not in the frontend at all.
Instrumented rather than guessed: probes in the under-application path and the
method path showed the method path firing with `expanded=2`, so HIR received
BOTH arguments. The drop happened in MIR.

`crates/smelt-mir/src/lower/expr.rs`, the `ExprKind::Index` arm:

```rust
if matches!(self.krate.types.get(expr.ty), Some(Type::Optional(_))) {
    ... Rvalue::OptionalIndex ...
}
```

An optional RESULT type is taken as proof the read can miss -- true for `xs[i]`
on a list, where `Option<T>` means "index may be out of range". A TUPLE read is
not that: its index is a constant field position that always exists, and an
`Optional` result is the ELEMENT's own declared type. So `args.1` on
`(String, Option<Init>)` asked "is this slot present?" of a slot that is always
present, and answered `None`. MIR showed it exactly: `%4[0]` for element 0
(`String`) beside `%6 = copy %4?.[1]` for element 1.

Guarding that arm on the receiver not being a tuple (with a constant index --
a VARIABLE tuple index must keep the optional route, since codegen's
`tuple_index` rejects a non-constant) produced the acceptance criterion:

```rust
move |closure_arg_0: String, closure_arg_1: Option<Init>| {
    let args: (String, Option<Init>) = (closure_arg_0.clone(), closure_arg_1.clone());
    let __smelt_spread: (String, Option<Init>) = args.clone();
    this.inner(__smelt_spread.0.clone(), __smelt_spread.1.clone())
```

A runtime fixture over the `context.ts:658` shape printed all four lines
correctly, with and without the `Parameters<F>` cast:
`data=first init=present` / `data=second init=<none>` / `a/1` / `b/2`.

### Then remeda stopped transpiling

`EmitError: "tuple index must be a non-negative constant integer"`, 0 files
emitted. Isolated by reverting one change at a time:

| tree | remeda |
| --- | --- |
| frontend sites 1-4 + MIR fix | **fails** |
| frontend sites 1-4, no MIR fix | **fails** |
| MIR fix alone, no frontend sites | passes (391 files) |

So the MIR fix is safe and it is the FRONTEND expansion that breaks remeda.

### Why, and why it is a design question rather than a bug

Typing a rest binding as a tuple is only sound when the binding is used ONLY in
spreads. `args` is an ordinary value: source is free to write `args[i]`,
`args.length`, `args.map(..)`, or pass it on as a list. remeda does something of
that kind, and a tuple has no answer for a variable index -- which is precisely
the error, raised from codegen's tuple-index path.

So half (b) as ruled -- "a rest parameter whose contextual type gives its width
is a tuple" -- is true about TypeScript's `Parameters<F>` and false as a
lowering rule, unless it is conditioned on how the binding is USED. That
condition is a use-site analysis over the closure body: tuple if every use is a
spread, list otherwise. It is not a fifth site to patch; it is a new fact to
compute, and it wants to be ruled on before it is built.

Note also that the MIR fix, though correct in principle, has no demonstrable
trigger without the expansion: a hand-written tuple with an optional element
read by constant index (`slot[1]` on `[string, number | undefined]`) emits
`slot.clone().1.clone()` identically before and after. So it is not shippable on
its own either -- it would be an untested change whose only known witness is a
feature that cannot land yet.

### State

Nothing from this round is committed. The working tree is back to the merged
head, verified: es-toolkit transpiles and runs 1055/4, remeda transpiles 391
files, the hono slice is 7. The fixture built for the acceptance criterion was
removed with the rest, since it fails without the expansion.
