//! Calls decided by the syntactic form of the callee: a computed member, a
//! private member, `console`, a constructed value, the result of another call, an
//! asserted callee, or a name bound to a value rather than an item.

use crate::lowering::erasure::NotNewErasure;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::ModuleBuilder;
use crate::lowering::peel::{Wrappers, peel};
use crate::SmeltError;
use oxc::ast::ast::Expression;
use oxc::span::GetSpan;
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, Literal, Type};

impl ModuleBuilder<'_> {
    /// Call `object[key](..)` through the value the member read produces.
    pub(in crate::lowering) fn computed_member_value_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // JavaScript evaluates the callee (`object[key]`) before the
        // arguments, so the member read is lowered first.
        let callee = self.computed_member(member, body)?;
        let callee_ty = Self::expr_ty(body, callee);
        if self.callable_through_erased_abi(callee_ty) {
            return self.erased_value_call(callee, member.span, call, body);
        }
        if let Some(Type::Function(function)) = self.ctx.krate.types.get(callee_ty).cloned() {
            if call.arguments.len() < function.rest.unwrap_or(function.params.len()) {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "computed member call argument count does not match function",
                ));
            }
            let args = self.value_call_arguments(&function, call, body)?;
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: function.return_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        // Any other callee type is not callable in TypeScript, so `tsc`
        // rejects the call before Smelt runs. Report it rather than
        // inventing a value: substituting the first argument (or
        // `undefined`) for the call's result silently drops the call.
        Err(SmeltError::unsupported(
            self.span(member.span.start, member.span.end),
            format!(
                "computed member callee is not callable ({:?})",
                self.ctx.krate.types.get(callee_ty)
            ),
        ))
    }

    /// `console.log`, `console.warn` and `console.error`.
    pub(in crate::lowering) fn console_output_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut args = Vec::new();
        for arg in &call.arguments {
            let value = self.argument(arg, body)?;
            // An absent `null` prints as `null`. Only where printing the
            // value's string conversion is printing the value itself: a
            // top-level string prints raw, and a boolean prints its word
            // (a number does not: `-0` prints as `-0`, `String(-0)` is `0`).
            let prints_as_its_string = match self.ctx.krate.types.get(Self::expr_ty(body, value)) {
                Some(Type::Optional(inner)) => {
                    matches!(self.ctx.krate.types.get(*inner), Some(Type::String | Type::Bool))
                }
                _ => false,
            };
            args.push(if prints_as_its_string {
                self.null_absent_string_operand(value, arg.span().start, arg.span().end, body)
            } else {
                value
            });
        }
        let ty = self.ctx.krate.types.intern(Type::None);
        let callee_item =
            self.ensure_console_log_item(self.span(member.span.start, member.span.end));
        let callee = body.push_expr(Expr {
            kind: ExprKind::Item(callee_item),
            ty,
            span: self.span(member.span.start, member.span.end),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::Call { callee, args },
            ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }

    /// Call `receiver.#method(..)`.
    pub(in crate::lowering) fn private_member_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::PrivateFieldExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // The member name carries the `#`, because a private name is its
        // own namespace: `this.#newResponse()` must not resolve to the
        // public `newResponse` field of the same class (Hono's `Context`
        // declares both, and resolving the private call to the public
        // arrow — whose body calls the private method — recurses forever).
        // See `intern_private_name`; a `#`-spelled name matches no builtin
        // or stdlib member either, which is exactly right.
        let private_name = format!("#{}", member.field.name.as_str());
        if let Some(expr) = self.private_callable_field_call(call, member, body)? {
            return Ok(expr);
        }
        self.member_call(
            call,
            &member.object,
            &private_name,
            member.span,
            member.optional,
            body,
        )
    }

    /// Call the value of a `new` expression: `new Ctor(..)(..)`.
    pub(in crate::lowering) fn constructed_value_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        callee_new: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // `new Ctor(...)(...)` — constructing a callable and invoking it
        // immediately (e.g. the lodash-compat `new Function(...)(...)`
        // template idiom). Lower the construction, then dispatch the call
        // through the constructed value's function type; spread arguments
        // beyond the callee's declared parameters are evaluated for their
        // effects only.
        let callee = self.new_expression_with_hint(callee_new, body, None)?;
        let callee_ty = Self::expr_ty(body, callee);
        let probed_arg_tys = self.probe_argument_types(&call.arguments, body);
        if let Some(function_ty) = self.function_member_type_for_args(
            callee_ty,
            Some(call.arguments.len()),
            &probed_arg_tys,
        ) && let Some(Type::Function(function)) =
                self.ctx.krate.types.get(function_ty).cloned()
        {
            let args = if function.rest.is_some() {
                self.value_call_arguments(&function, call, body)?
            } else {
                let expanded = self.expanded_call_arguments(&call.arguments, body)?;
                let expanded =
                    self.distribute_trailing_list_spread(expanded, &function.params, body)?;
                let mut args = Vec::with_capacity(function.params.len());
                for argument in expanded.into_iter().take(function.params.len()) {
                    args.push(self.lower_call_arg(argument, None, body)?);
                }
                args
            };
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: function.return_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        if self.erased_or_union_surface(callee_ty) {
            let unknown_ty = self.erase_unless_unknown(callee_ty, ErasureReason::ByName);
            if Self::call_has_spread_arguments(call) {
                let args = self.packed_spread_call_arguments(unknown_ty, call, body)?;
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::ClosureCallSpread { callee, args },
                    ty: unknown_ty,
                    span: self.span(call.span.start, call.span.end),
                }));
            }
            let args = call
                .arguments
                .iter()
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: unknown_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(call.span.start, call.span.end),
            "constructed value is not callable",
        ))
    }

    /// Call the result of a call: `f(..)(..)`.
    pub(in crate::lowering) fn call_result_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        callee_call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(expr) = self.slice_string_nested_call(callee_call, call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.partial_bind_nested_call(callee_call, call, body)? {
            return Ok(expr);
        }
        let callee = self.call_expression(callee_call, body)?;
        let callee_ty = Self::expr_ty(body, callee);
        let probed_arg_tys = self.probe_argument_types(&call.arguments, body);
        let Some(function_ty) = self.function_member_type_for_args(
            callee_ty,
            Some(call.arguments.len()),
            &probed_arg_tys,
        ) else {
            if self.ctx.krate.types.get(callee_ty) == Some(&Type::Unknown) {
                // `f(..)(..)` where the inner call's declared return type is
                // `any`/`unknown`. The value is still callable at runtime —
                // JavaScript looks the call up on the value, not on its
                // static type — so dispatch it through the erased callable
                // ABI, the same route the arity-shortfall branch below takes
                // for `negate(fn)()`. Lowering it to `undefined` instead
                // discarded the outer call *and* its arguments' effects, and
                // any assertion over the result then compared the wrong
                // value (`expect(makeAdder(2)(3)).toBe(5)` became a test that
                // `5` is not nullish).
                // not a new erasure: the inner call already returned `unknown`
                let unknown_ty = self.unknown_type(NotNewErasure::ErasedUpstream);
                let args = call
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall { callee, args },
                    ty: unknown_ty,
                    span: self.span(call.span.start, call.span.end),
                }));
            }
            return Err(SmeltError::unsupported(
                self.span(callee_call.span.start, callee_call.span.end),
                "call expression callee must return a function",
            ));
        };
        let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned()
        else {
            return Err(SmeltError::unsupported(
                self.span(callee_call.span.start, callee_call.span.end),
                "call expression callee must return a function",
            ));
        };
        let callee = if callee_ty == function_ty {
            callee
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: function_ty,
                span: self.span(callee_call.span.start, callee_call.span.end),
            })
        };
        // Optional and rest parameters relax the required arity:
        // `negate(fn)()` calls a `(...args: any[]) => boolean` value with
        // zero arguments, where the rest slot absorbs the empty tail.
        let mut required = function.required_params.unwrap_or(function.params.len());
        if let Some(rest_index) = function.rest {
            required = required.min(rest_index);
        }
        if call.arguments.len() < required {
            // The call omits at least one *required* parameter. `required`
            // already excludes the rest slot (it is clamped to `rest_index`
            // above), and a rest parameter only absorbs the surplus tail
            // *after* the required prefix — it can never satisfy a missing
            // required argument before the rest index. So a shortfall here is
            // a genuinely missing required argument regardless of whether a
            // rest slot exists. Such a call is rejected by `tsc` before Smelt
            // runs, but generic erased wrappers (`negate(fn)()`) still reach
            // here with unknown arity; dispatch those through the
            // arity-independent erased ABI rather than synthesizing a typed
            // `None` for a non-optional parameter.
            let unknown_ty = self.erase_with(ErasureReason::UnfilledGeneric, "arity shortfall");
            let callee = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: unknown_ty,
                span: self.span(callee_call.span.start, callee_call.span.end),
            });
            let args = call
                .arguments
                .iter()
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: unknown_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        // Statically typed under-application: every omitted parameter is
        // optional or absorbed by the rest slot, so the call keeps its typed
        // ABI. Missing optional parameters become typed `None` values.
        let args =
            self.typed_under_applied_closure_arguments(&function, &call.arguments, call.span, body)?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: function.return_ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }

    /// Lower a call's arguments for a call through a function value: spreads and
    /// surplus arguments fill the rest list, and without a rest parameter the
    /// surplus goes.
    fn value_call_arguments(
        &mut self,
        function: &smelt_hir::FunctionType,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let rest = self.list_rest_param(function);
        if rest.is_some() && Self::call_has_spread_arguments(call) {
            return self.spread_closure_call_arguments(function, rest, call, body);
        }
        let limit = if rest.is_some() { usize::MAX } else { function.params.len() };
        let args = call
            .arguments
            .iter()
            .take(limit)
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        let span = self.span(call.span.start, call.span.end);
        Ok(self.pack_value_call_arguments(function, args, &[], span, body))
    }

    /// Call through a transparent assertion: `(f as Fn)(..)`.
    pub(in crate::lowering) fn asserted_callee_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        callee_expression: &Expression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut callee = self.expression(callee_expression, body)?;
        let mut callee_ty = Self::expr_ty(body, callee);
        // The call is made at the asserted function type: that is the signature
        // the arguments were written against.
        if matches!(
            peel(&call.callee, Wrappers::Parens),
            Expression::TSAsExpression(_) | Expression::TSSatisfiesExpression(_)
        ) {
            let asserted = self.expression(&call.callee, body)?;
            let asserted_ty = Self::expr_ty(body, asserted);
            if matches!(self.ctx.krate.types.get(asserted_ty), Some(Type::Function(_))) {
                callee = asserted;
                callee_ty = asserted_ty;
            }
        }
        if let Some(&Type::Optional(inner)) = self.ctx.krate.types.get(callee_ty) {
            callee = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: inner,
                span: self.span(call.span.start, call.span.end),
            });
            callee_ty = inner;
        }
        // A callee asserted out of a wider type (`(x as Fn)(..)` over a union)
        // is the assertion's type, so the cast itself is the callee.
        if !matches!(self.ctx.krate.types.get(callee_ty), Some(Type::Function(_)))
            && !self.callable_through_erased_abi(callee_ty)
        {
            callee = self.expression(&call.callee, body)?;
            callee_ty = Self::expr_ty(body, callee);
        }
        let Some(Type::Function(function)) = self.ctx.krate.types.get(callee_ty).cloned()
        else {
            if self.callable_through_erased_abi(callee_ty) {
                return self.erased_value_call(callee, callee_expression.span(), call, body);
            }
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "asserted call callee must be a function",
            ));
        };
        let args = self.value_call_arguments(&function, call, body)?;
        // A function item referenced as a value is the item itself or its
        // typed closure view (`function_item`).
        let callee_expr = body.expr_at(callee);
        let item = match callee_expr.map(|expr| &expr.kind) {
            Some(ExprKind::Item(item)) => Some(*item),
            Some(ExprKind::Closure(closure)) => closure.function_item,
            _ => None,
        };
        let ty = match item {
            Some(item) => self.instantiated_item_value_call_result(item, &function, &args, call, body)?,
            None => function.return_ty,
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::ClosureCall { callee, args },
            ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }

    /// Call a name bound in the current scope.
    pub(in crate::lowering) fn bound_local_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        callee_ident: &oxc::ast::ast::IdentifierReference<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let callee = self.identifier_expression(
            callee_ident.name.as_str(),
            callee_ident.span.start,
            callee_ident.span.end,
            body,
        )?;
        let callee_ty = Self::expr_ty(body, callee);
        let callable_ty = match self.ctx.krate.types.get(callee_ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => callee_ty,
        };
        let Some(Type::Function(function)) = self.ctx.krate.types.get(callable_ty).cloned()
        else {
            if self.erased_or_union_surface(callee_ty) {
                // The same erased call `erased_value_call` lowers for any
                // other callee shape.
                return self.erased_value_call(callee, callee_ident.span, call, body);
            }
            return Err(SmeltError::unsupported(
                self.span(callee_ident.span.start, callee_ident.span.end),
                format!(
                    "local `{}` is not callable ({:?})",
                    callee_ident.name,
                    self.ctx.krate.types.get(Self::expr_ty(body, callee))
                ),
            ));
        };
        let mut args = Vec::new();
        for arg in &call.arguments {
            args.push(self.argument(arg, body)?);
        }
        if function
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
        if Self::call_has_spread_arguments(call) {
            let item_ty = function
                .rest
                .and_then(|index| function.params.get(index).copied())
                .or_else(|| function.params.first().copied())
                .and_then(|param| match self.ctx.krate.types.get(param) {
                    Some(Type::List(item_ty)) => Some(*item_ty),
                    _ => None,
                })
                .unwrap_or_else(|| self.erase_with(ErasureReason::DynamicOperator, "spread call"));
            let spread_args = self.packed_spread_call_arguments(item_ty, call, body)?;
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread {
                    callee,
                    args: spread_args,
                },
                ty: function.return_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: function.return_ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }

    /// Call a name that resolves to no item: a module global, an import, or a
    /// callable declared later in the source.
    pub(in crate::lowering) fn unresolved_callee_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        callee_ident: &oxc::ast::ast::IdentifierReference<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(callee_ty) =
            self.module_globals.get(callee_ident.name.as_str()).copied()
        {
            let args = call
                .arguments
                .iter()
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            if let Some(Type::Function(function)) =
                self.ctx.krate.types.get(callee_ty).cloned()
            {
                let callee = self.identifier_expression(
                    callee_ident.name.as_str(),
                    callee_ident.span.start,
                    callee_ident.span.end,
                    body,
                )?;
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall { callee, args },
                    ty: function.return_ty,
                    span: self.span(call.span.start, call.span.end),
                }));
            }
            let ty = self.erase_with(ErasureReason::ByName, "module global");
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
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
        if self.source_contains_forward_callable(callee_ident.name.as_str()) {
            for arg in &call.arguments {
                let _ = self.argument(arg, body)?;
            }
            let ty = self.erase_with(ErasureReason::ByName, "forward callable");
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(callee_ident.span.start, callee_ident.span.end),
            format!("unresolved function `{}`", callee_ident.name),
        ))
    }
}
