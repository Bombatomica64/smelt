//! `Function.prototype.bind` and partial application of a nested call.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Argument, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, CaptureMode, ClosureCapture, Expr, ExprKind, FunctionType, LocalDecl, Param, Pattern,
    Span, Stmt, Type,
};

impl ModuleBuilder<'_> {
    /// Lower `callable.bind(this_arg, ...bound)` into a closure with captured arguments.
    ///
    /// JavaScript `Function.prototype.bind` produces a new callable value. Smelt
    /// models the callable value directly: the generated closure captures the
    /// original callee plus each bound argument, then forwards the remaining
    /// parameters when the closure is called.
    pub(in crate::lowering) fn function_bind_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        outer_body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "bind" {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, outer_body)?;
        let receiver_ty = Self::expr_ty(outer_body, receiver);
        let Some(function_ty) = self.function_member_type(receiver_ty) else {
            return Ok(None);
        };
        let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned() else {
            return Ok(None);
        };
        let bound_source_args = call.arguments.iter().skip(1).collect::<Vec<_>>();
        if bound_source_args.len() > function.params.len() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Function.prototype.bind received more bound arguments than callable parameters",
            ));
        }

        let span = self.span(call.span.start, call.span.end);
        // `bind`'s LEADING operand is the receiver, not a bound argument (the
        // bound arguments start at index 1, which is why they are skipped above).
        // It used to be read by nobody at all, so `fn.bind(context)` produced a
        // callable that forwarded the bound arguments correctly and lost the
        // context entirely -- es-toolkit's `spread` spec binds a greeting object
        // and then reads `this.greeting` inside the callee. Bind it as the
        // receiver installed for every call of the resulting callable, which is
        // exactly what `Function.prototype.bind` specifies.
        let receiver = match call.arguments.first() {
            Some(argument) => {
                let this_arg = self.argument(argument, outer_body)?;
                self.bind_this_receiver(receiver, this_arg, outer_body, span)
            }
            None => receiver,
        };
        let callee_symbol = self.intern_source_name("__smelt_bind_callee");
        let callee_local =
            self.capture_bind_value(receiver, function_ty, callee_symbol, span, outer_body);
        let mut bound_captures = Vec::new();
        for (index, argument) in bound_source_args.iter().enumerate() {
            let param_ty = function.params.get(index).copied();
            let value = self.argument_with_hint(argument, outer_body, param_ty)?;
            let value_ty = Self::expr_ty(outer_body, value);
            let symbol = self.intern_source_name(&format!("__smelt_bind_arg_{index}"));
            let local = self.capture_bind_value(value, value_ty, symbol, span, outer_body);
            bound_captures.push((local, symbol, value_ty));
        }

        let remaining_params = function
            .params
            .iter()
            .copied()
            .skip(bound_captures.len())
            .collect::<Vec<_>>();
        let mut closure_body = Body::new(None, span);
        let mut captures = Vec::new();
        let callee_body_local = Self::push_bind_capture_local(
            callee_local,
            callee_symbol,
            function_ty,
            span,
            &mut closure_body,
            &mut captures,
        );
        let mut call_args = Vec::new();
        for (source_local, symbol, ty) in bound_captures {
            let body_local = Self::push_bind_capture_local(
                source_local,
                symbol,
                ty,
                span,
                &mut closure_body,
                &mut captures,
            );
            call_args.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(body_local),
                ty,
                span,
            }));
        }

        let mut closure_params = Vec::new();
        for (index, ty) in remaining_params.iter().copied().enumerate() {
            let symbol = self.synthetic_param_symbol(index);
            let local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span,
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name: symbol,
                local,
                ty,
                span,
            });
            call_args.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span,
            }));
        }

        let callee = closure_body.push_expr(Expr {
            kind: ExprKind::Local(callee_body_local),
            ty: function_ty,
            span,
        });
        let call_expr = closure_body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee,
                args: call_args,
            },
            ty: function.return_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(call_expr)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: remaining_params,
            rest: None,
            required_params: None,
                    mutable_params: Vec::new(),
            return_ty: function.return_ty,
            is_async: function.is_async,
            may_throw: function.may_throw,
        }));
        Ok(Some(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest: None,
                required_params: None,
                return_ty: function.return_ty,
                captures,
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        })))
    }

    /// Store a bind component in an outer local so a generated closure can capture it.
    pub(in crate::lowering) fn capture_bind_value(
        &self,
        value: smelt_hir::ExprId,
        ty: smelt_hir::TypeId,
        symbol: smelt_hir::Symbol,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::LocalId {
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span,
        });
        let pat = body.push_pattern(Pattern::Binding(local));
        let stmt = Stmt::Let {
            pat,
            ty,
            value: Some(value),
        };
        self.push_stmt_to_sink(body, stmt);
        local
    }

    /// Add one captured bind component to a generated closure body.
    pub(in crate::lowering) fn push_bind_capture_local(
        source_local: smelt_hir::LocalId,
        symbol: smelt_hir::Symbol,
        ty: smelt_hir::TypeId,
        span: Span,
        closure_body: &mut Body,
        captures: &mut Vec<ClosureCapture>,
    ) -> smelt_hir::LocalId {
        let body_local = closure_body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span,
        });
        captures.push(ClosureCapture {
            source_local,
            body_local: Some(body_local),
            symbol,
            ty,
            mode: CaptureMode::ByRef,
        });
        body_local
    }

    /// Fold Remeda partial-bind nested calls into direct closure calls.
    pub(in crate::lowering) fn partial_bind_nested_call(
        &mut self,
        callee_call: &oxc::ast::ast::CallExpression<'_>,
        outer_call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &callee_call.callee else {
            return Ok(None);
        };
        let append_partial_last = match callee.name.as_str() {
            "partialBind" => false,
            "partialLastBind" => true,
            _ => return Ok(None),
        };
        let Some((function_arg, prefix_args)) = callee_call.arguments.split_first() else {
            return Err(SmeltError::unsupported(
                self.span(callee_call.span.start, callee_call.span.end),
                "partialBind requires a function argument",
            ));
        };
        let callee_expr = self.argument(function_arg, body)?;
        let function_ty = self
            .ctx
            .krate
            .types
            .get(Self::expr_ty(body, callee_expr))
            .cloned();
        let Some(Type::Function(function)) = function_ty else {
            return Err(SmeltError::unsupported(
                self.span(function_arg.span().start, function_arg.span().end),
                "partialBind function argument must be callable",
            ));
        };
        let mut args = Vec::new();
        let ordered_args: Box<dyn Iterator<Item = &Argument<'_>>> = if append_partial_last {
            Box::new(outer_call.arguments.iter().chain(prefix_args.iter()))
        } else {
            Box::new(prefix_args.iter().chain(outer_call.arguments.iter()))
        };
        for arg in ordered_args {
            args.push(self.argument(arg, body)?);
        }
        if args.len() != function.params.len() {
            return Err(SmeltError::unsupported(
                self.span(outer_call.span.start, outer_call.span.end),
                "partialBind folded call argument count does not match function",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: callee_expr,
                args,
            },
            ty: function.return_ty,
            span: self.span(outer_call.span.start, outer_call.span.end),
        })))
    }
}
