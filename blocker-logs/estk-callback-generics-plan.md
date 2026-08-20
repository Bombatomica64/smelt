# Callback generics: lifting `type_param_in_callback`

Design plan only. Nothing in this document has been implemented. Written against
`claude/estoolkit-throwing-callbacks` at `76dcfae` **plus that branch's
uncommitted working tree** (5 modified emitter files), because this feature
stacks directly on it.

Target: delete the `classes.rs:271` gate so a type parameter that appears inside
a callback parameter can still be emitted as a real Rust generic.

### Delivery shape

**Mode:** stacked pull requests. The bottom PR expands and freezes this design;
the next PR adds the shared, behavior-preserving type-binding analysis. Rust
signature and callback-adapter rendering deliberately wait for the planned Rust
AST emitter migration so those changes are implemented once against structured
Rust syntax rather than first against strings and then against the AST.

The initial stack is therefore:

| # | Boundary | Base | Owns | Release state |
| --- | --- | --- | --- | --- |
| 1 | Callback-generics design | `main` | Binding semantics, AST boundary, staging, and validation contract | Documentation only |
| 2 | Shared generic type bindings | PR 1 branch | Pure MIR analysis, argument alignment, binding-state tests, and behavior-preserving predicate adoption | Buildable; emitted Rust byte-identical |

After the Rust AST migration lands, new behavior-changing PRs implement
composite generic returns, directly pinned callback generics, the borrowed
`F: Fn(..) + ?Sized` representation, and finally callback-only inference. Owned
callbacks and per-type-parameter generic emission remain separate plans.

---

## 1. Why the restriction exists

### 1.1 What the code says

`function_emits_rust_generics` (`crates/smelt-codegen-rust/src/classes.rs:244`)
is an all-or-nothing, per-function decision with four conditions:

1. no type parameter may be **constrained** (`classes.rs:245-252`);
2. every type parameter must be **directly inferable** from a non-callback value
   parameter (`type_param_directly_inferable`, `classes.rs:396`);
3. no type parameter may appear **inside a callback parameter**
   (`type_param_in_callback`, `classes.rs:455`, invoked at `classes.rs:271`);
4. no call site may pass an **erased argument** into a bare type-parameter
   position (`called_with_erased_type_param_argument`, `classes.rs:298`).

The docstring at `classes.rs:238-241` gives the motivation for (3):

> codegen synthesizes default callbacks as `move |arg: SmeltUnknown| ...`, which
> cannot unify with a generic callback signature `Fn(T) -> _` (`E0631`)

### 1.2 Was that a real failure?

Yes, but the failure is **not** in Rust's inference — it is in *our renderer's
choice of scope*. Two concrete render sites prove it:

* `param_type_text` (`emitter/types.rs:671`) renders the callback's **parameter**
  types with `&HashSet::new()` (`types.rs:678`) — i.e. an *empty* type-parameter
  scope, which forces every `T` inside a callback parameter to `SmeltUnknown` —
  while rendering the callback's **return** with
  `&self.current_function_type_params()` (`types.rs:688`, as just introduced by
  the throwing-callbacks branch). The two halves of the same function type are
  rendered in different scopes. `T` inside a callback is not erased because Rust
  cannot handle it; it is erased because we pass an empty set.
