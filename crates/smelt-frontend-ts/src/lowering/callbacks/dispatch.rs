//! Callback-argument dispatch: the entry points that turn a source argument
//! into a closure value. An inline literal lowers through its closure body; a
//! name or a callable value is wrapped in a forwarding closure.

use crate::lowering::erasure::NotNewErasure;
use super::forwarding::ForwardingCallback;
use crate::lowering::{
    Argument, Body, ClosureCallback, ExprKind, Expression, Item, ModuleBuilder, SmeltError, Type,
};
use oxc::span::GetSpan;
use smelt_hir::ErasureReason;
use smelt_stdlib::DiagnosticCategory;

impl ModuleBuilder<'_> {
    /// Lower a callback argument that NAMES its callable: a local, an item, a
    /// recognized global builtin, an imported predicate, or an inlined local
    /// callback literal.
    ///
    /// Extracted from [`Self::callback_argument`] so the identical ladder can
    /// serve a name written under a type assertion. It takes the name and span
    /// rather than the AST node for exactly that reason: `xs.filter(Boolean as
    /// any)` and `xs.filter(Boolean)` must select the same callback, and only
    /// the second reached this ladder before.
    fn callback_identifier_argument(
        &mut self,
        name: &str,
        span_start: u32,
        span_end: u32,
        expected_param_tys: &[smelt_hir::TypeId],
        context: &'static str,
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        if let Some(local) = self.scope.lookup(name) {
            let local_ty = Self::local_ty(body, local);
            if let Some(Type::Function(function)) = self.ctx.krate.types.get(local_ty).cloned()
            {
                let expr = self.identifier_expression(
                    name,
                    span_start,
                    span_end,
                    body,
                )?;
                return Ok(ClosureCallback {
                    expr,
                    return_ty: function.return_ty,
                });
            }
        }
        if let Some(item) = self.items.get(name).copied() {
            let span = self.span(span_start, span_end);
            let Item::Function(function) = self.item_ref(item) else {
                return Err(SmeltError::unsupported(
                    span,
                    format!("{context} callback item `{name}` is not a function"),
                ));
            };
            // JavaScript adapts callback arity at the call site: an item
            // declaring fewer parameters than the receiver supplies (down
            // to zero, e.g. `values.map(stubTrue)`) ignores the extra
            // arguments, and one declaring more (e.g. `xs.map(orderBy)`
            // with a four-parameter `orderBy`) receives `undefined` for the
            // unsupplied optional tail. Wrap the item capped at the
            // receiver's supplied arity so the generated closure matches
            // what the callback caller actually passes.
            let return_ty = function.return_ty;
            let expr = self.item_function_closure_expression_with_max_params(
                item,
                expected_param_tys.len(),
                span_start,
                span_end,
                body,
            )?;
            return Ok(ClosureCallback { expr, return_ty });
        }
        // The global `Object` function passed as a callback
        // (`xs.map(Object)`) boxes each element into its wrapper object.
        // Smelt does not model wrapper objects separately from their
        // primitive values — a boxed string coerces back to the same
        // string everywhere it is used — so the conversion is the identity
        // on the receiver's element type. Lowering it as a typed identity
        // closure keeps the mapped list's concrete element type instead of
        // erasing it.
        if name == "Object"
            && !self.builtin_call_identifier_is_shadowed("Object")
        {
            let span = self.span(span_start, span_end);
            let param_ty = expected_param_tys
                .first()
                .copied()
                .unwrap_or_else(|| {
                    self.erase_with(ErasureReason::MissingAnnotation, "callback parameter")
                });
            let expr = self.builtin_unary_closure_expression(
                param_ty,
                param_ty,
                span,
                body,
                |value_expr| ExprKind::TypeAssert { value: value_expr },
            );
            return Ok(ClosureCallback {
                expr,
                return_ty: param_ty,
            });
        }
        // `parseInt` takes a radix as its second argument, so a caller that
        // passes a second number (`xs.map(parseInt)` passes the index) parses
        // in that radix, as JavaScript does.
        if name == "parseInt"
            && !self.builtin_call_identifier_is_shadowed(name)
            && expected_param_tys.get(1).is_some_and(|&ty| {
                matches!(self.ctx.krate.types.get(ty), Some(Type::Int | Type::Float))
            })
        {
            let span = self.span(span_start, span_end);
            let expr = self.builtin_parse_int_closure_expression(true, span, body);
            let return_ty = self.closure_value_return_ty(expr, body);
            return Ok(ClosureCallback { expr, return_ty });
        }
        // Recognized global builtin *functions* passed as callbacks
        // (`xs.map(Number)`, `xs.filter(Boolean)`, `xs.map(parseInt)`).
        // Lower them to the same concrete single-argument closures used in
        // ordinary value position so the array method runs the builtin's
        // real behavior instead of a placeholder.
        if let Some(host) = self.host_global_named(name)
            && let Some(expr) = self.builtin_function_value_expression(
            host,
            span_start,
            span_end,
            body,
        ) {
            let return_ty = self.closure_value_return_ty(expr, body);
            return Ok(ClosureCallback { expr, return_ty });
        }
        if self.is_opaque_callback_value(name) {
            // The callback names an imported or forward-declared callable
            // whose body is opaque here. Lower it like an opaque member
            // callback: a closure that calls the value with the receiver's
            // element arguments. This matches how a direct call to the same
            // value lowers, and lets array methods accept named-local
            // callbacks instead of requiring an inline arrow.
            self.reject_unresolved_callback_import(name, span_start, span_end)?;
            let callback = self.opaque_member_callback(expected_param_tys);
            let return_ty = callback.ty;
            let expr = self.forwarding_closure(callback, expected_param_tys, self.span(span_start, span_end), body)?;
            return Ok(ClosureCallback { expr, return_ty });
        }
        if !self.scope.is_bound(name) {
            return Err(SmeltError::unsupported(
                self.span(span_start, span_end),
                format!("{context} local callback `{name}` is not in scope"),
            ));
        }
        // The name is a local holding a value. If its (possibly erased) type is
        // a callable surface (`any`/`unknown`, a type parameter, or a union that
        // includes a function), call it through a wrapper closure that captures
        // the local and forwards the receiver's element arguments, the same way
        // a direct `fn(...)` call would lower.
        let local = self.scope.lookup(name).expect("local checked present above");
        let local_ty = Self::local_ty(body, local);
        if self.callback_local_value_is_callable_surface(local_ty) {
            let callback = self.opaque_local_callback(local, local_ty, expected_param_tys);
            let return_ty = callback.ty;
            let expr = self.forwarding_closure(callback, expected_param_tys, self.span(span_start, span_end), body)?;
            return Ok(ClosureCallback { expr, return_ty });
        }
        Err(SmeltError::unsupported(
            self.span(span_start, span_end),
            format!("{context} local callback `{name}` is not defined"),
        ))
    }

    /// Peel type assertions off a callback argument that is otherwise a bare
    /// name.
    ///
    /// `as`, `satisfies`, `!` and parentheses make a type-level claim about a
    /// value; none of them changes WHICH function a callback names, so callback
    /// selection has to see through them. Hono spells its truthiness filter
    /// `res.filter<string>(Boolean as any)` (`src/utils/html.ts`), which
    /// reported "array callback methods currently require arrow function
    /// callbacks" while the identical `res.filter(Boolean)` lowered to the real
    /// builtin. Returns the name and its span, or `None` when the argument is
    /// not an assertion around a plain identifier.
    fn callback_argument_asserted_identifier<'src>(
        argument: &'src Argument<'src>,
    ) -> Option<(&'src str, u32, u32)> {
        /// Strips type assertions and parentheses to reach a plain identifier, returning its name and span.
        fn peel<'src>(expression: &'src Expression<'src>) -> Option<(&'src str, u32, u32)> {
            match expression {
                Expression::Identifier(identifier) => Some((
                    identifier.name.as_str(),
                    identifier.span.start,
                    identifier.span.end,
                )),
                Expression::TSAsExpression(cast) => peel(&cast.expression),
                Expression::TSSatisfiesExpression(cast) => peel(&cast.expression),
                Expression::TSTypeAssertion(cast) => peel(&cast.expression),
                Expression::TSNonNullExpression(cast) => peel(&cast.expression),
                Expression::ParenthesizedExpression(parenthesized) => {
                    peel(&parenthesized.expression)
                }
                _ => None,
            }
        }
        match argument {
            Argument::TSAsExpression(cast) => peel(&cast.expression),
            Argument::TSSatisfiesExpression(cast) => peel(&cast.expression),
            Argument::TSTypeAssertion(cast) => peel(&cast.expression),
            Argument::TSNonNullExpression(cast) => peel(&cast.expression),
            Argument::ParenthesizedExpression(parenthesized) => peel(&parenthesized.expression),
            _ => None,
        }
    }

    /// Lower either an inline arrow callback or a local closure callback value.
    ///
    /// Routes a bare identifier, an asserted identifier, a direct value, or an
    /// inline arrow to its matching lowering path.
    pub(in crate::lowering) fn callback_argument(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        context: &'static str,
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        if let Argument::Identifier(identifier) = argument {
            return self.callback_identifier_argument(
                identifier.name.as_str(),
                identifier.span.start,
                identifier.span.end,
                expected_param_tys,
                context,
                body,
            );
        }
        // A type assertion around a bare name is transparent to callback
        // selection: `xs.filter(Boolean as any)` names the same builtin as
        // `xs.filter(Boolean)`.
        if let Some((name, span_start, span_end)) =
            Self::callback_argument_asserted_identifier(argument)
        {
            return self.callback_identifier_argument(
                name,
                span_start,
                span_end,
                expected_param_tys,
                context,
                body,
            );
        }
        // not a new erasure: `unknown` is the "no return hint" sentinel
        let no_return_hint = self.unknown_type(NotNewErasure::NoHint);
        if let Some(callback) = self.inline_callback(
            argument,
            expected_param_tys,
            no_return_hint,
            context,
            body,
        )? {
            return Ok(callback);
        }
        // A callback factory whose callee is only *declared* here has no
        // lowerable value, so a failed attempt falls through to the opaque
        // callback below instead of failing the whole call. A missing host
        // surface is a blocker the opaque callback must not absorb.
        let direct = match self.argument(argument, body) {
            Ok(expr) => Some(expr),
            Err(error)
                if matches!(argument, Argument::CallExpression(_))
                    && error.category != DiagnosticCategory::MissingStdlib =>
            {
                drop(error);
                None
            }
            Err(error) => return Err(error),
        };
        if let Some(direct_expr) = direct {
            let direct_ty = Self::expr_ty(body, direct_expr);
            if let Some(Type::Function(function)) =
                self.ctx.krate.types.get(direct_ty).cloned()
            {
                return Ok(ClosureCallback {
                    expr: direct_expr,
                    return_ty: function.return_ty,
                });
            }
            if let Some(callback) = self.bound_callable_value_callback(
                argument,
                direct_expr,
                direct_ty,
                expected_param_tys,
                body,
            ) {
                let span = self.span(argument.span().start, argument.span().end);
                let return_ty = callback.ty;
                let expr =
                    self.forwarding_closure(callback, expected_param_tys, span, body)?;
                return Ok(ClosureCallback { expr, return_ty });
            }
        } else {
            let span = self.span(argument.span().start, argument.span().end);
            let callback = self.opaque_member_callback(expected_param_tys);
            let return_ty = callback.ty;
            let expr =
                self.forwarding_closure(callback, expected_param_tys, span, body)?;
            return Ok(ClosureCallback { expr, return_ty });
        }
        let callback = self.forwarding_callback(argument, expected_param_tys, body)?;
        let return_ty = callback.ty;
        let expr = self.forwarding_closure(callback, expected_param_tys, self.span(argument.span().start, argument.span().end), body)?;
        Ok(ClosureCallback { expr, return_ty })
    }

    /// Model a callback argument that is a *call* — a callback factory such as
    /// `xs.filter(negate(isEven))` — as a call of its already lowered result.
    ///
    /// The factory's result is bound to a local in the enclosing body, evaluated
    /// exactly once, as
    /// JavaScript does, not once per element — and the callback calls that
    /// local, the same shape a named local of an erased callable type uses.
    ///
    /// Returns `None` when the argument is not a call, or when its lowered type
    /// is not a callable surface at all; callers then keep their existing
    /// handling. The returned callback's type is the call's result type, so a
    /// predicate caller must still coerce it to a boolean.
    fn bound_callable_value_callback(
        &mut self,
        argument: &Argument<'_>,
        direct_expr: smelt_hir::ExprId,
        direct_ty: smelt_hir::TypeId,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &mut Body,
    ) -> Option<ForwardingCallback> {
        if !matches!(argument, Argument::CallExpression(_)) {
            return None;
        }
        if !self.callback_local_value_is_callable_surface(direct_ty) {
            return None;
        }
        let span = self.span(argument.span().start, argument.span().end);
        let symbol = self.ctx.krate.symbols.intern("__callback_value");
        let local = self.capture_bind_value(direct_expr, direct_ty, symbol, span, body);
        Some(self.opaque_local_callback(local, direct_ty, expected_param_tys))
    }

    /// Lower an array predicate callback, coercing JavaScript truthy returns into booleans.
    pub(in crate::lowering) fn truthy_callback_argument_with_return_hint(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        context: &'static str,
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        if let Some(callback) =
            self.inline_callback(argument, expected_param_tys, bool_ty, context, body)?
        {
            return Ok(ClosureCallback { expr: callback.expr, return_ty: bool_ty });
        }
        match self.forwarding_callback(argument, expected_param_tys, body) {
            Ok(callback) => {
                let span = self.span(argument.span().start, argument.span().end);
                let expr = self.forwarding_predicate_closure(callback, expected_param_tys, span, body)?;
                Ok(ClosureCallback { expr, return_ty: bool_ty })
            }
            Err(error) => {
                drop(error);
                // A predicate built by a factory call must have its erased
                // result turned into a boolean *here*, while the callback is
                // still an expression tree: the closure this becomes is typed
                // `-> bool`, and only the truthiness coercion can put a
                // JavaScript truthy result into it.
                let span = self.span(argument.span().start, argument.span().end);
                if matches!(argument, Argument::CallExpression(_)) {
                    // The factory's value when it can be lowered; the opaque
                    // callback when the callee is only declared here.
                    let direct = match self.argument(argument, body) {
                        Err(error) if error.category == DiagnosticCategory::MissingStdlib => {
                            return Err(error);
                        }
                        direct => direct.ok(),
                    };
                    let callback = direct
                        .and_then(|direct_expr| {
                            let direct_ty = Self::expr_ty(body, direct_expr);
                            self.bound_callable_value_callback(
                                argument,
                                direct_expr,
                                direct_ty,
                                expected_param_tys,
                                body,
                            )
                        })
                        .unwrap_or_else(|| self.opaque_member_callback(expected_param_tys));
                    let expr = self.forwarding_predicate_closure(callback, expected_param_tys, span, body)?;
                    return Ok(ClosureCallback {
                        expr,
                        return_ty: bool_ty,
                    });
                }
                let callback =
                    self.callback_argument(argument, expected_param_tys, context, body)?;
                if callback.return_ty == bool_ty {
                    Ok(callback)
                } else if matches!(
                    self.ctx.krate.types.get(callback.return_ty),
                    Some(Type::Unknown | Type::TypeParam { .. })
                ) || self.erased_or_union_surface(callback.return_ty)
                {
                    // An opaque/named predicate (`xs.some(matchFunc)`) lowers to a
                    // wrapper closure whose result is an erased `unknown` value.
                    // JavaScript predicates use the truthiness of that result, and
                    // the downstream array predicate op coerces an erased callback
                    // result to bool, so accept the erased return type instead of
                    // rejecting the named-callback form.
                    Ok(ClosureCallback {
                        expr: callback.expr,
                        return_ty: bool_ty,
                    })
                } else {
                    Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        format!(
                            "{context} callback returns an unsupported type ({:?})",
                            self.ctx.krate.types.get(callback.return_ty)
                        ),
                    ))
                }
            }
        }
    }

    /// Lower an array callback; `return_hint` types an inline literal's body.
    pub(in crate::lowering) fn callback_argument_with_return_hint(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        return_hint: smelt_hir::TypeId,
        context: &'static str,
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        if let Some(callback) =
            self.inline_callback(argument, expected_param_tys, return_hint, context, body)?
        {
            return Ok(callback);
        }
        self.callback_argument(argument, expected_param_tys, context, body)
    }

    /// Lower an inline callback literal through its closure body.
    ///
    /// A type assertion around the literal is transparent. Answers `None` for
    /// any other argument. An `unknown` `return_hint` means no hint; a return
    /// still erased after the body is lowered is that erasure decision.
    fn inline_callback(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        return_hint: smelt_hir::TypeId,
        context: &'static str,
        body: &mut Body,
    ) -> Result<Option<ClosureCallback>, SmeltError> {
        let Some(mut expression) = argument.as_expression() else {
            return Ok(None);
        };
        let expr = loop {
            match expression {
                Expression::ParenthesizedExpression(inner) => expression = &inner.expression,
                Expression::TSAsExpression(inner) => expression = &inner.expression,
                Expression::TSTypeAssertion(inner) => expression = &inner.expression,
                Expression::TSSatisfiesExpression(inner) => expression = &inner.expression,
                Expression::TSNonNullExpression(inner) => expression = &inner.expression,
                Expression::ArrowFunctionExpression(arrow) => {
                    break self.arrow_closure_body_expr(arrow, expected_param_tys, return_hint, body)?;
                }
                Expression::FunctionExpression(function) => {
                    break self.function_closure_body_expr(
                        function,
                        expected_param_tys,
                        return_hint,
                        body,
                    )?;
                }
                _ => return Ok(None),
            }
        };
        let return_ty = match self.ctx.krate.types.get(Self::expr_ty(body, expr)) {
            Some(Type::Function(function)) => function.return_ty,
            _ => return_hint,
        };
        if self.ctx.krate.types.get(return_hint) == Some(&Type::Unknown)
            && self.ctx.krate.types.get(return_ty) == Some(&Type::Unknown)
        {
            let _ = self.erase_with(ErasureReason::MissingAnnotation, context);
        }
        Ok(Some(ClosureCallback { expr, return_ty }))
    }

    /// Collapse tuple item types into the element type used by array callbacks.
    pub(in crate::lowering) fn tuple_items_element_type(&mut self, items: &[smelt_hir::TypeId]) -> smelt_hir::TypeId {
        match items {
            [] => self.erase_with(ErasureReason::MissingAnnotation, "empty tuple element"),
            [single] => *single,
            [first, rest @ ..] if rest.iter().all(|item| item == first) => *first,
            _ => self.ctx.krate.types.intern(Type::Union(items.to_vec())),
        }
    }
}
