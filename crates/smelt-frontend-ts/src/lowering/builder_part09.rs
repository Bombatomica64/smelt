impl ModuleBuilder<'_> {
    /// Lower a TypeScript `instanceof` binary expression into a HIR predicate.
    fn instanceof_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(&binary.left, body)?;
        let value_ty = Self::expr_ty(body, value);
        if Self::instanceof_date_target(&binary.right) && !self.instanceof_concrete_class(value_ty) {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(false)),
                ty,
                span: self.span(binary.span.start, binary.span.end),
            }));
        }
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

    /// Return true when an expression is the built-in `Date` constructor target.
    fn instanceof_date_target(target: &Expression<'_>) -> bool {
        matches!(target, Expression::Identifier(class_ident) if class_ident.name == "Date")
    }

    /// Return true when `instanceof` can be emitted as a concrete HIR class check.
    fn instanceof_concrete_class(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(self.ctx.krate.types.get(ty), Some(Type::Class { .. }))
    }

    /// Lower `typeof value === "kind"` checks using known HIR types when possible.
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
        let expected = kind_lit.value.as_str();
        let value = self.expression(&unary.argument, body)?;
        let value_ty = Self::expr_ty(body, value);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        if self.ctx.krate.types.get(value_ty) != Some(&Type::Unknown) {
            let matches_kind = self.typeof_type_name(value_ty).is_some_and(|actual| actual == expected);
            let result = if matches!(
                binary.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            ) {
                !matches_kind
            } else {
                matches_kind
            };
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(result)),
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            })));
        }
        if kind == UnknownKind::Function {
            return Err(SmeltError::unsupported(
                self.span(kind_lit.span.start, kind_lit.span.end),
                "runtime typeof function checks are not supported for unknown values",
            ));
        }
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

    /// Return the JavaScript `typeof` string represented by a lowered type.
    fn typeof_type_name(&self, ty: smelt_hir::TypeId) -> Option<&'static str> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Bool) => Some("boolean"),
            Some(Type::Int | Type::Float) => Some("number"),
            Some(Type::String) => Some("string"),
            Some(Type::Function(_)) => Some("function"),
            Some(Type::None) => Some("undefined"),
            Some(
                Type::List(_)
                | Type::Set(_)
                | Type::Dict(_, _)
                | Type::Tuple(_)
                | Type::Class { .. }
                | Type::Optional(_),
            ) => Some("object"),
            Some(
                Type::Unknown
                | Type::Never
                | Type::Union(_)
                | Type::Future(_)
                | Type::TypeParam { .. },
            )
            | None => {
                None
            }
        }
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
        if Self::is_const_type_assertion(annotation) {
            return self.expression(expression, body);
        }
        let target = self.ts_type_to_hir(annotation)?;
        if self.concrete_type_requires_never_value(target) {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "type assertion cannot construct a never value",
            ));
        }
        let value = self.expression_with_hint(expression, body, Some(target))?;
        if Self::expr_ty(body, value) == target {
            return Ok(value);
        }
        if self.ctx.krate.types.get(Self::expr_ty(body, value)) == Some(&Type::Unknown)
            && target != Self::expr_ty(body, value)
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast { value, target },
                ty: target,
                span: self.span(span.start, span.end),
            }));
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: target,
            span: self.span(span.start, span.end),
        }))
    }

    /// Return whether a TypeScript assertion is the runtime-erased `as const` form.
    fn is_const_type_assertion(annotation: &TSType<'_>) -> bool {
        matches!(
            annotation,
            TSType::TSTypeReference(reference)
                if matches!(
                    &reference.type_name,
                    TSTypeName::IdentifierReference(name) if name.name == "const"
                )
        )
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
            Argument::ChainExpression(chain) => self.chain_expression(chain, body),
            Argument::TemplateLiteral(template) => self.template_literal_expression(template, body),
            Argument::TSAsExpression(as_expr) => self.type_assertion_expression(
                &as_expr.expression,
                &as_expr.type_annotation,
                as_expr.span,
                body,
            ),
            Argument::TSTypeAssertion(assertion) => self.type_assertion_expression(
                &assertion.expression,
                &assertion.type_annotation,
                assertion.span,
                body,
            ),
            Argument::TSSatisfiesExpression(satisfies) => {
                let target = self.ts_type_to_hir(&satisfies.type_annotation)?;
                self.expression_with_hint(&satisfies.expression, body, Some(target))
            }
            Argument::NewExpression(new_expr) => {
                let Expression::Identifier(callee) = &new_expr.callee else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "call argument new expressions require a direct class name",
                    ));
                };
                if callee.name == "Date" {
                    self.new_date_expression(new_expr, body)
                } else {
                    Err(SmeltError::unsupported(
                        self.span(callee.span.start, callee.span.end),
                        format!("unsupported call argument constructor `{}`", callee.name),
                    ))
                }
            }
            Argument::ComputedMemberExpression(member) => self.computed_member(member, body),
            Argument::StaticMemberExpression(member) => self.static_member(member, body),
            _ => Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                format!("call argument kind is not lowered yet: {argument:?}"),
            )),
        }
    }

    /// Lower a call argument with an expected type for literals that need contextual typing.
    fn argument_with_hint(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match argument {
            Argument::ArrayExpression(array) => self.array_expression(array, body, type_hint),
            Argument::ObjectExpression(object) => self.object_expression(object, body, type_hint),
            Argument::TSAsExpression(as_expr) => self.type_assertion_expression(
                &as_expr.expression,
                &as_expr.type_annotation,
                as_expr.span,
                body,
            ),
            Argument::TSTypeAssertion(assertion) => self.type_assertion_expression(
                &assertion.expression,
                &assertion.type_annotation,
                assertion.span,
                body,
            ),
            Argument::TSSatisfiesExpression(satisfies) => {
                let target = self.ts_type_to_hir(&satisfies.type_annotation)?;
                self.expression_with_hint(&satisfies.expression, body, Some(target))
            }
            _ => self.argument(argument, body),
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
            _ if member.property.name == "replaceAll" => {
                "TypeScript String.replaceAll is not supported yet; replacement semantics need a dedicated mapping"
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
        if matches!(&call.callee, Expression::StaticMemberExpression(_))
            && stdlib_dispatch::call_rule(call) == Some(RuleId::TsDateNow)
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
        if let Some(expr) = self.date_utc_call(member, call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.date_member_call(member, call, body)? {
            return Ok(Some(expr));
        }
        if stdlib_dispatch::call_rule(call) != Some(RuleId::TsDateToIsoString) {
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

    /// Lower `Date.UTC(year, month, ...)` into Smelt's timestamp-from-parts form.
    fn date_utc_call(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Date" || member.property.name != "UTC" {
            return Ok(None);
        }
        if !(2..=7).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Date.UTC requires between two and seven numeric arguments",
            ));
        }
        let mut parts = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            let part = self.argument(argument, body)?;
            if !matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, part)),
                Some(Type::Int | Type::Float)
            ) {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "Date.UTC arguments must be numeric",
                ));
            }
            parts.push(part);
        }
        let ty = self.ctx.krate.types.intern(Type::Int);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DateFromParts { parts },
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

    /// Lower `new (date.constructor as DateCtor)(value)` for timestamp-mode Dates.
    fn dynamic_date_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !Self::is_constructor_member_reference(&new_expr.callee) {
            return Ok(None);
        }
        let [value] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "dynamic Date constructor calls require exactly one value argument",
            ));
        };
        Ok(Some(self.argument(value, body)?))
    }

    /// Return true for expressions that reference a `.constructor` member.
    fn is_constructor_member_reference(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::StaticMemberExpression(member) => member.property.name == "constructor",
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::is_constructor_member_reference(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::is_constructor_member_reference(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::is_constructor_member_reference(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::is_constructor_member_reference(&non_null.expression)
            }
            _ => false,
        }
    }

    /// Return the timestamp expression represented by a supported `new Date(...)`.
    fn date_constructor_timestamp(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() >= 2 {
            if new_expr.arguments.len() > 7 {
                return Err(SmeltError::unsupported(
                    self.span(new_expr.span.start, new_expr.span.end),
                    "new Date(year, month, ...) supports at most seven numeric arguments",
                ));
            }
            let mut parts = Vec::with_capacity(new_expr.arguments.len());
            for argument in &new_expr.arguments {
                let part = self.argument(argument, body)?;
                if !matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, part)),
                    Some(Type::Int | Type::Float)
                ) {
                    return Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        "Date constructor parts must be numeric",
                    ));
                }
                parts.push(part);
            }
            let ty = self.ctx.krate.types.intern(Type::Int);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DateFromParts { parts },
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        }
        let [timestamp_arg] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new Date() currently supports exactly one numeric timestamp argument",
            ));
        };
        let timestamp_ms = self.argument(timestamp_arg, body)?;
        let timestamp_ty = Self::expr_ty(body, timestamp_ms);
        if matches!(self.ctx.krate.types.get(timestamp_ty), Some(Type::Int | Type::Float)) {
            return Ok(timestamp_ms);
        }
        if self.is_date_constructor_arg_type(timestamp_ty) {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToFloat,
                    operand: timestamp_ms,
                },
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(new_expr.span.start, new_expr.span.end),
            "new Date(timestamp) requires a numeric or DateArg-compatible timestamp",
        ))
    }

    /// Return true for types accepted by JavaScript's one-argument Date constructor.
    fn is_date_constructor_arg_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Int
                | Type::Float
                | Type::String
                | Type::Unknown
                | Type::TypeParam { .. }
                | Type::Class { .. },
            ) => true,
            Some(Type::Optional(item)) => self.is_date_constructor_arg_type(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| {
                    matches!(self.ctx.krate.types.get(item), Some(Type::None))
                        || self.is_date_constructor_arg_type(item)
                }),
            _ => false,
        }
    }

    /// Lower supported Date receiver methods using Smelt's timestamp Date model.
    fn date_member_call(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let method = member.property.name.as_str();
        if method == "getTime" {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.getTime() does not accept arguments",
                ));
            }
            return Ok(Some(self.expression(&member.object, body)?));
        }
        let getter_part = match method {
            "getFullYear" => Some(DatePart::FullYear),
            "getMonth" => Some(DatePart::Month),
            "getDate" => Some(DatePart::Date),
            "getDay" => Some(DatePart::Day),
            "getHours" => Some(DatePart::Hour),
            "getMinutes" => Some(DatePart::Minute),
            "getSeconds" => Some(DatePart::Second),
            "getMilliseconds" => Some(DatePart::Millisecond),
            _ => None,
        };
        if let Some(part) = getter_part {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("Date.{method}() does not accept arguments"),
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let timestamp_ms = self.date_receiver_timestamp(receiver, member, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateGetPart { part, timestamp_ms },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let setter_part = match method {
            "setFullYear" => Some(DatePart::FullYear),
            "setMonth" => Some(DatePart::Month),
            "setDate" => Some(DatePart::Date),
            "setHours" => Some(DatePart::Hour),
            "setMinutes" => Some(DatePart::Minute),
            "setSeconds" => Some(DatePart::Second),
            "setMilliseconds" => Some(DatePart::Millisecond),
            _ => None,
        };
        let Some(part) = setter_part else {
            return Ok(None);
        };
        if call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Date.{method}() requires at least one numeric argument"),
            ));
        }
        let receiver = self.expression(&member.object, body)?;
        let timestamp_ms = self.date_receiver_timestamp(receiver, member, body)?;
        let mut values = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            let value = self.argument(argument, body)?;
            if !matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, value)),
                Some(Type::Int | Type::Float)
            ) {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    format!("Date.{method}() arguments must be numeric"),
                ));
            }
            values.push(value);
        }
        let ty = Self::expr_ty(body, timestamp_ms);
        let value = body.push_expr(Expr {
            kind: ExprKind::DateSetPart {
                part,
                timestamp_ms,
                values,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        });
        if let Expression::Identifier(identifier) = &member.object
            && let Some(local) = self.locals.get(identifier.name.as_str()).copied()
        {
            let target = body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span: self.span(identifier.span.start, identifier.span.end),
            });
            body.push_stmt(Stmt::Assign { target, value });
        }
        Ok(Some(value))
    }

    /// Convert a Date-like receiver into the timestamp expression used by Date operations.
    fn date_receiver_timestamp(
        &mut self,
        receiver: smelt_hir::ExprId,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let receiver_ty = Self::expr_ty(body, receiver);
        if matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::Int | Type::Float)) {
            return Ok(receiver);
        }
        if self.is_date_constructor_arg_type(receiver_ty) {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToFloat,
                    operand: receiver,
                },
                ty,
                span: self.span(member.object.span().start, member.object.span().end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(member.object.span().start, member.object.span().end),
            "Date method receiver must be a timestamp or Date-like value",
        ))
    }

    // Continued in the next split builder file.
}
