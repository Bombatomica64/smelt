# Throwing callbacks: the function-value type/ABI disagreement

Branch: `claude/estoolkit-throwing-callbacks` (stacked on `claude/estoolkit-closure-cfg`)

## The invariant being violated

> The Rust type emitted for a `Type::Function` **value** must agree with its MIR
> `FunctionType` in all three refinements — the erased-unknown-rest shape,
> `may_throw`, and a `Future` return — at *every* rendering site.

`type_text_with_scoped_type_params`'s `Type::Function` arm (`emitter/types.rs:884`)
is the canonical renderer and applies all three. Three other sites render function
values and apply *none* or *some* of them. Every symptom below is downstream of
that single disagreement.

## Reproduction

`scratchpad/repro` — a 3-function TypeScript fixture, built with
`smelt build --manifest-path <repro>/Smelt.toml`, then `cargo build` in `dist/`.

### Symptom 1 — a throwing callback parameter loses its `Result` (silent panic)

```ts
function thrower(x: number): string { if (x > 0) throw new Error("boom"); return "ok"; }
export function callThrowing(cb: (x: number) => string): string { return cb(1); }
export function useIt(): string { return callThrowing(thrower); }
```

```rust
pub(crate) fn thrower(x: f64) -> Result<String, Box<dyn std::error::Error>> { .. }   // correct

pub(crate) fn call_throwing(cb: &dyn Fn(f64) -> String) -> String {                  // WRONG: no Result
    let _smelt_tmp_1: String = cb(1.0);
    return _smelt_tmp_1;
}

pub(crate) fn use_it() -> String {
    let _smelt_tmp_0 = ::std::rc::Rc::new(|closure_arg_0: f64| -> Result<String, _> { .. });
    // the adapter swallows the throw into an abort:
    let _smelt_tmp_1: String = call_throwing(&mut { let _smelt_adapted_callback = _smelt_tmp_0.clone();
        move |arg0: f64| (_smelt_adapted_callback)(arg0).unwrap_or_else(|error| panic!("{}", error)) });
    return _smelt_tmp_1;
}
```

A recoverable JavaScript exception becomes a Rust `panic!` at the parameter
boundary. `try { cb(..) } catch { .. }` can therefore *never* observe a callback
throw: the throw is already an abort before the handler is reached. This is the
load-bearing defect — the others are compile errors, this one is silently wrong
runtime behaviour.

Cause: `param_type_text` (`emitter/types.rs:632`) renders
`&dyn Fn({params}) -> {type_text_with_impl_trait(function.return_ty)}`, consulting
neither `function.may_throw` nor a `Future` return.

### Symptom 2 — E0658 + E0308 on an owned erased-rest handle (live on `main`)

```ts
export function viaLocal(cb: (...args: unknown[]) => unknown): unknown {
  const g = cb;
  return g(3);
}
```

```rust
let g = ::std::rc::Rc::new(move |arg0: SmeltList<SmeltUnknown>| cb(arg0));
_smelt_tmp_3 = g.call(_smelt_tmp_2);
//               ^^^^ E0658 unstable `fn_traits`; E0308 expected `(SmeltList<..>,)`
```

Cause: `borrowed_function_handle_text` (`emitter/core.rs:2331`) wraps the borrowed
parameter in a bare `Rc<closure>`, ignoring that the target type's canonical
rendering is `SmeltErasedFunction`. MIR still types `g` as erased-rest, so the call
site takes the `.call(..)` branch and the two disagree. This needs no
defect-A change to reproduce — it is a live bug on `main`.

### Symptom 3 — the `catch` clause is dropped entirely (defect A)

Every `try`/`catch` around a call to a function-typed **parameter** loses its
handler:

```ts
export function guarded(cb: (...args: unknown[]) => unknown): unknown {
  try { return cb(1); } catch { return "caught"; }
}
```

```rust
pub(crate) fn guarded(cb: &dyn Fn(SmeltList<SmeltUnknown>) -> SmeltUnknown) -> SmeltUnknown {
    _smelt_tmp_1 = ..;
    _smelt_tmp_2 = cb(_smelt_tmp_1);     // no unwind edge, no handler
    return _smelt_tmp_2;
}
```

Cause: `ExprKind::ClosureCall` in `crates/smelt-mir/src/lower/expr.rs:2217` chooses
the unwind-carrying `Terminator::Call` form only when `function.may_throw` is set
on the callee type. A callback *parameter* of unknown provenance has
`may_throw == false`, so it takes the `Rvalue::ClosureCall` statement form, which
has no unwind edge — and the active exception handler is discarded.

