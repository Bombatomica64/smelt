# TS7 parity, phase 2, round 5: virtual slots, self-returning records, typed spreads

Branch `feat/hono-url-model`. This round fixes the four "found and not fixed"
items of round 4 (`ts7-parity-phase2-round4-aliases-mount-inheritance.md`),
plus what they exposed. Every rule is covered by example 180, whose output
matches Node with zero avoidable `SmeltUnknown`, and by a unit test. Each unit
test was checked against its defect: it fails when its rule is disabled.

## 1. Virtual method slots on a constructed instance

Round 4 logged this as an abstract-base bug; it was general. A frontend-made
slot (a function field named like an overridable method) was only ever bound
when a subclass value was CONVERTED to a base type. A directly constructed
instance kept the slot's default closure:

| program | Node | Smelt before |
| --- | --- | --- |
| `new A().f()`, `f() { return this.g() * 10 }`, `g() { return 1 }` | `10` | `0` |
| `new Bc().f()`, where `Bc` overrides `g` | `20` | `0` |

Fixes:

- **Reference classes.** A class whose flattened layout stores a slot is a
  reference class (`classify::collect_virtual_slot_triggers`); the heritage
  closure then covers the chain.
- **Binding at construction.** The class's own constructor binds each slot right
  after allocation, through a `Weak` handle on the new cell, to `Self`'s method
  (`bind_own_virtual_slots_text`). `Self` is the most-derived class, because an
  inherited method is emitted into its `impl`, and `Weak` keeps the instance
  from owning itself.
- **Inherited slots.** A slot a class inherits counts as a slot
  (`is_virtual_method_storage_field` walks `extends`).
- **Conversion to a base.** Converting a subclass to a base binds an inherited
  method on the SOURCE value, not on a fresh default target. Before,
  `StrictYear` to `Parser<number>` called `Parser::parse`, which does not exist
  (E0599).
- **Source stored callables.** A reference-class source's stored callable is
  read through its cell, with the guard dropped before the call (Hono's
  `FetchEventLike`).
- **Return types of inherited methods.** Calling an inherited method on a
  subclass receiver types its return at the subclass's instantiation of the
  declaring class (`declaring_class_instantiation`). A method returning its
  receiver returns the receiver's type.

## 2. A generic record whose member returns the record (`Pair<T>.swap()`)

| cause | before | fix |
| --- | --- | --- |
| Method shorthand in an object literal took no contextual type | `swap() { .. }` returned `Unknown` | Takes the property's contextual type |
| Converting `Pair<SmeltUnknown>` to `Pair<f64>` re-requests the same pair through `swap`'s return; the cycle guard cut it to `Default::default()` | `0 0` | The structural adapter is hoisted into a helper and the cycle calls it (`hoisted_record_adapter_call`); the erased-record conversion does the same. Both reserve the helper before rendering its body |
| Self-recursive generic free functions erased their type parameter. The call-site gate read the recursive call's `T` as "a type parameter from another scope", and the body trial could never see the function as generic, since the crate-wide set grows from empty | `makePair<T>` erased | The gate accepts the function's own `T` in a self-call. The trial assumes the function under trial generic for its own calls, from its body or a closure in it (`trial_generic_function`) |

Recursive generics now keep `T` (`repeat<T>(x, n)`, `makePair<T>`).

## 3. A callback whose body is `v.join(..)`

The compact callback classifier typed `v.join(..)` by name (erased), which
erased the callback's return: `(v) => v.join("-")` was `fn(List<String>) ->
Unknown` even with an annotated parameter. `join` on a list-like, unknown or
type-parameter receiver now answers `String`, unless the receiver declares
its own `join`. Those are exactly the receivers the compact lowering accepts.

## 4. Object spreads of records, and `Partial<T>`

**`Partial<T>`.** The spread case exposed a miscompile: `Partial<X>` lowered as
`X`, so `merge({ credentials: true })` filled `origin` with `""`, and
`{ ...defaults, ...options }` then overwrote the default with it. `Partial` of
a plain data interface record is now its own interface (`PartialX`), with every
field optional and `may_be_absent`, cached crate-wide.

**Typed spreads** (`lowering::typed_spread`). An object spread at a plain record
type builds the struct directly, with no erased `dict_assign`:

- Each field takes its last writer in source order. An optional source field
  writes only when present (`src.f ?? previous`).
- Sources must be identifiers or plain field reads, so declining after reading
  a source's type has no effect.
- Sources and values are bound to locals in source order.

**Annotated module constants.** An annotated record constant
(`const defaults: Options = {..}`) keeps its record type, so every inlined copy
is the struct.

## Typedness measurement

The typedness report substituted a receiver's type arguments into a field
read only when the field's type was a bare `T`. A field like
`swap: () => Pair<T>` read off a `Pair<f64>` was measured at its declared
`T`, which renders as erased in the caller's scope, although the emitted field
is `Rc<dyn Fn() -> Pair<f64>>`. It now substitutes through the whole type,
like the emitter does. This only ever counts more reads as typed.

## Ratchets and corpora

| | before | after |
| --- | ---: | ---: |
| examples SmeltUnknown avoidable (invariant) | 0 | 0 |
| es-toolkit SmeltUnknown avoidable | 22,185 | 22,183 (re-snapshotted) |
| es-toolkit typedness | | no program less typed; re-snapshotted |
| Hono tests | 421 / 33 | 421 / 33, none lost |
| es-toolkit tests | 1052 / 7 | 1052 / 7, the same 7 |

## Found and not fixed

- `Pick`, `Omit` and `Required` still lower to their base type. That is wider
  than TypeScript but not wrong, unlike `Partial` was.
- The compact callback classifier types other array methods by name too
  (`v.slice(1)` records a `by-name` erasure). The final closure type is right
  because the outer `join` decides it.
- `Options & { extra }` (an intersection) has no struct, so a spread into it
  keeps the record path.
