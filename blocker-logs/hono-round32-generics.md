# Round 32, Agent G — generic-class type parameters, overload instantiation, `router`

Three briefed items. Items 1 and 2 landed. Item 3 is a diagnosis with a verified
minimal reproduction, a one-line candidate fix, and the second defect that fix
uncovers — which is why it is not in the tree.

Every number here was measured from a CLEAN Hono checkout at
`eebdf7be39abf0a872671835ccce0c4f03ea497a` plus `.github/compat/hono/.`, built
with a freshly built full-feature `smelt` and an absolute `--manifest-path`, then
`cargo check --message-format=short` on `dist-smelt`.

## The table

The brief's "45" is 35 + 5 + 2 + 1 + 1 = **44**; the per-code split reproduced
exactly.

| code | baseline | item 1 | item 2 | delta |
| --- | ---: | ---: | ---: | ---: |
| E0308 mismatched types | 35 | 14 | 14 | **−21** |
| E0631 closure signature | 5 | 0 | 0 | **−5** |
| E0425 unresolved name | 2 | 2 | 2 | 0 |
| E0609 no field | 1 | 1 | 1 | 0 |
| E0271 associated type | 1 | 1 | 0 | **−1** |
| E0507 cannot move out | 0 | 2 | 2 | **+2** (unmasked) |
| **total** | **44** | **20** | **19** | **−25** |

The two `E0507`s are at `main.rs:11060` and `:11114` — the same two sites that
previously reported `Context_1<..>` mismatches, i.e. code that did not type-check
at all before and now gets as far as the borrow checker. They are a new family
(`cannot move out of `closure_arg_1`, a captured variable in an `Fn` closure`),
not a regression of anything that worked.

## Item 1 — a generic class declares only the type parameters its Rust spells (landed, −24)

`crates/smelt-codegen-rust/src/generic_elision.rs`.

**The rule.** For every generated nominal type (a `MirClass` or a
`MirInterface` — both are emitted as a Rust `struct` with a `PhantomData` filler
over their declared parameters), compute the LEAST fixpoint of

> parameter position `i` of `N` is CARRIED when the parameter's symbol occurs
> somewhere `N`'s own emitted Rust spells it — a field type, a static field, a
> descriptor, a base/heritage type argument, a method or constructor signature, a
> local of one of those bodies, or a closure nested in one —
> where an occurrence nested inside another generated nominal type
> `M<.., a_j, ..>` counts only when position `j` of `M` is itself carried, an
> occurrence inside a name the crate does not DECLARE counts for nothing, and the
> `PhantomData` filler counts for nothing.

Starting from "nothing is carried" is what makes a self-referential position
drop. `Context_1<E, P, I>`'s only mention of `E` is
`_not_found_handler: Option<Rc<dyn Fn(Context_1<E, .., ..>) -> SmeltUnknown>>`,
which reaches `E` only through position 0 of `Context_1` itself, so nothing
forces it. A greatest fixpoint — or a plain "does the symbol occur" scan —
answers the opposite.

**The undeclared-name clause is what made it work.** The first implementation
dropped only `CurrentPath`. The reason was `Optional(ContextOptions<E>)` and
`HonoBase<.., S, BasePath, ..>`: neither `ContextOptions` nor `HonoBase` is a
class or interface the crate declares (they are ambient / renamed-away names), so
`FunctionEmitter::rust_type` renders them as `SmeltUnknown` or a fixed prelude
type and spells NONE of their arguments. Counting an occurrence inside one was
counting a spelling that does not exist. With that clause the fixpoint drops
`Context_1`'s `E`, `P` and `I`, `HonoRequest`'s `P` and `I`, all five parameters
of `HandlerInterface`, all three of `OnHandlerInterface` and
`MiddlewareHandlerInterface`, and all four of `Hono_1`; `Router<T>`, `Node<T>`,
`SmartRouter<T>`, `TrieRouter<T>`, `RegExpRouter<T>` and friends keep theirs,
because they hold a `T`.

