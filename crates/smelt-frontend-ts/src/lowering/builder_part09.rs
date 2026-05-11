impl ModuleBuilder<'_> {
    fn instanceof_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(&binary.left, body)?;
        let value_ty = Self::expr_ty(body, value);
        if !matches!(self.ctx.krate.types.get(value_ty), Some(Type::Class { .. })) {
            return Err(SmeltError::unsupported(
                self.span(binary.left.span().start, binary.left.span().end),
                "TypeScript instanceof requires a concrete class-typed left operand",
            ));
        }
        let Expression::Identifier(class_ident) = &binary.right else {
            return Err(SmeltError::unsupported(
                self.span(binary.right.span().start, binary.right.span().end),
                "TypeScript instanceof requires a direct class constructor on the right side",
            ));
        };
        let class_text = class_ident.name.as_str();
        if !self.classes.contains_key(class_text) {
            return Err(SmeltError::unsupported(
                self.span(class_ident.span.start, class_ident.span.end),
                format!("TypeScript instanceof target `{class_text}` is not a lowered class"),
            ));
        }
        let class = self.intern_type_name(class_text);
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::InstanceOf { value, class },
            ty,
            span: self.span(binary.span.start, binary.span.end),
        }))
    }

    /// Lower `typeof value === "kind"` checks for TypeScript `unknown` values.
    fn unknown_typeof_comparison(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality
                | BinaryOperator::StrictInequality
                | BinaryOperator::Equality
                | BinaryOperator::Inequality
        ) {
            return Ok(None);
        }
        let Expression::UnaryExpression(unary) = &binary.left else {
            return Ok(None);
        };
        if unary.operator != UnaryOperator::Typeof {
            return Ok(None);
        }
        let Expression::StringLiteral(kind_lit) = &binary.right else {
            return Ok(None);
        };
        let Some(kind) = unknown_kind_from_typeof(kind_lit.value.as_str()) else {
            return Err(SmeltError::unsupported(
                self.span(kind_lit.span.start, kind_lit.span.end),
                format!(
                    "typeof narrowing kind `{}` is not supported yet",
                    kind_lit.value
                ),
            ));
        };
        let value = self.expression(&unary.argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, value)) != Some(&Type::Unknown) {
            return Ok(None);
        }
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let check = body.push_expr(Expr {
            kind: ExprKind::UnknownIs { value, kind },
            ty: bool_ty,
            span: self.span(binary.span.start, binary.span.end),
        });
        if matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ) {
            return Ok(Some(self.unary_bool_expr(
                UnaryOp::Not,
                check,
                binary.span,
                body,
            )));
        }
        Ok(Some(check))
    }

    /// Lower `value === null` checks for TypeScript `unknown` values.
    fn unknown_null_comparison(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality
                | BinaryOperator::StrictInequality
                | BinaryOperator::Equality
                | BinaryOperator::Inequality
        ) {
            return Ok(None);
        }
        let ((Expression::NullLiteral(_), value_expr) | (value_expr, Expression::NullLiteral(_))) =
            (&binary.left, &binary.right)
        else {
            return Ok(None);
        };
        let value = self.expression(value_expr, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, value)) != Some(&Type::Unknown) {
            return Ok(None);
        }
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let check = body.push_expr(Expr {
            kind: ExprKind::UnknownIs {
                value,
                kind: UnknownKind::Null,
            },
            ty: bool_ty,
            span: self.span(binary.span.start, binary.span.end),
        });
        let negated = matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        );
        if negated {
            return Ok(Some(self.unary_bool_expr(
                UnaryOp::Not,
                check,
                binary.span,
                body,
            )));
        }
        Ok(Some(check))
    }

    /// Lower TypeScript type assertions against `unknown` as checked extractions.
    fn type_assertion_expression(
        &mut self,
        expression: &Expression<'_>,
        annotation: &TSType<'_>,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(expression, body)?;
        let target = self.ts_type_to_hir(annotation)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, value)) == Some(&Type::Unknown)
            && target != Self::expr_ty(body, value)
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast { value, target },
                ty: target,
                span: self.span(span.start, span.end),
            }));
        }
        Ok(value)
    }

    /// Lower a function call argument.
    fn argument(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match argument {
            Argument::NumericLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Float);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::StringLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(lit.value.to_string())),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::BooleanLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::NullLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::Identifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            Argument::BinaryExpression(binary) => self.binary_expression(binary, body),
            Argument::LogicalExpression(logical) => self.logical_expression(logical, body),
            Argument::UnaryExpression(unary) => self.unary_expression(unary, body),
            Argument::ArrayExpression(array) => self.array_expression(array, body, None),
            Argument::ObjectExpression(object) => self.object_expression(object, body, None),
            Argument::CallExpression(call) => self.call_expression(call, body),
            Argument::ComputedMemberExpression(member) => self.computed_member(member, body),
            Argument::StaticMemberExpression(member) => self.static_member(member, body),
            _ => Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                format!("call argument kind is not lowered yet: {argument:?}"),
            )),
        }
    }

    /// Lower supported `Promise.*` calls into shared async runtime operations.
    fn promise_static_call(
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
        if object.name != "Promise" {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "all" => AsyncOp::All,
            "race" => AsyncOp::Race,
            "allSettled" => AsyncOp::AllSettled,
            other => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("Promise.{other} is not lowered yet"),
                ));
            }
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Promise combinators require exactly one array argument",
            ));
        }
        let Some(first_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Promise combinators require exactly one array argument",
            ));
        };
        let Argument::ArrayExpression(array) = first_argument else {
            return Err(SmeltError::unsupported(
                self.span(first_argument.span().start, first_argument.span().end),
                "Promise combinators require an array literal argument",
            ));
        };
        let args = self.promise_array_args(array, body)?;
        let output_ty = match op {
            AsyncOp::All | AsyncOp::AllSettled => {
                let outputs = args
                    .iter()
                    .map(|arg| {
                        self.future_inner_type(Self::expr_ty(body, *arg))
                            .ok_or_else(|| {
                                SmeltError::unsupported(
                                    self.span(array.span.start, array.span.end),
                                    "Promise combinator entries must be Promise<T> values",
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.ctx.krate.types.intern(Type::Tuple(outputs))
            }
            AsyncOp::Race => {
                let Some(first) = args.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "Promise.race requires at least one promise",
                    ));
                };
                self.future_inner_type(Self::expr_ty(body, *first))
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(array.span.start, array.span.end),
                            "Promise.race entries must be Promise<T> values",
                        )
                    })?
            }
            AsyncOp::Sleep | AsyncOp::CreateTask | AsyncOp::WaitFor | AsyncOp::HttpGetText => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("Promise.{op:?} is not lowered yet"),
                ));
            }
        };
        let ty = self.ctx.krate.types.intern(Type::Future(output_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp { op, args },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower small TypeScript timer shims used by async fixtures.
    fn timer_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "setTimeout" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "setTimeout lowering supports the Smelt timer shim shape setTimeout(milliseconds)",
            ));
        }
        let Some(duration_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "setTimeout lowering supports the Smelt timer shim shape setTimeout(milliseconds)",
            ));
        };
        let duration = self.argument(duration_argument, body)?;
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let ty = self.ctx.krate.types.intern(Type::Future(none_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op: AsyncOp::Sleep,
                args: vec![duration],
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return targeted diagnostics for deferred object and collection APIs.
    fn unsupported_object_collection_call(
        &self,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Option<SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        let message = match &member.object {
            Expression::Identifier(object)
                if object.name == "Object"
                    && matches!(member.property.name.as_str(), "fromEntries" | "assign") =>
            {
                "TypeScript Object.fromEntries/Object.assign are not supported yet; object merge/projection semantics need a dedicated mapping"
            }
            _ if matches!(member.property.name.as_str(), "splice" | "replaceAll") => {
                "TypeScript Array.splice/String.replaceAll are not supported yet; mutation and replacement semantics need a dedicated mapping"
            }
            _ => return None,
        };
        Some(SmeltError::unsupported(
            self.span(call.span.start, call.span.end),
            message,
        ))
    }

    /// Lower TypeScript `fetch(url)` into an async HTTP GET text operation.
    fn fetch_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::call_rule(call) != Some(RuleId::TsFetch) {
            return Ok(None);
        }
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "fetch" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch lowering supports fetch(url) with one string argument",
            ));
        }
        let Some(url_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch lowering supports fetch(url) with one string argument",
            ));
        };
        let url = self.argument(url_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, url)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch requires a string URL argument",
            ));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let ty = self.ctx.krate.types.intern(Type::Future(string_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op: AsyncOp::HttpGetText,
                args: vec![url],
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower supported TypeScript `Date` calls.
    fn date_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Expression::StaticMemberExpression(member) = &call.callee
            && matches!(&member.object, Expression::Identifier(object) if object.name == "Date")
            && member.property.name == "now"
        {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.now() does not accept arguments",
                ));
            }
            let ty = self.ctx.krate.types.intern(Type::Int);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateNow,
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }

        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "toISOString" {
            return Ok(None);
        }
        let Expression::NewExpression(new_expr) = &member.object else {
            return Ok(None);
        };
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "Date" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Date.toISOString() does not accept arguments",
            ));
        }
        let timestamp_ms = self.date_constructor_timestamp(new_expr, body)?;
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DateToIsoString { timestamp_ms },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower supported `new Date(...)` expressions to a timestamp value.
    fn new_date_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.date_constructor_timestamp(new_expr, body)
    }

    /// Return the timestamp expression represented by a supported `new Date(...)`.
    fn date_constructor_timestamp(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let [timestamp_arg] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new Date() currently supports exactly one numeric timestamp argument",
            ));
        };
        let timestamp_ms = self.argument(timestamp_arg, body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, timestamp_ms)),
            Some(Type::Int | Type::Float)
        ) {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new Date(timestamp) requires a numeric timestamp",
            ));
        }
        Ok(timestamp_ms)
    }

    // Continued in the next split builder file.
}
