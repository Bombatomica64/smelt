//! Debug-only agreement checks for the emitter's self-consistency seams.
//!
//! Five of the six defects the callback-generics campaign shipped were not
//! syntax or formatting problems: they were the emitter disagreeing with
//! *itself* about one value. A callback adapter declared its parameters at the
//! substituted type and converted its body from the unsubstituted one; the same
//! adapter left its return unsubstituted while its parameters were substituted;
//! a call site claimed the callee's declared return where the emitted call
//! really evaluates to the substituted one; an argument the callee expects
//! behind an `F{n}` bound was claimed by a branch that renders a value; a
//! binding the substitution reported as `Concrete` rendered as `SmeltUnknown`.
//!
//! Every one of those had a precise moment where both sides were in scope and
//! could have been compared as `TypeId`s or decision values. Instead each
//! surfaced as an `E0308`/`E0631` hundreds of characters into one generated
//! line, in a crate of tens of thousands of lines, and only after a compile of
//! the generated output. This module is the comparison that was missing.
//!
//! Rules the checks here follow, and that new ones must keep:
//!
//! * **Compiled out in release.** The whole module is `#[cfg(debug_assertions)]`
//!   and so is every call site that feeds it, so a release build contains
//!   neither the checks, the argument set-up, nor the `Debug`/`Eq` derives that
//!   exist only for their messages.
//! * **Cheap.** Comparisons are on `TypeId`s and decision values, never on
//!   rendered type *text* — rendering a type twice to compare the strings would
//!   double the cost of the very path being checked. Type text appears only
//!   inside assertion messages, which `debug_assert!` formats only when the
//!   check has already failed.
//! * **Both sides named.** A message states what each side believed and which
//!   function was being emitted, because the reader's next question is always
//!   "which of the two is wrong".
//! * **Byte-inert.** A check never influences what is emitted. It observes, and
//!   on disagreement it panics; it never repairs.
//!
//! Seams that could only be checked by re-rendering a type are deliberately
//! absent. Two examples, both real: whether a caller-side and a callee-side
//! rendering of one `TypeId` spell the same Rust text (already answered where it
//! matters by `argument_renders_alike_across_call`, at emit cost the emitter
//! chose to pay for correctness, not for checking), and whether the erasing
//! branch of the mutable-list adapter renders its temporary at the callee's
//! spelling. Adding either here would double the render cost of a hot path for
//! a check, which the "cheap" rule above forbids.
//!
//! Each check below was validated by reintroducing the defect it exists for and
//! confirming it fires on a repro project — not merely by observing that it
//! passes. A check that only ever passes is indistinguishable from one that is
//! never reached, so "it is green" is not evidence and must not be treated as
//! such when a new seam is added here.

use super::*;
use call::CallMonomorphization;

