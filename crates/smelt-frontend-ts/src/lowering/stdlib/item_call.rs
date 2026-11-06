//! Direct calls of a named function: `f(..)` where `f` resolves to a module item.
//!
//! The decision runs in stages: resolve the item, pick its rest slot and overload,
//! lower the arguments, then settle the return type the call site sees.

use crate::lowering::spread_arguments::CallArg;
use crate::lowering::{ModuleBuilder, RestParam};
use crate::{OverloadSignature, SmeltError};
use oxc::ast::ast::Argument;
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, FunctionType, Item, Literal, Type, TypeParamDef};
use std::collections::HashMap;

/// What a resolved function item declares about its own call surface.
struct ItemCallee {
    /// The resolved function item.
    item: smelt_hir::ItemId,
    /// Declared parameter types, type parameters still raw.
    params: Vec<smelt_hir::TypeId>,
    /// Index of the item's own rest parameter.
    rest: Option<usize>,
    /// Declared count of required parameters, when it differs from `params`.
    required_params: Option<usize>,
    /// The implementation's declared return type.
    return_ty: smelt_hir::TypeId,
    /// Whether the item is declared `async`.
    is_async: bool,
    /// The item's own type parameters.
    type_params: Vec<TypeParamDef>,
}

impl ModuleBuilder<'_> {
    /// Call a bare name: an error constructor, a bound local, or a module item.
    pub(in crate::lowering) fn identifier_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        callee_ident: &oxc::ast::ast::IdentifierReference<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if smelt_stdlib::is_error_class_name(callee_ident.name.as_str()) {
            return self.error_function_call(call, body);
        }
        if self.scope.is_bound(callee_ident.name.as_str()) {
            return self.bound_local_call(call, callee_ident, body);
        }
        let Some(item) = self.items.get(callee_ident.name.as_str()).copied() else {
            return self.unresolved_callee_call(call, callee_ident, body);
        };
        let Item::Function(function) = self.item_ref(item) else {
            return self.non_function_item_call(call, callee_ident, item, body);
        };
        let callee = ItemCallee {
            item,
            params: function
                .params
                .iter()
                .map(|param| param.ty)
                .collect::<Vec<_>>(),
            rest: function.rest,
            required_params: function.required_params,
            return_ty: function.return_ty,
            is_async: function.is_async,
            type_params: self.callee_own_type_params(item),
        };
        self.function_item_call(call, callee_ident, &callee, body)
    }

    /// Call a resolved function item: pick its rest slot and overload, lower the
    /// arguments, then settle the return type the call site sees.
    fn function_item_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        callee_ident: &oxc::ast::ast::IdentifierReference<'_>,
        callee: &ItemCallee,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let name = callee_ident.name.as_str();
        let mut rest = self.declared_rest_param(call, name, callee);
        let selected_overload = self.item_call_overload(call, name, callee, body)?;
        if rest.is_none() {
            rest = self.overload_rest_param(call, callee, selected_overload.as_ref());
        }
        let return_ty = selected_overload
            .as_ref()
            .map_or(callee.return_ty, |signature| signature.return_ty);
        let return_ty =
            self.representable_overload_return_ty(callee.return_ty, return_ty);
        if callee
            .params
            .iter()
            .any(|param| self.concrete_type_requires_never_value(*param))
            && !Self::call_has_spread_arguments(call)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "calls through function types with never parameters are not lowered",
            ));
        }
        let fixed_param_count = rest.map_or(callee.params.len(), |rest| rest.index);
        if rest.is_none() && call.arguments.len() > fixed_param_count {
            for arg in call.arguments.iter().skip(fixed_param_count) {
                let _ = self.argument(arg, body)?;
            }
        }
        let function_ty = FunctionType {
            params: callee.params.clone(),
            rest: rest.map(|rest| rest.index),
            required_params: callee.required_params,
            mutable_params: Vec::new(),
            return_ty: callee.return_ty,
            is_async: callee.is_async,
            may_throw: false,
        };
        let args = if rest.is_some() && call.arguments.iter().any(Argument::is_spread) {
            self.spread_closure_call_arguments(&function_ty, rest, call, body)?
        } else {
            self.positional_item_arguments(call, callee, rest, selected_overload.as_ref(), body)?
        };
        // A single-signature generic function (`function shuffle<T>(arr:
        // readonly T[]): T[]`) has no overload entry, so its declared
        // return still carries the raw `TypeParam`. Instantiate it from
        // the lowered argument types the same way overload selection
        // does, so `shuffle([1, 2]).sort()` sees `number[]`, not `T[]`.
        let return_ty = if selected_overload.is_none() {
            self.instantiated_call_return(&callee.type_params, &callee.params, &args, return_ty, call, body)?
        } else {
            return_ty
        };
        let return_ty = self.unbound_overload_return_ty(callee, return_ty, &args, call.span, body);
        let callee = body.push_expr(Expr {
            kind: ExprKind::Item(callee.item),
            ty: self.ctx.krate.types.intern(Type::Function(function_ty)),
            span: self.span(callee_ident.span.start, callee_ident.span.end),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::Call { callee, args },
            ty: return_ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }

    /// The rest parameter a call of this item packs into, from the item itself or,
    /// failing that, from the name-keyed table.
    fn declared_rest_param(
        &self,
        call: &oxc::ast::ast::CallExpression<'_>,
        name: &str,
        callee: &ItemCallee,
    ) -> Option<RestParam> {
        let params = &callee.params;
        let item_rest = callee.rest;
        // The concretely resolved `item` is the source of truth for a rest
        // parameter. The name-keyed `function_rests` map is a fallback for
        // callees whose resolved item does not itself carry a rest index
        // (re-exported stubs, arrow bindings), but keying it on the bare
        // function name lets a rest slot from a *different* same-named
        // function in another module leak in — e.g. compat's
        // `delay(fn, wait, ...args)` contaminating promise `delay(ms,
        // options)`, synthesizing a spurious empty rest list for the
        // omitted `options`. So derive the rest from this item first, and
        // consult `function_rests` only when it names a valid rest
        // position within *this* item's own parameters.
        let mut rest = if let Some(index) = item_rest
            && let Some(Type::List(item_ty)) = params
                .get(index)
                .and_then(|param| self.ctx.krate.types.get(*param))
        {
            Some(RestParam {
                index,
                item_ty: *item_ty,
            })
        } else {
            None
        };
        // The resolved item has no rest slot of its own, so a name-keyed
        // rest is only credible when this call cannot be explained by the
        // item's fixed arity: a non-variadic function cannot absorb more
        // arguments than it has parameters. Without that condition a
        // same-named variadic export elsewhere (compat's `omit(object,
        // ...paths)`) is adopted for a fixed-arity callee whose matching
        // slot merely happens to be a list (`omit(obj, keys: K[])`), and
        // the array argument is packed into a spurious one-element list.
        if rest.is_none()
            && call.arguments.len() > params.len()
            && let Some(candidate) =
                self.functions.rest(name)
            && candidate.index < params.len()
            && matches!(
                params
                    .get(candidate.index)
                    .and_then(|param| self.ctx.krate.types.get(*param)),
                Some(Type::List(_))
            )
        {
            rest = Some(candidate);
        }
        rest
    }

    /// The overload a call of this item selects, if any.
    fn item_call_overload(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        name: &str,
        callee: &ItemCallee,
        body: &mut Body,
    ) -> Result<Option<OverloadSignature>, SmeltError> {
        match self.selected_overload_signature(
            name,
            &call.arguments,
            call.span,
            body,
        ) {
            Ok(selected) => Ok(selected),
            // A sibling in the same library may call the *implementation*
            // signature directly (e.g. es-toolkit's `sortedLastIndexBy`
            // passing the private `retHighest` flag that no public
            // overload declares). When the implementation's own arity
            // accepts the call, lower against the implementation types
            // instead of aborting on the overload table.
            Err(error) => {
                let required = callee.required_params.unwrap_or(callee.params.len());
                let arity_fits = call.arguments.len() >= required
                    && (callee.rest.is_some() || call.arguments.len() <= callee.params.len());
                if arity_fits {
                    Ok(None)
                } else {
                    Err(error)
                }
            }
        }
    }

    /// A rest slot adopted from the selected overload.
    ///
    /// Adopt a rest slot from the selected overload only when it names a
    /// valid rest position within *this* resolved item's own parameters.
    /// `selected_overload` is keyed by the bare callee name, so a
    /// same-named export in another module (compat's variadic
    /// `merge(object, ...sources)`) can be picked even when the explicit
    /// relative import resolved to a different, fixed-arity function
    /// (object's `merge(target, source)`). Without this guard the
    /// compat rest leaks in and packs the trailing positional argument
    /// into a spurious array. Mirrors the `function_rests` guard above:
    /// the concretely resolved item is the source of truth for whether a
    /// parameter slot is variadic. The same arity condition applies: a
    /// call the item's own fixed arity already accounts for needs no
    /// rest, so adopting one here can only mis-pack an argument.
    fn overload_rest_param(
        &self,
        call: &oxc::ast::ast::CallExpression<'_>,
        callee: &ItemCallee,
        selected_overload: Option<&OverloadSignature>,
    ) -> Option<RestParam> {
        let params = &callee.params;
        if call.arguments.len() > params.len()
            && let Some(signature) = selected_overload
            && let Some(index) = signature.rest
            && let Some(Type::List(item_ty)) = signature
                .params
                .get(index)
                .and_then(|param| self.ctx.krate.types.get(*param))
            && index < params.len()
            && matches!(
                params
                    .get(index)
                    .and_then(|param| self.ctx.krate.types.get(*param)),
                Some(Type::List(_))
            )
        {
            return Some(RestParam {
                index,
                item_ty: *item_ty,
            });
        }
        None
    }

    /// Lower a call's arguments against the item's parameters by position.
    fn positional_item_arguments(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        callee: &ItemCallee,
        rest: Option<RestParam>,
        selected_overload: Option<&OverloadSignature>,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let params = &callee.params;
        let callee_type_params = &callee.type_params;
        let fixed_param_count = rest.map_or(params.len(), |rest| rest.index);
        // A generic callee's declared parameter types still carry raw
        // `TypeParam`s, so handing one to a function *literal* argument
        // contextually types its parameters with a type variable that is
        // not in the closure's own scope — which lowers to `SmeltUnknown`
        // and then forces the concrete sibling arguments to be erased to
        // match. `mapArr(nums, x => x * 2)` with `nums: number[]` erased
        // the closure, re-erased an already concrete `SmeltList<f64>` to
        // call the instantiation, and un-erased the result: four erasure
        // sites from one missing annotation.
        //
        // The instantiation is recoverable from the *other* arguments,
        // and the machinery already exists below for the return type. So
        // function-literal arguments are lowered in a second pass, with
        // their hint substituted from the bindings the value arguments
        // imply.
        //
        // Only syntactic function literals are deferred. Constructing a
        // closure cannot observe or mutate state, so moving it after its
        // siblings preserves JavaScript's left-to-right argument
        // evaluation order; any other argument keeps its source position.
        let defer_literal_callbacks = params
            .iter()
            .any(|param| self.overload_constraint_contains_unresolved_type_param(*param));
        let hint_at = |builder: &mut Self, index: usize, arg: &Argument<'_>| {
            let implementation_hint = params.get(index).copied();
            if let Some(bound) = implementation_hint.and_then(|hint| {
                builder.object_literal_bound_hint(hint, callee_type_params, arg)
            }) {
                return Some(bound);
            }
            if matches!(arg, Argument::RegExpLiteral(_)) {
                selected_overload
                    .and_then(|signature| signature.params.get(index).copied())
                    .or(implementation_hint)
            } else {
                implementation_hint
            }
        };
        // Spreads are expanded before the positional walk: a spread of
        // a tuple contributes one argument PER ELEMENT, so the mapping
        // from source arguments to parameters is only positional after
        // expansion. See `lowering::spread_arguments`.
        let expanded = self.expanded_call_arguments(&call.arguments, body)?;
        let fixed_arguments = expanded
            .iter()
            .copied()
            .take(fixed_param_count)
            .collect::<Vec<_>>();
        let mut lowered: Vec<Option<smelt_hir::ExprId>> = vec![None; fixed_arguments.len()];
        let mut deferred: Vec<usize> = Vec::new();
        for (index, arg) in fixed_arguments.iter().enumerate() {
            let CallArg::Source(source) = arg else {
                // An expanded spread element is already lowered, and it
                // is a tuple field read: never a literal callback, so it
                // cannot be deferred and needs no hint.
                lowered[index] = Some(self.lower_call_arg(*arg, None, body)?);
                continue;
            };
            if defer_literal_callbacks
                && matches!(
                    source,
                    Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_)
                )
            {
                deferred.push(index);
                continue;
            }
            let hint = hint_at(self, index, source);
            lowered[index] = Some(self.lower_call_arg(*arg, hint, body)?);
        }
        if !deferred.is_empty() {
            let substitutions =
                self.deferred_callback_bindings(call, callee, &lowered, body);
            for index in deferred {
                let arg = fixed_arguments[index];
                // Only a `Source` entry is ever deferred (the loop above
                // defers literal callbacks, which an expanded spread
                // element cannot be), so the hint still has source to
                // apply to.
                let CallArg::Source(source) = arg else {
                    lowered[index] = Some(self.lower_call_arg(arg, None, body)?);
                    continue;
                };
                let hint = hint_at(self, index, source).map(|hint| {
                    if substitutions.is_empty() {
                        hint
                    } else {
                        self.substitute_type_params(hint, &substitutions)
                    }
                });
                lowered[index] = Some(self.lower_call_arg(arg, hint, body)?);
            }
        }
        let mut args = lowered.into_iter().flatten().collect::<Vec<_>>();
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
        Ok(args)
    }

    /// Type bindings the already-lowered value arguments imply, for hinting the
    /// function literals deferred behind them.
    fn deferred_callback_bindings(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        callee: &ItemCallee,
        lowered: &[Option<smelt_hir::ExprId>],
        body: &Body,
    ) -> HashMap<smelt_hir::Symbol, smelt_hir::TypeId> {
        let params = &callee.params;
        let callee_type_params = &callee.type_params;
        let mut substitutions = HashMap::new();
        match self.explicit_call_type_bindings(callee_type_params, call) {
            Some(explicit) => substitutions = explicit,
            None => {
                for (index, lowered_arg) in lowered.iter().enumerate() {
                    let (Some(param), Some(lowered_arg)) =
                        (params.get(index).copied(), *lowered_arg)
                    else {
                        continue;
                    };
                    let arg_ty = Self::expr_ty(body, lowered_arg);
                    let _ = self.infer_overload_type(param, arg_ty, &mut substitutions);
                }
            }
        }
        substitutions
    }

    /// The return of a call whose overload promises a concrete type over an
    /// implementation type parameter.
    ///
    /// An overload may promise a concrete return where the
    /// IMPLEMENTATION returns a type parameter no argument bound.
    /// es-toolkit `reduceAsync` is the shape: the implementation is
    /// `<T, U>(array, reducer, initialValue?: U): Promise<U>`, and a
    /// two-argument call supplies nothing that fixes `U` (the erased
    /// `vi.fn` reducer included), so the lowered body hands back a
    /// runtime-tagged value -- for an empty array, `undefined`.
    /// Asserting the overload's `Promise<number>` over that makes the
    /// emitter insert a JS `ToNumber` whose nullish arm MANUFACTURES a
    /// value (`undefined` becomes `f64::NAN`) and then fold
    /// `result === undefined` to `false`: the assertion the source
    /// wrote can never hold. The unbound parameter is the evidence that
    /// the concrete claim has no basis, so the erased return wins --
    /// the same reasoning as `renders_as_erased_function` above.
    fn unbound_overload_return_ty(
        &mut self,
        callee: &ItemCallee,
        return_ty: smelt_hir::TypeId,
        args: &[smelt_hir::ExprId],
        span: oxc::span::Span,
        body: &Body,
    ) -> smelt_hir::TypeId {
        let params = &callee.params;
        let callee_type_params = &callee.type_params;
        let implementation_return_ty = callee.return_ty;
        match self.return_leaf_type_param(implementation_return_ty) {
            Some(name) if return_ty != implementation_return_ty => {
                let mut substitutions = HashMap::new();
                for (param, arg) in params.iter().zip(args) {
                    let arg_ty = Self::expr_ty(body, *arg);
                    let _ = self.infer_callee_type_bindings(
                        callee_type_params,
                        *param,
                        arg_ty,
                        &mut substitutions,
                    );
                }
                // An assertion around the call is evidence too, as for
                // the single-signature instantiation above.
                self.bind_from_asserted_return(
                    callee_type_params,
                    implementation_return_ty,
                    span,
                    &mut substitutions,
                );
                if substitutions.contains_key(&name) {
                    return_ty
                } else {
                    // Still instantiate the implementation's unbound
                    // parameters (this one included) to `unknown`, so the
                    // erased return never leaks a raw callee `TypeParam`.
                    self.complete_callee_type_substitution(
                        callee_type_params,
                        &mut substitutions,
                    );
                    self.substitute_type_params(implementation_return_ty, &substitutions)
                }
            }
            _ => return_ty,
        }
    }

    /// Call a name that is not a function item: an import, or a const holding a
    /// callable, whose value is read and called.
    fn non_function_item_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        callee_ident: &oxc::ast::ast::IdentifierReference<'_>,
        item: smelt_hir::ItemId,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if self.imports.is_value(callee_ident.name.as_str()) {
            for arg in &call.arguments {
                let _ = self.argument(arg, body)?;
            }
            let ty = self.erase(ErasureReason::UntypedPackage);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        let item_ty = self
            .item_expr_type(
                item,
                self.span(callee_ident.span.start, callee_ident.span.end),
            )
            // not a new erasure: a non-function falls through to the
            // recorded `ByName` site below
            .unwrap_or_else(|_| self.unknown_type());
        if let Some(Type::Function(function)) =
            self.ctx.krate.types.get(item_ty).cloned()
        {
            // Read the item's VALUE as the callee. This used to push
            // `Literal::None` typed as the function — a fabricated
            // NULL callee — so `export const alias = original;
            // alias(s)` compiled and answered a default-constructed
            // value instead of calling anything. `identifier_expression`
            // is the general "read this name" path and inlines a
            // module const's initializer, so it produces the real
            // callable for a const aliasing a function, an arrow, or
            // a builtin global.
            let callee = self.identifier_expression(
                callee_ident.name.as_str(),
                callee_ident.span.start,
                callee_ident.span.end,
                body,
            )?;
            let args = call
                .arguments
                .iter()
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: function.return_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        for arg in &call.arguments {
            let _ = self.argument(arg, body)?;
        }
        let ty = self.erase(ErasureReason::ByName);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }
}
