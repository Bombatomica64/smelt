# H48 — deferred: a contextual rest binding as the parameter tuple

Rounds 18-23. Deferred per the round-23 ruling: half (b) did not land complete,
so it is numbered here with everything learned. **Nothing about it is shipped.**

## The goal

A rest parameter whose contextual signature has no rest of its own covers a
known number of parameters, so `Parameters<F>` is a tuple and TypeScript types
it that way. Hono's `context.ts:658` is the shape:

```ts
respond: Respond = (...args) => this.inner(...(args as Parameters<Respond>));
```

Lowered correctly, the closure takes the contextual parameters positionally and
the spread distributes them.

## It works. Twice verified, on the target shape

Round 23 reached the acceptance criterion exactly:

```rust
move |closure_arg_0: String, closure_arg_1: Option<Init>| {
    let __smelt_spread: (String, Option<Init>) = args.clone();
    this.inner(__smelt_spread.0.clone(), __smelt_spread.1.clone())
```

with a runtime fixture printing `data=first init=present` /
`data=second init=<none>`, both arguments received, with and without the cast.

## Five sites, and the fifth is in MIR

| # | site | decides |
| --- | --- | --- |
| 1 | `arrow_callback_param_types_with_hint` | the closure's parameter TYPES |
| 2 | `arrow_function_expression_with_hint`'s `rest` binding | whether the closure stays variadic |
| 3 | `arrow_callback_from_params` (compact IR) | the rest BINDING's type |
| 4 | `closure_body_expr_from_parts` (fallback) | the parameter LOCALS and the `let args = (p0, p1, …)` tuple |
| 5 | `lower/expr.rs`'s `ExprKind::Index` arm | whether a tuple read is an OPTIONAL index |

Site 5 is worth keeping in mind independently. It treats an optional RESULT type
as proof the read can miss — true for `xs[i]` on a list, false for a tuple,
whose constant index always names a slot that exists. So `args.1` on
`(String, Option<Init>)` asked whether an always-present slot was present and
answered `None`, dropping the argument. MIR showed it as `%4[0]` beside
`%6 = copy %4?.[1]`. The guard needs the receiver to be a tuple **and** the
index to be a constant operand, because a variable tuple index cannot be a field
position and codegen rejects it outright.

**Site 5 has no witness of its own.** A source-level `slot[1]` on
`[string, number | undefined]` never reaches that arm — verified by writing the
MIR-level `format_compact` test the round-23 ruling asked for and finding it
passes without the fix, for both a primitive and an interface element type. The
arm is only reachable from synthesized HIR, i.e. from this feature. So site 5
lands with H48 or not at all; committing it alone would be untested code.

## Why it is deferred: the use-site condition is necessary but not sufficient

Typing the binding as a tuple is unsound when the binding is used as a value —
a tuple has no answer for a variable index. Round 23 implemented the ruling's
use-site pre-scan: count references to the binding, count those that are the
operand of a spread argument (looking through `as`, `satisfies`, `!` and
parentheses so Hono's spelling counts), and expand only when the two agree and
are non-zero.

That is the right condition and it is not enough. With the scan wired to a hook
that actually fires (`visit_argument` is not a hook in this oxc version and
silently counted zero; `visit_call_expression`/`visit_new_expression` are),
remeda **still** fails to transpile:

```
EmitError: "tuple index must be a non-negative constant integer"
```

So remeda contains at least one `(...args) => f(...args)` whose every use IS a
spread, and expanding it still produces a tuple index codegen cannot resolve.
The isolation table across rounds 22-23:

| tree | remeda |
| --- | --- |
| sites 1-5, no use-site condition | fails |
| sites 1-5 with the condition, scan counting 0 (dead hook) | passes — but the feature is off |
| sites 1-5 with the condition, scan counting correctly | **fails** |
| none of it | passes (391 files) |

The third row is the informative one: the condition works, the feature turns on
for remeda too, and something downstream of the expansion still cannot handle it.

## What the next attempt should establish first

Not another site. The open question is **which remeda site expands and why its
tuple index is non-constant** — that is one `smelt build` with the expansion on
and the emitter's blocker taught to name its function (the `Mir::file_paths` /
`current_function_site` plumbing from round 17 already exists; the tuple-index
error does not use it yet). Adding that to the tuple-index blocker is a small,
independently useful change and would have answered this in one build rather
than a round.

Two candidate explanations to distinguish there:

1. The expansion fires for a rest whose covered parameters include a rest or an
   unbounded tail, so the tuple width is wrong and an index runs past it.
2. The spread reaches a VARIADIC callee, where the expansion should not apply at
   all — the choke point hands a spread of unknown width back unexpanded
   (`UnexpandedSpread`), but an expanded tuple binding spread into a variadic
   callee is a case neither rule covers.

## Cost recorded honestly

Six rounds on this family: the choke point and five other sites landed and are
correct; this last piece has now been implemented twice, verified twice on its
target shape, and reverted twice because a blocking corpus rejects it. Hono
needs it for `context.ts:658`, which is not on the slice's error list — the
slice is 7 = 6 H42 + 1 H44 without it. So nothing currently measured regresses
by deferring, and the cost of continuing to guess exceeds the cost of waiting
for the one diagnostic above.