**Why no erasure.** An elided parameter is, by construction, spelled nowhere:
every occurrence the analysis found was inside a position that is itself dropped.
Dropping it changes `<..>` lists and nothing else, so no value is converted and
no `SmeltUnknown` is introduced — and a mistake is loud (`cannot find type`)
rather than silent.

**A deliberate conservatism, documented at the module.** A parameter a method
mentions only in its RETURN type counts as CARRIED, even though the data never
holds it. The alternative — lifting it to a method-level generic — makes the
generated class trait non-object-safe and leaves a return-position-only parameter
uninferable at call sites. The analysis answers "must the emitted Rust spell it",
which is a superset of "does the data hold it". `a_parameter_a_method_signature_
spells_stays_declared` pins it.

**Both halves read one answer.** `classes::{class,interface}_{type_params,
type_args,impl_generics}_text` on the declaration side, `rust_type`'s
`Type::Class` arm on the reference side, plus the `PhantomData` filler and every
struct literal that fills it (`emitter::core`, `emitter::call_runtime`). A class
whose every parameter is elided is not generic in Rust at all, so `is_generic`
follows the CARRIED list and the ordinary derives come back.

`SMELT_ELISION_DEBUG=1` prints each position as it is forced, with a shallow
rendering of the type that forced it. That is how the undeclared-name clause was
found, and it is the cheapest way to answer "why does this class still declare
that parameter" on a corpus-sized crate.

**Tests.** Fixture `100_elided_type_parameters` (four shapes, against Node 22)
and five unit tests in `codegen-rust/src/tests/generics_tests.rs`: a phantom-only
parameter, a stored parameter, the self-referential-only parameter, transitivity
in both directions at once, and the method-signature conservatism.

### FOUND AND NOT FIXED — a record rebuilt at a substituted argument erases its field

Not from this round; reproduced with NO elidable parameter present, so it is
independent of item 1. It is the construction-side sibling of round 31 item 3's
"constructing an interface record at a substituted ABI".

```ts
class Cell<T> { value: T; constructor(value: T) { this.value = value; } }
class Pair<A> { left: Cell<A>; constructor(left: Cell<A>) { this.left = left; } }
new Pair<number>(new Cell<number>(1));
```

emits

```rust
Pair::new({ let smelt_struct_value = _smelt_tmp_1.clone();
            Cell { value: SmeltUnknown::Number(smelt_struct_value.value.clone() as f64), .. } })
```

— `expected f64, found SmeltUnknown`. The record adapter rebuilds `Cell<A>` for
the substituted argument and renders the field under
`TypeSubstitution::erased()`, so the field's own `T` erases. Fixture 100 takes
the raw pieces in its constructor to stay off this seam.

## Item 2 — the arrow-const re-land, and what it actually unmasked (landed, −1)

The brief's diagnosis — "two instantiations of the same overloaded generic
function, at the same argument type but different type arguments, share a
result" — **does not hold**. Round 31 inferred it from the symptom; dumping MIR
for the bisected radash pair shows a different cause, and no instantiation is
keyed by argument types anywhere on that path. `selected_overload_signature`
recomputes the overload and its substitutions per call site.

### 2a. A binding declared inside a function body is not in scope outside it

`ClosureBodyKind::Statements` lowering restored only the names IT bound: the
parameters and the captures (`saved_locals`). Every `const`/`let` the body's own
statements declared — and every slot `predeclare_forward_referenced_locals`
reserved for one — stayed bound in the enclosing scope after the closure was
finished, so a later SIBLING closure saw them. A `LocalId` is an index into the
body that owns it, and the reuse check (`local_arrow_existing_body_local`) can
only verify that the index EXISTS in the current body, so a stale binding
silently named a different slot of the new body.

Measured on radash `src/tests/curry.test.ts`, trimmed to the two `test(..)`
closures the round-31 note bisected to. The second closure lowered to

