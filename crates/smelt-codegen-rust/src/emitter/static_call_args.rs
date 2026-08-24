//! The static-call argument ladder, as a total function.
//!
//! Emitting one argument of a direct (`Callee::Static`) call is a choice between
//! eight mutually exclusive outcomes. That choice used to live as a chain of
//! `if .. { push(..); continue; }` blocks inside the call emitter, where the
//! precedence between the branches was encoded *only* by their source order and
//! documented only by comments warning that the order was load-bearing. Two of
//! the six defects shipped by the callback-generics campaign were ordering
//! defects in exactly that chain: a later branch silently claimed an argument an
//! earlier branch owned, and the generated Rust failed with `E0308`.
//!
//! This module makes the ladder total. [`StaticArgumentKind`] names every
//! outcome, [`FunctionEmitter::classify_static_call_argument`] is the ONE place
//! that decides which one applies, and
//! [`FunctionEmitter::render_static_call_argument`] dispatches through an
//! exhaustive `match` with no wildcard arm, so a seventh renderer added later
//! cannot be bolted on at the wrong rung: it must become a variant, must be
//! given a documented position in the classifier, and must be handled at every
//! dispatch site or the crate does not compile.
//!
//! The classifier evaluates its predicates strictly in precedence order and
//! short-circuits, which matters for cost as well as for meaning:
//! [`FunctionEmitter::argument_renders_alike_across_call`] (the demoting-erased
//! predicate) *renders types* to compare their spellings, so it must never be
//! reached for an argument an earlier rung would have claimed. Every other
//! predicate is pure apart from returning `Result`, so classifying in this order
//! reaches the same decision at the same cost as the original chain.
//!
//! There is a SECOND, shorter argument ladder in the same emitter: the
//! rest-parameter pre-pass in [`super::call`], which renders the fixed arguments
//! that precede a trailing erased-list parameter. It has only three rungs
//! (borrowed callback, mutable reference, value) and deliberately lacks the
//! callee-generic, monomorphization-passthrough and demoting-erased rungs, so it
//! is NOT the same function and cannot share this classifier without changing
//! emitted bytes.
//!
//! And a THIRD: `call::static_call_mut_list_adapter_text`, the convert-in-place
//! adapter, recognisable by the `smelt_mut_call_result` wrapper it emits. It
//! intercepts most `&mut` LIST arguments before this classifier is ever reached
//! and carries its own argument loop and its own unifier
//! (`mut_list_adapter_arg`). It is easy to miss precisely because it runs
//! earlier: a precedence test written without accounting for it passes under
//! reversed precedence, because the adapter — not this ladder — emitted the
//! call. `static_call_arg_precedence_tests` documents how its fixtures make the
//! adapter decline so the contested argument actually reaches this classifier.
//!
//! Two untotalized ladders remain, then, not one. Neither can share this
//! classifier byte-inertly; both are named here so the next person does not
//! have to rediscover them.

use super::*;
use crate::generic_bindings::{CalleeTypeParamBindings, substitution_matches};

