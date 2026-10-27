# TS7 parity, phase 2, round 4: object aliases, Hono mount, generic inheritance

Branch `feat/hono-url-model`. This round closes the first two items round 3 left
open (`ts7-parity-phase2-round3-follow-ups.md`, "Still open"):

- object-shaped type aliases;
- Hono's `mount` failures.

It also fixes the defects those two exposed. Every rule has an end-to-end
example whose output is compared against Node. The third open item,
`T | null | undefined`, stays as decided: `Option<T>` (see "Not changed").

Hono v5 `0f5ad7b6`: **421 passed / 33 failed** of 454 (round 3: 413 / 41). Every test that passed in round 3 still passes.

## Object-shaped type aliases are structs

`type R = { a: string; b?: number }` now lowers like an interface: a real
generic struct.

- **Before:** an erased record (`Class { R }` with no struct behind it). In a
  crate with no other erasure, reading `r.b` aborted the build.
- **Which aliases:** a type-literal RHS whose members are all property or method
  signatures (`types_iface.rs`, `push_lowered_interface`). The same path builds
  interfaces.
- **Generic defaults:** they are collected for aliases as well as for classes
  and interfaces (`collect_class_type_parameter_defaults`). A
  `TypedResponse<O>` written with fewer arguments than parameters is padded with
  the declared defaults in two places: the renderer and field substitution.
  (Was E0107: struct takes 3 args.)

What the structs then needed:

- **1-tuple adapters.** `(A,)` gets `SmeltFromUnknown`/`IntoSmeltUnknown`.
- **Map keys of a type parameter.** A type parameter used as a dict key in a
  field gets the `SmeltJsKeyEq` bound. Such dicts stop deriving `PartialEq`.
- **Absent optional fields.** The erased view of an interface leaves an absent
  optional field out instead of writing `undefined`. That way
  `{ origin: '*', ...options }` keeps the default (Hono cors).
- **`never` receivers.** A read off a `never`-typed receiver takes the erased
  path. It used to fall through to a struct field of the same name (E0609).
- **Object rest of a struct.** `const { strict, ...rest } = options` copies
  `options` through its erased object view, the same as an unknown-like
  source. Absent optional fields are left out of that view.
  - **Before:** the copy rendered `Default::default()`, an empty record.
  - **Effect:** `Object.assign(this, rest)` set nothing.
  - **Caught by:** the runtime fixture `object_assign_onto_instance`.

Example 178.

## Hono mount: 13 failures to 5

Mount failures went from 13 to 5. Three fixes:

- **Symbol-keyed getter through `unknown`.**
  `(c.req as unknown)[GET_MATCH_RESULT]` reads a symbol-keyed getter through a
  type-only `unknown`. The read now sees through an assertion of a class value
  (`asserted_class_value`).
- **Throwing getters inside `try`.** A getter read (`c.executionCtx`) inside
  `try` is a MIR call with an unwind edge. It used to be a plain field place, so
  its throw skipped the `catch`.
- **`try` narrowing.** Narrowings made inside `try`/`catch` are scoped to them.
  Any name assigned in either block falls back to its declared type after the
  statement. The previous narrowing of `found` survived the `try` wrongly.

Example 178.

## Methods inherited from a generic base

This defect surfaced once `ParseResult<T>` became a struct. A base method
copied into a subclass (`class YearParser extends Parser<number>`) spelled the
base's `T`, which the subclass `impl` does not declare (E0425). HEAD already
failed the same way with the interface spelling. Fixes:

- **MIR interning** (`passes/inherited_types.rs`). The types every inherited
  method and constructor body needs, at each subclass's instantiation, are
  interned. The arguments are composed down the chain:
  `Leaf extends Middle<string>`, `Middle<T> extends Base<T[]>` gives `Base`'s `T`
  the type `string[]`.
- **Emitting the copy.** Codegen emits the copy from a type-substituted clone of
  the function (`classes::substituted_inherited_member`), so every emitter rule
  sees the concrete type. Its receiver is the subclass itself.
- **Closures.** The copy's closures are separate MIR items, so they render
  through the same map (`inherited_type_args`). The copy's own emitter applies
  the map only to names the subclass does not declare, because
  `Middle<T> extends Base<T[]>` reuses `T`.
- **Inherited field layout.** A subclass's flattened layout stores inherited
  fields at the substituted type (`effective_class_fields`), so a copy and the
  struct agree.
- **Absent literals.** An `undefined` literal that lowering spilled into a `T`
  temp (`v !== undefined`) stays a literal in the copy
  (`opt::inline_absent_constant_temps`). Otherwise it would read as `0.0` once
  `T = f64`.
- **Method member types.** A method's member type is instantiated at the
  receiver's type arguments (`class_field_type`). `map(fn: (value: T) => ..)`
  read off `Base<number>` takes a `(value: number) => ..` callback.
- **Inherited methods named like array builtins.** An inherited method whose
  name an array builtin also uses (`nums.map(..)` where `Base` declares `map`)
  is the receiver's method, not `Array.prototype.map`. HEAD printed `NaN`.

Example 179. The unit test `emits_virtual_method_slots_for_abstract_base_adapters`:

- **Old requirement:** the `parse` slot is `Rc<dyn Fn(String) -> SmeltUnknown>`.
  That was the erased alias.
- **New requirement:** `ParseResult<T>` on the base and `ParseResult<f64>` on
  the subclass. The inherited `run` returns `f64`.
- **Defect check:** disabling the copy substitution makes the test fail.

## Ratchets

| baseline | before | after |
| --- | ---: | ---: |
| examples SmeltUnknown avoidable (invariant) | 0 | 0 |
| es-toolkit SmeltUnknown avoidable | 22,338 | 22,185 |
| examples typedness | | no program less typed; re-snapshotted (178, 179 new) |
| es-toolkit typedness | | no program less typed; re-snapshotted |

es-toolkit tests: 1052 passed / 7 failed, the same 7 as HEAD (at, delay,
isBrowser, isPlainObject, mergeWith null, timeout, zipWith).

## Found and not fixed (pre-existing on HEAD)

- **Virtual slots of an abstract base are not bound on a subclass instance.**
  `abstract class B { push(v) { return this.check(v) ? 1 : -1 } check(v) {..} }`
  with `class L extends B {}`: `new L().push(1)` calls the slot's default
  closure and prints `-1` (Node `1`). A call through a converted base value
  (`const p: B = new L()`) binds the slots and works.
- **A hinted callback can be lowered speculatively at an unsubstituted `T`.**
  `new Base<string[]>().map((v) => v.length)` records a `by-name` erasure for
  `length`. The final closure is fully typed, so the generated code has no
  avoidable `SmeltUnknown`; the erasure sidecar still counts the decision.
- **A hinted callback whose body is `v.join(..)` returns `Unknown`** when the
  call is itself a `.join` receiver (`b.map((v) => v.join("+")).join(",")`).
  This also happens on a non-generic class.
- **A generic interface method returning its own type** (`Pair<T>.swap()`)
  prints `0 0`. The interface spelling fails the same way.
- **The 5 remaining mount failures.**
  - Two come from a type lie: `undefined as unknown as Response`.
  - One is `basePath`.
  - One is the env/execution-context JSON.
  - One (fall-through when `anotherApp` returns nothing) is not investigated.
- Typed object spread into a struct (`{ ...options, extra }` where `options` is
  now a struct) erases to a record.

## Not changed

`T | null | undefined` keeps one absent case (`Option<T>`), as decided in round
3: a strict type over JavaScript's two nullish values.
