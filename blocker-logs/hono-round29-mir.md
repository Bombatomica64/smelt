# Round 29, Agent A — MIR semantics (H47, H48, H70, H69, H14)

Owner: Hono implementer (agent A). Date: 2026-09-14.
Branch: `worktree-agent-a8d527518990ca508`, cut from
`claude/estoolkit-test-failures-4fuf9e` and merged at `6bf4d2c3`.

Four commits landed. Two of the five assigned items are **not landed** and are
written up here with the evidence and the proposed alternative, per the brief's
rule that an item is never rejected on its own.

| item | landed | commit |
| --- | --- | --- |
| H47 out-of-range element read | **no** — see §H47 | — |
| H48 contextual rest tuple (incl. MIR site 5) | **no** — see §H48 | — |
| H70 throw through an awaited erased call | yes (+ a second defect found under it) | `cb3fef3c` |
| H69 synthesized derived-class constructor | yes | `558af905` |
| H14 Unknown-interning fallback | yes, as diagnostics; no reproduction exists | `f8af8284` |
| (not assigned) `ConstructorParameters<C>` | yes — it blocks the Hono build | `03dc15a1` |

---

## H70 — landed, and it was two defects, not one

The round-27 note numbered H70 as "the rethrow inside `_guard` leaves through
the awaited erased call and is not caught by the source's own `try`/`catch`".
Building the recorded two-module reproduction confirmed the symptom exactly
(no output, exit 101, where Node prints `user1|default-user|unknown error`) and
then showed that **two independent defects** stood in the way. Both are fixed.

### 1. The guard's condition vanished

`_guard` is

```ts
const _guard = (err: any) => {
  if (shouldGuard && !shouldGuard(err)) throw err
  return undefined as any
}
```

and its MIR was one block:

```
closure ClosureId(0) -> Unknown throws
  bb0:
    throw copy %0
```

The condition, the branch and the tail return were all gone, so radash's
`guard` rethrew **every** error it was written to swallow.

Cause: the compact callback IR. `CallbackExprKind::Throw` lowers to a body
STATEMENT (`Stmt::Throw`), not to a value, so it runs where it is *emitted*,
not where its value is *used*. `if (c) { throw e } return v` is modeled as a
ternary, and `callback_expr_to_body_expr` lowers both arms eagerly — so the
statement hoisted out of the guard and became unconditional.

This is the same shape as the capture-assignment guard already sitting beside
it in `body_lowering.rs` (the other kind that emits a statement), and it takes
the same remedy: a conditionally-evaluated throw surfaces a fallback-eligible
error (`callback throws inside a conditional; needs closure-body lowering`) and
the whole arrow retries through full closure-body lowering, which emits a real
`switch` terminator. A throw in an unconditionally evaluated position is still
modeled by the compact IR, so those emissions are byte-identical — **no
existing golden moved.**

The narrowest reproduction has no promise in it at all:

```ts
const inner = (e: string) => { if (flag) { throw e } return "tail"; };
```

The same body as a top-level `function` lowered correctly the whole time; only
the closure form collapsed.

### 2. The rethrow escaped through the `await` (H70 proper)

With (1) fixed the program still exited 101. A local arrow binding interns its
function type BEFORE its body is lowered, so `may_throw` was the syntactic guess
`false`. Every call that passed the binding on was typed from that, so the
throwing handler handed to the erased promise's `.catch(..)` was adapted DOWN
to the non-throwing ABI through the panic route:

```rust
move |arg0: &SmeltUnknown| (_smelt_adapted_callback)(arg0)
    .unwrap_or_else(|error| smelt_panic_throw(error))
```

A panic route only reaches a source `catch` when the `catch_unwind` is around
the **invocation** — and a promise continuation is invoked later, during the
`await`, where no `catch_unwind` is in scope. So the panic left the process.

The binding now takes the lowered closure's own `may_throw` (and only that bit,
so every other part of the resolved signature is untouched), and the error rides
the `Result` channel the `await` already propagates. The reproduction then
prints `user1|default-user|unknown error`, byte-identical to Node 22.

Note for whoever revisits the `throwing.rs` idea in the brief: extending the
throwing-propagation pass so an `await` becomes a fallible terminator would NOT
have fixed this. The error never reached the `Result` channel at all — it was
converted to a panic one frame earlier, at the argument coercion.

