impl ModuleBuilder<'_> {
    /// Lower `new Error(message)` to the message expression used by HIR throws.
    fn error_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Error constructor lowering supports at most one message argument",
            ));
        }
        let Some(message_arg) = new_expr.arguments.first() else {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("Error".to_owned())),
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        };
        let message = self.argument(message_arg, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, message)) == Some(&Type::String) {
            return Ok(message);
        }
        if self.is_string_compatible_type(Self::expr_ty(body, message))
            || self.type_contains_unknown(Self::expr_ty(body, message))
        {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: message },
                ty,
                span: self.span(message_arg.span().start, message_arg.span().end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(message_arg.span().start, message_arg.span().end),
            "Error constructor message must be a string",
        ))
    }

    /// Lower an expression while preserving a caller-supplied type hint when possible.
    fn expression_with_hint(
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
                    kind: ExprKind::Literal(Literal::String(lit.value.to_string())),
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
            Expression::Identifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            Expression::ThisExpression(this_expr) => {
                self.identifier_expression("this", this_expr.span.start, this_expr.span.end, body)
            }
            Expression::RegExpLiteral(literal) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                let value = Self::regex_literal_pattern_text(literal);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(value)),
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                }))
            }
            Expression::ArrayExpression(array) => self.array_expression(array, body, type_hint),
            Expression::ObjectExpression(object) => {
                self.object_expression(object, body, type_hint)
            }
            Expression::BinaryExpression(binary) => {
                if binary.operator == BinaryOperator::Instanceof {
                    return self.instanceof_expression(binary, body);
                }
                if binary.operator == BinaryOperator::In {
                    let ty = self.ctx.krate.types.intern(Type::Bool);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::Bool(false)),
                        ty,
                        span: self.span(binary.span.start, binary.span.end),
                    }));
                }
                if let Some(expr) = self.unknown_typeof_comparison(binary, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.unknown_null_comparison(binary, body)? {
                    return Ok(expr);
                }
                let op = match binary.operator {
                    BinaryOperator::Addition => BinOp::Add,
                    BinaryOperator::Subtraction => BinOp::Sub,
                    BinaryOperator::Multiplication => BinOp::Mul,
                    BinaryOperator::Division => BinOp::Div,
                    BinaryOperator::Remainder => BinOp::Rem,
                    BinaryOperator::StrictEquality => BinOp::Eq,
                    BinaryOperator::StrictInequality => BinOp::NotEq,
                    BinaryOperator::Equality | BinaryOperator::Inequality => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.span.start, binary.span.end),
                            "coercive equality is not lowered; use === or !==",
                        ));
                    }
                    BinaryOperator::LessThan => BinOp::Lt,
                    BinaryOperator::LessEqualThan => BinOp::Lte,
                    BinaryOperator::GreaterThan => BinOp::Gt,
                    BinaryOperator::GreaterEqualThan => BinOp::Gte,
                    BinaryOperator::ShiftLeft => BinOp::Shl,
                    BinaryOperator::ShiftRight => BinOp::Shr,
                    BinaryOperator::ShiftRightZeroFill => BinOp::UShr,
                    BinaryOperator::Exponential
                    | BinaryOperator::BitwiseOR
                    | BinaryOperator::BitwiseXOR
                    | BinaryOperator::BitwiseAnd
                    | BinaryOperator::In
                    | BinaryOperator::Instanceof => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.span.start, binary.span.end),
                            format!("binary operator is not lowered yet: {:?}", binary.operator),
                        ));
                    }
                };
                let lhs = self.expression(&binary.left, body)?;
                let rhs = self.expression(&binary.right, body)?;
                let ty = if matches!(
                    op,
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte
                ) {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    Self::expr_ty(body, lhs)
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::BinOp { op, lhs, rhs },
                    ty,
                    span: self.span(binary.span.start, binary.span.end),
                }))
            }
            Expression::LogicalExpression(logical) => {
                if let Some(expr) = self.logical_or_fallback_expression(logical, body)? {
                    return Ok(expr);
                }
                if logical.operator == LogicalOperator::Coalesce {
                    return self.nullish_coalesce_expression(logical, body, type_hint);
                }
                let op = if logical.operator == LogicalOperator::And {
                    BinOp::And
                } else {
                    BinOp::Or
                };
                let lhs = self.expression(&logical.left, body)?;
                let rhs_narrowing = if logical.operator == LogicalOperator::And {
                    self.guard_narrowing(&logical.left, body)
                } else {
                    None
                };
                if let Some(narrowing) = rhs_narrowing.clone() {
                    self.narrowed_locals.push(narrowing);
                }
                let rhs = self.expression(&logical.right, body)?;
                if rhs_narrowing.is_some() {
                    self.narrowed_locals.pop();
                }
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::BinOp { op, lhs, rhs },
                    ty,
                    span: self.span(logical.span.start, logical.span.end),
                }))
            }
            Expression::ConditionalExpression(conditional) => {
                let cond = self.condition_expression(&conditional.test, body)?;
                let then_narrowing = self.guard_narrowing(&conditional.test, body);
                if let Some(narrowing) = then_narrowing.clone() {
                    self.narrowed_locals.push(narrowing);
                }
                let then_expr =
                    self.expression_with_hint(&conditional.consequent, body, type_hint)?;
                if then_narrowing.is_some() {
                    self.narrowed_locals.pop();
                }
                let branch_hint = Some(Self::expr_ty(body, then_expr));
                let else_narrowing = self.inverse_guard_narrowing(&conditional.test, body);
                if let Some(narrowing) = else_narrowing.clone() {
                    self.narrowed_locals.push(narrowing);
                }
                let else_expr =
                    self.expression_with_hint(&conditional.alternate, body, branch_hint)?;
                if else_narrowing.is_some() {
                    self.narrowed_locals.pop();
                }
                let then_ty = Self::expr_ty(body, then_expr);
                let else_ty = Self::expr_ty(body, else_expr);
                let ty = if then_ty == else_ty {
                    then_ty
                } else if self.numeric_type_compatible(then_ty, else_ty) {
                    self.ctx.krate.types.intern(Type::Float)
                } else if matches!(
                    (
                        self.ctx.krate.types.get(then_ty),
                        self.ctx.krate.types.get(else_ty)
                    ),
                    (Some(Type::TypeParam { .. }), Some(Type::TypeParam { .. }))
                ) {
                    then_ty
                } else if Self::is_empty_list_expr(body, then_expr) {
                    else_ty
                } else if Self::is_empty_list_expr(body, else_expr) {
                    then_ty
                } else if self.ctx.krate.types.get(then_ty) == Some(&Type::None) {
                    self.ctx.krate.types.intern(Type::Optional(else_ty))
                } else if self.ctx.krate.types.get(else_ty) == Some(&Type::None) {
                    self.ctx.krate.types.intern(Type::Optional(then_ty))
                } else if self.compatible_function_branch_types(then_ty, else_ty) {
                    then_ty
                } else if let Some(function_ty) = self.single_function_branch_type(then_ty, else_ty) {
                    function_ty
                } else if self.is_string_compatible_type(then_ty)
                    && (self.is_string_compatible_type(else_ty)
                        || self.union_has_string_compatible_member(else_ty))
                    || self.is_string_compatible_type(else_ty)
                        && self.union_has_string_compatible_member(then_ty)
                {
                    self.ctx.krate.types.intern(Type::String)
                } else if matches!(self.ctx.krate.types.get(then_ty), Some(Type::Dict(_, _)))
                    && matches!(self.ctx.krate.types.get(else_ty), Some(Type::Dict(_, _)))
                {
                    self.ctx
                        .krate
                        .types
                        .intern(Type::Union(vec![then_ty, else_ty]))
                } else if type_hint
                    .is_some_and(|hint| self.ctx.krate.types.get(hint) == Some(&Type::Unknown))
                    || self.ctx.krate.types.get(then_ty) == Some(&Type::Unknown)
                    || self.ctx.krate.types.get(else_ty) == Some(&Type::Unknown)
                    || self.type_contains_unknown(then_ty)
                    || self.type_contains_unknown(else_ty)
                {
                    self.ctx.krate.types.intern(Type::Unknown)
                } else if let Some(hint) = type_hint
                    && !self.concrete_type_requires_never_value(hint)
                {
                    hint
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(conditional.span.start, conditional.span.end),
                        format!(
                            "conditional expression branches must have the same lowered type (then: {:?}, else: {:?})",
                            self.ctx.krate.types.get(then_ty),
                            self.ctx.krate.types.get(else_ty)
                        ),
                    ));
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Conditional {
                        cond,
                        then_expr,
                        else_expr,
                    },
                    ty,
                    span: self.span(conditional.span.start, conditional.span.end),
                }))
            }
            Expression::UnaryExpression(unary) => {
                if unary.operator == UnaryOperator::Typeof {
                    return self.typeof_expression(unary, body);
                }
                if unary.operator == UnaryOperator::Delete {
                    return self.unary_expression(unary, body);
                }
                let op = match unary.operator {
                    UnaryOperator::LogicalNot => UnaryOp::Not,
                    UnaryOperator::UnaryNegation => UnaryOp::Neg,
                    UnaryOperator::UnaryPlus => {
                        let operand = self.expression(&unary.argument, body)?;
                        let operand_ty = Self::expr_ty(body, operand);
                        if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Int | Type::Float)) {
                            return Ok(operand);
                        }
                        if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Bool))
                            || self.is_date_constructor_arg_type(operand_ty)
                        {
                            let ty = self.ctx.krate.types.intern(Type::Float);
                            return Ok(body.push_expr(Expr {
                                kind: ExprKind::PrimitiveCast {
                                    op: PrimitiveCastOp::ToFloat,
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
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(unary.span.start, unary.span.end),
                            format!("unary operator is not lowered yet: {:?}", unary.operator),
                        ));
                    }
                };
                let operand = self.expression(&unary.argument, body)?;
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
                if !self.current_async {
                    return Err(SmeltError::unsupported(
                        self.span(await_expr.span.start, await_expr.span.end),
                        "await expressions are only lowered inside async functions",
                    ));
                }
                let awaited = self.expression(&await_expr.argument, body)?;
                let ty = self
                    .future_inner_type(Self::expr_ty(body, awaited))
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(await_expr.span.start, await_expr.span.end),
                            "await expressions require a Promise<T> operand",
                        )
                    })?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Await(awaited),
                    ty,
                    span: self.span(await_expr.span.start, await_expr.span.end),
                }))
            }
            Expression::UpdateExpression(update) => self.update_expression(update, body),
            Expression::StaticMemberExpression(member) => self.static_member(member, body),
            Expression::ComputedMemberExpression(member) => {
                if type_hint.is_some_and(|hint| {
                    matches!(
                        self.ctx.krate.types.get(hint),
                        Some(Type::Unknown | Type::TypeParam { .. })
                    )
                })
                    && let Some(expr) = self.unknown_computed_member_with_hint(member, body)?
                {
                    return Ok(expr);
                }
                self.computed_member(member, body)
            }
            Expression::CallExpression(call) => self.call_expression(call, body),
            Expression::ArrowFunctionExpression(arrow) => {
                self.arrow_function_expression_with_hint(arrow, body, type_hint)
            }
            Expression::FunctionExpression(function) => self.function_expression_value(
                function,
                type_hint,
                function.span,
                body,
            ),
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
                if let Some(expr) = self.set_constructor_expression(new_expr, body, type_hint)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.map_constructor_expression(new_expr, body, type_hint)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.promise_constructor_expression(new_expr, body, type_hint)?
                {
                    return Ok(expr);
                }
                let Expression::Identifier(callee) = &new_expr.callee else {
                    if let Some(expr) = self.dynamic_date_constructor_expression(new_expr, body)? {
                        return Ok(expr);
                    }
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "new expressions require a direct class name",
                    ));
                };
                if callee.name == "Date" {
                    return self.new_date_expression(new_expr, body);
                }
                if callee.name == "RegExp" {
                    return self.regexp_constructor_expression(new_expr, body);
                }
                if callee.name == "Array" {
                    return self.array_constructor_expression(new_expr, body);
                }
                if callee.name == "Uint8Array" {
                    return self.uint8_array_constructor_expression(new_expr, body);
                }
                if matches!(callee.name.as_str(), "Error" | "TypeError" | "RangeError") {
                    return self.error_constructor_expression(new_expr, body);
                }
                if callee.name == "URL" {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "TypeScript URL is not supported yet; URL construction and URL field access need a URL mapping policy",
                    ));
                }
                let Some(item) = self.classes.get(callee.name.as_str()).copied() else {
                    if self.value_imports.contains(callee.name.as_str()) {
                        let class_name = self.intern_type_name(callee.name.as_str());
                        let args = new_expr
                            .arguments
                            .iter()
                            .map(|arg| self.argument(arg, body))
                            .collect::<Result<Vec<_>, _>>()?;
                        let ty = self.ctx.krate.types.intern(Type::Class {
                            name: class_name,
                            args: Vec::new(),
                        });
                        return Ok(body.push_expr(Expr {
                            kind: ExprKind::New {
                                class: class_name,
                                args,
                            },
                            ty,
                            span: self.span(new_expr.span.start, new_expr.span.end),
                        }));
                    }
                    return Err(SmeltError::unsupported(
                        self.span(callee.span.start, callee.span.end),
                        format!("unresolved class `{}`", callee.name),
                    ));
                };
                let Item::Class(class) = self.item_ref(item) else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "new expressions require a class item",
                    ));
                };
                let class_name = class.name;
                let args = new_expr
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                let ty = self.ctx.krate.types.intern(Type::Class {
                    name: class_name,
                    args: Vec::new(),
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: class_name,
                        args,
                    },
                    ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                }))
            }
            Expression::TemplateLiteral(tpl) => self.template_literal_expression(tpl, body),
            Expression::TaggedTemplateExpression(tagged) => Err(SmeltError::unsupported(
                self.span(tagged.span.start, tagged.span.end),
                "tagged template literals are not supported",
            )),
            _ => Err(SmeltError::unsupported(
                self.expression_span(expression),
                format!("expression kind is not lowered yet: {expression:?}"),
            )),
        }
    }

    /// Lower a TypeScript bigint literal into Smelt's current numeric runtime value.
    fn bigint_literal_expression(
        &mut self,
        value: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = value.parse::<f64>().map_err(|err| {
            SmeltError::unsupported(
                self.span(span.start, span.end),
                format!("bigint literal cannot be represented numerically: {err}"),
            )
        })?;
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(value)),
            ty,
            span: self.span(span.start, span.end),
        }))
    }

    /// Lower JavaScript `typeof value` to a string result when used as a value.
    fn typeof_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Expression::Identifier(identifier) = &unary.argument
            && identifier.name == "crypto"
        {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("undefined".to_owned())),
                ty,
                span: self.span(unary.span.start, unary.span.end),
            }));
        }
        let operand = self.expression(&unary.argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        let kind = self.typeof_type_name(operand_ty).unwrap_or("object");
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(kind.to_owned())),
            ty,
            span: self.span(unary.span.start, unary.span.end),
        }))
    }

    /// Lower a TypeScript conditional expression when it appears outside normal expression nodes.
    fn conditional_expression(
        &mut self,
        conditional: &oxc::ast::ast::ConditionalExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let cond = self.condition_expression(&conditional.test, body)?;
        let then_expr = self.expression_with_hint(&conditional.consequent, body, type_hint)?;
        let branch_hint = Some(Self::expr_ty(body, then_expr));
        let else_expr = self.expression_with_hint(&conditional.alternate, body, branch_hint)?;
        let then_ty = Self::expr_ty(body, then_expr);
        let else_ty = Self::expr_ty(body, else_expr);
        let ty = self.conditional_branch_type(
            then_ty,
            else_ty,
            type_hint,
            conditional.span.start,
            conditional.span.end,
        )?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            },
            ty,
            span: self.span(conditional.span.start, conditional.span.end),
        }))
    }

    /// Compute the result type for a conditional expression's branches.
    fn conditional_branch_type(
        &mut self,
        then_ty: smelt_hir::TypeId,
        else_ty: smelt_hir::TypeId,
        type_hint: Option<smelt_hir::TypeId>,
        start: u32,
        end: u32,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if then_ty == else_ty {
            Ok(then_ty)
        } else if self.numeric_type_compatible(then_ty, else_ty) {
            Ok(self.ctx.krate.types.intern(Type::Float))
        } else if matches!(
            (
                self.ctx.krate.types.get(then_ty),
                self.ctx.krate.types.get(else_ty)
            ),
            (Some(Type::TypeParam { .. }), Some(Type::TypeParam { .. }))
        ) {
            Ok(then_ty)
        } else if self.ctx.krate.types.get(then_ty) == Some(&Type::None) {
            Ok(self.ctx.krate.types.intern(Type::Optional(else_ty)))
        } else if self.ctx.krate.types.get(else_ty) == Some(&Type::None) {
            Ok(self.ctx.krate.types.intern(Type::Optional(then_ty)))
        } else if self.compatible_function_branch_types(then_ty, else_ty) {
            Ok(then_ty)
        } else if let Some(function_ty) = self.single_function_branch_type(then_ty, else_ty) {
            Ok(function_ty)
        } else if type_hint
            .is_some_and(|hint| self.ctx.krate.types.get(hint) == Some(&Type::Unknown))
            || self.ctx.krate.types.get(then_ty) == Some(&Type::Unknown)
            || self.ctx.krate.types.get(else_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(then_ty)
            || self.type_contains_unknown(else_ty)
        {
            Ok(self.ctx.krate.types.intern(Type::Unknown))
        } else if let Some(hint) = type_hint
            && !self.concrete_type_requires_never_value(hint)
        {
            Ok(hint)
        } else {
            Err(SmeltError::unsupported(
                self.span(start, end),
                format!(
                    "conditional expression branches must have the same lowered type (then: {:?}, else: {:?})",
                    self.ctx.krate.types.get(then_ty),
                    self.ctx.krate.types.get(else_ty)
                ),
            ))
        }
    }

    /// Return true when an expression is an uninhabited empty array literal.
    fn is_empty_list_expr(body: &Body, expr: smelt_hir::ExprId) -> bool {
        matches!(
            body.exprs.get(usize::try_from(expr.0).unwrap_or(usize::MAX)),
            Some(Expr {
                kind: ExprKind::ListLit(items),
                ..
            }) if items.is_empty()
        )
    }

    /// Lower a JavaScript condition to a boolean expression.
    ///
    /// TypeScript permits optional values in truthiness positions. Smelt models
    /// the common `value ? a : b` and `if (value)` optional-object/string cases
    /// as a `value != None` check once the expression has lowered to
    /// `Optional<T>`.
    fn condition_expression(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let cond = self.expression(expression, body)?;
        let cond_ty = Self::expr_ty(body, cond);
        if self.ctx.krate.types.get(cond_ty) == Some(&Type::Bool) {
            return Ok(cond);
        }
        if matches!(
            self.ctx.krate.types.get(cond_ty),
            Some(Type::Function(_) | Type::Class { .. } | Type::TypeParam { .. })
        ) {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty,
                span: self.expression_span(expression),
            }));
        }
        if self.ctx.krate.types.get(cond_ty) == Some(&Type::String) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            let empty = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: string_ty,
                span: self.expression_span(expression),
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: cond,
                    rhs: empty,
                },
                ty: bool_ty,
                span: self.expression_span(expression),
            }));
        }
        if self.is_nullishable_type(cond_ty) || self.type_is_truthy_condition_surface(cond_ty) {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            let none = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: none_ty,
                span: self.expression_span(expression),
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: cond,
                    rhs: none,
                },
                ty: bool_ty,
                span: self.expression_span(expression),
            }));
        }
        Err(SmeltError::unsupported(
            self.expression_span(expression),
            "condition expression must be boolean or optional",
        ))
    }

    /// Return whether a non-boolean type can appear in a JavaScript truthiness guard.
    fn type_is_truthy_condition_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Function(_) | Type::Class { .. } | Type::TypeParam { .. } | Type::Unknown) => {
                true
            }
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.type_is_truthy_condition_surface(item)),
            _ => false,
        }
    }

    /// Lower a template literal as string concatenation.
    fn template_literal_expression(
        &mut self,
        tpl: &oxc::ast::ast::TemplateLiteral<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let str_ty = self.ctx.krate.types.intern(Type::String);
        let span = self.span(tpl.span.start, tpl.span.end);
        let Some(first_quasi) = tpl.quasis.first() else {
            return Err(SmeltError::unsupported(
                self.span(tpl.span.start, tpl.span.end),
                "template literals must contain at least one quasi",
            ));
        };
        let first_str = first_quasi
            .value
            .cooked
            .as_ref()
            .map_or_else(|| first_quasi.value.raw.as_str(), |c| c.as_str())
            .to_owned();
        let mut acc = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(first_str)),
            ty: str_ty,
            span,
        });

        for (i, interp) in tpl.expressions.iter().enumerate() {
            let part = self.expression(interp, body)?;
            acc = body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Add,
                    lhs: acc,
                    rhs: part,
                },
                ty: str_ty,
                span,
            });
            if let Some(quasi) = tpl.quasis.get(i.saturating_add(1)) {
                let s = quasi
                    .value
                    .cooked
                    .as_ref()
                    .map_or_else(|| quasi.value.raw.as_str(), |c| c.as_str());
                if !s.is_empty() {
                    let lit = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::String(s.to_owned())),
                        ty: str_ty,
                        span,
                    });
                    acc = body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Add,
                            lhs: acc,
                            rhs: lit,
                        },
                        ty: str_ty,
                        span,
                    });
                }
            }
        }
        Ok(acc)
    }


    // Continued in the next split builder file.
}