/// The single outcome that renders one argument of a static call.
///
/// The variants are listed in PRECEDENCE order: when an argument satisfies the
/// conditions of more than one, the earliest variant wins, and each variant's
/// docstring says why it outranks the ones below it. Reversing any two of them
/// is a miscompile, not a style change.
pub(super) enum StaticArgumentKind<'arg> {
    /// 1. A surplus trailing argument: the callee has no parameter at this
    ///    position and no rest parameter absorbs it.
    ///
    /// Outranks everything below because there is no target type at all, and
    /// every lower rung is a statement *about* the target type. This is the one
    /// variant that renders NOTHING: JavaScript ignores surplus positional
    /// arguments, and the operand has already been evaluated into a temporary
    /// before the call, so its side effects survive without being forwarded.
    Surplus,
    /// 2. The target is one of the callee's own free type parameters (`x: T` of
    ///    `function identity<T>(x: T)`).
    ///
    /// Outranks the rungs below because a bare `T` target is not a type to
    /// coerce toward: Rust binds `T` *from* this argument. Coercing would erase
    /// the argument and pin `T = SmeltUnknown`. It cannot conflict with the
    /// borrowed-callback rung below it — that rung claims only `Type::Function`
    /// targets, and a bare `Type::TypeParam` target is not one.
    CalleeGeneric {
        /// The callee's declared parameter type (a bare `Type::TypeParam`).
        target_ty: TypeId,
    },
    /// 3. A borrowed callback parameter: a `Type::Function` target the callee
    ///    does not require by value.
    ///
    /// Outranks the monomorphization passthrough (rung 5), and the order is
    /// load-bearing — it is the first shipped defect this module exists to make
    /// unrepeatable. Once a callback-bearing callee can emit real generics, a
    /// pinned call site satisfies `substitution_matches` for an `Fn(T)` target
    /// too, so the passthrough would claim the callback and render it as an
    /// owned `Rc<closure>` value against a `&dyn Fn` parameter (`E0308`). The
    /// passthrough exists for value parameters; this rung owns EVERY borrowed
    /// callback, pinned or not.
    BorrowedCallback {
        /// The callee's declared parameter type (a `Type::Function`).
        target_ty: TypeId,
    },
    /// 4. A `&mut` value parameter: the callee mutates it through a mutable
    ///    reference, so a reference must be passed, not a value.
    ///
    /// Also outranks the monomorphization passthrough, and for the same reason —
    /// the second shipped defect. A parameter can be BOTH a monomorphizing
    /// composite (`xs: T[]` -> `SmeltList<T>`) and in `mutable_params`; the
    /// passthrough would claim it and render it by value against a
    /// `&mut SmeltList<f64>` parameter (`E0308`). The two rungs that need a
    /// specific *reference* form run first; the passthrough takes what is left.
    MutableReference {
        /// The target with this call site's bindings applied, NOT the declared
        /// type. A monomorphizing site instantiates `&mut SmeltList<T>` as
        /// `&mut SmeltList<f64>`, so rendering against the declared
        /// `SmeltList<T>` would erase every element and hand the callee a
        /// `Vec<SmeltUnknown>` where it declared `Vec<f64>` (`E0308`). Falls
        /// back to the declared type at a site that did not monomorphize.
        substituted_target_ty: TypeId,
    },
    /// 5. A composite VALUE parameter that mentions one of the callee's own
    ///    generics (`arr: T[]` -> `SmeltList<T>`), instantiated at this call
    ///    site to the concrete argument shape.
    ///
    /// Rendered at the argument's OWN concrete type (`SmeltList<f64>`) so Rust
    /// binds `T = f64` from it; coercing to the declared `SmeltList<T>` would
    /// erase every element and clash with the monomorphization (`E0308`).
    /// Outranks the two rungs below because it is the more specific statement:
    /// they describe what to do with a target the call site did NOT pin.
    /// `substitution_matches` is the whole test — for a target mentioning no
    /// type parameter it degenerates to `source == target`, which the
    /// classifier's own `source_ty != target_ty` guard already excluded.
    MonomorphizingPassthrough {
        /// The argument's own type, which is what it renders at.
        source_ty: TypeId,
    },
    /// 6. An argument whose source and target are the same `TypeId` but whose
    ///    two SIDES render as different Rust types.
    ///
    /// MIR type identity is not Rust type identity: [`smelt_hir::Symbol`] is
    /// name-interned, so a *lifted* caller's `SmeltList<T>` local and a
    /// *demoted* callee's declared `T[]` parameter are one `TypeId` rendering as
    /// `SmeltList<T>` and `SmeltList<SmeltUnknown>` (`E0308`). Outranks the
    /// terminal rung because the terminal rung's `value_at_type` passes an
    /// argument straight through whenever the two `TypeId`s are equal, which is
    /// precisely the case that is wrong here. Its predicate,
    /// [`FunctionEmitter::argument_renders_alike_across_call`], renders types to
    /// compare them, so the classifier must reach it only after rungs 1-5 have
    /// declined.
    DemotingErased {
        /// The caller-side place, which the demotion signal is spelled from.
        place: &'arg Place,
        /// The argument's own type, equal to the target's `TypeId`.
        source_ty: TypeId,
    },
    /// 7a. An erased-function source handed to a target that will not accept
    ///     one, which is emitted as the target's default value.
    ///
    /// The terminal rung's first half. It is below everything above it because
    /// every rung above it describes a target that CAN receive this argument;
    /// this one is the admission that no value of the source can be spelled at
    /// the target.
    ErasedFunctionDefault {
        /// The callee's declared parameter type, whose default value is emitted.
        target_ty: TypeId,
    },
    /// 7b. The ordinary case: coerce the argument to the callee's declared
    ///     parameter type.
    ///
    /// The terminal rung's second half and the ladder's default outcome. Every
    /// variant above it is an exception to it.
    Coerced {
        /// The callee's declared parameter type, which the argument coerces to.
        target_ty: TypeId,
    },
}

