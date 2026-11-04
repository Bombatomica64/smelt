# Bounded generics as trait-bounded Rust generics (plan)

Status: agreed direction (user decision, 2026-11-04): **trait bounds**, not
per-call monomorphization and not a Date-only rule. Tracks the deferred
"bounded" slice of issue #99. Source of demand: date-fns family 1
(`blocker-logs/probe-date-fns.md`), but the rule is general.

## Today

Any type parameter with a constraint erases, whatever the bound:

```ts
function loud<A extends Animal>(a: A): A { console.log(a.speak(), a.legs); return a; }
function longest<L extends HasLen>(a: L, b: L): L { return a.len >= b.len ? a : b; }
function addDays<D extends Date>(d: D, n: number): D { d.setDate(d.getDate() + n); return d; }
```

all emit `fn f(x: SmeltUnknown, ..) -> SmeltUnknown`. Three layers cause it:

1. **Frontend.** Member access on a `TypeParam` receiver is typed through
   `class_field_type`'s `Type::TypeParam => erase(ByName)` arm. So `a.speak()`
   becomes a field read of `speak: Unknown` plus a dynamic `closure_call`, and
   `a.legs` is erased too. The bound is recorded (`TypeParamDef.constraint`)
   but never consulted for members.
2. **MIR.** `resolve_method` only accepts `Type::Class` receivers, so even a
   typed method call on a type parameter has nowhere to go.
3. **Codegen.** `liftable_type_params` (`crates/smelt-codegen-rust/src/classes.rs`)
   drops every constrained parameter ("a constrained parameter has no emitted
   bound"), and no trait is ever generated for a user class or interface. The
   test `erases_generic_free_function_with_bounded_type_param` pins this
   behaviour.

## Target shape (what a hand-written port would do)

```rust
trait AnimalLike { fn speak(&self) -> String; fn legs(&self) -> f64; }
impl AnimalLike for Animal { .. }
impl AnimalLike for Dog { .. }          // every subclass
fn loud<A: AnimalLike + Clone + 'static>(a: A) -> A { .. a.speak() .. a.legs() .. }
```

## Increments (each keeps CI and the baselines green)

Increments 1-3 must land together as **one vertical slice** (one PR, several
commits). Typing a member through the bound in the frontend while MIR still
only resolves methods on `Type::Class` receivers would turn programs that
build today (erased) into MIR errors. Increments 4 and 5 are separate PRs.

1. **Frontend: type members of a constrained type parameter through its
   bound.** A field read, method call or getter on a receiver of type `T`,
   where `T extends B`, resolves its type from `B`. `this`-typed and
   `T`-typed results are substituted, and the receiver stays `T`. This is
   pure typing, so the HIR is more typed even before codegen lifts
   anything. Erasure accounting must not rise: the reads that were
   `ByName` erasures become typed.
2. **MIR: bound-dispatch rvalues.** These are a method call and a field
   read on a `TypeParam` receiver, resolved against the bound's declaration
   (base chain included) but dispatched dynamically by the receiver's
   concrete type. They are lowered as new `Rvalue` variants (or a flag on
   the existing ones) rather than routed through `resolve_method`'s
   class-only path.
3. **Codegen: bound traits.** For each class or interface used as a bound
   of a lifted parameter, emit `trait <Bound>Like` from the members the
   program actually reads through the bound (methods, plus fields as
   getters), with impls for the bound and every subclass or implementor.
   Then relax `liftable_type_params` to lift a constrained parameter whose
   bound has a trait, and emit `T: <Bound>Like + <the existing common
   bounds>`. Update `erases_generic_free_function_with_bounded_type_param`
   in the same PR, recording the old and new requirement. The erased
   fallback stays for call sites that pass an erased argument
   (`called_with_erased_type_param_argument`).
4. **Date as a bound.** Give `Date` a concrete newtype (`SmeltDate`)
   instead of bare `f64`, so a `DateLike` trait is not implemented on
   `f64`, and let `D extends Date` lift to `D: DateLike`. This also
   removes the 342 `unresolved-class Date` decisions in date-fns and the
   E0382 setter moves (family 4).
5. **Generic abstract bases** (`Parser<Value>`, `Setter`'s
   `set<DateType extends Date>`). Use the same traits for the
   virtual-slot adapters, so a concrete override (`value: number`) is
   called with a concrete argument. That clears the post-#301 date-fns
   tail (695 × E0308, 63 × E0631, ~60 × E0277).

## Checks for every increment

- Behaviour: run each new source shape under Node and as generated Rust,
  and compare stdout.
- Types: the emitted signature carries `T: <Bound>Like`, not `SmeltUnknown`.
  The typedness report's `generic` count rises, and `erased` does not rise
  on the examples, es-toolkit or date-fns.
- Defect injection: dropping the bound, or erasing the parameter, must fail
  the new tests.
- The `SmeltUnknown` and typedness baselines stay green. Re-snapshot only
  on improvement, in the same commit.
