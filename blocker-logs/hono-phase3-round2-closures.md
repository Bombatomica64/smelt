# Hono phase 3 round 2 — closure-body joins and interface-typed receivers (Agent T)

## Item 1 — closure-body forks rejoin once (landed)

**Cause.** The closure emitter (`emit_closure_block_inner`) walked a closure's CFG as a tree: a
`Switch` emitted both arms with the enclosing `stop`, so the block where the arms met was emitted
inside each arm. Every `&&`/`||` operand is such a `Switch`, so a k-long chain emitted 2^k copies
of the rest of the closure. Throwing `Call`/`Await` terminators inside a closure had the same
per-arm emission (`throwing_join_for` returned `None` for `Continuation::Closure`).

**Rule.** Closure-body `Switch` (`emit_closure_switch`) and throwing terminators under
`Continuation::Closure` ask the shared `forked_region_join` finder, with the region's `stop` plus the
emission stack as exits (`closure_region_exits`). With a join, the arms are emitted with
`stop = join` and `in_loop = true` (a `Return` inside them is an explicit `return` statement, since
the join follows the `if`/`match` and the arms are no longer the tail expression), and the join is
emitted once after it with the original `stop`/`in_loop`. Without a join the old per-arm emission
stays, so a fully-diverging `if`/`else` can still be the closure's tail value.

**Follow-on fix.** A fallible, non-awaited async closure whose body now mixes `return Ok(..)`
statements with an `Ok(..)` tail pinned its inner `smelt_async_value` block as the OUTPUT type
(the old "explicit return seen" heuristic); it is now pinned as `Result<..>` whenever the body is
fallible (the returns leave the `async` block and say nothing about the inner one).

**Measured.** `middleware/trailing-slash/index.ts`: 3 046 855 B / 11 357 lines / 448 `redirect`
→ **59 819 B / 367 lines / 4 `redirect`** (4 in the source). Plain `cargo check` on `dist-smelt`
with the test admitted: 0 errors. Six end-to-end goldens shrank (join hoisted out of both arms).
Fixture `124_closure_short_circuit_join` (stdout = Node 22); unit test
`closure_short_circuit_chain_emits_linear_rust` (k=8 < 2× k=4, tail emitted once).

**Overlay.** `trailing-slash/index.test.ts` stays excluded: admitting it adds 48 E0308 to
`cargo check --tests` (24+10 `String` passed where the variadic `...handlers: SmeltList<Rc<dyn Fn>>`
is expected, 12 `Option<String>` where a string-literal union is expected). Comment updated.

## Item 2 — interface-typed receivers (landed for fields + methods)

**Cause.** An interface is emitted as a record whose methods are callable slots. A class instance
flowing into an interface-typed position (parameter, list element, a factory callback's result)
went through `structural_record_adapter_fields`, which declined because the class has no FIELD
named like the method. The class value then flowed unconverted (E0308 in plain programs), and in
Hono's `router/common.case.test.ts` harness the erasure of `() => new RegExpRouter()` (typed
`<T>() => Router<T>`) read `name` / `match` / `add` as fields of `RegExpRouter` (E0609 / E0615).

**Rule.** `class_method_implements_interface_field`: an interface slot implemented by a method of
the source class (own or inherited) is filled by the existing virtual-method storage builder — a
closure dispatching to the class's method on the SAME instance (a reference-class handle shares its
cell) — with the method's parameter/return types read at the receiver's instantiation of the class
type parameters (`class_instance_type_substitutions`). Interface properties read the class field,
through `.0.borrow()` for a reference class. Data properties are copied when the view is built (the
record ABI stores them by value): exact for `readonly`, a later write through the class is not
observed by the view. Methods are always live.

Fixture `125_interface_receiver_dispatch` (two classes, a readonly property + methods, used through
an interface-typed parameter, list and factory callback; a method's side effect is visible through
the class afterwards). Unit test `class_viewed_through_interface_dispatches_its_methods`.

**Not done.** A GETTER implementing an interface property is rejected by the frontend
(`class is missing implemented interface field`, `lowering/ty/interface_lookup.rs`, Agent S's
area); left for a follow-up. The fixture is non-generic because `new Square(3)` against a
contextual `Shape<string>` infers `T = unknown` and a generic `<T>() => Shape<T>` callback field
erases (both pre-existing, both would add avoidable erasure to the examples invariant).

**E0560 (22) does not share the cause.** `vercel/handler.test.ts` passes `app` (`Hono_1`, the
`hono.ts` subclass) where `handle` declares `Hono` (the `hono-base.ts` class of the same name):
a class-to-class structural adaptation into a REFERENCE class via a struct literal, which a handle
newtype cannot take. Separate family.

## Hono `cargo check --tests` (committed overlay)

402 → **308**: E0308 285 → 285, E0609 69 → **1** (`json` on `SmeltResponse`), E0615 26 → **0**,
E0560 22 → 22. `reg-exp-router/router` 79 → 13 (remaining: `SmeltUnion` vs `SmeltUnknown` E0308),
`pattern-router/router` 6 → 0.