```
%0 user repeat_x: fn(Float) -> String      <- also holds `make`
%1 user two_x:    fn(Float) -> Float       <- also holds `add_five`
...
%7 = closure ClosureId(103) []   // repeatX
%0 = copy %7                     // overwrites `add`
%8 = closure_call copy %0(2.0)   // `add(2)` calls repeatX
```

Four source bindings, two slots. The first closure — lowered when nothing was
bound yet — was correct, which is exactly why each test alone emitted fine and
only the pair failed.

The fix snapshots the bindings AFTER the parameters and captures are bound (so
the body still sees everything it legitimately captures) and restores them once
the body is lowered, which drops exactly the bindings the body introduced.

It also removes a spurious capture: an IIFE was recorded as capturing the local
its OWN body declares (`captures 1` -> `captures 0` three times in example 39,
whose `expected.stdout` is unchanged).

### 2b. A closure that returns a callable returns it at the declared callable type

A callable is an `Rc<dyn Fn(..)>` trait object; the value a body hands back is an
`Rc` of the CONCRETE closure type, because a `Function`-typed local is
deliberately left unannotated (`emitter::control_flow`'s `annotation` choice) so
it keeps that type. Rust unsizes one into the other only at a coercion site, and
a closure with no return annotation has none — its return type is whatever the
tail expression is. So the concrete closure type escaped into the closure's own
type:

```
expected `{closure@curry_test.rs:33:43}` to return `Rc<dyn Fn() -> f64>`,
but it returns `Rc<{closure@curry_test.rs:34:43}>`
```

(radash's `useZero`, and the same shape twice more.) Spelling the declared return
type makes the return the coercion site. Only reachable once 2a plus the
arrow-const inference types such a binding concretely, which is why it appears
with this change and not before.

### The re-land

`git show 9dd4afea` applied verbatim (`inferred_return_function_ty` plus its five
call-site lines). With it alone radash does not EMIT (2a) and, once it does, does
not COMPILE (2b). With 2a and 2b: radash **84 / 84**, remeda **1789 / 0**, Hono's
E0271 gone. Its fixture is re-landed as `101_arrow_const_infers_its_return` (97
is taken by round 31's `97_generic_class_default_bounds`).

## Item 3 — `router` on `Hono` (E0609): diagnosed, NOT fixed

`struct Hono {}` is emitted with NO fields at all, and `impl Hono` has only
`new` — so `this.router = ..` in its constructor is `E0609`, and every inherited
member is missing too. The class is

```ts
// hono.ts
import { Hono as HonoBase } from './hono-base'
export class Hono<E, S, BasePath> extends HonoBase<E, S, BasePath> { .. }
```

**The rule it breaks: an `extends` clause names a TYPE, so it resolves like any
other type reference** — through this module's import aliases, and through the
crate's rename map for a name two modules both declare.
`class_extends_clause` instead interns the LOCAL spelling
(`let base = self.intern_type_name(name)`), so the recorded base is a class named
`HonoBase`, which no module declares. `effective_class_fields` and
`effective_class_methods` find the base by symbol, so they find nothing.

**Reproduced minimally**, two modules:

```ts
// base.ts
export class Store { label: string; constructor(l: string) { this.label = l } describe() { .. } }
// index.ts
import { Store as StoreBase } from './base'
export class Store extends StoreBase { extra: number; .. }
```

emits `struct Store { extra: f64 }` — no `label`, no `describe`. The same file
with the subclass renamed to `Shop` (alias, no collision) is broken too.

**The candidate fix is one line** —
`let base = self.resolve_type_reference_symbol(name);`, the same resolution the
field annotations beside it already use, identity for a local unambiguous name.
It fixes the alias-only case outright (`struct Shop { label, extra }`). It is NOT
in the tree because the alias-PLUS-collision case (Hono's exact shape) then
**overflows the stack in the frontend**. Traced: the base resolves correctly to
`Store_1`, and then

```rust
// lowering/ty/annotations.rs, in_progress_class_base_method
let class_name = self.ctx.krate.names.get(class)      // Store_1 -> "Store"
    .or_else(|| self.ctx.krate.symbols.get(class))?;
let (base, base_args) = self.classes.base(&class_name).cloned()?;   // -> Store_1
self.resolve_method(base_ty, method, span)                          // -> forever
```

The in-progress-class registry is keyed by the SOURCE name, and a renamed class
keeps its source spelling as its original name (`instanceof` and `__smelt_class`
read that). So asking it about another module's `Store_1` looks up `Store` and
hands back THIS module's in-progress `class Store extends Store_1`. A `base ==
class` guard was tried and is not sufficient — the recursion survives it, so
there is at least one more name-keyed base walk on the path (`resolve_method`'s
own base recursion and `super_call.rs:179` are the candidates).

**The shape of the real fix**, for whoever takes it: every base-chain walk must
key on the SYMBOL, and the in-progress registry must answer only for symbols the
CURRENT module declares. That is the same H61 family the rename map was
introduced for, and it wants its own round.

## SmeltUnknown

No conversion was introduced. Item 1 removes type parameters, never values; item
2 replaces erased bindings with concrete ones (which is why es-toolkit's
avoidable count FELL twice).

| baseline | before | after item 1 | after item 2 |
| --- | ---: | ---: | ---: |
| examples (hard invariant) avoidable | 0 | 0 | **0** |
| es-toolkit (ratchet) avoidable | 31716 | 31667 | **31665** |

Both baselines are re-snapshot: `smelt-unknown-baseline.json` for the three new
fixtures' prelude/legitimate growth (avoidable +0 throughout),
`smelt-unknown-baseline-es-toolkit.json` for the **−51** fall. The remeda
baseline is advisory and untouched.

## Gates

| gate | result |
| --- | --- |
| examples invariant (`--fail-on-regression`) | avoidable **0**, delta +0 |
| es-toolkit ratchet (`--fail-on-regression`) | avoidable **31665**, −51 against the stored 31716; re-snapshot |
| remeda generated `cargo test` | **1789 passed / 0 failed** |
| radash generated `cargo test` | **84 passed / 0 failed** |
| `cargo test -p smelt-codegen-rust --no-default-features` | **1068 passed / 0 failed** |
| `cargo test -p smelt-frontend-ts --no-default-features` | **1092 passed / 0 failed** |
| end-to-end `end_to_end_examples_match_expected_outputs` | **ok**, 103 fixtures |
| `cargo clippy --all-targets --no-default-features` | no new findings in the files touched |

Corpora were rebuilt from clean checkouts with the final binary: hono
`eebdf7be39abf0a872671835ccce0c4f03ea497a`, es-toolkit
`e008a2818cd8d07469a5cc12ee0c02405d523e07`, remeda
`3c80f28bb394edbf89f1fc9978571dec8ed20edc`, radash
`4cab1900d08e0997abc4f17aec3cbfe18958d766`.

## Fixtures and tests added

| test | what it pins |
| --- | --- |
| fixture `100_elided_type_parameters` | a phantom-only parameter, a self-referential-only one, transitivity in both directions, and a stored one that survives — against Node 22 |
| fixture `101_arrow_const_infers_its_return` | round 31's, re-landed and renumbered |
| fixture `102_body_locals_are_body_scoped` | two sibling closures with the same binding names; `expected.mir` pins one slot per binding in each body, and `expected.rs` pins the callable-return annotation |
| `a_type_parameter_no_field_carries_is_not_declared_in_rust` | the elision, on the simplest shape |
| `a_type_parameter_a_field_carries_stays_declared` | the rule is a fixpoint, not a blanket drop |
| `a_parameter_reached_only_through_its_own_class_is_not_declared` | LEAST fixpoint, not greatest |
| `a_parameter_reaching_another_class_follows_that_class_answer` | transitivity, both answers at once |
| `a_parameter_a_method_signature_spells_stays_declared` | the documented conservatism |