impl FunctionEmitter<'_> {
    /// Classify one argument of a static call into the single kind that renders
    /// it.
    ///
    /// This is the ladder. The rungs are tested in [`StaticArgumentKind`]'s
    /// declaration order and the first match wins; see each variant's docstring
    /// for why it outranks the rungs below it. `target_ty` is `None` for a
    /// surplus trailing argument, `param` is the callee's `LocalId` for this
    /// position when it has one, and `callee_bindings` is this call site's
    /// monomorphization (`None` when the site did not pin the callee).
    ///
    /// Predicates are evaluated lazily in rung order: nothing an earlier rung
    /// would have claimed pays for a later rung's predicate. That is a
    /// correctness-neutral cost property for the pure predicates, and a real
    /// cost property for rung 6, whose predicate renders types.
    pub(super) fn classify_static_call_argument<'arg>(
        &self,
        function: &MirFunction,
        index: usize,
        arg: &'arg Operand,
        param: Option<LocalId>,
        target_ty: Option<TypeId>,
        free_function_type_params: &HashSet<Symbol>,
        callee_bindings: Option<&CalleeTypeParamBindings>,
    ) -> Result<StaticArgumentKind<'arg>, EmitError> {
        // Rung 1: extra trailing argument beyond the callee's fixed arity (no
        // rest parameter absorbs it). `tsc` only admits this shape when the
        // callee is erased/`any`-typed, so a genuine over-application is already
        // rejected upstream.
        let Some(target_ty) = target_ty else {
            return Ok(StaticArgumentKind::Surplus);
        };
        // Rung 2: a concrete argument bound to one of the callee's own generic
        // type parameters (`identity(3)` against `x: T`).
        if matches!(
            self.mir.types.get(target_ty),
            Some(Type::TypeParam { name })
                if free_function_type_params.contains(name)
        ) {
            return Ok(StaticArgumentKind::CalleeGeneric { target_ty });
        }
        // Rung 3: a borrowed callback parameter, which the borrowed-callback
        // renderer reborrows as `&dyn Fn(..)` while resolving the callee's type
        // parameters through this call site's bindings.
        if matches!(self.mir.types.get(target_ty), Some(Type::Function(_)))
            && param.is_some_and(|target_param| {
                !self
                    .function_parameter_requires_owned_in(function, target_param)
                    .unwrap_or(false)
            })
        {
            return Ok(StaticArgumentKind::BorrowedCallback { target_ty });
        }
        // Seam 4 (`emitter::seam_assertions`): reaching here means the
        // borrowed-callback rung declined this argument, so one of the
        // value-shaped rungs below will claim it. A parameter the callee
        // declares behind an `F{n}` bound cannot survive that.
        #[cfg(debug_assertions)]
        self.debug_assert_callback_generic_arg_is_borrowed(function, index, param, target_ty);
        #[cfg(not(debug_assertions))]
        let _ = index;
        // Rung 4: a `&mut` value parameter, rendered against the SUBSTITUTED
        // target so a monomorphizing site does not erase its elements.
        if param.is_some_and(|target_param| {
            self.parameter_needs_mutable_reference_in(function, target_param)
        }) {
            return Ok(StaticArgumentKind::MutableReference {
                substituted_target_ty: self.substituted_param_ty(target_ty, callee_bindings),
            });
        }
        let source_ty = self.operand_ty(arg)?;
        // Rung 5: a composite value parameter this call site monomorphizes at
        // the concrete argument shape.
        if let Some(bindings) = callee_bindings
            && source_ty != target_ty
            && substitution_matches(self.mir, target_ty, source_ty, bindings)
        {
            return Ok(StaticArgumentKind::MonomorphizingPassthrough { source_ty });
        }
        // Rung 6: the caller's and the callee's renderings of one shared
        // `TypeId` disagree, so emit the demotion signal instead of a
        // pass-through the caller's body-cleanliness trial would never see.
        if source_ty == target_ty
            && let Operand::Copy(place) | Operand::Move(place) = arg
            && !self.argument_renders_alike_across_call(
                source_ty,
                target_ty,
                Some(free_function_type_params),
            )?
        {
            return Ok(StaticArgumentKind::DemotingErased { place, source_ty });
        }
        // Rung 7: the terminal split.
        if matches!(self.mir.types.get(source_ty), Some(Type::Function(_)))
            && !self.type_accepts_erased_function(target_ty)
        {
            return Ok(StaticArgumentKind::ErasedFunctionDefault { target_ty });
        }
        Ok(StaticArgumentKind::Coerced { target_ty })
    }

    /// Render one classified static-call argument into `rendered_args`.
    ///
    /// The `match` is exhaustive and has NO wildcard arm on purpose: a wildcard
    /// would reintroduce the silent fallthrough this module exists to remove, by
    /// letting a newly added [`StaticArgumentKind`] variant be rendered by
    /// whichever branch happened to catch it.
    ///
    /// Note that this pushes ZERO strings for [`StaticArgumentKind::Surplus`],
    /// so the number of rendered arguments is not in general the number of
    /// classified ones.
    pub(super) fn render_static_call_argument(
        &self,
        kind: StaticArgumentKind<'_>,
        function: &MirFunction,
        arg: &Operand,
        free_function_type_params: &HashSet<Symbol>,
        callee_bindings: Option<&CalleeTypeParamBindings>,
        rendered_args: &mut Vec<String>,
    ) -> Result<(), EmitError> {
        match kind {
            StaticArgumentKind::Surplus => {}
            StaticArgumentKind::CalleeGeneric { target_ty } => {
                rendered_args.push(self.callee_generic_argument_text(
                    arg,
                    function,
                    target_ty,
                    free_function_type_params,
                )?);
            }
            StaticArgumentKind::BorrowedCallback { target_ty } => {
                rendered_args.push(self.borrowed_function_argument_text(
                    arg,
                    target_ty,
                    callee_bindings,
                )?);
            }
            StaticArgumentKind::MutableReference {
                substituted_target_ty,
            } => {
                rendered_args.push(self.mutable_reference_argument_text(
                    arg,
                    substituted_target_ty,
                    Some(free_function_type_params),
                )?);
            }
            StaticArgumentKind::MonomorphizingPassthrough { source_ty } => {
                rendered_args.push(self.value_at_type(arg, source_ty)?);
            }
            StaticArgumentKind::DemotingErased { place, source_ty } => {
                rendered_args.push(self.demoting_erased_argument_text(place, source_ty)?);
            }
            StaticArgumentKind::ErasedFunctionDefault { target_ty } => {
                rendered_args.push(self.default_value(target_ty)?);
            }
            StaticArgumentKind::Coerced { target_ty } => {
                rendered_args.push(self.value_at_type(arg, target_ty)?);
            }
        }
        Ok(())
    }
}