impl FunctionEmitter<'_> {
    /// The Rust name of the function currently being emitted, for messages.
    ///
    /// Falls back to a placeholder rather than propagating, because a check may
    /// not change emission — including by failing differently.
    fn seam_context_name(&self) -> String {
        self.function_rust_name(self.function)
            .unwrap_or_else(|_| "<unnameable function>".to_owned())
    }

    /// Render `ty` for an assertion message only.
    ///
    /// Called exclusively from inside a `debug_assert!` message, which is
    /// evaluated only after the check has failed, so this never costs a render
    /// on a passing emit.
    ///
    /// The call site's bindings are deliberately dropped
    /// ([`TypeSubstitution::without_bindings`]): the two sides of a seam
    /// disagreement are typically a declared `T` and the type it was pinned to,
    /// and re-applying the bindings while rendering would spell both of them
    /// `f64` and hide the very difference the message exists to show. Without
    /// them the declared side reads `SmeltUnknown` and the substituted side
    /// reads `f64`, which is the contrast. The lexical scope is preserved, so a
    /// non-monomorphizing site still spells the caller's own parameters.
    fn seam_type_text(&self, ty: TypeId, substitution: &TypeSubstitution<'_>) -> String {
        self.rust_type(ty, false, &substitution.without_bindings())
            .map_or_else(|_| "<unrenderable>".to_owned(), RustType::into_string)
    }

    /// Seam 1: a callback adapter's parameter *declaration* and its body
    /// conversion must start from one type.
    ///
    /// `function_shape_adapter_text` emits `|arg{index}: D| inner(convert(arg{index}))`.
    /// `D` comes from `callback_arg_decls` rendering the callee's declared
    /// parameter under `substitution`; the conversion starts from `body_ty`,
    /// resolved through the call site's bindings. When the site monomorphized
    /// the callee and only one of the two was substituted, the body computes an
    /// unwrapping ladder for an erased `SmeltUnknown` against a declared `f64`
    /// (or the reverse) — the shipped `E0308`.
    ///
    /// The check re-derives the declaration's type from the declaration's *own*
    /// environment (`substitution.bindings()`) and compares `TypeId`s, so it
    /// fails exactly when the two environments differ, not when they merely
    /// spell alike.
    pub(super) fn debug_assert_adapter_param_agrees(
        &self,
        index: usize,
        declared_param_ty: TypeId,
        body_ty: TypeId,
        substitution: &TypeSubstitution<'_>,
    ) {
        let decl_ty = self.substituted_param_ty(declared_param_ty, substitution.bindings());
        debug_assert_eq!(
            decl_ty,
            body_ty,
            "callback adapter in `{}` declares arg{index} as `{}` but converts its body from `{}` \
             (declared parameter type {declared_param_ty:?}): the declaration environment and the \
             body substitution disagree",
            self.seam_context_name(),
            self.seam_type_text(decl_ty, substitution),
            self.seam_type_text(body_ty, substitution),
        );
    }

    /// Seam 1b: the same adapter's *return* must be resolved in the same
    /// environment as its parameters.
    ///
    /// The second defect at this seam: parameters were substituted through the
    /// call site's bindings while the return conversion kept the callee's
    /// declared `T`, so an `f64` was rewrapped into `SmeltUnknown::Number(..)`
    /// against an `Fn(f64) -> f64` target.
    pub(super) fn debug_assert_adapter_return_agrees(
        &self,
        declared_return_ty: TypeId,
        conversion_return_ty: TypeId,
        substitution: &TypeSubstitution<'_>,
    ) {
        let decl_ty = self.substituted_param_ty(declared_return_ty, substitution.bindings());
        debug_assert_eq!(
            decl_ty,
            conversion_return_ty,
            "callback adapter in `{}` declares its return as `{}` but converts its result to `{}` \
             (declared return type {declared_return_ty:?}): the parameter declarations and the \
             return conversion resolve in different environments",
            self.seam_context_name(),
            self.seam_type_text(decl_ty, substitution),
            self.seam_type_text(conversion_return_ty, substitution),
        );
    }

    /// Seam 2: one call site monomorphizes its arguments and its return under
    /// one decision.
    ///
    /// `call_text_for_dest` renders the arguments (inside `call_text`) and then
    /// the return (inside `call_emitted_source_ty`), and each recomputes
    /// `static_call_monomorphization` for itself. The whole design rests on that
    /// recomputation being a pure function of `(function, args)`: if emitter
    /// state observed by the analysis — the crate-wide generic-emission
    /// decision, a body-cleanliness trial's `suppress_type_params`, the emitted
    /// signature tables — moved between the two, the arguments would pass
    /// through concretely while the return claimed the erased type, which is
    /// exactly the shipped `6 x E0308` in radash.
    ///
    /// The comment on `CallMonomorphization` says the two cannot disagree. This
    /// asserts it.
    pub(super) fn debug_assert_call_monomorphization_stable(
        &self,
        callee_name: &str,
        before_arguments: Option<&CallMonomorphization>,
        after_return: Option<&CallMonomorphization>,
    ) {
        debug_assert_eq!(
            before_arguments,
            after_return,
            "call to `{callee_name}` in `{}` monomorphized its arguments under {before_arguments:?} \
             but its return under {after_return:?}: the two halves of one call site disagree",
            self.seam_context_name(),
        );
    }

    /// Seam 3: the declared return type versus the type the emitted call really
    /// evaluates to.
    ///
    /// `call_source_ty` answers "what did the callee declare"; the emitted call
    /// evaluates to `call_emitted_source_ty`, which additionally accounts for a
    /// signature that erased a return type parameter and for a call site that
    /// monomorphized one. A coercion site that asks the first question while the
    /// value in hand answers the second coerces from the wrong type. One such
    /// site produced six `E0308`s in radash and reached CI.
    ///
    /// Called from the sites that coerce a *call result* through
    /// `call_source_ty`, where both answers are computable from arguments
    /// already in scope.
    pub(super) fn debug_assert_declared_return_is_emitted_return(
        &self,
        callee: &Callee,
        args: &[Operand],
        dest_ty: TypeId,
        declared_return_ty: TypeId,
    ) {
        let Ok(emitted) = self.call_emitted_source_ty(callee, args, dest_ty) else {
            return;
        };
        let scope = self.current_function_type_params();
        let substitution = TypeSubstitution::lexical(&scope);
        debug_assert_eq!(
            declared_return_ty,
            emitted,
            "call in `{}` coerces its result from the DECLARED return `{}` but the emitted call \
             evaluates to `{}`: use `call_emitted_source_ty` at this site",
            self.seam_context_name(),
            self.seam_type_text(declared_return_ty, &substitution),
            self.seam_type_text(emitted, &substitution),
        );
    }

    /// Seam 4: an argument for a parameter the callee declares behind an `F{n}`
    /// bound must be rendered by the borrowed-callback path.
    ///
    /// A generic free function renders a direct required borrowed callback
    /// parameter as `&F{n}` with an `F{n}: Fn(..) + ?Sized` bound. Only
    /// `borrowed_function_argument_text` produces a `&dyn Fn`-shaped argument
    /// that binds such a parameter; the monomorphization passthrough renders an
    /// owned `Rc<..>` handle and the ordinary coercion renders a value, either
    /// of which is `E0308`/`E0277` against `&F{n}`. That is the shipped defect
    /// in which the passthrough claimed an argument the borrowed-callback branch
    /// owns.
    ///
    /// Called at the point in the argument ladder immediately *after* the
    /// borrowed-callback branch has declined, so reaching it means some later
    /// branch will claim the argument. The branches ahead of it cannot be at
    /// risk: they claim only a bare `Type::TypeParam` target, and an `F{n}`
    /// parameter is function-typed by construction.
    ///
    /// The callee's generated names are read from `crate::classes`, the single
    /// authority the callee's own signature emission uses, rather than
    /// re-deriving the rule here.
    pub(super) fn debug_assert_callback_generic_arg_is_borrowed(
        &self,
        function: &MirFunction,
        index: usize,
        param: Option<LocalId>,
        target_ty: TypeId,
    ) {
        if !matches!(self.mir.types.get(target_ty), Some(Type::Function(_))) {
            return;
        }
        let Some(target_param) = param else {
            return;
        };
        let generated = crate::classes::callback_generic_params(
            self.mir,
            function,
            &self.context.owned_callback_params,
        )
        .unwrap_or_default();
        let bound = generated
            .iter()
            .find(|(generic_param, _)| *generic_param == target_param);
        debug_assert!(
            bound.is_none(),
            "argument {index} of the call to `{}` in `{}` targets a parameter the callee declares \
             as `&{}` (an `F{{n}}` bound), but the borrowed-callback branch declined it, so a \
             later branch will render it as an owned handle or a value",
            self.function_rust_name(function)
                .unwrap_or_else(|_| "<unnameable callee>".to_owned()),
            self.seam_context_name(),
            bound.map_or("F?", |(_, name)| name.as_str()),
        );
    }

    /// Seam 5: a type parameter the substitution resolved to `Concrete` must not
    /// render as `SmeltUnknown`.
    ///
    /// `Resolved::Substituted(bound)` means a call site pinned this parameter to
    /// a concrete MIR type, and the whole point of the binding is that the
    /// rendering spells that type. If it comes back `SmeltUnknown` the binding
    /// was carried all the way to the renderer and then dropped, and the
    /// emitted text silently agrees with an erased callee while the sibling
    /// arguments were passed through concretely (CLAUDE.md, "SmeltUnknown
    /// boundaries": erasure must be a boundary decision, never a rendering
    /// accident).
    ///
    /// Four MIR shapes render the unknown carrier *faithfully*, and a binding to
    /// one of them is not a dropped binding: a source `unknown`, `never`, a
    /// union (whose Rust representation genuinely erases), and a class the crate
    /// does not emit. Those are excluded by MIR shape rather than by text.
    ///
    /// The rendered text is already in hand at the check, so this compares a
    /// string that was going to be built anyway — it renders nothing twice.
    pub(super) fn debug_assert_substituted_binding_is_not_erased(
        &self,
        name: Symbol,
        bound: TypeId,
        rendered: &RustType,
    ) {
        let faithfully_unknown = matches!(
            self.mir.types.get(bound),
            Some(Type::Unknown | Type::Never | Type::Union(_) | Type::Class { .. })
        );
        let dropped_binding = rendered.as_str() == "SmeltUnknown" && !faithfully_unknown;
        debug_assert!(
            !dropped_binding,
            "type parameter `{}` was reported Concrete at type {bound:?} while emitting `{}`, but \
             rendered as `SmeltUnknown`: the call site's binding reached the renderer and was then \
             erased",
            self.symbol_name(name).unwrap_or("<unnameable>"),
            self.seam_context_name(),
        );
    }
}
