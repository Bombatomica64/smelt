# `any` parameters as implicit generics

## Problem

`function forward(x: any, label: string) { return sink(x, label); }` erases
`x` to `SmeltUnknown`, and so does every caller: `forward(5, "a")` builds
`SmeltUnknown::Number(5.0)` before the call. A team porting this by hand would
not take the erased carrier: the body only hands `x` on, so they would write
`fn forward<A>(x: A, label: JsString)` and keep each caller's value typed up
to the place where it really meets a dynamic boundary (here, `sink`'s
`String(x)`).

`explicit-any` is the largest avoidable erasure reason in es-toolkit (1213
records at a6582eb). Most of it is not this shape (`(...args: any[]) => any`
in generic bounds, overload signatures, `as any`, `any[]`), and most `any`
parameters of es-toolkit's `compat` functions really inspect their value. This
rule covers the shape that is a generic in disguise and nothing else.

## Rule

A parameter of a module-level `function` (declaration or `export const f =
function`) is a **candidate** when it is

- required and undefaulted (`x: any`, not `x?: any` or `x: any = d`),
- bound to a plain name (no destructuring), not the rest parameter,
- annotated exactly `any`, and still lowered to the erased carrier (not an
  `as any` object's unchecked parameter or an overload-widened slot),
- in a function that is not a generator and does not read `arguments`.

The TypeScript frontend declares, for each candidate, a fresh type parameter
`AnyN` with `TypeParamDef::implicit_param = Some(index)`. Nothing else changes
in HIR: the parameter's local, the item's signature and every expression keep
the erased type the checker gave them, so MIR lowering, contextual typing at
call sites and every caller see the program they saw before.

After MIR lowering, `smelt_codegen_rust::settle_implicit_generics` decides each
candidate once, before the emitter or the typedness measurement reads MIR. A
candidate **lifts** (its local is retyped to `AnyN`) when both hold:

1. **The body passes the value along** (`passes_through`). Every use of the
   parameter's local is one of:
   - a whole copy into an erased local (the `IntoSmeltUnknown` crossing every
     lifted parameter carries);
   - a whole argument of a static call at a parameter that is erased or one of
     the callee's own unbounded bare type parameters;
   - a whole return from a function whose return is erased;
   - a scalar question answered on an erased copy (`x == null`, `typeof x`,
     `!x`; `generic_inspection`).

   Any other read (member or index access, an operator, a branch, a store into
   a container, a closure capture, an indirect call, `await`, `throw`) is an
   inspection; any write to the local (a reassigned parameter) refills the slot
   from elsewhere. Both reject. The list is an allow-list, so a new MIR form
   cannot make an inspected `any` generic by default.
2. **Every caller passes a value the parameter carries as the carrier did**
   (`callers_pass_carriable_values`). No static caller passes, at that
   position, a function, a future, a nullish value (`null`, `undefined`, an
   optional) or a container of one. Functions and futures have no
   `IntoSmeltUnknown`/`SmeltFromUnknown` conversion. A nullish value renders as
   `()` or `Option<_>`, which keep one absent value where the erased carrier
   keeps `null` apart from `undefined` (`isUndefined(undefined)` would read
   back `null`).

A candidate that fails either check is dropped from the function's type
parameters and recorded as `explicit-any` at its annotation, with a detail
naming the check (`AnyN of f: the body does more than pass the value along`,
or `.. a caller passes a function, a future or a nullish value`). The frontend
retracted its own `explicit-any` record at that annotation when it declared
the candidate, so each `any` keeps exactly one record while it stays erased.

A lifted candidate is an ordinary type parameter from then on: the crate-wide
lifting decision (`liftable_type_params`, the body trial) treats it like a
declared one, with one difference. A caller that passes an erased value at an
implicit parameter does not demote the function
(`called_with_erased_type_param_argument`): the caller passed an `any`, and
Rust instantiates the parameter at `SmeltUnknown`, which is what the source
said. If the function as a whole is demoted (for a declared parameter's
reasons), the implicit parameter renders erased and is recorded as
`explicit-any` at its annotation, not as `erased-generic`.

## Requirements

- **Behaviour.** A lifted function computes what it computed before. Its body
  holds the caller's value instead of the carrier, and every use of it is a
  use the carrier supported through the parameter's bounds: the copy, call
  argument or return into an erased slot converts with `into_smelt_unknown()`
  at that point, where the old caller converted before the call; the scalar
  questions are asked of an erased copy, as for any lifted `T`. Values whose
  conversion would differ by where it happens (functions, futures, nullish
  values) never instantiate the parameter (check 2).
- **Type preservation.** Callers pass their values at their own types
  (`forward(n, ..)` with `n: f64`), and the value stays typed through every
  pass-through callee until it reaches a real dynamic use. A candidate that is
  not lifted emits exactly what it emitted before this rule: its function's
  MIR is untouched except for the dropped declaration.
- **Measurement.** The settle step runs inside `lower_to_emittable_mir`, so
  the generated crate, the typedness sidecar and the erasure sidecar describe
  the same program.

## What stays a boundary

- An `any` the body inspects (`compat`'s `isMap(value)`, `has(object, path)`):
  the value really is dynamic there, and lifting it would need a typed
  dispatch for every operation.
- An `any` some caller fills with a function, a future or a nullish value. A
  call-site erased instantiation (`f::<SmeltUnknown>(adapter(arg))`) for just
  those callers would keep the rest typed; it is left out because the
  SmeltUnknown report classifies whole lines, and removing the adapter from a
  line that also holds an erased callback adapter re-files that adapter's
  tokens from boundary to avoidable (es-toolkit's `isEqualWith` spec: total
  tokens -161, avoidable +139). That needs a per-token classification first.
- Callback parameter and return positions (`(x: any) => any`), `any[]`,
  overload signatures, methods and closures: not candidates. A callback return
  that only flows through would need the callers' result types to follow the
  instantiation, which the checker-typed call results do not.

## Evidence (es-toolkit at the CI pin)

- SmeltUnknown avoidable 12447 -> 12396; typedness erased values 6889 -> 6869 (generic 370 -> 390),
  no program less typed; remeda, hono, radash and ts-pattern byte-identical.
- The generated suite compiles and the runtime ratchet holds (1048 passed, 11
  pinned failures).
- Planted defects: with check 1 reduced to "never inspected through a
  projection" (the partial-lift opacity walk), es-toolkit's `has` lifted while
  indexing `object?.[path]` and the crate failed to compile (E0308); without
  check 2, `isUndefined(undefined)` read back `null` and the runtime ratchet
  failed (`isUndefined.spec.ts:6`). `generic_lifting_cli_tests::
  any_parameter_passed_along_is_an_implicit_generic` pins both cases.