### Guarded by

`crates/smelt-codegen-rust/tests/guarded_throw_runtime.rs`, registered in the
`throwing` shard of `.github/workflows/runtime-tiers.yml`. Three cases: the
swallow case fails without (1), the rethrow case fails without (2), and a
promise-free case pins the branch collapse on its own.

**Why a tier and not an `examples/` fixture.** radash's `guard` is
`TFunction extends () => any` over an `err: any` handler, so the generated crate
carries 43 genuinely-`any` erased lines. Adding it to the examples corpus moved
that corpus from avoidable 0 to avoidable 43, and that corpus holds a hard
zero invariant. Measured both ways; the corpus is back at 0.

---

## H69 — landed

`synthesize_default_class_constructor` gave every derived class with no explicit
constructor one `Option<SmeltUnknown>` "super argument". JavaScript's implicit
derived constructor is `constructor(...args) { super(...args) }`, and `...args`
is not open-ended: TypeScript types `new Derived(..)` against the BASE
constructor's signature.

The erased slot was wrong three ways at once, and all three are one rule:

* it **dropped every argument past the first**, so
  `new Point3(1, 2)` over `class Point { constructor(public x: number, public y: number) {} }`
  did not compile at all (E0061 at the call site — a blocker, not just erasure);
* nothing forwarded to `super(..)`, so the base's field initializers and
  parameter properties never ran: `p.x` silently read `0`, and `d.label` read
  `""` where the base initializes it to `"base"`;
* it was four avoidable-erasure lines per such class.

The synthesized constructor now declares the base constructor's own parameters
by name and type and forwards them through `lower_declared_base_super_call` —
the same lowering an explicit `constructor(..) { super(..) }` uses — so the
base's parameter defaults, field initializers, parameter properties and its own
`super(..)` all run exactly once and in source order, and a two-level chain of
implicit constructors forwards the original parameters the whole way down with
no chain walk.

The erased forwarded argument survives only where the base is NOT reproducible:
a host/prelude constructor (`Date`), an `Error`-like base, an abstract base, or
a generic one. There the base's arity and parameter types are not available to
this lowering at all, so no concrete type, generated union, or scoped generic
can carry the forwarded value, and the call-compatible erased slot is what keeps
`new Subclass(x)` from becoming a blocker. Documented at the emit site and
pinned by `a_non_reproducible_base_keeps_the_erased_forwarded_constructor_argument`.

Fixture `examples/typescript/end-to-end/89_derived_default_constructor`:
no-parameter base, parameter properties, an optional base parameter, and a
two-level chain. `expected.stdout` is Node 22's, byte for byte.

**Found and not fixed:** a GENERIC base (`class StringBox extends Box<string>`)
keeps the erased slot, and its field reads `SmeltUnknown::Null` rather than the
constructed value — a silent wrong value, pre-existing, and not in this item's
scope. Worth numbering.

---

## H14 — landed as a diagnostics fix; the recorded reproduction does not reproduce

`place_ty`'s `_ =>` arms answer "this read has no static type", which the
emitter spells as the erased carrier — and they asked for it through
`type_id(Type::Unknown)`, a LOOKUP. That made the answer depend on something
unrelated to the read: a crate with no erased value anywhere has no
`Type::Unknown` entry in its type table, so the blocker read

    type table does not contain literal operand type Unknown at emitter/types.rs:823

naming a line of the COMPILER rather than the read in the user's program.

The six fallbacks now degrade at the site through one documented helper
(`erased_fallback_ty`). When the crate already interns the erased carrier the
answer is exactly what it was — every existing emission is byte-identical — and
when it does not, the blocker names the member being read and the receiver's
rendered type, with the enclosing function attached by `with_site`. **Nothing
interns `Unknown`**, which is the repair the campaign plan's row 102 warns
against: it would flip `stdlib::needs_unknown_type` crate-wide and emit the
whole erased prelude for a program with no erased value.

**The reproduction could not be produced.** The shape the plan records
(`Code = 200 | 404` literal union reaching a generic interface and a generic
function) transpiles and runs cleanly on this head. Four variations were tried:

