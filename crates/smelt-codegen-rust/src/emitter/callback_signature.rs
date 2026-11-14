//! A function value's Rust signature: parameter passing, argument declarations
//! and the generic `Fn` bounds a callback parameter is spelled with.

use super::*;
use crate::rust::{RustExpr, RustShape, RustType};
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// Whether a callback parameter is passed by shared reference (`&T`).
    ///
    /// A JavaScript array callback receives the array itself as its third argument,
    /// and in JavaScript that costs nothing — it is the same object. Lowered by
    /// value, it costs a full `SmeltList` deep copy on *every element*, which turns
    /// every `map`/`filter`/`groupBy`/`sumBy` over n elements into O(n^2) copying.
    /// Ablating that one argument in the generated es-toolkit `groupBy` took it from
    /// 0.50 to 3.83 ops/s — 7.7x — with an unchanged result checksum.
    ///
    /// A hand-writing Rust team would take `&[T]` here without a second thought, so
    /// list-typed callback parameters are passed by shared reference. The rule is
    /// deliberately about the *type*, not about any particular library's callback:
    /// it fires wherever a callback declares a list parameter.
    ///
    /// A `rest` parameter is excluded. `...args` is not a list the caller already
    /// holds — the callee-side adapter *builds* it by packing the erased argument
    /// vector (`function_args_from_smelt_args_text`), so there is nothing to borrow
    /// and a reference buys nothing. Excluding it also keeps the erased-callback
    /// adapters working: they receive such a parameter and re-erase it into a JS
    /// array, which needs ownership of the elements.
    ///
    /// `Type::Unknown` is included for the same reason, and it is the case that
    /// matters most in practice. A `SmeltUnknown` is a tagged enum whose payload
    /// arms own `Rc`s, `String`s and boxed values; passing one by value clones the
    /// tag *and* bumps or copies whatever it carries, once per element. The rule
    /// fired on 3 of the 341 first callback parameters in the es-toolkit corpus
    /// while it was `List`-only; with `Unknown` it reaches 221 of them, which is
    /// where the per-element traffic in `partition`/`groupBy`/`countBy`/`uniqueBy`
    /// actually lives.
    ///
    /// Dict and Set parameters have the same argument-side cost and should follow,
    /// but each needs its own pass over the erased-callback adapters before the ABI
    /// can move.
    /// Whether a SYNTHESIZED callback parameter of type `param` is passed by shared
    /// reference.
    ///
    /// Emitter-synthesized callables -- the promise executor's `resolve`/`reject` are
    /// the case that needs this -- have no `FunctionType` to consult, but the closure
    /// they build still has to match the spelling `function_type_param` gave the
    /// `dyn Fn` it is assigned to. This answers the type-shape half of
    /// [`Self::callback_param_is_shared_reference`]; the two marker halves cannot apply
    /// to a synthesized parameter, which is never `&mut` and never a rest.
    ///
    /// radash's `async_` is what caught the omission: a `Promise<T[]>` executor
    /// expects `Rc<dyn Fn(&SmeltList<..>)>` once the rule is in force, and the
    /// synthesized resolver was still built as `Rc<dyn Fn(SmeltList<..>)>`.
    pub(super) fn synthesized_callback_param_is_shared_reference(&self, param: TypeId) -> bool {
        self.param_type_is_by_shared_reference(param)
    }

    /// The type-shape half of the by-shared-reference rule, decided on the
    /// parameter's RENDERED Rust type rather than on its source `Type` variant.
    ///
    /// The distinction matters once `SmeltUnknown` joins the rule. Several source
    /// types render as `SmeltUnknown` — `unknown` itself, a concrete union, an
    /// erased type parameter — and the emitter freely assigns a value of one to a
    /// slot spelled by another, because in Rust they are the same type. If the ABI
    /// were decided per source variant, two spellings of one Rust type would
    /// disagree about `&`, and every such assignment would be an E0308: es-toolkit's
    /// `matches` returns `Rc<dyn Fn(<union>) -> bool>` and `dropWhile` binds it to a
    /// parameter declared `(value: unknown) => boolean`. Deciding on the rendered
    /// type keeps one Rust type to one ABI.
    ///
    /// `Type::List` is answered without rendering: a list renders as
    /// `SmeltList<..>` under every substitution, and skipping the render keeps the
    /// common case off the recursive path through `rust_type`.
    pub(super) fn param_type_is_by_shared_reference(&self, param: TypeId) -> bool {
        match self.mir.types.get(param) {
            // A list renders as `SmeltList<..>` under every substitution.
            Some(Type::List(_)) => true,
            // A type parameter is NOT decided here. Whether `T` renders as `T` or
            // erases to `SmeltUnknown` depends on which scope is doing the
            // rendering, and the declaring function and its callers do not share
            // one: `dropRightWhile<T>` declares its callback as `Fn(T, ..)` and
            // renders it by value, while a caller with no `T` in scope would see
            // `SmeltUnknown` and render `&`. The ABI has to be the callee's, so a
            // type parameter keeps the by-value ABI its own declaration gives it.
            // ..except a declaring base's parameter inside an inherited copy,
            // which IS its argument in the receiving class (the layout stores
            // the slot at that type; `classes::effective_class_fields`).
            Some(Type::TypeParam { name }) => self.inherited_bound(*name).is_none_or(|bound| {
                self.with_inherited_bound(|| self.param_type_is_by_shared_reference(bound))
            }),
            // Everything else is decided on the RENDERED Rust type rather than the
            // source variant, because several source spellings share one Rust type.
            // `unknown`, a concrete union and `never` all render `SmeltUnknown`, and
            // the emitter freely assigns a value of one to a slot spelled by
            // another — in Rust they ARE the same type. Deciding per source variant
            // would give one Rust type two ABIs and make every such assignment an
            // E0308.
            _ => {
                let scope = self.current_function_type_params();
                let substitution = TypeSubstitution::lexical(&scope);
                matches!(
                    self.rust_type(param, false, &substitution),
                    Ok(rendered) if rendered.shape() == RustShape::Erased
                )
            }
        }
    }

    /// Wrap one already-rendered callback argument for a by-shared-reference
    /// parameter.
    ///
    /// The argument ladders that build a call from MIR operands
    /// (`indirect_call_args_expr` and friends) know how to borrow a place
    /// directly. The runtime-method lowerings do not go through those ladders —
    /// they hand-write argument text (`left.clone()`, `index as f64`,
    /// `SmeltUnknown::Null`) against a callback whose ABI is decided by
    /// [`Self::callback_param_is_shared_reference`]. This is the one place that
    /// reconciles the two: it adds the `&` those sites owe the parameter, and
    /// nothing else. Rust extends the temporary's lifetime to the end of the
    /// enclosing statement, so `&(expr)` is valid for a value with no name.
    pub(super) fn callback_call_arg_expr(
        &self,
        function: &FunctionType,
        index: usize,
        param: TypeId,
        value_text: String,
    ) -> RustExpr {
        if self.callback_param_is_shared_reference(function, index, param) {
            RustExpr::loose(format!("&({value_text})"))
        } else {
            RustExpr::from(value_text)
        }
    }

    /// Whether parameter `index` of a callback type is passed as a shared `&T`.
    ///
    /// A mutable or rest parameter never is, whatever its type; every other
    /// parameter follows [`Self::param_type_is_by_shared_reference`].
    pub(super) fn callback_param_is_shared_reference(
        &self,
        function: &FunctionType,
        index: usize,
        param: TypeId,
    ) -> bool {
        !self.param_passed_by_mut_ref(function, index)
            && function.rest != Some(index)
            && self.param_type_is_by_shared_reference(param)
    }

    /// Render `arg0: T0, arg1: T1, ..` declarations for one callback shape.
    ///
    /// The caller supplies both axes visible in emitted bytes: which type-parameter
    /// environment the parameter types are lowered in, and whether
    /// `FunctionType::mutable_params` contributes a `&mut ` prefix. Renderers
    /// differ on both, so neither is a choice this helper makes.
    pub(super) fn callback_arg_decls(
        &self,
        function: &FunctionType,
        substitution: &TypeSubstitution<'_>,
        mutability: MutablePrefix,
    ) -> Result<Vec<String>, EmitError> {
        function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                Ok(format!(
                    "smelt_arg{index}: {}",
                    self.callback_param_type(function, index, *param, substitution, mutability)?
                ))
            })
            .collect()
    }

    /// Render one callback parameter type, honoring the `&mut ` axis.
    pub(super) fn callback_param_type(
        &self,
        function: &FunctionType,
        index: usize,
        param: TypeId,
        substitution: &TypeSubstitution<'_>,
        mutability: MutablePrefix,
    ) -> Result<String, EmitError> {
        match mutability {
            MutablePrefix::Apply => self
                .function_type_param(function, index, param, substitution)
                .map(RustType::into_string),
            // `Ignore` drops only the `&mut ` axis. The shared-reference axis is
            // independent of it and must NOT be dropped: an arity-widening adapter
            // rendered here is cast to the very `dyn Fn(..)` that
            // `function_type_param` spelled, so a parameter the type says is
            // `&T` has to be bound as `&T` here too or the cast does not compile.
            MutablePrefix::Ignore => {
                let param_text = self.rust_type(param, false, substitution)?;
                if self.callback_param_is_shared_reference(function, index, param) {
                    Ok(format!("&{param_text}"))
                } else {
                    Ok(param_text.into_string())
                }
            }
        }
    }

    /// Render the Rust return type of a function *value* type.
    ///
    /// This is the single source of truth for the two return-position
    /// refinements a `Type::Function` carries, so every site that renders a
    /// function value (the canonical `Type::Function` arm of
    /// `rust_type`, and the borrowed callback parameter rendering in
    /// `param_type`) agrees on the emitted Rust type:
    ///
    /// * a `Future` return renders as the promise value `SmeltFuture<T>`. A
    ///   synchronous throw from an async function surfaces as a *rejected
    ///   future*, not an outer `Result`, so `may_throw` adds no wrapper here.
    /// * otherwise `may_throw` wraps the return in
    ///   `Result<T, Box<dyn std::error::Error>>`.
    ///
    /// Disagreement between two renderings of the same MIR function type is
    /// what silently turned a recoverable callback throw into a `panic!` at the
    /// parameter boundary, so the logic lives here rather than being repeated.
    pub(super) fn function_value_return_type(
        &self,
        function: &FunctionType,
        substitution: &TypeSubstitution<'_>,
    ) -> Result<RustType, EmitError> {
        if let Some(Type::Future(item)) = self.mir.types.get(function.return_ty) {
            return Ok(RustType::raw(format!(
                "SmeltFuture<{}>",
                self.rust_type(*item, false, substitution)?
            )));
        }
        let inner_return_ty = self
            .rust_type(function.return_ty, false, substitution)?
            .into_string();
        if function.may_throw {
            return Ok(RustType::raw(format!(
                "Result<{inner_return_ty}, {ERROR_BOX}>"
            )));
        }
        Ok(RustType::from(inner_return_ty))
    }

    /// Whether a call to a function VALUE of this type answers a `Result`.
    ///
    /// The mirror of [`Self::function_value_return_type`], and the reason
    /// it exists: a future-returning function reports even a SYNCHRONOUS throw
    /// as a rejected future, so no `Result` wraps its emitted return type and
    /// its call site must not render `?`. Reading `may_throw` directly at a call
    /// site made the two disagree — `let p = boom()?;` on an async arrow that
    /// throws before its first `await`, where `boom()` is a `SmeltFuture<f64>`
    /// and `?` needs `Try` (E0277).
    pub(super) fn function_value_call_is_fallible(&self, function: &FunctionType) -> bool {
        function.may_throw
            && !matches!(
                self.mir.types.get(function.return_ty),
                Some(Type::Future(_))
            )
    }

    /// Convert a function parameter type to Rust.
    ///
    /// Callback parameters are borrowed as reentrant functions so callers can
    /// forward the same callback through multiple helper calls without
    /// consuming it. Generated closure state is stored in shared cells rather
    /// than requiring `FnMut` receiver access.
    pub(super) fn param_type(&self, ty: TypeId) -> Result<RustType, EmitError> {
        if let Some(Type::Function(function)) = self.mir.types.get(ty) {
            // BOTH halves of the callback type render in the lexical scope of
            // the item being defined. Split scopes would emit
            // `Fn(SmeltUnknown) -> T` for a callee whose `T` is in scope.
            //
            // `param_type` is only ever reached from
            // `parameter_decl_type`, and that only from the four signature
            // emitters, where `self` is always the emitter for the item being
            // DEFINED. The scope is therefore the callee's own, which is the
            // correct owner of a `T` inside its declared callback. Call-site
            // adapters are rendered elsewhere, under the call site's bindings.
            let scope = self.current_function_type_params();
            let substitution = TypeSubstitution::lexical(&scope);
            // Both halves go through the canonical `callback_fn_trait_text`,
            // which is also what renders an `F{n}` bound: a borrowed callback
            // parameter must carry the SAME Rust type as the function value
            // bound to it, or a throwing callback's `Result` is dropped at the
            // parameter boundary and its error is forced into a `panic!` by the
            // argument adapter.
            return Ok(RustType::raw(format!(
                "&dyn {}",
                self.callback_fn_trait_text(function, &substitution)?
            )));
        }
        self.signature_type(ty)
    }

    /// Render the `Fn({params}) -> {return}` trait text for one callback shape.
    ///
    /// The single producer of that inner text, shared by the borrowed
    /// `&dyn Fn(..)` parameter rendering in [`Self::param_type`] and by the
    /// `F{n}: Fn(..) + ?Sized` bound declared for a monomorphized callback
    /// parameter. Sharing it is what guarantees the parameter spelling and the
    /// bound spelling cannot drift — a signature whose `&F0` and whose
    /// `F0: Fn(..)` disagreed would either fail to compile or silently re-erase.
    ///
    /// Both halves go through the canonical helpers:
    /// [`Self::function_type_param`] (so `FunctionType::mutable_params`
    /// still contributes its `&mut ` prefix) and
    /// [`Self::function_value_return_type`] (so `may_throw` composes into
    /// `Result<T, Box<dyn std::error::Error>>` and a `Future` return into
    /// `SmeltFuture<T>`, with a generic return type substituted the same way in
    /// both positions).
    pub(super) fn callback_fn_trait_text(
        &self,
        function: &FunctionType,
        substitution: &TypeSubstitution<'_>,
    ) -> Result<String, EmitError> {
        let params = function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| self.function_type_param(function, index, *param, substitution))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let return_ty = self.function_value_return_type(function, substitution)?;
        Ok(format!("Fn({params}) -> {return_ty}"))
    }

    /// The generated `F{n}` type-parameter names this signature declares, paired
    /// with the callback parameter each one types.
    ///
    /// The one place both Increment-2 consumers ask — the `&F{n}` parameter
    /// rendering in [`Self::parameter_decl_type`] and the bound rendering
    /// in [`Self::callback_generic_bounds_text`]. Building both from one list is
    /// what keeps a declared bound and a used name in lockstep.
    ///
    /// Returns nothing in exactly the situations where
    /// [`Self::current_function_type_params`] returns an empty set for a free
    /// function: text destined for a hoisted module-level item may not spell a
    /// type parameter, and a body-cleanliness trial that set
    /// `OwnGenerics::Erased` has already forced the whole signature back to the
    /// erased shape.
    pub(super) fn callback_generic_params(&self) -> Result<Vec<(LocalId, String)>, EmitError> {
        if self.hoisted_module_item.get() || self.role.own_generics_erased() {
            return Ok(Vec::new());
        }
        if !matches!(self.function.origin, HirOrigin::Body(_)) {
            return Ok(Vec::new());
        }
        crate::generic_lifting::callback_generic_params(
            self.mir,
            self.function,
            &self.context.owned_callback_params,
            &self.context.bound_traits,
            &self.context.erased_arguments,
            self.context.names(),
        )
    }

    /// The generated `F{n}` name for one parameter, if it is rendered `&F{n}`.
    pub(super) fn callback_generic_name(
        &self,
        local: LocalId,
    ) -> Result<Option<String>, EmitError> {
        Ok(self
            .callback_generic_params()?
            .into_iter()
            .find(|(param, _)| *param == local)
            .map(|(_, name)| name))
    }

    /// Render the `F{n}: Fn(..) -> R + ?Sized` bounds this signature declares.
    ///
    /// `?Sized` is load-bearing twice over, and neither use is optional:
    /// it lets `F{n}` unify with `dyn Fn(..)` so an existing `&*rc_handle`
    /// argument still binds, and it lets a caller forward its own `&F{n}`
    /// parameter into another generic helper (`helper(&*cb)`), which would
    /// otherwise be an immediate `E0277` because the caller's `F{n}` is itself
    /// not known to be `Sized`.
    ///
    /// The bound carries `Fn(..)` and `?Sized` and nothing else — no marker
    /// traits and no `'static`. `'static` would break the `&*rc_handle`
    /// argument, whose `dyn Fn(..) + 'a` is not `'static`.
    pub(super) fn callback_generic_bounds_text(&self) -> Result<Vec<String>, EmitError> {
        let scope = self.current_function_type_params();
        let substitution = TypeSubstitution::lexical(&scope);
        let mut bounds = Vec::new();
        for (local, name) in self.callback_generic_params()? {
            let ty = self.local_decl(local)?.ty;
            let Some(Type::Function(function)) = self.mir.types.get(ty) else {
                return Err(EmitError::new(
                    "callback generic parameter is not a function type",
                ));
            };
            bounds.push(format!(
                "{name}: {} + ?Sized",
                self.callback_fn_trait_text(function, &substitution)?
            ));
        }
        Ok(bounds)
    }

    /// Convert a concrete function parameter declaration to Rust.
    pub(super) fn parameter_decl_type(&self, local: LocalId) -> Result<RustType, EmitError> {
        let ty = self.local_decl(local)?.ty;
        if self.parameter_needs_mutable_reference(local) {
            return Ok(RustType::raw(format!("&mut {}", self.rendered_type(ty)?)));
        }
        if matches!(self.mir.types.get(ty), Some(Type::Function(_))) {
            if !self.function_parameter_requires_owned(local)? {
                // A direct required borrowed callback parameter of a generic free function
                // is monomorphized as `&F{n}` rather than dispatched dynamically through
                // `&dyn Fn(..)`. `callback_generic_bounds_text` declares the matching bound
                // from the same list.
                if let Some(name) = self.callback_generic_name(local)? {
                    return Ok(RustType::raw(format!("&{name}")));
                }
                return self.param_type(ty);
            }
            return self.rendered_type(ty);
        }
        self.param_type(ty)
    }
}
