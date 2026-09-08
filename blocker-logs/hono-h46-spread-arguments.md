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