| shape | result |
| --- | --- |
| literal union field read through `Envelope<T>` | transpiles |
| `codeOf<T extends HasCode>(box: Box<T>)` reading `box.value.code` | transpiles |
| `codeOf<T extends Envelope<string>>(value: T)` reading `value.code` | transpiles |
| the union through `pick`/`report` over a `Envelope<T>[]` | transpiles and runs |

So the committed test is a **shape guard**, not a reproduction: it pins the
recorded shape emitting, and pins the compiler-line blocker out of the output
for every shape, so the "just intern `Unknown`" repair cannot land silently.
If a reproduction is wanted, it needs a hand-built `Mir` (a `Place::Field` on a
receiver that is neither dict, optional, class nor interface, in a type table
with no `Type::Unknown`); there is no such test harness in
`smelt-codegen-rust` today, and building one is its own small piece of work.

---

## H47 — NOT landed. It needs its own round, and here is the measurement that says so

The ruling was option 1 (throw). Implementing the throw ALONE is not an
improvement, and the measurement below is why.

### The defect, reproduced

```ts
type Route = [string, string, number];
const oneRoute: Route[] = [["PATCH", "/maybe", 4]];
console.log(record(...oneRoute[0]));
try { console.log(record(...oneRoute[1])); }
catch (error) { console.log("caught", error instanceof TypeError); }
const nums: number[] = [1, 2, 3];
console.log(nums[5]);
console.log(nums[5] ?? -1);
console.log(nums[5] === undefined);
```

| line | Node 22 | Smelt today |
| --- | --- | --- |
| `record(...oneRoute[0])` | `PATCH /maybe @4` | `PATCH /maybe @4` |
| `record(...oneRoute[1])` | `caught true` | `  @0` |
| `console.log(nums[5])` | `undefined` | `0` |
| `nums[5] ?? -1` | `-1` | `0` |
| `nums[5] === undefined` | `true` | `false` |

Four of five lines are wrong, not one. The note's framing ("the spread
distributes defaults") is the *narrowest* symptom.

### Why the throw alone is not the fix

The emitted read is already `list.get(i).cloned()` — an `Option<T>` — followed
by `.unwrap_or_else(|| <default>)`, and the two consumers that CAN absorb the
miss already route around it (`optional_element_read_text` for an `Option`
target, `erased_element_read_text` for an erased target). So the only site left
is `place_ty`'s total read, i.e. exactly "the consumer needs a `T`".

Replacing that default with a throw introduces a NEW wrong answer wherever
JavaScript does not throw:

| consumer | JS | default today | throw-at-read |
| --- | --- | --- | --- |
| spread, member read, call | **TypeError** | wrong value | correct |
| `nums[5] + 1` | `NaN` | `1` | **wrong (throws)** |
| `` `${nums[5]}` `` | `"undefined"` | `"0"` | **wrong (throws)** |
| `console.log(nums[5])` | `undefined` | `0` | **wrong (throws)** |
| `nums[5] ?? -1` | `-1` | `0` | **wrong (throws)** |

Three of those five rows are the common case. Trading a silently wrong value
for a spurious throw is not a net improvement, and the brief's own design says
so: the throw is only correct once the read's MIR type is `Optional(T)` "where
the consumer can absorb it". Both halves are one change; the throw half alone
regresses.

### And the throw half cannot be spelled where it is needed

A throw must be catchable by the enclosing source `try`. MIR's exception edges
live on TERMINATORS (`standards-throwing-rvalue.md`), and a `try` is emitted as
a `catch_unwind` around a CALL — so a panic raised while evaluating a hoisted
spread operand, one statement before the call, is not caught. Making the read
itself a terminator means turning every `arr[i]` into a block split, which is
not a change to make in passing.

There is a third, purely mechanical obstacle worth recording because it is easy
to miss: the panic-route prelude is gated by `stdlib::needs_panic_route`, and a
program whose only throw is this one would emit a call to
`smelt_panic_throw` that was never declared (E0425). The gate would have to
learn to scan for list-index reads.

### Proposed alternative (a round of its own, in this order)

1. Type the read `Optional(T)` in MIR — `ExprKind::Index` already has the
   `Rvalue::OptionalIndex` path for an `Optional`-typed `expr.ty`, so the change
   is in what the FRONTEND types an index read as, not in a new MIR shape. This
   is the wide half: every consumer either narrows or propagates, and it is the
   same shape of work as H42.
