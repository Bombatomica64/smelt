//! The `Math` global: its call forms, its constants, and reads of its members as
//! values.

use crate::lowering::body_arena::BodyArena;
use crate::SmeltError;
use crate::lowering::{ModuleBuilder, stdlib_dispatch};
use oxc::ast::ast::{Argument, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    BinOp, Body, CaptureMode, ClosureCapture, Expr, ExprKind, FunctionType, Literal, LocalDecl,
    NumericExtremaOp, NumericRoundOp, Param, Span, Stmt, Type,
};
use smelt_stdlib::{MathFunction, RuleId};

impl ModuleBuilder<'_> {
    /// Lower a supported `Math.pow` call into a HIR numeric runtime call.
    pub(in crate::lowering) fn math_pow_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !self.names_host(object.name.as_str(), "Math") || member.property.name != "pow" {
            return Ok(None);
        }
        if call.arguments.len() != 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.pow requires exactly two arguments",
            ));
        }
        let [base_argument, exponent_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.pow requires exactly two arguments",
            ));
        };
        let base = self.argument(base_argument, body)?;
        let exponent = self.argument(exponent_argument, body)?;
        if [base, exponent]
            .iter()
            .any(|arg| self.ctx.krate.types.get(Self::expr_ty(body, *arg)) != Some(&Type::Float))
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.pow requires number arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericPow { base, exponent },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.imul(a, b)` calls to the int32 product operator.
    pub(in crate::lowering) fn math_imul_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !self.names_host(object.name.as_str(), "Math") || member.property.name != "imul" {
            return Ok(None);
        }
        let [lhs_argument, rhs_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.imul requires exactly two arguments",
            ));
        };
        let lhs = self.argument(lhs_argument, body)?;
        let rhs = self.argument(rhs_argument, body)?;
        if [lhs, rhs]
            .iter()
            .any(|arg| self.ctx.krate.types.get(Self::expr_ty(body, *arg)) != Some(&Type::Float))
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.imul requires number arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::IMul,
                lhs,
                rhs,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.atan2` calls.
    pub(in crate::lowering) fn math_atan2_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !self.names_host(object.name.as_str(), "Math") || member.property.name != "atan2" {
            return Ok(None);
        }
        if call.arguments.len() != 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.atan2 requires exactly two arguments",
            ));
        }
        let [y_argument, x_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.atan2 requires exactly two arguments",
            ));
        };
        let y_coord = self.argument(y_argument, body)?;
        let x_coord = self.argument(x_argument, body)?;
        if [y_coord, x_coord]
            .iter()
            .any(|arg| self.ctx.krate.types.get(Self::expr_ty(body, *arg)) != Some(&Type::Float))
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.atan2 requires number arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericAtan2 {
                y: y_coord,
                x: x_coord,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.abs(...)` calls.
    pub(in crate::lowering) fn math_abs_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !self.names_host(object.name.as_str(), "Math") || member.property.name != "abs" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.abs requires exactly one argument",
            ));
        }
        let Some(argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.abs requires exactly one argument",
            ));
        };
        let operand = self.argument(argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.abs requires a number argument",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericAbs { operand },
            ty: operand_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript numeric rounding calls.
    pub(in crate::lowering) fn math_round_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !self.names_host(object.name.as_str(), "Math") {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "floor" => NumericRoundOp::Floor,
            "ceil" => NumericRoundOp::Ceil,
            "round" => NumericRoundOp::Round,
            "trunc" => NumericRoundOp::Trunc,
            _ => return Ok(None),
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        }
        let Some(argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        };
        let mut operand = self.argument(argument, body)?;
        let mut operand_ty = Self::expr_ty(body, operand);
        if !self.is_numeric_like_type(operand_ty) {
            if self.type_contains_unknown(operand_ty)
                || matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Bool))
                || self.is_date_constructor_arg_type(operand_ty)
            {
                let target = self.ctx.krate.types.intern(Type::Float);
                operand = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: operand,
                        target,
                    },
                    ty: target,
                    span: self.span(argument.span().start, argument.span().end),
                });
                operand_ty = target;
            } else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("Math.{} requires a number argument", member.property.name),
                ));
            }
        }
        let result_ty = self.type_param_constraint_or_self(operand_ty);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericRound { op, operand },
            ty: result_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.max` and `Math.min` calls.
    pub(in crate::lowering) fn math_extrema_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !self.names_host(object.name.as_str(), "Math") {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "max" => NumericExtremaOp::Max,
            "min" => NumericExtremaOp::Min,
            _ => return Ok(None),
        };
        // A single spread argument (`Math.max(...values)`) reduces the numeric
        // list rather than treating it as one scalar operand. Separate any
        // spread from the scalar arguments so the extrema can fold the list.
        // Bail (returning `None`) on the uncommon multi-spread case so the
        // generic call path is left untouched.
        let spread_count = call
            .arguments
            .iter()
            .filter(|argument| matches!(argument, Argument::SpreadElement(_)))
            .count();
        if spread_count > 1 {
            return Ok(None);
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        let mut args = Vec::new();
        let mut spread = None;
        for argument in &call.arguments {
            let lowered = self.argument(argument, body)?;
            if matches!(argument, Argument::SpreadElement(_)) {
                // The extrema reduction coerces each spread element to `Float`
                // in the backend, so the list is taken as lowered.
                spread = Some(lowered);
                continue;
            }
            let arg = if matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, lowered)),
                Some(&Type::Float)
            ) {
                lowered
            } else {
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: lowered },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })
            };
            args.push(arg);
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericExtrema { op, args, spread },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.hypot` calls.
    pub(in crate::lowering) fn math_hypot_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !self.names_host(object.name.as_str(), "Math") || member.property.name != "hypot" {
            return Ok(None);
        }
        let args = call
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        if args
            .iter()
            .any(|arg| self.ctx.krate.types.get(Self::expr_ty(body, *arg)) != Some(&Type::Float))
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.hypot requires number arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericHypot { args },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript unary `Math.*` numeric calls.
    pub(in crate::lowering) fn math_unary_func_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !self.names_host(object.name.as_str(), "Math") {
            return Ok(None);
        }
        let Some(op) = smelt_stdlib::math_function(member.property.name.as_str())
            .and_then(stdlib_dispatch::math_unary_op)
        else {
            return Ok(None);
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        }
        let Some(argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        };
        let operand = self.argument(argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Math.{} requires a number argument", member.property.name),
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericUnaryFunc { op, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.random` calls.
    pub(in crate::lowering) fn math_random_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.recognized_call(call) != Some(RuleId::TsMathRandom) {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.random requires no arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericRandom,
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower a supported `Math.<fn>` member reference to a first-class closure.
    ///
    /// Utility libraries such as Remeda pass `Math.ceil` or `Math.floor` as
    /// callbacks. The direct-call lowering handles `Math.ceil(value)`, but a
    /// bare member reference must become a callable value instead of resolving
    /// `Math` as a normal identifier.
    pub(in crate::lowering) fn math_member_expression(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if !self.names_host(object.name.as_str(), "Math") {
            return None;
        }
        let span = self.span(member.span.start, member.span.end);
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        // `Math.PI` and the other `Math.*` numeric constants are values, not
        // callables: fold them to their IEEE-754 double literal so a bare
        // `Math.PI` reference (e.g. `chunk(xs, Math.PI)` in the chunk spec)
        // resolves to a concrete number instead of an unresolved `Math`
        // identifier. These match the ECMAScript spec constant values.
        if let Some(constant) = smelt_stdlib::math_constant(member.property.name.as_str()) {
            return Some(outer_body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(constant)),
                ty: number_ty,
                span,
            }));
        }
        let value_name = self.intern_source_name("value");
        let mut closure_body = Body::new(None, span);
        let value_local = closure_body.push_local(LocalDecl {
            name: Some(value_name),
            ty: number_ty,
            mutable: false,
            span,
        });
        closure_body.params.push(value_local);
        let value_expr = closure_body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty: number_ty,
            span,
        });
        let function = smelt_stdlib::math_function(member.property.name.as_str())?;
        let result_kind = match function {
            MathFunction::Abs => ExprKind::NumericAbs {
                operand: value_expr,
            },
            MathFunction::Clz32 => return None,
            _ => {
                if let Some(op) = stdlib_dispatch::math_round_op(function) {
                    ExprKind::NumericRound {
                        op,
                        operand: value_expr,
                    }
                } else {
                    ExprKind::NumericUnaryFunc {
                        op: stdlib_dispatch::math_unary_op(function)?,
                        operand: value_expr,
                    }
                }
            }
        };
        let result = closure_body.push_expr(Expr {
            kind: result_kind,
            ty: number_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![number_ty],
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: number_ty,
            is_async: false,
            may_throw: false,
        }));
        Some(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: vec![Param {
                    name: value_name,
                    local: value_local,
                    ty: number_ty,
                    span,
                }],
                rest: None,
                required_params: None,
                return_ty: number_ty,
                captures: Vec::new(),
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Lower `Math[method]` for supported numeric method-key unions to a closure.
    ///
    /// Date-fns stores a selected rounding method as a string literal union and
    /// then calls the selected function. Smelt keeps that value dynamic by
    /// emitting a captured closure that dispatches over supported rounding names.
    pub(in crate::lowering) fn dynamic_math_member_expression(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if !self.names_host(object.name.as_str(), "Math") {
            return None;
        }
        let Expression::Identifier(method_ident) = &member.expression else {
            return None;
        };
        let source_local = self.scope.lookup(method_ident.name.as_str())?;
        let source_decl = outer_body.local_at(source_local)?;

        let span = self.span(member.span.start, member.span.end);
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        let value_name = self.intern_source_name("value");
        let method_name = source_decl
            .name
            .unwrap_or_else(|| self.intern_source_name(method_ident.name.as_str()));
        let method_ty = source_decl.ty;
        let mut closure_body = Body::new(None, span);
        let method_local = closure_body.push_local(LocalDecl {
            name: Some(method_name),
            ty: method_ty,
            mutable: source_decl.mutable,
            span: source_decl.span,
        });
        let value_local = closure_body.push_local(LocalDecl {
            name: Some(value_name),
            ty: number_ty,
            mutable: false,
            span,
        });
        closure_body.params.push(value_local);
        let value_expr = closure_body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty: number_ty,
            span,
        });

        let mut result = Self::dynamic_math_round_expr(
            &mut closure_body,
            NumericRoundOp::Trunc,
            value_expr,
            number_ty,
            span,
        );
        for (method, op) in [
            ("round", NumericRoundOp::Round),
            ("floor", NumericRoundOp::Floor),
            ("ceil", NumericRoundOp::Ceil),
        ] {
            let method_expr = closure_body.push_expr(Expr {
                kind: ExprKind::Local(method_local),
                ty: method_ty,
                span,
            });
            let method_text = closure_body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(method.to_owned())),
                ty: self.ctx.krate.types.intern(Type::String),
                span,
            });
            let cond = closure_body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Eq,
                    lhs: method_expr,
                    rhs: method_text,
                },
                ty: self.ctx.krate.types.intern(Type::Bool),
                span,
            });
            let then_expr =
                Self::dynamic_math_round_expr(&mut closure_body, op, value_expr, number_ty, span);
            result = closure_body.push_expr(Expr {
                kind: ExprKind::Conditional {
                    cond,
                    then_expr,
                    else_expr: result,
                },
                ty: number_ty,
                span,
            });
        }
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![number_ty],
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: number_ty,
            is_async: false,
            may_throw: false,
        }));
        Some(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: vec![Param {
                    name: value_name,
                    local: value_local,
                    ty: number_ty,
                    span,
                }],
                rest: None,
                required_params: None,
                return_ty: number_ty,
                captures: vec![ClosureCapture {
                    source_local,
                    body_local: Some(method_local),
                    symbol: method_name,
                    ty: method_ty,
                    mode: CaptureMode::ByRef,
                }],
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Push one numeric rounding operation inside a generated dynamic Math closure.
    pub(in crate::lowering) fn dynamic_math_round_expr(
        body: &mut Body,
        op: NumericRoundOp,
        operand: smelt_hir::ExprId,
        number_ty: smelt_hir::TypeId,
        span: Span,
    ) -> smelt_hir::ExprId {
        body.push_expr(Expr {
            kind: ExprKind::NumericRound { op, operand },
            ty: number_ty,
            span,
        })
    }
}