This is why es-toolkit's `attempt`, `attemptAsync`, and every `try`-around-a-callback
site fails: the source wrapped the call in `try` precisely *because* the callee's
throw behaviour is not statically known.

### Symptom 4 — the two call ABIs are decided in two places, differently

`Rvalue::ClosureCall` (`emitter/call_runtime.rs:1311-1325`) has an explicit
precedence ladder — function-parameter place, function-parameter name, borrowed
callback capture, *then* erased-rest — with a comment stating that the parameter
branches must win or `.call()` resolves to unstable `Fn::call`.

`call_text`'s `Callee::Indirect` (`emitter/call.rs:1002`) has no ladder: it checks
`is_erased_unknown_rest_function && !may_throw` first and unconditionally emits
`({callee}).call({args})`.

So routing a call from the statement form to the terminator form (which Symptom 3's
fix requires) flips its ABI and produces E0658 + E0308. The ladder also does not
cover Symptom 2's case: it enumerates *syntactic* categories of callee that happen
to render as a bare `dyn Fn`, rather than asking what Rust type was actually
emitted for that binding.

## Fix plan

Establish the invariant, then let defect A fall out.

1. **`param_type_text`** — render the return type through the canonical
   `Type::Function` logic: `may_throw` ⇒ `Result<T, Box<dyn std::error::Error>>`,
   a `Future` return ⇒ `SmeltFuture<T>` with no outer `Result` (an async throw is
   a rejected future, per the existing comment at `types.rs:895`).
2. **The panicking adapters** — where a throwing source callback is adapted to a
   target callback position that also `may_throw`, propagate with `?` instead of
   `.unwrap_or_else(|error| panic!(..))`. The panic is correct *only* when the
   target genuinely cannot carry the error.
3. **`borrowed_function_handle_text`** — when the target is the erased-rest shape,
   build a `SmeltErasedFunction` (the canonical rendering) rather than an
   `Rc<closure>`, so the value and its call ABI agree.
4. **One authoritative ABI helper** — extract the `Rvalue::ClosureCall` precedence
   ladder into a single emitter method answering "does this callee's *emitted Rust
   value* carry the inherent `SmeltErasedFunction::call` method, or is it a bare
   `dyn Fn` handle invoked directly?" Consult it from both `Rvalue::ClosureCall`
   and `call_text`'s `Callee::Indirect`. This is deduplication: the question is
   answered once, in the emitter, from the emitter's own rendering decisions.
   It must NOT be reimplemented inside MIR.
5. **`ExprKind::ClosureCall`** (MIR) — take the `Terminator::Call` form whenever an
   exception handler is active, not only when `callee.may_throw`. Exclude an async
   callee, whose rejection surfaces at the `await` rather than at the call.

## Deliberately out of scope (surfaced, not skipped)

`attempt<T>(func: () => T)` erases `T` to `SmeltUnknown`:

```rust
pub(crate) fn attempt(func: &dyn Fn() -> SmeltUnknown) -> SmeltUnknown
```

`classes::function_emits_rust_generics` (`crates/smelt-codegen-rust/src/classes.rs:271`)
refuses real Rust generics for any function whose type parameter appears inside a
*callback* parameter:

```rust
if type_param_in_callback(mir, param_ty, name) { return false; }
```

That restriction is conservative rather than fundamental. Rust infers `T` fine
through a callback when the callback itself becomes a generic parameter with an
`Fn` bound — which is what a hand-writing Rust team would produce:

```rust
pub fn attempt<T, F: Fn() -> Result<T, Box<dyn std::error::Error>>>(func: F) -> ..
```

Lifting it means moving callback parameters from `&dyn Fn(..)` to `F: Fn(..)`,
which changes every callback-parameter signature in the corpus. That is a separate
feature with its own validation, tracked here as the next step rather than folded
into this PR.

## Outcome (implemented)

All five steps landed. What each one turned into:

1. **`param_type_text`** now renders its return through a new shared helper,
   `FunctionEmitter::function_value_return_type_text`, which is also the only
   return-type logic left in the canonical `Type::Function` arm. The two sites
   can no longer drift. Note the honest limitation: no TypeScript source shape
   currently produces a *borrowed* callback parameter whose MIR type carries
   `may_throw` (the frontend hard-codes `may_throw: false` for a declared
   callback type; `may_throw` arrives either from body analysis on a real
   function, from the MIR closure-widening pass on a *local*, or from
   specialization's `materialized_static_type` — and a specialized callback
   parameter is not reachable while `function_emits_rust_generics` refuses
   generics through callbacks, see "Deliberately out of scope"). The `may_throw`
   arm of the helper is therefore an invariant guard today, and the reachable
   half of Symptom 1 — the `panic!` inside the *fallible adapter closure* — is
   what step 2 fixed.