2. Route the stringify/erase consumers (`console.log`, template literals,
   `String(x)`) through the erased read so they answer `undefined`, and
   arithmetic through the numeric coercion so it answers `NaN` — both are what
   `Optional(T)` gives them for free once (1) lands.
3. Only then make the `T`-needing consumers (spread, member read, call) fallible
   terminators, which is where the ruling's `TypeError` belongs.
4. Measure es-toolkit and remeda between (1) and (3): a suite that passes today
   may be relying on a defaulted read, and if so that is itself worth knowing —
   which is what the original note asked for and what has still not been done.

`array_hole_value` is deliberately NOT touched: its only caller is the WRITE
path's `Vec::resize` filler, which needs a value and cannot throw. The brief's
"delete `array_hole_value`'s concrete-type default arm" refers to the read
fallback, which has since been factored out as `element_missing_value_text`.

---

## H48 — NOT landed

The note is explicit that site 5 "lands with H48 or not at all; committing it
alone would be untested code", and that the feature has been implemented and
reverted twice because remeda rejects it with
`EmitError: "tuple index must be a non-negative constant integer"` even with the
use-site condition wired to a hook that fires.

The note also names exactly what the next attempt must establish first, and it
is not a sixth site: **which remeda site expands and why its tuple index is
non-constant**, which needs the tuple-index blocker taught to name its function
(the `Mir::file_paths` / `current_function_site` plumbing exists; that blocker
does not use it). That is a small, independently useful change.

It was not attempted this round because the four items above, the gate sweep and
the Hono blocker below consumed the round. The stricter use-site condition the
brief asks for (every use is a spread into a call **or a constant-index read**)
is still the right rule; it just cannot be validated without the diagnostic
above, and landing it blind is what failed twice.

Recommendation: make the tuple-index blocker name its function as a standalone
commit, run one remeda build with the expansion on, and only then decide between
the note's two candidate explanations (a rest inside the covered parameters, or
a spread reaching a variadic callee).

---

## The Hono whole-crate `cargo check` could NOT be produced this round

`smelt build` on the pinned ref (`eebdf7be39abf0a872671835ccce0c4f03ea497a`)
with the committed `.github/compat/hono/` overlay **does not emit a crate**, so
there is nothing to run `cargo check` on and no table to report against the 326
baseline.

At the merge base `6bf4d2c3`, `smelt check --message-format json` reports four
blockers:

| file | message |
| --- | --- |
| `src/client/types.ts` | `rest parameter type must resolve to an array type` |
| `src/client/utils.ts` | `string replace requires string-compatible receiver, pattern, and replacement` |
| `src/client/client.ts` | `call expression is not lowered yet` |
| `src/client/client.ts` | ``unresolved identifier `proxyCallback` `` |

This was verified by rebuilding this worktree's frontend crates at the merge
base and re-running the same check, so **none of it is a consequence of this
round's work.**

The first one is fixed here (`03dc15a1`): `ConstructorParameters<C>` was not
modeled at all, so the name fell through to the ordinary type-reference path and
produced a type that is neither a list, a tuple, nor erased — and a rest
parameter annotated with it blocked the whole build. It is now the constructor
position of `Parameters<F>`: `typeof C` for a class already resolves to that
class's constructor function type, so both spellings read `params` off the same
`Type::Function`. Hono's spelling is
`webSocket?: (...args: ConstructorParameters<typeof WebSocket>) => WebSocket`.

The remaining three are in `src/client/**`, which the overlay **excludes**, and
they are the exact features the overlay's comment says are out of profile
(`new Proxy` dynamic member dispatch). So the open question is not "fix the
three blockers", it is **why files under an excluded glob are being lowered at
all**. What was established:

* The exclusion machinery itself is correct. A minimal project mirroring both
  Hono edges — a value import `../../client/utils` from
  `src/helper/ssg/ssg.ts`, and an `export type { .. } from './client/types'`
  re-export from `src/index.ts` — prunes the excluded module in both cases and
  reports the value use by name
  (``` `replaceUrlParam` is imported from `../../client/utils`, which the manifest excludes ```).
  `manifest::tests::excluded_dependency_is_pruned_and_recorded` passes.
