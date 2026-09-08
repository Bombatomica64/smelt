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

### `Option<tuple>`: written, then withdrawn as unreachable

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

### A pre-existing gap this exposed, worth its own item

`oneRoute[1]` on a one-element array **prints `  @0` instead of throwing**. The
array-hole path defaults a missing element, so an out-of-range read yields
`("", "", 0.0)` and the spread distributes those defaults. JavaScript throws
`TypeError: undefined is not iterable`. That is a silent wrong answer of exactly
the kind this family is about, but it is upstream of the spread — the read is
already wrong before the spread sees it — so it is not fixed here and not
folded in. It is the reason the fixture's absent case was withdrawn.

### The fixture

`examples/typescript/end-to-end/60_tuple_spread_arguments`, listed in
`END_TO_END_EXAMPLES` in the same commit. It prints what each callee actually
received, across a named top-level function, a leading fixed argument before a
spread, a closure, a method, a spread of a call result (asserting `evaluations:
1`, since indexing the operand directly used to re-evaluate it per element), and
a list element read. Runtime, because the broken lowering compiled.