2. **The panicking adapters.** The three callback adapters in `core.rs`
   (`function_shape_adapter_text`, `rest_vector_function_adapter_text`,
   `rendered_function_shape_adapter_text`) already propagated with `?` when the
   target callback position also `may_throw`. Two call sites did not and now do:
   `Rvalue::ClosureCall` (`call_runtime.rs`) and `closure_call_text_for_dest`
   (`call.rs`), both gated on the new `body_can_propagate_error()` — the
   enclosing emitted body returns a `Result` and is not a generator. The `panic!`
   remains where the target genuinely cannot carry an error: the erased
   `SmeltErasedFunction` callback field (`Vec<SmeltUnknown> -> SmeltUnknown`),
   and the `Option::map(|f| ..)` closures of the optional-callee paths.
3. **The owned erased-rest handle.** `borrowed_function_handle_text` could not be
   the fix: `SmeltErasedFunction::callback` is an
   `Rc<dyn Fn(Vec<SmeltUnknown>) -> SmeltUnknown>`, i.e. a `'static` trait
   object, so a *borrowed* `&dyn Fn` can never be wrapped into one. The general
   fix is one step earlier, in the ownership analysis: a callback parameter bound
   to a local whose Rust type is the erased-rest struct now counts as escaping
   (`bound_to_erased_handle` in `compute_owned_callback_params`), so the
   parameter enters owned as `SmeltErasedFunction` and `g.call(..)` matches the
   value. The predicate is shared with the renderer through the new free function
   `is_erased_unknown_rest_function_in`.
4. **One authoritative ABI helper.** `callee_uses_erased_call_method` (plus its
   companion `callee_is_borrowed_function_handle`) now answers "does this
   callee's emitted Rust value carry the inherent `SmeltErasedFunction::call`
   method?" for all three call-emitting sites: `Rvalue::ClosureCall`,
   `Rvalue::ClosureCallSpread`, and `call_text`'s `Callee::Indirect`. Nothing was
   reimplemented in MIR. `Callee::Indirect` also had to learn the rest-vector
   argument packing the statement form already had, since a borrowed erased-rest
   parameter now reaches it.
5. **`ExprKind::ClosureCall`** takes the `Terminator::Call` form whenever an
   exception handler is active, excluding an async callee (its rejection surfaces
   at the `await`, which has its own unwind edge).

### SmeltUnknown delta (a justified rise, not a fall)

es-toolkit avoidable erasure went 35403 -> 35411 (+8); the baseline was
re-snapshotted in the same commit. Every new occurrence is inside a `catch`
clause that *previously was not emitted at all* — this change is what makes that
code reachable — and every one of them handles a caught exception value, which
TypeScript types `unknown`:

| Occurrences | Shape |
| ---: | --- |
| 6 | the panic-recovery payload record, `SmeltUnknown::String(__smelt_error)` |
| 2 | `SmeltUnknown::Array(vec![error.clone(), SmeltUnknown::Null])` (`attempt`'s `[error, null]`) |
| 2 | `let mut _smelt_tmp_N: SmeltUnknown = SmeltUnknown::Null;` (the catch result temp) |
| 2 | `SmeltRecord::from([.., SmeltUnknown::String(_smelt_tmp_N)])` |
| 1 | `matches!(e.clone(), SmeltUnknown::Object(value) if value.contains_key(..))` |
| 1 | `SmeltUnknown::Object(SmeltObject::from_unknown_record(..))` |

The first 6 were reclassified as `legitimate-boundary`: they are the
exception-payload ABI, identical to the record `smelt_thrown_value` synthesizes
for a foreign error, which was already a boundary. The emit site is documented
(`thrown::panic_payload_record_expr`, which also deduplicates the three copies of
that record text) and the classification is covered by
`unknown_report::tests::panic_recovery_payload_is_a_boundary`. The remaining 8
are ordinary `catch`-body statements on a statically-`unknown` caught value; the
scanner is textual and cannot see that source type, so they stay classified as
avoidable rather than being papered over with a broader marker.
