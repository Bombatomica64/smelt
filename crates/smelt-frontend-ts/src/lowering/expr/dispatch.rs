//! Expression dispatch: `expression_with_hint` and the match over every
//! expression shape, which hands each shape to the module that lowers it.

use super::super::ModuleBuilder;
use crate::SmeltError;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::js_text::js_str_literal;
use crate::lowering::peel::{Wrappers, peel};
use oxc::ast::ast::Expression;
use oxc::syntax::operator::{BinaryOperator, LogicalOperator, UnaryOperator};
use smelt_hir::{BinOp, Body, Expr, ExprKind, Literal, PrimitiveCastOp, Type, UnaryOp};

impl ModuleBuilder<'_> {
    /// Lower an expression while preserving a caller-supplied type hint when possible.
    pub(in crate::lowering) fn expression_with_hint(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(&(_, temp, present_ty)) = self
            .pinned_nodes
            .iter()
            .rev()
            .find(|(node, _, _)| *node == std::ptr::from_ref(expression).addr())
        {
            let span = self.expression_span(expression);
            let stored_ty = Self::local_ty(body, temp);
            let read = body.push_expr(Expr {
                kind: ExprKind::Local(temp),
                ty: stored_ty,
                span,
            });
            // A pin at the binding's own type needs no assertion: the read is
            // already the value at that type.
            if stored_ty == present_ty {
                return Ok(read);
            }
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: read },
                ty: present_ty,
                span,
            }));
        }
        let anchor = self.expression_span(expression);
        let lowered = self.with_erasure_anchor(anchor, |this| {
            this.expression_shape_with_hint(expression, body, type_hint)
        })?;
        // A read of a Bun `int32` slot is a `number` to every consumer.
        let lowered = self.int32_read_as_number(lowered, body);
        Ok(self.in_own_language_view(lowered, body))
    }

    /// A value whose type carries Python text (it comes from a Python module:
    /// a call result, a field, a constant) as TypeScript sees it: a
    /// `TypeAssert` to the TypeScript view, which the backend converts. A
    /// function value is left alone; its calls convert their own arguments
    /// and results. See `smelt_hir::language_view`.
    pub(in crate::lowering) fn in_own_language_view(
        &mut self,
        value: smelt_hir::ExprId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let ty = Self::expr_ty(body, value);
        let language = smelt_hir::language_view::SourceLanguage::TypeScript;
        if matches!(self.ctx.krate.types.get(ty), Some(Type::Function(_)))
            || !smelt_hir::language_view::crosses_into(&self.ctx.krate.types, ty, language)
        {
            return value;
        }
        let span = body
            .exprs
            .get(usize::try_from(value.0).unwrap_or(usize::MAX))
            .map_or_else(|| self.span(0, 0), |expr| expr.span);
        let viewed = smelt_hir::language_view::view_in(&mut self.ctx.krate.types, ty, language);
        body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: viewed,
            span,
        })
    }

    /// Lower one expression; see [`Self::expression_with_hint`].
    pub(in crate::lowering) fn expression_shape_with_hint(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match expression {
            Expression::NumericLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Float);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::BigIntLiteral(lit) => {
                self.bigint_literal_expression(lit.value.as_str(), lit.span, body)
            }
            Expression::StringLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(js_str_literal(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::BooleanLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::NullLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::Identifier(ident) => {
                // A function-typed local that collected `fn.prop = …` writes and
                // coerces to a callable-interface class is bundled into a
                // typed `CallableObjectAssign` here, at the coercion position,
                // instead of leaking the props (the `debounce`/`throttle` shape).
                if let Some(expr) = self.try_consume_callable_local(
                    ident.name.as_str(),
                    ident.span.start,
                    ident.span.end,
                    type_hint,
                    body,
                )? {
                    return Ok(expr);
                }
                self.identifier_expression(
                    ident.name.as_str(),
                    ident.span.start,
                    ident.span.end,
                    body,
                )
            }
            Expression::ThisExpression(this_expr) => {
                self.identifier_expression("this", this_expr.span.start, this_expr.span.end, body)
            }
            Expression::Super(super_expr) => {
                self.identifier_expression("this", super_expr.span.start, super_expr.span.end, body)
            }
            Expression::RegExpLiteral(literal) => {
                let ty = self.regexp_type();
                let pattern = Self::regex_literal_pattern_text_without_flags(literal);
                let flags = literal.regex.flags.to_string();
                let pattern = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(pattern)),
                    ty: self.ctx.krate.types.intern(Type::String),
                    span: self.span(literal.span.start, literal.span.end),
                });
                let flags = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(flags)),
                    ty: self.ctx.krate.types.intern(Type::String),
                    span: self.span(literal.span.start, literal.span.end),
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: self.intern_type_name("RegExp"),
                        args: vec![pattern, flags],
                    },
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                }))
            }
            Expression::ArrayExpression(array) => self.array_expression(array, body, type_hint),
            Expression::ObjectExpression(object) => self.object_expression(object, body, type_hint),
            Expression::BinaryExpression(binary) => {
                if binary.operator == BinaryOperator::Instanceof {
                    return self.instanceof_expression(binary, body);
                }
                if binary.operator == BinaryOperator::In {
                    return self.in_expression(binary, body);
                }
                if let Some(expr) = self.unknown_typeof_comparison(binary, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.unknown_null_comparison(binary, body)? {
                    return Ok(expr);
                }
                if binary.operator == BinaryOperator::Exponential {
                    let base = self.expression(&binary.left, body)?;
                    let exponent = self.expression(&binary.right, body)?;
                    let span = self.span(binary.span.start, binary.span.end);
                    if let Some(power) = self.bigint_power(base, exponent, span, body) {
                        return Ok(power);
                    }
                    let ty = self.ctx.krate.types.intern(Type::Float);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::NumericPow { base, exponent },
                        ty,
                        span: self.span(binary.span.start, binary.span.end),
                    }));
                }
                let op = match binary.operator {
                    BinaryOperator::Addition => BinOp::Add,
                    BinaryOperator::Subtraction => BinOp::Sub,
                    BinaryOperator::Multiplication => BinOp::Mul,
                    BinaryOperator::Division => BinOp::Div,
                    BinaryOperator::Remainder => BinOp::Rem,
                    // `===`/`!==` carry JS reference-identity semantics for
                    // erased objects (`BinOp::JsStrictEq`), distinct from `==`'s
                    // structural/deep `BinOp::Eq` that the deep-equality matchers
                    // and `isDeepEqual` rely on. (`x === null`, `typeof x === …`,
                    // and the `=== || Object.is` idiom are intercepted earlier.)
                    BinaryOperator::StrictEquality => BinOp::JsStrictEq,
                    BinaryOperator::Equality => BinOp::Eq,
                    BinaryOperator::StrictInequality => BinOp::JsStrictNotEq,
                    BinaryOperator::Inequality => BinOp::NotEq,
                    BinaryOperator::LessThan => BinOp::Lt,
                    BinaryOperator::LessEqualThan => BinOp::Lte,
                    BinaryOperator::GreaterThan => BinOp::Gt,
                    BinaryOperator::GreaterEqualThan => BinOp::Gte,
                    BinaryOperator::ShiftLeft => BinOp::Shl,
                    BinaryOperator::ShiftRight => BinOp::Shr,
                    BinaryOperator::ShiftRightZeroFill => BinOp::UShr,
                    BinaryOperator::BitwiseAnd => BinOp::BitAnd,
                    BinaryOperator::BitwiseOR => BinOp::BitOr,
                    BinaryOperator::BitwiseXOR => BinOp::BitXor,
                    BinaryOperator::Exponential
                    | BinaryOperator::In
                    | BinaryOperator::Instanceof => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.span.start, binary.span.end),
                            format!("binary operator is not lowered yet: {:?}", binary.operator),
                        ));
                    }
                };
                let mut lhs = self.equality_operand(op, &binary.left, body)?;
                let mut rhs = self.equality_operand(op, &binary.right, body)?;
                // Strict-equality operands narrowed by a `typeof` guard must stay
                // the erased originals: re-materializing a narrowed function
                // through an adapter builds a fresh `Rc` whose `Rc::ptr_eq` never
                // matches, so `f === f` would wrongly be `false`. See the peel in
                // `binary_expression` for the full rationale.
                if matches!(op, BinOp::JsStrictEq | BinOp::JsStrictNotEq) {
                    lhs = self.peel_narrowing_cast_for_identity(body, lhs);
                    rhs = self.peel_narrowing_cast_for_identity(body, rhs);
                }
                (lhs, rhs) = self.null_absent_concat_operands(op, lhs, rhs, binary, body);
                let lhs_ty = Self::expr_ty(body, lhs);
                let rhs_ty = Self::expr_ty(body, rhs);
                let op = self.loose_equality_op(op, lhs_ty, rhs_ty);
                // Erased operands say nothing about `+`; the checker's contextual
                // types may know it concatenates, as in `binary_expression`.
                let ty = if op == BinOp::Add
                    && (self.checker_types_as_string(binary.span.start, binary.span.end)
                        || type_hint.is_some_and(|hint| {
                            self.ctx.krate.types.get(hint) == Some(&Type::String)
                        }) && (self.is_string_compatible_type(lhs_ty)
                            || self.is_string_compatible_type(rhs_ty)
                            || self.type_contains_unknown(lhs_ty)
                            || self.type_contains_unknown(rhs_ty)))
                {
                    self.ctx.krate.types.intern(Type::String)
                } else if let Some(dynamic) = self.dynamic_addition_type(
                    op,
                    lhs_ty,
                    rhs_ty,
                    binary.span.start,
                    binary.span.end,
                ) {
                    dynamic
                } else {
                    self.binary_result_type(op, lhs_ty, rhs_ty)
                };
                let span = self.span(binary.span.start, binary.span.end);
                if let Some(call) = self.bigint_fallible_binary(op, lhs, rhs, span, body) {
                    return Ok(call);
                }
                Ok(body.push_expr(Expr {
                    kind: ExprKind::BinOp { op, lhs, rhs },
                    ty,
                    span,
                }))
            }
            Expression::LogicalExpression(logical) => {
                // Fold the `(typeof X === 'object' && X) || ...` global-detection
                // chain before lowering its operands, so dead absent-alias clauses
                // (e.g. `&& window`) never resolve their identifier.
                if let Some(expr) = self.global_detection_chain_expression(logical, body) {
                    return Ok(expr);
                }
                // A guard the target profile already decides short-circuits the
                // whole expression before any operand-shape helper runs: several
                // of them lower the RIGHT operand first, and the dead operand is
                // exactly the one the profile cannot resolve.
                if let Some(expr) = self.short_circuited_static_guard(logical, body) {
                    return Ok(expr);
                }
                // `view && guard` over an assertion view tests the erased value
                // (see `lowering::assertion_views`).
                if let Some(expr) = self.assertion_view_guard(logical, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.same_value_zero_logical(logical, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.logical_or_fallback_expression(logical, body)? {
                    return Ok(expr);
                }
                if logical.operator == LogicalOperator::Coalesce {
                    return self.nullish_coalesce_expression(logical, body, type_hint);
                }
                let lhs = self.expression(&logical.left, body)?;
                let rhs_narrowing = if logical.operator == LogicalOperator::And {
                    self.guard_narrowing(&logical.left, body)
                } else {
                    None
                };
                let rhs = self
                    .with_narrowing(rhs_narrowing, |this| this.expression(&logical.right, body))?;
                // JavaScript's `&&`/`||` select an OPERAND, so in this value
                // position the result is the union of the operand types, not a
                // boolean. `logical_operand_value_expression` builds that
                // selection and returns `None` only where the boolean shape
                // below is still the right one (both operands already boolean,
                // or two operand types with no common lowered shape).
                if let Some(expr) =
                    self.logical_operand_value_expression(logical, body, lhs, rhs)?
                {
                    return Ok(expr);
                }
                let cond = self.lowered_condition_expression(
                    lhs,
                    self.expression_span(&logical.left),
                    body,
                )?;
                let ty = self.ctx.krate.types.intern(Type::Bool);
                let identity = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(logical.operator == LogicalOperator::Or)),
                    ty,
                    span: self.expression_span(&logical.left),
                });
                let (then_expr, else_expr) = if logical.operator == LogicalOperator::And {
                    (rhs, identity)
                } else {
                    (identity, rhs)
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Conditional {
                        cond,
                        then_expr,
                        else_expr,
                    },
                    ty,
                    span: self.span(logical.span.start, logical.span.end),
                }))
            }
            Expression::ConditionalExpression(conditional) => {
                self.conditional_expression(conditional, body, type_hint)
            }
            Expression::UnaryExpression(unary) => {
                if unary.operator == UnaryOperator::Typeof {
                    return self.typeof_expression(unary, body);
                }
                if unary.operator == UnaryOperator::Delete {
                    return self.unary_expression(unary, body);
                }
                // `!(a && b)` tests truthiness only; `unary_expression` lowers
                // the logical operand as a condition.
                if unary.operator == UnaryOperator::LogicalNot
                    && matches!(
                        peel(&unary.argument, Wrappers::Parens),
                        Expression::LogicalExpression(_)
                    )
                {
                    return self.unary_expression(unary, body);
                }
                if unary.operator == UnaryOperator::Void {
                    return self.void_expression(unary, body, Literal::Undefined);
                }
                let op = match unary.operator {
                    UnaryOperator::LogicalNot => UnaryOp::Not,
                    UnaryOperator::UnaryNegation => UnaryOp::Neg,
                    UnaryOperator::UnaryPlus => {
                        let operand = self.expression(&unary.argument, body)?;
                        let operand_ty = Self::expr_ty(body, operand);
                        if matches!(
                            self.ctx.krate.types.get(operand_ty),
                            Some(Type::Int | Type::Float)
                        ) {
                            return Ok(operand);
                        }
                        if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Bool))
                            || self.is_date_constructor_arg_type(operand_ty)
                        {
                            let ty = self.ctx.krate.types.intern(Type::Float);
                            return Ok(body.push_expr(Expr {
                                kind: ExprKind::PrimitiveCast {
                                    op: PrimitiveCastOp::ToJsNumber,
                                    operand,
                                },
                                ty,
                                span: self.span(unary.span.start, unary.span.end),
                            }));
                        }
                        return Err(SmeltError::unsupported(
                            self.span(unary.span.start, unary.span.end),
                            "unary plus requires a numeric or DateArg-compatible operand",
                        ));
                    }
                    UnaryOperator::BitwiseNot => return self.bitwise_not_expression(unary, body),
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(unary.span.start, unary.span.end),
                            format!("unary operator is not lowered yet: {:?}", unary.operator),
                        ));
                    }
                };
                // `!view` probes an assertion view's erased value (see
                // `lowering::assertion_views`).
                let view = if matches!(op, UnaryOp::Not) {
                    self.assertion_view_read(&unary.argument, body)
                } else {
                    None
                };
                let operand = match view {
                    Some(view) => view,
                    None => self.expression(&unary.argument, body)?,
                };
                let operand = if matches!(op, UnaryOp::Not) {
                    let span = self.expression_span(&unary.argument);
                    self.optional_known_date_presence_condition(operand, span, body)
                        .or_else(|| self.keyed_read_presence_condition(operand, span, body))
                        .unwrap_or(operand)
                } else {
                    operand
                };
                let ty = if matches!(op, UnaryOp::Not) {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    Self::expr_ty(body, operand)
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::UnaryOp { op, operand },
                    ty,
                    span: self.span(unary.span.start, unary.span.end),
                }))
            }
            Expression::AwaitExpression(await_expr) => {
                self.await_expression(await_expr, type_hint, body)
            }
            Expression::UpdateExpression(update) => self.update_expression(update, body),
            Expression::StaticMemberExpression(member) => self.static_member(member, body),
            Expression::ComputedMemberExpression(member) => {
                if type_hint.is_some_and(|hint| {
                    matches!(
                        self.ctx.krate.types.get(hint),
                        Some(Type::Unknown | Type::TypeParam { .. })
                    )
                }) && let Some(expr) = self.unknown_computed_member_with_hint(member, body)?
                {
                    return Ok(expr);
                }
                let read = self.computed_member(member, body)?;
                Ok(self.keyed_read_into_optional_slot(read, type_hint, body))
            }
            Expression::CallExpression(call) => {
                // An untyped `JSON.parse` flowing into a declared type is a
                // typed entry boundary; see `json_parse_call_with_hint`.
                if let Some(parsed) = self.json_parse_call_with_hint(call, type_hint, body)? {
                    return Ok(parsed);
                }
                let value = self.call_expression_with_hint(call, body, type_hint)?;
                let value = self.adopt_json_body_read_target(value, type_hint, body);
                // A bare `Array(n)` allocation takes the contextual list type
                // when it has one, exactly as the `new Array(n)` spelling does
                // below; the two forms must stay in lockstep.
                let value = self.adopt_contextual_list_allocation_type(value, type_hint, body);
                let statically_callable =
                    body.expr_at(value)
                        .and_then(|lowered_expr| match &lowered_expr.kind {
                            ExprKind::Call { callee, .. }
                            | ExprKind::ClosureCall { callee, .. } => Some(*callee),
                            _ => None,
                        })
                        .is_some_and(|callee| {
                            matches!(
                                self.ctx.krate.types.get(Self::expr_ty(body, callee)),
                                Some(Type::Function(_))
                            )
                        });
                if let Some(hint) = type_hint
                    && statically_callable
                    && matches!(self.ctx.krate.types.get(hint), Some(Type::Future(_)))
                    && matches!(
                        self.ctx.krate.types.get(Self::expr_ty(body, value)),
                        Some(Type::None | Type::Unknown)
                    )
                    && let Some(contextual_expr) = body.expr_at_mut(value)
                {
                    // Validated contextual future typing recovers an unresolved
                    // conditional/generic call result without changing the call.
                    contextual_expr.ty = hint;
                }
                Ok(value)
            }
            Expression::AssignmentExpression(assign) => {
                if let Some(expr) = self.try_global_assignment_expression(assign, body)? {
                    return Ok(expr);
                }
                self.assignment_expression_value(assign, body)
            }
            Expression::YieldExpression(yield_expr) => {
                if yield_expr.delegate && self.fn_ctx.generator.is_some() {
                    return self.generator_delegate_expression(yield_expr, body);
                }
                if self.fn_ctx.generator.is_some() {
                    return self.generator_yield_expression(yield_expr, body);
                }
                Err(SmeltError::unsupported(
                    self.span(yield_expr.span.start, yield_expr.span.end),
                    "yield is only valid inside a generator",
                ))
            }
            Expression::ArrowFunctionExpression(arrow) => {
                self.arrow_function_expression_with_hint(arrow, body, type_hint)
            }
            Expression::FunctionExpression(function) => {
                self.function_expression_value(function, type_hint, function.span, body)
            }
            Expression::ClassExpression(class) => self.class_expression_value(class, body),
            Expression::ChainExpression(chain) => self.chain_expression(chain, body),
            Expression::TSAsExpression(as_expr) => self.type_assertion_expression(
                &as_expr.expression,
                &as_expr.type_annotation,
                as_expr.span,
                body,
            ),
            Expression::TSTypeAssertion(assertion) => self.type_assertion_expression(
                &assertion.expression,
                &assertion.type_annotation,
                assertion.span,
                body,
            ),
            Expression::TSSatisfiesExpression(satisfies) => {
                self.expression(&satisfies.expression, body)
            }
            Expression::TSNonNullExpression(non_null) => self.non_null_assertion_expression(
                &non_null.expression,
                self.span(non_null.span.start, non_null.span.end),
                body,
            ),
            Expression::ParenthesizedExpression(parenthesized) => {
                self.expression_with_hint(&parenthesized.expression, body, type_hint)
            }
            Expression::NewExpression(new_expr) => {
                self.reject_proxy_into_typed_record(new_expr, type_hint)?;
                let value = self.new_expression_with_hint(new_expr, body, type_hint)?;
                Ok(self.adopt_contextual_list_allocation_type(value, type_hint, body))
            }
            Expression::TemplateLiteral(tpl) => self.template_literal_expression(tpl, body),
            Expression::PrivateFieldExpression(member) => self.private_field_member(
                &member.object,
                member.field.name.as_str(),
                member.span,
                body,
            ),
            Expression::TaggedTemplateExpression(tagged) => {
                self.tagged_template_expression(tagged, body)
            }
            // `(a, b, c)` evaluates every operand in order and yields the last:
            // the earlier ones run as expression statements of the block it is
            // lowered into, as `void x` runs its operand.
            Expression::SequenceExpression(sequence) => {
                let Some((last, effects)) = sequence.expressions.split_last() else {
                    return Err(SmeltError::unsupported(
                        self.expression_span(expression),
                        "an empty comma expression is not valid source",
                    ));
                };
                for effect in effects {
                    let operand = self.expression(effect, body)?;
                    self.push_stmt_to_sink(body, smelt_hir::Stmt::Expr(operand));
                }
                self.expression_with_hint(last, body, type_hint)
            }
            _ => Err(SmeltError::unsupported(
                self.expression_span(expression),
                format!("expression kind is not lowered yet: {expression:?}"),
            )),
        }
    }
}