* `src/helper/ssg/**` is not the route: adding it to the exclude list leaves all
  three `src/client/**` blockers in place.
* `smelt probe` reports **258 files scanned**, the same number round 28's note
  records — so the file set has not grown; those files were in the closure then
  too, and what changed is their lowering.

That last point is the one to start from: round 28 reached 326 `cargo check`
errors with the same 258 files, so something between round 28 and the current
integration head made three previously-lowering `src/client/**` files block.
Bisecting that is the first thing round 30 should do, because until it is done
neither agent can report the whole-crate table the round-29 brief asks for.

---

## Gate results (fresh binary, clean `dist-smelt` for every corpus)

| gate | result |
| --- | --- |
| `cargo test -p smelt-frontend-ts --no-default-features` | 1085 passed / 0 failed |
| `cargo test -p smelt-codegen-rust` | all suites green (1056 lib + every integration target) |
| `cargo test --bin smelt` | 57 passed / 0 failed |
| `cargo test -p smelt-transpiler --test hir_cli_cross_language_tests` | 17 passed / 0 failed |
| `guarded_throw_runtime` tier (`-- --ignored`) | 3 passed / 0 failed |
| `cargo clippy --all-targets` | 0 errors, no new findings in touched files |
| examples invariant | avoidable **0** (baseline 0, +0); prelude +0, boundary +0 |
| remeda generated `cargo test` | **1789 passed / 0 failed** |
| radash generated `cargo test` | **84 passed / 0 failed** |
| es-toolkit ratchet | avoidable **31716** vs baseline 31701 — **+15, blocking** |
| Hono whole-crate `cargo check` | **not runnable** — the crate does not emit (above) |

### The es-toolkit +15, and why it is not re-snapshotted here

The rise is not erasure that was added; it is erasure that became VISIBLE when a
silently-broken lowering started working. Every one of the fifteen lines is in
`src/compat/util/cond.ts`, whose `pairs.map(...)` callback is exactly the
H70 conditional-throw shape:

```ts
const processedPairs = pairs.map(pair => {
  const predicate = pair[0];
  const func = pair[1];
  if (!isFunction(func)) { throw new TypeError('Expected a function'); }
  return [iteratee(predicate), func] as const;
});
```

Before this round the compact IR hoisted the throw, and the emitted callback
was not merely mis-shaped — it read `pair[0]` as `SmeltUnknown::Undefined` and
tried to CALL undefined:

```
- _smelt_tmp_N = ({ let smelt_value = (SmeltUnknown::Undefined.clone()); let smelt_function = match smelt_value.clone() { ... } })
+ _smelt_tmp_N = ({ let smelt_value = pair.borrow().get({ .. 0 .. }).cloned().unwrap_or_else(|| SmeltUnknown::Undefined).clone(); ... })
```

and it returned a single value where the source returns a two-element pair. The
new `cond.rs` has the guard, the throw, the `iteratee(predicate)` call and the
pair, and its fifteen new erased lines are the direct spelling of es-toolkit's
own `pairs: any[][]` / `(...args: any[]) => unknown` signature:
`SmeltList<SmeltList<SmeltUnknown>>` for `any[][]`, `SmeltUnknown` for the two
`any` element reads, and the closure signature over them.

By the policy's own words that is a legitimate boundary (source `any`), but the
classifier cannot see the provenance from the generated text, and the marker
that would reclassify it (`SmeltUnknown` inside a list-index read) is
indistinguishable from genuinely avoidable erasure elsewhere — reclassifying it
would blunt the metric across the whole corpus. So the baseline is deliberately
**left un-snapshotted**: the ratchet stays red and a human decides between

1. accepting the +15 and re-snapshotting
   `blocker-logs/smelt-unknown-baseline-es-toolkit.json` (the reading this note
   argues for: the lines are `any` from the source, and the alternative is a
   `cond` that calls `undefined`), or
2. keeping the old `cond` lowering, which is not an option — it is wrong.

The remaining diff entries are neutral: the `SmeltCallableObject<hash>` names in
`memoize.rs` / `memoize_spec.rs` changed hash suffix (±93, ±89, ±78, ±15, ±3)
because the callable-object shape's identity is derived from its field types,
and those balance to zero.
