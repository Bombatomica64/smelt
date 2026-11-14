//! Calls of a callable value held in a local, and the receiver types that make a
//! call dynamic.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::erasure::NotNewErasure;
use oxc::ast::ast::{Argument, Expression};
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, FunctionType, Literal, Type};

impl ModuleBuilder<'_> {
    /// Lower a call whose callee is a local closure or function-typed local.
    pub(in crate::lowering) fn local_callable_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee_ident) = &call.callee else {
            return Ok(None);
        };
        if self.items.contains_key(callee_ident.name.as_str())
            && !self.scope.is_bound(callee_ident.name.as_str())
        {
            return Ok(None);
        }
        let callee = self.identifier_expression(
            callee_ident.name.as_str(),
            callee_ident.span.start,
            callee_ident.span.end,
            body,
        )?;
        let callee_ty = Self::expr_ty(body, callee);
        // A `typeof callee === "function"` guard narrows the *expression* type to
        // a concrete function, but the backing local keeps its declared type. When
        // that declared type is a dynamic surface (union/unknown/type parameter/
        // erased callable), codegen still dispatches the call through the runtime
        // `SmeltUnknown` ABI. Track the declared type so a spread call is packed
        // into a single flattened argument vector even after narrowing hides the
        // dynamic surface behind a function type.
        let callee_dispatches_dynamically = self.type_is_dynamic_call_surface(callee_ty)
            || self
                .scope
                .lookup(callee_ident.name.as_str())
                .is_some_and(|local| {
                    self.type_is_dynamic_call_surface(Self::local_ty(body, local))
                });
        let optional_function_ty = match self.ctx.krate.types.get(callee_ty) {
            Some(Type::Optional(inner))
                if call.optional
                    && matches!(self.ctx.krate.types.get(*inner), Some(Type::Function(_))) =>
            {
                Some(*inner)
            }
            _ => None,
        };
        let probed_arg_tys = self.probe_argument_types(&call.arguments, body);
        let function_member_ty = optional_function_ty.or_else(|| {
            self.function_member_type_for_args(
                callee_ty,
                Some(call.arguments.len()),
                &probed_arg_tys,
            )
        });
        let (function_ty, function, optional_call) = match function_member_ty {
            Some(function_ty) => {
                let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned()
                else {
                    return Ok(None);
                };
                (function_ty, function, optional_function_ty.is_some())
            }
            _ if matches!(
                self.ctx.krate.types.get(callee_ty),
                Some(Type::Unknown | Type::TypeParam { .. })
            ) || self.is_callable_object_erased_class(callee_ty) =>
            {
                let ty = if self.is_callable_object_erased_class(callee_ty) {
                    self.erase(ErasureReason::UnmodeledType)
                } else {
                    // `unknown` propagates without a record; a type-parameter
                    // callee's result is resolved by name at runtime.
                    self.erase_unless_unknown(callee_ty, ErasureReason::ByName)
                };
                let callee = if self.is_callable_object_erased_class(callee_ty) {
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: callee },
                        ty,
                        span: self.span(callee_ident.span.start, callee_ident.span.end),
                    })
                } else {
                    self.stored_erased_callee(callee, body)
                };
                // Dynamically dispatched callees (unknown/type-parameter/erased
                // callable surfaces) are invoked through the runtime
                // `SmeltUnknown` call ABI, which flattens the entire argument
                // list. A spread call must therefore pack its flattened
                // arguments into a single runtime list operand and lower as
                // `ClosureCallSpread`. Emitting a dynamic `ClosureCall` here
                // would lose the spread, leaving the call-site flatten
                // heuristic to misread the individual operands.
                if Self::call_has_spread_arguments(call) {
                    let args = self.packed_spread_call_arguments(ty, call, body)?;
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::ClosureCallSpread { callee, args },
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                }
                let args = call
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall { callee, args },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            _ => {
                return Ok(None);
            }
        };
        // For an optional call `f?.(..)` keep the callee at its declared
        // `Option<Function>` type. Asserting it down to the bare function type
        // would coerce an absent callee into a null-returning default callback
        // that is then called unconditionally, so `f?.(..)` would wrongly yield
        // `null` instead of `undefined`. The `Option<Function>` callee lets the
        // emitter render the short-circuiting `callee.map(|f| f(..))`.
        let callee = if optional_call || callee_ty == function_ty {
            callee
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: function_ty,
                span: self.span(callee_ident.span.start, callee_ident.span.end),
            })
        };
        let call_return_ty = if optional_call {
            self.optional_chain_result_type(function.return_ty)
        } else {
            function.return_ty
        };
        let supplied_arg_count = call.arguments.len();
        // Dynamically dispatched callees (union/unknown/type-parameter/erased
        // callable surfaces) are invoked through the runtime `SmeltUnknown` call
        // ABI, which flattens the entire argument list. A spread must therefore
        // be packed into a single runtime argument vector even when the callee
        // carries local-callback metadata: the typed-shape `[fixed.., rest_list]`
        // lowering below is only correct for a concrete function whose Rust rest
        // parameter absorbs the trailing list, not for a dynamic dispatch that
        // would otherwise wrap the rest list in a single extra `Array` argument.
        if Self::call_has_spread_arguments(call) && callee_dispatches_dynamically {
            // not a new erasure: names the tagged item type of the runtime
            // call ABI's packed argument list; the call keeps its typed result
            let unknown_ty = self.unknown_type(NotNewErasure::ErasedUpstream);
            let packed = self.packed_spread_call_arguments(unknown_ty, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread {
                    callee,
                    args: packed,
                },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let variadic_item_ty = function.params.first().and_then(|param| {
            if function.rest.is_some() {
                return None;
            }
            if function.params.len() != 1 {
                return None;
            }
            match self.ctx.krate.types.get(*param) {
                Some(Type::List(item_ty)) => Some(*item_ty),
                Some(Type::Never) => {
                    Some(self.erase_with(ErasureReason::TypeLevel, "never parameter"))
                }
                _ => None,
            }
        });
        if Self::call_has_spread_arguments(call)
            && let Some(item_ty) = variadic_item_ty
        {
            let packed = self.packed_spread_call_arguments(item_ty, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread {
                    callee,
                    args: packed,
                },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let rest = self.list_rest_param(&function);
        let fixed_param_count = rest.map_or(function.params.len(), |rest| rest.index);
        if rest.is_some() && Self::call_has_spread_arguments(call) {
            let args = self.spread_closure_call_arguments(&function, rest, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        // Spreads are resolved BEFORE the arity checks below, because
        // `supplied_arg_count` counts a spread as a single argument: a
        // `sink(...triple)` looked like one argument for three parameters and
        // took the shortfall branch, which returned an EMPTY argument list that
        // the emitter then padded with `default_value(..)` per parameter. So
        // `sink(...triple)` became `sink(String::new(), String::new(), 0.0)` --
        // every argument a default, the tuple never read, and it COMPILED. A
        // silent wrong answer is the worst outcome available here.
        //
        // A tuple spread has a statically known width, so it distributes
        // positionally and the call's real arity is known after expansion.
        // Anything else has no static arity and is a named blocker rather than
        // a guess.
        if rest.is_none() && call.arguments.iter().any(Argument::is_spread) {
            let expanded = self.expanded_call_arguments(&call.arguments, body)?;
            let fixed_params = function.params.get(..fixed_param_count).unwrap_or_default();
            let expanded = self.distribute_trailing_list_spread(expanded, fixed_params, body)?;
            let mut args = Vec::with_capacity(fixed_param_count);
            for argument in expanded.into_iter().take(fixed_param_count) {
                args.push(self.lower_call_arg(argument, None, body)?);
            }
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        // A parameter is omittable when its declaration says so
        // (`required_params`, optional and defaulted parameters) or when its
        // type can hold the absence: a closure's own `required_params` counts
        // its syntax, so a contextually typed `(d, o) => ..` against
        // `(v: number, o?: string) => string` counts `o` as required while the
        // slot is `Option`. Omitted ones are padded with `undefined` below.
        let declared_required =
            fixed_param_count.min(function.required_params.unwrap_or(fixed_param_count));
        let omittable = |this: &Self, index: usize| {
            index >= declared_required
                || function.params.get(index).is_some_and(|param| {
                    matches!(this.ctx.krate.types.get(*param), Some(Type::Optional(_)))
                })
        };
        let required_arg_count = (0..fixed_param_count)
            .rev()
            .find(|index| !omittable(self, *index))
            .map_or(0, |index| index + 1);
        if supplied_arg_count < required_arg_count
            || (rest.is_none() && supplied_arg_count > function.params.len())
        {
            if rest.is_none() {
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall {
                        callee,
                        args: Vec::new(),
                    },
                    ty: call_return_ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "closure call argument count does not match closure parameters",
            ));
        }

        // Carry the callee's parameter type into each argument, the way the
        // named-function and `new`-expression paths already do
        // (`new_expr.rs`'s `params.get(index)`, and the IIFE path above). This
        // loop lowered its arguments with NO hint, so an object literal written
        // inline against a closure parameter -- `showRequired({ status: 8 })`
        // where `showRequired` is a const-bound arrow taking `Opts` -- was
        // built as a bare record of erased values and only then converted to
        // the struct, erasing every value on the way in. The parameter type is
        // right here in `function.params`; a hand-written Rust port would
        // construct `Opts { .. }` directly, so the hint has to reach the
        // literal.
        let mut args = call
            .arguments
            .iter()
            .take(fixed_param_count)
            .enumerate()
            .map(|(index, arg)| {
                let hint = function
                    .params
                    .get(index)
                    .copied()
                    .filter(|param| self.hint_preserves_argument_shape(*param));
                self.argument_with_hint(arg, body, hint)
            })
            .collect::<Result<Vec<_>, _>>()?;
        for index in supplied_arg_count..fixed_param_count {
            // An omitted optional parameter receives `undefined`.
            let Some(param_ty) = function
                .params
                .get(index)
                .copied()
                .filter(|_| omittable(self, index))
            else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "closure call argument count does not match closure parameters",
                ));
            };
            args.push(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: param_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        if let Some(rest) = rest {
            let rest_args = call
                .arguments
                .iter()
                .skip(rest.index)
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            let rest_ty = self.ctx.krate.types.intern(Type::List(rest.item_ty));
            args.push(body.push_expr(Expr {
                kind: ExprKind::ListLit(rest_args),
                ty: rest_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: call_return_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Whether a parameter type is worth hinting an argument with, i.e. whether
    /// it PRESERVES the argument's own shape rather than erasing it.
    ///
    /// A hint makes the argument lower AT the hinted type. That is what turns
    /// an inline object literal into its struct, but it cuts the other way when
    /// the parameter itself is an erased surface: hinting `unknown[]` at
    /// `spreadFn([1, 2])` builds `Vec<SmeltUnknown>` directly, erasing each
    /// element at construction, where the unhinted lowering builds `Vec<f64>`
    /// and erases at the call through the boundary adapter the emitter already
    /// generates. Same runtime value, but the erasure moves from an explicit
    /// boundary into ordinary data flow, which is what the `SmeltUnknown`
    /// policy forbids — measured as +14 avoidable erasure in es-toolkit's
    /// `spread.spec.ts` when the hint was passed unconditionally.
    ///
    /// So an `unknown`, or a container whose elements/values are `unknown`, is
    /// not a shape to lower into. Peels `Optional` first, because
    /// `Optional<Opts>` is a real hint (fixture 66 covers it) while
    /// `Optional<unknown>` is not.
    pub(in crate::lowering) fn hint_preserves_argument_shape(&self, ty: smelt_hir::TypeId) -> bool {
        let peeled = match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => ty,
        };
        match self.ctx.krate.types.get(peeled) {
            Some(Type::Unknown) => false,
            Some(Type::List(item) | Type::Set(item)) => {
                !matches!(self.ctx.krate.types.get(*item), Some(Type::Unknown))
            }
            Some(Type::Dict(_, value) | Type::JsMap(_, value)) => {
                !matches!(self.ctx.krate.types.get(*value), Some(Type::Unknown))
            }
            _ => true,
        }
    }

    /// Return whether a local's structural class type is an erased callable object.
    pub(in crate::lowering) fn is_callable_object_erased_class(
        &self,
        ty: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Class { name, .. }) => self.types.is_callable_object_alias(*name),
            _ => false,
        }
    }

    /// Return whether a callee of this type is invoked through the runtime
    /// `SmeltUnknown` call ABI rather than a concrete Rust function.
    ///
    /// Such callees (unknown, type parameters, unions, and erased callable
    /// objects) flatten their entire argument list at the call site, so spread
    /// arguments must be packed into a single runtime vector rather than passed
    /// as a trailing rest list.
    pub(in crate::lowering) fn type_is_dynamic_call_surface(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_callable_object_erased_class(ty)
    }

    /// Return whether a `Type::Class` value erases to `SmeltUnknown` at codegen.
    ///
    /// This mirrors the Rust emitter's `is_erased_class_type` rule: a class-shaped
    /// type is emitted as `SmeltUnknown` when its name resolves to neither a
    /// declared class nor a declared interface. Generic structural type aliases
    /// such as `Funnel<Args>` (a `type` literal, not an `interface`) therefore
    /// erase, while `RegExp` keeps its dedicated runtime type. Keeping the
    /// predicate aligned with codegen lets the lowering decide, in the frontend,
    /// whether a receiver value will be a runtime `SmeltUnknown` at the call site.
    pub(in crate::lowering) fn class_type_erases_to_unknown(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty) else {
            return false;
        };
        let Some(type_name) = self.ctx.krate.symbols.get(*name) else {
            return false;
        };
        if Self::is_ts_stdlib_class_name(type_name, smelt_stdlib::StdlibClass::RegExp) {
            return false;
        }
        !self.classes.contains(type_name) && !self.interfaces.contains(type_name)
    }

    /// Return whether a value of this type dispatches member calls through the
    /// runtime `SmeltUnknown` ABI rather than as a typed Rust value.
    ///
    /// A receiver dispatches dynamically when it is itself `unknown`, a type
    /// parameter, a union, or a class-shaped type that erases to `SmeltUnknown`
    /// (see [`Self::class_type_erases_to_unknown`]). Optional receivers unwrap to
    /// their inner type first so `Funnel<Args> | undefined` is recognized too.
    pub(in crate::lowering) fn receiver_type_dispatches_dynamically(
        &self,
        ty: smelt_hir::TypeId,
    ) -> bool {
        let inner = match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => ty,
        };
        matches!(
            self.ctx.krate.types.get(inner),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.class_type_erases_to_unknown(inner)
    }

    /// Return whether the receiving object of a static member call erases to
    /// `SmeltUnknown` at codegen.
    ///
    /// The receiver is lowered to recover its type; the produced expression is
    /// otherwise unused here because the member-read callee already lowered the
    /// receiver. When the receiver erases, the member read yields a runtime
    /// `SmeltUnknown`, so a spread call through it must be packed as a
    /// `ClosureCallSpread` regardless of the field's declared function type.
    pub(in crate::lowering) fn static_member_receiver_dispatches_dynamically(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> bool {
        let Ok(receiver) = self.expression(&member.object, body) else {
            return false;
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        self.receiver_type_dispatches_dynamically(receiver_ty)
    }

    /// Lower call arguments for a statically typed, possibly under-applied
    /// closure call, padding missing optional parameters with typed `None`.
    ///
    /// A curried or partially applied call site may supply fewer arguments than
    /// the selected [`FunctionType`] declares. When the omitted parameters are
    /// optional (`Optional<T>` ABI) or absorbed by a trailing rest slot, the
    /// call is still statically typed: rather than erasing the callee, this
    /// synthesizes a `None` literal for each missing fixed parameter (typed with
    /// that parameter's declared `Optional<T>`) and an empty `List` for an
    /// unfilled rest slot. Supplied arguments beyond the fixed parameters are
    /// packed into the rest list, or evaluated and dropped when no rest slot
    /// exists, matching JavaScript's excess-argument semantics.
    pub(in crate::lowering) fn typed_under_applied_closure_arguments(
        &mut self,
        function: &FunctionType,
        arguments: &[Argument<'_>],
        call_span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let span = self.span(call_span.start, call_span.end);
        // Fixed parameters are those before a rest slot; a rest signature keeps
        // its trailing `List` parameter out of the fixed range.
        let fixed_param_count = function.rest.unwrap_or(function.params.len());
        // Lower every supplied argument FIRST, expanding a tuple spread into one
        // argument per element. The loop below is positional -- it pairs
        // argument `index` with parameter `index` -- so a spread has to have
        // become its elements before it runs. Treating the spread as a single
        // argument is what made `sink(...triple)` pair the whole tuple with
        // parameter 0 and synthesize typed `None` for the other two.
        let expanded = self.expanded_call_arguments(arguments, body)?;
        let mut supplied = Vec::with_capacity(expanded.len());
        for argument in expanded {
            supplied.push(self.lower_call_arg(argument, None, body)?);
        }
        let mut args = Vec::with_capacity(function.params.len());
        for index in 0..fixed_param_count {
            if let Some(argument) = supplied.get(index).copied() {
                args.push(argument);
            } else {
                // Missing optional parameter: synthesize a typed `None` carrying
                // the parameter's declared type so the closure call stays typed.
                let param_ty = function
                    .params
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| self.erase(ErasureReason::Unclassified));
                args.push(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: param_ty,
                    span,
                }));
            }
        }
        if let Some(rest_index) = function.rest {
            let rest_args = supplied.get(rest_index..).unwrap_or_default().to_vec();
            let rest_param_ty = function.params.get(rest_index).copied().unwrap_or_else(|| {
                let item = self.erase(ErasureReason::Unclassified);
                self.ctx.krate.types.intern(Type::List(item))
            });
            args.push(body.push_expr(Expr {
                kind: ExprKind::ListLit(rest_args),
                ty: rest_param_ty,
                span,
            }));
        } else {
            // No rest slot: surplus arguments are evaluated for their side
            // effects and discarded, exactly as JavaScript ignores extra call
            // arguments.
            for argument in arguments.iter().skip(fixed_param_count) {
                let _ = self.argument(argument, body)?;
            }
        }
        Ok(args)
    }

    /// Fold Remeda `sliceString(start, end?)(data)` into a string slice.
    pub(in crate::lowering) fn slice_string_nested_call(
        &mut self,
        callee_call: &oxc::ast::ast::CallExpression<'_>,
        outer_call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &callee_call.callee else {
            return Ok(None);
        };
        if callee.name != "sliceString" {
            return Ok(None);
        }
        let [data_arg] = outer_call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(outer_call.span.start, outer_call.span.end),
                "sliceString data-last call requires one data argument",
            ));
        };
        let (start_arg, end_arg) = match callee_call.arguments.as_slice() {
            [start] => (start, None),
            [start, end] => (start, Some(end)),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(callee_call.span.start, callee_call.span.end),
                    "sliceString data-last call requires start and optional end",
                ));
            }
        };
        let operand = self.argument(data_arg, body)?;
        let start = Some(self.slice_index_argument(start_arg, body)?);
        let end = end_arg
            .map(|argument| self.slice_index_argument(argument, body))
            .transpose()?;
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSlice {
                operand,
                start,
                end,
                kind: smelt_hir::StringSliceKind::Slice,
            },
            ty,
            span: self.span(outer_call.span.start, outer_call.span.end),
        })))
    }
}