* Every *call-site* adapter renders the callee's callback parameter types in the
  **caller's** scope, not the callee's:
  `function_shape_adapter_text` `arg_decls` (`emitter/core.rs:2946-2956`),
  `rendered_function_shape_adapter_text` (`core.rs:2439`),
  `borrowed_default_function_text` (`core.rs:2338`),
  `borrowed_function_handle_text` (`core.rs:2369`). A callee `T` has no meaning
  in the caller's scope, so it renders `SmeltUnknown`. Combined with a callee
  signature that *did* say `Fn(T)`, that is exactly E0631
  ("expected closure that implements `Fn(T)`, found one implementing
  `Fn(SmeltUnknown)`"). The historical E0631 is a scope bug, not an inference
  limit.

So condition (3) is **conservative, not fundamental** — as
`estk-throwing-callbacks.md:165` already states. But it is guarding a real hole:
the call-site renderers do not know what `T` was instantiated to.

### 1.3 Is there an analogue of the `E0283` hard failure?

`called_with_erased_type_param_argument` (`classes.rs:298`) documents a genuine
hard failure: at a call site where the argument's static type is already erased
(`Type::Unknown | Never | Union | TypeParam` — `operand_type_is_erased`,
`classes.rs:350`), nothing pins the callee's type parameter and monomorphization
fails with E0283. That check only scans **bare** type-parameter positions
(`classes.rs:304-316`).

The callback case has a **direct analogue, and it is a hard failure**:

* If `T` appears *only* inside a callback (`attempt<T>(func: () => T)`) and the
  callback argument at some call site is an erased callable — a
  `SmeltUnknown::Function` / `SmeltErasedFunction`, e.g. a `vi.fn()` spy — then
  the adapter we build for it is
  `move |..| T::smelt_from_unknown(cb.call(vec![..]))`, whose return type is
  `T` itself. `T` is unconstrained at that site ⇒ E0283. Real, not theoretical:
  es-toolkit's spec files pass `vi.fn()` into callback positions constantly.
* If `T` also appears in a direct value parameter (`takeWhile<T>(arr: T[], cb:
  (item: T) => boolean)`), `arr` pins `T` and the callback position needs no
  inference power at all. This case has **no** hard failure.

That split is the whole staging axis of this plan: the *pinned-elsewhere* case is
safe today; the *callback-only* case needs a per-call-site binding check that
generalises `called_with_erased_type_param_argument`.

### 1.4 Two adjacent restrictions that cap the win

Discovered while measuring; both are separate from (3) and both bound how much
the lift can achieve.

* **`free_function_returns_own_type_param` (`emitter/call.rs:1249`)** only
  accepts a **bare** `TypeParam` return. `takeWhile<T>(...): T[]` returns
  `List<T>`, so the composite-argument monomorphization path
  (`call.rs:920-933`) is not taken and `SmeltList<f64>` would be coerced against
  a bare `SmeltList<T>` target (E0308 / re-erasure). Increment 1 must widen this
  to composite returns or its own signature change is unusable.
* **Condition (1), constrained parameters, is per-function all-or-nothing.**
  `groupBy<T, K extends PropertyKey>`, `keyBy`, `countBy`, `orderBy`, `sortBy`
  and the `partition<T, U extends T>` overload stay fully erased no matter what
  we do here. Of es-toolkit's 800 generic exported functions, 215 have at least
  one constrained parameter. Making the decision **per type parameter** instead
  of per function is the single largest adjacent win and is explicitly *not* in
  this plan (see §5, Increment 5).

---

## 2. The representation decision

Current: `param_type_text` (`types.rs:671`) renders
`&dyn Fn({params}) -> {return}` for a borrowed callback parameter;
`parameter_decl_type_text` (`types.rs:695`) picks the owned rendering
(`Rc<dyn Fn(..)>`) instead when `function_parameter_requires_owned`
(`core.rs:2180`) says the parameter escapes. The owned/borrowed decision is a
crate-wide fixpoint (`emitter/mod.rs:250-345`, `callback_param_escapes_locally`
at `mod.rs:380+`, `callee_param_is_owned_callback_sink` at `mod.rs:352`) covering
seven distinct escape reasons: direct return, async body, local rebinding,
`setTimeout`/`setInterval`, erasure into unknown state, erased return, and (new
on this branch) binding to a `SmeltErasedFunction` local (`mod.rs:530-570`).

Any representation change has to survive all seven.

### Option A — `F: Fn(..) -> R` by value

```rust
pub(crate) fn take_while_145<T: .., F: Fn(T, f64, SmeltList<T>) -> bool>(
    arr: SmeltList<T>, should_continue_taking: F) -> SmeltList<T>
```

* **Inference**: works. `T` from `arr`, `F` from the argument's closure type.
  Also infers `T` *through* the bound when `T` appears only in the callback, and
  does so for `fn` items and `Rc<dyn Fn..>` arguments alike.
* **Forwarding**: `f(x)` needs only `&f` (`Fn::call(&self)`), so multiple calls
  in one body are fine. Forwarding to another helper by value moves it — but
  `impl<F: Fn> Fn for &F` exists, so `helper(&func)` forwards without consuming.
  Requires every forwarding site to learn to emit `&func`.
* **Storing / cloning**: an owned `Rc<dyn Fn>` sink needs `F: 'static`, which
  changes the bound and therefore changes *who* satisfies it — a caller
  forwarding its own borrowed `&dyn Fn` parameter into an `F: Fn + 'static`
  parameter fails borrowck. The `owned_callback_params` fixpoint would have to
  propagate ownership through generic callback sinks too (a new arm in
  `callee_param_is_owned_callback_sink`, `mod.rs:352`).
* **Erased call site**: fine — the adapter is a concrete closure, so `F` binds to
  it.
* **Cost**: touches all four owned/borrowed decision surfaces, plus a call-site
  text change (`&mut {closure}` ⇒ `{closure}`) at `core.rs:2841`, `core.rs:3213`,
  `call.rs:1445/1456/1481`. Multiple callback parameters need `F1, F2, ..`.
  `Optional<fn(..)>` parameters cannot become `Option<F>` (a `None` argument
  leaves `F` unconstrained ⇒ E0282), so they must stay
  `Option<Rc<dyn Fn..>>` — a permanent two-representation split.

### Option B — `&impl Fn(..) -> R`, or named `F` with a `&F` parameter

```rust
pub(crate) fn take_while_145<T: .., F: Fn(T, f64, SmeltList<T>) -> bool + ?Sized>(
    arr: SmeltList<T>, should_continue_taking: &F) -> SmeltList<T>
```

* **Inference**: identical power to Option A (APIT and a named parameter are the
  same desugaring). With `F: .. + ?Sized`, `F` unifies with **both** a concrete
  closure type *and* `dyn Fn(..)`, so `&*rc_handle` and `&mut {closure}` both
  still bind. This matters: it means the existing call-site argument text keeps
  working unchanged (`&mut X` reborrows to `&X`).
* **Forwarding / storing / cloning**: unchanged from today. `&F` is `Copy`, so
  forwarding the same callback through several helper calls is free — the exact
  property the `param_type_text` docstring (`types.rs:666-670`) exists to
  preserve. No `'static`, no new ownership propagation, all seven escape reasons
  keep their current answers.
* **Erased call site**: unchanged — adapter closure, `F` binds to it.
* **Cost**: one string in `param_type_text`, one bound in
  `function_impl_generics_text`. Prefer the **named** `F` over `impl Fn` because
  APIT makes the whole function non-turbofishable, and `callee_generic_argument_text`
  (`call.rs:1285`) is a place where an explicit type argument may later be wanted.
* **Downside vs A**: an owned-handle sink still pays the
  `Rc::new(move |..| cb(..))` wrapper. That is status quo, not a regression.

### Option C — keep `&dyn Fn(..) -> R`, let `T` appear inside it

```rust
pub(crate) fn take_while_145<T: ..>(
    arr: SmeltList<T>, should_continue_taking: &dyn Fn(T, f64, SmeltList<T>) -> bool) -> SmeltList<T>
```

* **Inference**: works *when `T` is pinned by another parameter*. The argument is
  an unsize-coercion site: `&{closure}` ⇒ `&dyn Fn(?T, f64, SmeltList<?T>)`.
  rustc infers a closure's signature from the expected type, and unifies `?T`
  with whatever the other arguments already bound, so `T = f64` resolves. When
  `T` is *only* reachable through the callback, coercion to a trait object is far
  weaker than a trait bound: rustc must know the target type before it can
  unsize, and a `dyn Fn` type is invariant in its argument positions. This is the
  case that would need to be verified empirically rather than assumed.
* **Forwarding / storing / cloning**: literally unchanged — the emitted Rust type
  string still starts with `&dyn Fn`, so every downstream predicate
  (`is_function_parameter_place`, `is_borrowed_callback_capture_name`,
  `callee_is_borrowed_function_handle` at `core.rs:2023`) keeps its current
  answer.
* **Erased call site**: unchanged.
* **Cost**: the smallest possible diff — replace `&HashSet::new()` with
  `&self.current_function_type_params()` at `types.rs:678`.
* **Downside**: `dyn` dispatch where a hand-writing team would monomorphize; and
  it cannot express the callback-only-`T` case.

### Recommendation

**Adopt Option B (named `F: Fn(..) + ?Sized` with a `&F` parameter) as the target
representation, and reach it by first landing Option C's one-line scope fix.**

Rationale:

* The erasure win is entirely in the *type arguments* (`SmeltUnknown` ⇒ `T`), not
  in the dyn-ness. Option C captures that win with a one-line signature change,
  which makes Increment 1 a clean, revertible experiment whose failure mode is a
  compile error in the generated corpus rather than a redesign.
* Option B is then a *pure representation swap* over the same plumbing — same
  borrow, same lifetime story, same call-site argument text (thanks to `?Sized`),
  same seven escape reasons — and it is what a hand-writing Rust team would
  produce (AGENTS.md "Project scope"): `T: Trait` in preference to `dyn Trait`,
  with monomorphized dispatch. Its extra inference power (`Fn` bound rather than
  unsize coercion) is exactly what Increment 3's callback-only-`T` case needs.
* Option A is rejected as the default: it buys nothing over B in erasure or
  inference, and it costs a rewrite of the owned/borrowed fixpoint plus a
  permanent second representation for optional callbacks. Keep it in reserve for
  the specific parameters that already *require* an owned handle (Increment 4).

Confidence: **high** for B-over-A; **medium-high** for the C-then-B staging (the
risk is that C's unsize-coercion inference is flakier than expected in some
corpus shape, in which case Increments 1 and 3 merge into one larger step).

---

## 3. Interaction with the throwing-callbacks change landing now

That branch's fix plan (`blocker-logs/estk-throwing-callbacks.md:123-147`)
introduces exactly the seams this feature needs. Build on these, do not
duplicate them:

| Their helper | Where | How this plan extends it |
| --- | --- | --- |
| `function_value_return_type_text(function, scoped_type_params)` | `emitter/types.rs:644` | **Already parameterised by scope** — pass the callee's own type params through it. No signature change needed. This is the single canonical renderer for a callback's return, so `may_throw ⇒ Result<T, Box<dyn Error>>` composes with `T` for free, giving the exact signature the brief predicts at `estk-throwing-callbacks.md:170`. |
| `param_type_text` | `emitter/types.rs:671` | Increment 1's one-line change lives here: `&HashSet::new()` (`types.rs:678`) ⇒ `&self.current_function_type_params()`. This *completes* their fix rather than fighting it: today the params and the return of the same callback type are rendered in different scopes. |
| `callee_uses_erased_call_method` / `callee_is_borrowed_function_handle` | `emitter/core.rs:2005`, `core.rs:2023` | Increment 2 turns this pair into a three-valued `CallbackHandleKind { ErasedCall, BorrowedDyn, MonomorphizedGeneric }`. `MonomorphizedGeneric` answers the same question ("direct call syntax") as `BorrowedDyn`, so Increment 1 needs **no** change here; only the Option-B swap does, and then only to keep the enum honest. |
| `borrowed_function_handle_text` | `emitter/core.rs:2369` | Their fix makes it build a `SmeltErasedFunction` for erased-rest targets. Increment 1 additionally renders its `arg{index}` declarations under the call-site binding substitution (§4.2) instead of `type_text_with_impl_trait`. |
| the panicking-adapter `?` propagation | inside `function_shape_adapter_text`, `core.rs:2901+` | Same function this plan edits (`arg_decls`, `core.rs:2946-2956`). **Sequencing requirement: land theirs first.** Two agents editing `function_shape_adapter_text` concurrently is the one real merge hazard here. |
| `bound_to_erased_handle` ownership reason | `emitter/mod.rs:530-570` | Untouched by Increments 1-3. Increment 4 would add a sibling arm for generic callback sinks. |

---

## 4. The one piece of new machinery

Everything below reduces to a single missing capability: **at a call site, know
what the callee's type parameters were instantiated to, and render the callee's
parameter types under that substitution.**

### 4.1 Shared `generic_bindings` analysis

The binding engine is semantic analysis, not rendering. Put it in a new focused
module, `crates/smelt-codegen-rust/src/generic_bindings.rs`, so the crate-wide
generic-safety decision in `classes.rs` and call emission in `emitter/call.rs`
cannot grow subtly different unifiers.

```rust
pub(crate) enum TypeParamBinding {
    Unbound,
    Concrete(TypeId),
    Erased,
    Conflict { first: TypeId, second: TypeId },
    Unsupported(BindingUnsupportedReason),
}

pub(crate) struct CalleeTypeParamBindings {
    pub(crate) by_param: IndexMap<Symbol, TypeParamBinding>,
}

pub(crate) fn collect_bindings(
    mir: &Mir,
    callee: &MirFunction,
    caller: &MirFunction,
    args: &[Operand],
) -> CalleeTypeParamBindings;
```

Structural matching is directional: the callee declaration is a pattern and
the caller argument supplies evidence. Matching collects
`TypeParam { name } ↦ concrete TypeId` without rendering Rust. The initial
supported grammar is deliberately conservative:

* bind through bare type parameters, `List`, `Set`, `Optional`, `Future`,
  `Dict`, `JsMap`, equal-length tuples, matching `Class { class, args }`,
  generators, and function parameter/return shapes;
* do not bind through `Union`, `Unknown`, `Never`, mismatched constructors,
  unresolved projections, or rest/arity transformations whose correspondence
  is not represented in MIR;
* do not zip union members by position: unions erase in emitted Rust and member
  order is not a sound correspondence;
* obtain the concrete `TypeId` of literal constants when they bind a bare type
  parameter. The existing boolean "constant is not erased" answer is
  insufficient because rendering needs to know *which* concrete type it bound.

Binding combination has explicit, testable semantics:

* `Unbound + X = X`;
* the same concrete type observed twice remains `Concrete`;
* distinct concrete observations become `Conflict`;
* an unsupported occurrence fails closed for that occurrence;
* erased callback evidence does not poison a type parameter already pinned by
  a direct concrete value argument (the Increment 1 `takeWhile` case);
* a parameter whose only evidence is erased or unsupported is not eligible for
  callback-only inference;
* every type parameter required by the selected generic signature must be
  concrete at every call site, otherwise that function is demoted.

Optional and missing arguments contribute no evidence. Argument alignment must
explicitly account for required arity, omitted/default arguments, rest
parameters, overload-selected MIR functions, and callback arity adaptation;
unsupported alignments fail closed rather than guessing.

This **subsumes** three existing partial mechanisms, which should be refactored
onto it rather than left beside it:

* `generic_param_instantiated_by` (`call.rs:1207`) — the same walk, answering
  only yes/no instead of returning the bindings;
* `free_function_returns_own_type_param` (`call.rs:1249`) and
  `method_returns_class_type_param` (`call.rs:1255`) — both exist only because
  bare-`TypeParam` is the only case a boolean can express; with real bindings the
  return conversion can be computed for `List<T>` too, which is §1.4's blocker;
* `mut_list_adapter_arg`'s `callee_generics` set (`call.rs:1396-1405`).

### 4.2 Substitution-aware Rust type lowering

Do not add a second string renderer before the Rust AST migration. The AST
emitter must instead expose one substitution-aware type lowering entry point:

```rust
fn rust_type(
    &self,
    ty: TypeId,
    substitution: &TypeSubstitution<'_>,
) -> Result<RustType, EmitError>;
```

`TypeSubstitution` combines the Rust type parameters already in lexical scope
with the concrete callee bindings collected at the call site. An unresolved
callee type parameter reaching this function is an invariant violation; it must
not silently render as `SmeltUnknown`.

The following consumers must all use this one entry point rather than
independently interpreting bindings:

* `function_shape_adapter_text` `arg_decls` — `emitter/core.rs:2946-2956`
* `rendered_function_shape_adapter_text` `params` — `emitter/core.rs:2439`
* `borrowed_default_function_text` `params` — `emitter/core.rs:2338-2360`
* `borrowed_function_handle_text` `params` — `emitter/core.rs:2369-2390`

All four are reached from the argument-rendering ladder in
`call_text`'s `Callee::Static` arm (`emitter/call.rs:874-985`), which already has
`emitted_params` and the callee `MirFunction` in hand, so threading the bindings
through is mechanical.

### 4.3 The gate rewrite

`function_emits_rust_generics` (`classes.rs:244`) becomes:

```rust
let signature_safe = function.type_params.iter().all(|type_param| {
    let name = type_param.name;
    param_types.iter().any(|&ty| type_param_directly_inferable(mir, ty, name))
        || type_param_inferable_through_callback(mir, &param_types, name)   // Increment 3
});
signature_safe
    && !called_with_erased_type_param_argument(mir, function)               // widened, Increment 1
```

* the `type_param_in_callback` early-return at `classes.rs:271` is **deleted**;
* `type_param_in_callback` (`classes.rs:455`) is **repurposed**, not removed — it
  becomes `type_param_inferable_through_callback`, restricted to positions Rust
  can actually infer from: a callback **parameter** position or a callback
  **return** position, but not e.g. a `T` buried in a union inside a callback
  parameter (unions erase, so it would not appear in the emitted `Fn` bound).
  `type_param_occurs` (`classes.rs:511`) stays as the "anywhere" walk;
* `called_with_erased_type_param_argument` (`classes.rs:298`) is widened from
  bare-`TypeParam` positions (`classes.rs:304-316`) to *all* positions that bind
  a type parameter, using `generic_bindings::collect_bindings`. A call site whose
  unification leaves any of the callee's type parameters unbound, or binds one to
  an erased type, demotes the whole function to erasure — preserving the existing
  E0283 guarantee and extending it to callback positions.

### 4.4 Option-B eligibility

The `F{n}: Fn(..) + ?Sized` representation applies initially only to a direct,
required, borrowed `Type::Function` parameter. These shapes retain their current
representation until a later increment gives them an independently tested
ownership/inference design:

* optional callbacks (`Option<Rc<dyn Fn(..)>>` avoids an unconstrained `F` for
  `None`);
* callbacks already classified as owned or escaping;
* function types nested inside another container;
* rest callback parameters.

Generated callback-generic names are deterministic by declaration index
(`F0`, `F1`, ...), skip names that collide with sanitized source type-parameter
identifiers, and appear after source generics in declaration order.

### 4.5 Required inference matrix

The shared analysis tests and later generated-Rust compile fixtures must cover:

| Callee shape | Call-site evidence | Expected result |
| --- | --- | --- |
| `f<T>(xs: List<T>, cb: Fn(T))` | concrete list, erased callback | bind from list; generic in Increment 1 |
| same | erased list, concrete callback | callback evidence only; defer to Increment 3 |
| same | arguments imply two different `T`s | `Conflict`; demote |
| `f<T>(cb: Fn() -> T)` | concrete callback return | generic in Increment 3 |
| same | erased callable | unbound/erased; demote |
| `f<T>(cb?: Fn(T))` | callback omitted | demote unless another argument pins `T` |
| `f<T>(cb: Fn(T \| string))` | any callback | union does not bind; demote unless pinned elsewhere |
| `f<T>(a: List<T>, b: List<T>)` | `List<f64>`, `List<String>` | `Conflict`; demote |
| `f<T>(cb: Fn(T) -> T)` | unknown callback input and return | demote unless pinned elsewhere |

Later behavior-changing PRs add generated compile fixtures for Option C with a
directly pinned `T`, Option B with both a concrete closure and `&dyn Fn`, generic
callback forwarding, default callbacks, multiple callbacks sharing one `T`, and
`may_throw` callbacks returning `Result<T, Box<dyn Error>>`.

---

## 5. Staging

Each increment is independently shippable and independently validated. Validation
bar for **every** increment: es-toolkit >= 909 passed / 150 failed with zero
newly-failing tests and `files_with_blockers` 0; remeda 1789/0; es-toolkit
avoidable-erasure <= 35403 (`blocker-logs/smelt-unknown-baseline-es-toolkit.json`);
examples avoidable == 0 (`blocker-logs/smelt-unknown-baseline.json`);
`cargo clippy --all-targets` and `cargo test` clean.

The Rust AST migration is a hard boundary between analysis and rendering:
Increment 0 lands before it; the behavior-changing increments below start only
after it. This avoids implementing the same signature and adapter changes once
as string concatenation and again as structured Rust syntax.

### Increment 0 — bindings machinery, zero behaviour change

Land the pure `generic_bindings` module from §4.1 and refactor
`generic_param_instantiated_by` (`call.rs:1207`) and the MIR-only safety scan in
`classes.rs` onto it where equivalence is exact. Do **not** add substitution-aware
rendering, widen composite returns, or delete the callback gate. Existing
boolean predicates whose behavior cannot yet be expressed exactly remain in
place until the post-AST increment that replaces their consumer.

*Extra validation bar, and the reason this is its own increment:* regenerate both
compat crates and assert the emitted Rust is **byte-identical** to before
(`git diff --stat` over `target/compat-repos/*/dist-smelt/src` = 0 files
changed). A refactor that changes one byte of output has changed semantics.

New unit tests implement the §4.5 matrix, including distinct `Erased`,
`Conflict`, and `Unsupported` outcomes; concrete evidence pinned outside an
erased callback; literal binding; optional/missing arguments; and conservative
union/rest handling.

### Rust AST migration — external prerequisite

The migration must provide the substitution-aware `rust_type` interface from
§4.2, or an equivalent single canonical entry point. Verify that function
signatures, callback closure declarations, default callbacks, owned handles,
adapters, and return conversions all lower types through it before proceeding.

### Increment 0b — composite generic returns, zero callback change

Replace the bare-only `free_function_returns_own_type_param` path with recursive
return substitution using the shared bindings. Keep the callback gate intact.
This isolates the `List<T>` return prerequisite from the callback behavior
change and gives it focused generated compile tests.

### Increment 1 — `T` in a callback **and** in a direct value parameter

Keep the requirement that every type parameter be directly inferable from a
non-callback parameter (`type_param_directly_inferable`, `classes.rs:396`);
delete only the `type_param_in_callback` early-return (`classes.rs:271`).
Representation: Option C (`&dyn Fn(T, ..)`).

Changes:

* `classes.rs:271` — delete the early return.
* `emitter/types.rs:678` — `&HashSet::new()` ⇒ `&self.current_function_type_params()`.
* the four adapter renderers listed in §4.2 — render under bindings.
* widen `called_with_erased_type_param_argument` (`classes.rs:298`) to callback
  positions, per §4.2.
* widen `free_function_returns_own_type_param` (`call.rs:1249`) to composite
  returns via the bindings (§1.4), otherwise `-> SmeltList<T>` callees regress to
  E0308.

`body_needs_erased_carrier` (`emitter/core.rs:4384`) and the
`populate_generic_functions` trial (`emitter/mod.rs:228`) need **no** change:
they already gate on the trial-rendered body, and `place.rs:340-366` already
renders an in-scope type parameter's missing element as `Default::default()`
rather than `SmeltUnknown::Undefined`, and
`generic_function_array_callback_preserves_type_param`
(`crates/smelt-codegen-rust/src/tests/generics_tests.rs:304`) already proves an
inline closure inside a generic function keeps `closure_arg_0: T`.

Corpus functions this lifts (unconstrained `<T>`, callback-typed parameter, body
that keeps `T` opaque): `takeWhile`, `takeRightWhile`, `dropWhile`,
`dropRightWhile`, `uniqWith`, `unionWith`, `xorWith`, `intersectionWith`,
`differenceWith`, `remove`, `partition`, `sumBy`, `meanBy`, `medianBy`, `minBy`,
`maxBy`, `isSubsetWith`, `pullAllWith`, `unzipWith`, `pickBy`, `omitBy`.

Tests to add in `generics_tests.rs`: the positive
`takeWhile` shape; a negative proving a callback-only `T` still erases (that is
Increment 3's job); a negative proving a call site passing an erased callable
still demotes the function.

### Increment 2 — representation swap to `F: Fn(..) + ?Sized`

Apply the eligibility rules from §4.4. Direct required borrowed callback
parameters become `&F{n}` and their bounds use the canonical AST type lowering
so `may_throw` composes with generic returns. Generalise
`callee_is_borrowed_function_handle` into the three-valued
`CallbackHandleKind { ErasedCall, BorrowedDyn, MonomorphizedGeneric }`.

Expected erasure delta: **zero**. That is the validation bar: the avoidable
count and compatibility test count must be unchanged, proving this is a
representation change rather than a semantic one.

### Increment 3 — `T` reachable only through a callback

Add `type_param_inferable_through_callback` so `unionBy<T, U>(arr1: T[],
arr2: T[], mapper: (item: T) => U)` and `attempt<T>(func: () => T)` qualify, with
the widened erased-argument check as the safety valve. Requires the adapter
closure to carry an explicit **return** annotation at the *source* callback's
return type so `U` is pinned from the argument side
(`function_shape_adapter_text`, `core.rs:3204-3216`).

This increment depends on Increment 2's Option-B `Fn` bound rather than a `dyn`
coercion. A call site is eligible only when the shared binding analysis reports
the concrete callback-only type parameter with no conflicts.

Corpus functions: `uniqBy`, `xorBy`, `unionBy`, `differenceBy`,
`intersectionBy`, `flatMapDeep`, `countBy`-shaped mappers whose key param is
unconstrained, `attempt`, `attemptAsync`.

Note honestly: `attempt` will **still** erase after this increment, for an
unrelated reason. Its return type is the union `[null, T] | [E, null]`, so the
body builds `SmeltUnknown::Array(vec![SmeltUnknown::Null, T])` and the
body-cleanliness trial (`core.rs:4384`) rejects it. `attempt` is the motivating
example in the brief but is *not* a winner here; the array/`*By` families are.

### Increment 4 — owned callback parameters (optional, Option A territory)

Only for parameters `function_parameter_requires_owned` (`core.rs:2180`) already
marks: `F: Fn(..) + 'static` taken by value and stored as `Rc<F>`, with a new arm
in `callee_param_is_owned_callback_sink` (`mod.rs:352`) propagating `'static`
back to callers. Do not start this until 1-3 are green; it is the one increment
that can deadlock the ownership fixpoint.

### Increment 5 — out of scope, but name it

Per-type-parameter (not per-function) generic decisions, so
`groupBy<T, K extends PropertyKey>` can emit `T` generically while `K` erases.
215 of es-toolkit's 800 generic functions have a constrained parameter; this is a
larger win than everything above and is orthogonal to callbacks.

Also out of scope, and the **multiplier** on this whole plan: the frontend does
not instantiate `T` when contextually typing a callback argument. In
`takeWhile_spec.rs:16` the source arrow `(item) => item < 4` is lowered with
param type `Unknown`, so it renders `|closure_arg_0: SmeltUnknown|` and the
call site needs a converting adapter even after this plan lands. Fixing that in
the frontend would delete the adapter entirely, roughly doubling the measured
win. Worth a separate blocker log.

### Performance evidence after the representation swap

Record clean and no-change incremental generated-crate `cargo check` wall time,
peak RSS, and test artifact size before Increment 2 and at the top of the stack.
Do not introduce an instantiation-count-based fallback unless those measurements
show a material regression; corpus-dependent signature selection would itself
increase complexity and reduce output predictability.

---

## 6. Expected before/after, from the generated es-toolkit crate

Measured on `target/compat-repos/es-toolkit/dist-smelt/src` (745 files):
87 function definitions take a `dyn Fn` parameter (49 borrowed `&dyn Fn`), and
they contain **10,462** `SmeltUnknown` tokens. 46 of them genuinely inspect
values (`mergeWith` alone is 4,168 tokens) and will keep erasing. **41** have
bodies that keep `T` opaque; those 41 definitions hold **488** tokens, and the
**321** call-site lines targeting them hold a further **4,089**.

### 6.1 `takeWhile<T>` — `dist-smelt/src/takeWhile_1.rs:7`

Source: `es-toolkit/src/array/takeWhile.ts:24`,
`export function takeWhile<T>(arr: readonly T[], shouldContinueTaking: (item: T, index: number, arr: readonly T[]) => boolean): T[]`

Before (definition, and the element read at `takeWhile_1.rs:22`):

```rust
pub(crate) fn take_while_145(arr: SmeltList<SmeltUnknown>, should_continue_taking: &dyn Fn(SmeltUnknown, f64, SmeltList<SmeltUnknown>) -> bool) -> SmeltList<SmeltUnknown> {
    let mut item: SmeltUnknown;
    let _smelt_tmp_5: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<SmeltUnknown> = vec![]; smelt_list_items }));
    ...
    item = arr.get({ .. }).cloned().unwrap_or(SmeltUnknown::Undefined).clone();
```

After Increment 1 (Option C):

```rust
pub(crate) fn take_while_145<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(arr: SmeltList<T>, should_continue_taking: &dyn Fn(T, f64, SmeltList<T>) -> bool) -> SmeltList<T> {
    let mut item: T;
    let _smelt_tmp_5: SmeltList<T> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<T> = vec![]; smelt_list_items }));
    ...
    item = arr.get({ .. }).cloned().unwrap_or(Default::default()).clone();
```

After Increment 2 (Option B):

```rust
pub(crate) fn take_while_145<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(T, f64, SmeltList<T>) -> bool + ?Sized>(arr: SmeltList<T>, should_continue_taking: &F0) -> SmeltList<T>
```

9 tokens deleted in the definition. Call site, `takeWhile_spec.rs:20` — before:

```rust
let _smelt_tmp_4: SmeltList<f64> = { let smelt_l: SmeltList<_> = (take_while_145({ let smelt_l: SmeltList<_> = (arr).clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| SmeltUnknown::Number(value as f64)).collect::<Vec<_>>()) }, &mut { let _smelt_adapted_callback = _smelt_tmp_3.clone(); move |arg0: SmeltUnknown, arg1: f64, arg2: SmeltList<SmeltUnknown>| (_smelt_adapted_callback)(arg0) })).clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| match value.clone() { SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get("__smelt_date") { Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }, /* 6 more arms */ }).collect::<Vec<_>>()) };
```

after (`T = f64` from `arr`; the adapter survives only because the spec closure's
own param is still MIR-typed `Unknown` — see §5, Increment 5):

```rust
let _smelt_tmp_4: SmeltList<f64> = take_while_145(arr.clone(), &mut { let _smelt_adapted_callback = _smelt_tmp_3.clone(); move |arg0: f64, arg1: f64, arg2: SmeltList<f64>| (_smelt_adapted_callback)(arg0.into_smelt_unknown()) });
```

~15 tokens deleted per call site; the one remaining `into_smelt_unknown`
reclassifies from avoidable to legitimate-boundary. `takeWhile_spec.rs` alone
carries 123 such tokens.

### 6.2 `sumBy<T>` — `dist-smelt/src/sumBy.rs:7`

Source: `es-toolkit/src/math/sumBy.ts:17`,
`export function sumBy<T>(items: readonly T[], getValue: (element: T, index: number) => number): number`

Before:

```rust
pub(crate) fn sum_by_674(items: SmeltList<SmeltUnknown>, get_value: &dyn Fn(SmeltUnknown, f64) -> f64) -> f64
```

After (Increment 2):

```rust
pub(crate) fn sum_by_674<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(T, f64) -> f64 + ?Sized>(items: SmeltList<T>, get_value: &F0) -> f64
```

The interesting site is `sumBy_spec.rs:40` — `sumBy(people, p => p.age)` where
`people: SmeltList<Person>` is a list of *class instances*. Before, the entire
list is rebuilt as erased objects:

```rust
let _smelt_tmp_3: f64 = sum_by_674({ let smelt_l: SmeltList<_> = (people).clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| { let smelt_object_value = value; let mut smelt_object_entries = ::std::collections::HashMap::new(); smelt_object_entries.insert("name".to_owned(), SmeltUnknown::String(smelt_object_value.name)); smelt_object_entries.insert("age".to_owned(), SmeltUnknown::Number(smelt_object_value.age as f64)); smelt_object_entries.insert("__smelt_class".to_owned(), SmeltUnknown::String("Person".to_owned())); SmeltUnknown::Object(SmeltObject::new(smelt_object_entries)) }).collect::<Vec<_>>()) }, &mut { let _smelt_adapted_callback = _smelt_tmp_2.clone(); move |arg0: SmeltUnknown, arg1: f64| match (_smelt_adapted_callback)(arg0).clone() { SmeltUnknown::Number(value) => value, /* 7 more arms */ } });
```

After (`T = Person`):

```rust
let _smelt_tmp_3: f64 = sum_by_674(people.clone(), &mut { let _smelt_adapted_callback = _smelt_tmp_2.clone(); move |arg0: Person, arg1: f64| match (_smelt_adapted_callback)(arg0.into_smelt_unknown()).clone() { SmeltUnknown::Number(value) => value, /* 7 more arms */ } });
```

~14 avoidable tokens deleted per site, and the object-map round trip on every
element disappears — a correctness-relevant win, not only a cosmetic one (object
identity is currently rebuilt).

### 6.3 `uniqWith<T>` — `dist-smelt/src/uniqWith_1.rs:6`

Source: `es-toolkit/src/array/uniqWith.ts:16`,
`export function uniqWith<T>(arr: readonly T[], areItemsEqual: (item1: T, item2: T) => boolean): T[]`

Before, including the synthesised default callback at `uniqWith_1.rs:10` — the
exact construct the `classes.rs:238-241` E0631 docstring is about:

```rust
pub(crate) fn uniq_with_151(arr: SmeltList<SmeltUnknown>, are_items_equal: &dyn Fn(SmeltUnknown, SmeltUnknown) -> bool) -> SmeltList<SmeltUnknown> {
    let mut item: SmeltUnknown;
    let mut _smelt_tmp_9: ::std::rc::Rc<dyn Fn(SmeltUnknown, i64, SmeltList<SmeltUnknown>) -> bool> = { let smelt_default_callback: ::std::rc::Rc<dyn Fn(SmeltUnknown, i64, SmeltList<SmeltUnknown>) -> bool> = ::std::rc::Rc::new(move |arg0: SmeltUnknown, arg1: i64, arg2: SmeltList<SmeltUnknown>| -> bool { false }); smelt_default_callback };
```

After (Increment 2):

```rust
pub(crate) fn uniq_with_151<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(T, T) -> bool + ?Sized>(arr: SmeltList<T>, are_items_equal: &F0) -> SmeltList<T> {
    let mut item: T;
    let mut _smelt_tmp_9: ::std::rc::Rc<dyn Fn(T, i64, SmeltList<T>) -> bool> = { let smelt_default_callback: ::std::rc::Rc<dyn Fn(T, i64, SmeltList<T>) -> bool> = ::std::rc::Rc::new(move |arg0: T, arg1: i64, arg2: SmeltList<T>| -> bool { false }); smelt_default_callback };
```

Note the default callback is emitted *inside* the generic function, so it renders
in the callee's own scope and no E0631 arises. The E0631 risk is only at
*call-site* defaults (`borrowed_default_function_text`, `core.rs:2338`), which
§4.2 fixes by rendering under bindings.

### 6.4 Ratchet estimate

* Increment 1 alone: the ~21 lifted functions plus their call sites. Estimate
  **1,500-2,500** avoidable tokens removed, i.e. 35,403 ⇒ roughly
  **32,900-33,900**.
* Increments 1+3 together: the full clean set (41 definitions, 488 tokens;
  321 call-site lines, 4,089 tokens), less the fraction that reclassifies to
  legitimate-boundary rather than vanishing. Estimate **2,500-4,000** removed,
  i.e. 35,403 ⇒ roughly **31,400-32,900**.
* Increment 2: **0** by construction.
* Fixing the frontend contextual-typing gap (§5) on top would remove most of the
  remaining adapter boilerplate — plausibly another 1,000-2,000.

**Increment 1 is the one that moves the ratchet most per unit of risk**;
Increment 3 adds roughly half again for materially more inference risk.

remeda's generated crate has the same shape (85 `dyn Fn`-taking definitions, 50
borrowed) but no ratchet, so it is a compile-and-test bar only.

---

## 7. Risks and kill criteria

Abandon-vs-push decisions, stated in advance.

1. **Increment 0 changes generated bytes.** Kill immediately. The refactor was
   supposed to be behaviour-preserving; if output moves, one of the three
   subsumed predicates was not equivalent, and finding out *why* is cheaper than
   debugging it downstream of a gate change.
2. **A single lifted function needs a special case to compile.** Hard stop
   (AGENTS.md "Type lowering": no per-library, per-function rules). If
   `takeWhile` compiles and `dropWhile` needs a carve-out, the general rule is
   wrong — narrow the *rule* (e.g. require direct inferability, i.e. fall back to
   Increment 1's condition) rather than adding an exception.
3. **`files_with_blockers` rises above 0.** Hard stop. A frontend blocker means
   the gate change pushed a shape into a lowering path that cannot represent it;
   that is a redesign signal, not a fix-forward signal.
4. **Avoidable erasure rises.** Hard stop by policy, and specifically watch for
   the failure mode where erasure is *traded*: a callee becomes generic but every
   call site grows a `SmeltFromUnknown`/`smelt_from_unknown` un-erasure. If the
   avoidable count falls by less than ~500 while `smelt_from_unknown`
   occurrences rise, the increment is moving erasure rather than removing it —
   abandon it. Note `into_smelt_unknown` at a genuine adapter boundary is *not*
   this failure (it classifies as legitimate-boundary and is the correct
   rendering); a *net* avoidable decrease is still required.
5. **E0283/E0282 appears anywhere in the generated crate.** Do not chase it with
   turbofish or `let _: T =` annotations. It means the bindings unification
   (§4.1) reported a binding the compiler cannot reproduce; tighten the
   unification to report "unbound" for that shape and let the function demote.
6. **Increment 2 changes the erasure count in either direction.** Kill and
   investigate: a representation swap that changes erasure means `param_type_text`
   and one of the four adapter renderers still disagree, i.e. the §4.2
   substitution is incomplete.
7. **Monomorphization blowup.** After Increment 2, if the generated crate's build
   time or binary size rises disproportionately (watch the compat CI wall clock),
   fall back to Option C for callbacks with more than one distinct instantiation
   — but only as a *general* rule keyed on instantiation count, never on names.
8. **Merge conflict with the throwing-callbacks branch in
   `function_shape_adapter_text`.** Not a kill criterion, a sequencing one: do
   not start Increment 1 until that branch's edits to `core.rs:2901+` and
   `types.rs:644-692` are committed.

No increment in this plan requires `unsafe`, and none introduces a new
`SmeltUnknown` conversion; every one of them deletes conversions or moves them to
an explicit boundary adapter.
