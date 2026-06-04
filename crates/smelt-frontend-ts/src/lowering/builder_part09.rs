impl ModuleBuilder<'_> {
    /// Lower a TypeScript `instanceof` binary expression into a HIR predicate.
    fn instanceof_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(&binary.left, body)?;
        let value_ty = Self::expr_ty(body, value);
        if let Expression::StaticMemberExpression(member) = &binary.right
            && (self
                .namespace_member_name(member)
                .is_some_and(|(namespace, _)| self.namespace_imports.contains(namespace))
                || matches!(
                    &member.object,
                    Expression::Identifier(object) if self.value_imports.contains(object.name.as_str())
                ))
        {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(false)),
                ty,
                span: self.span(binary.span.start, binary.span.end),
            }));
        }
        let Expression::Identifier(class_ident) = &binary.right else {
            return Err(SmeltError::unsupported(
                self.span(binary.right.span().start, binary.right.span().end),
                "TypeScript instanceof requires a direct class constructor on the right side",
            ));
        };
        let class_text = class_ident.name.as_str();
        if class_text == "Date" && self.expression_is_known_date_value(value, body) {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty,
                span: self.span(binary.span.start, binary.span.end),
            }));
        }
        if Self::instanceof_fold_false_builtin_target(class_text)
            && !(class_text == "Date"
                && matches!(
                    self.ctx.krate.types.get(value_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
                ))
            && !self.instanceof_concrete_class(value_ty)
        {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(false)),
                ty,
                span: self.span(binary.span.start, binary.span.end),
            }));
        }
        let builtin_target = Self::instanceof_builtin_target(class_text);
        if !self.instanceof_supported_left_operand(value_ty) {
            return Err(SmeltError::unsupported(
                self.span(binary.left.span().start, binary.left.span().end),
                "TypeScript instanceof requires a concrete class-typed left operand",
            ));
        }
        if !builtin_target
            && !self.classes.contains_key(class_text)
            && !self.value_imports.contains(class_text)
        {
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

    /// Return true when an expression is a built-in constructor target.
    fn instanceof_builtin_target(target: &str) -> bool {
        matches!(
            target,
            "Date"
                | "Map"
                | "Set"
                | "RegExp"
                | "Promise"
                | "Error"
                | "EvalError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "TypeError"
                | "URIError"
                | "AggregateError"
        )
    }

    /// Return true for builtin targets represented by non-class HIR values today.
    fn instanceof_fold_false_builtin_target(target: &str) -> bool {
        matches!(target, "Date" | "Map" | "Set" | "RegExp")
    }

    /// Return true when `instanceof` can be emitted as a concrete HIR class check.
    fn instanceof_concrete_class(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(self.ctx.krate.types.get(ty), Some(Type::Class { .. }))
    }

    /// Return true when a timestamp-backed expression is statically known to be a JavaScript Date.
    ///
    /// Date values use numeric timestamps in generated Rust, so runtime Rust
    /// type inspection cannot distinguish them from arbitrary source numbers.
    /// TypeScript still guarantees `Date` and `T extends Date` values satisfy
    /// `instanceof Date`, and direct Date constructors retain that provenance
    /// until this predicate is lowered.
    fn type_is_known_date_value(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Class { name, .. }) => {
                self.ctx.krate.symbols.get(*name) == Some("Date")
            }
            Some(Type::Optional(inner)) => self.type_is_known_date_value(*inner),
            Some(Type::Union(items)) => {
                let values = items
                    .iter()
                    .copied()
                    .filter(|item| self.ctx.krate.types.get(*item) != Some(&Type::None))
                    .collect::<Vec<_>>();
                !values.is_empty()
                    && values
                        .into_iter()
                        .all(|item| self.type_is_known_date_value(item))
            }
            _ => false,
        }
    }

    /// Return true when an expression carries JavaScript `Date` identity despite timestamp storage.
    fn expression_is_known_date_value(&self, value: smelt_hir::ExprId, body: &Body) -> bool {
        let Some(expr) = body
            .exprs
            .get(usize::try_from(value.0).unwrap_or(usize::MAX))
        else {
            return false;
        };
        if self.type_is_known_date_value(expr.ty) {
            return true;
        }
        match &expr.kind {
            ExprKind::DateFromParts { .. } | ExprKind::DateFromValue { .. } => true,
            ExprKind::Local(local) => self.date_value_locals.contains(local),
            ExprKind::TypeAssert { value: asserted_value } => {
                self.expression_is_known_date_value(*asserted_value, body)
            }
            ExprKind::Call { callee, .. } => body
                .exprs
                .get(usize::try_from(callee.0).unwrap_or(usize::MAX))
                .and_then(|callee| match callee.kind {
                    ExprKind::Item(item) => Some(item),
                    _ => None,
                })
                .is_some_and(|item| self.ctx.date_returning_functions.contains(&item)),
            _ => false,
        }
    }

    /// Return true when an `instanceof` left operand can participate in a lowered guard.
    fn instanceof_supported_left_operand(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(
                Type::Class { .. }
                | Type::Unknown
                | Type::TypeParam { .. }
                | Type::Future(_)
                | Type::Float
                | Type::Int
                | Type::String
                | Type::Bool
                | Type::None,
            ) => true,
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.instanceof_supported_left_operand(item)),
            Some(Type::Optional(item)) => self.instanceof_supported_left_operand(*item),
            _ => false,
        }
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
        if kind_lit.value.as_str() == "undefined"
            && let Expression::Identifier(identifier) = &unary.argument
            && identifier.name == "crypto"
        {
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            let result = !matches!(
                binary.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            );
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(result)),
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            })));
        }
        if kind_lit.value.as_str() == "undefined" {
            let value = self.expression(&unary.argument, body)?;
            let value_ty = Self::expr_ty(body, value);
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            if matches!(self.ctx.krate.types.get(value_ty), Some(Type::Optional(_))) {
                let check = body.push_expr(Expr {
                    kind: ExprKind::UnknownIs {
                        value,
                        kind: UnknownKind::Null,
                    },
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
                return Ok(Some(check));
            }
            let matches_kind = self.type_matches_typeof(value_ty, "undefined");
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
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(value_ty).cloned()
            && self.static_typeof_match(inner, expected) == Some(true)
        {
            let absent = body.push_expr(Expr {
                kind: ExprKind::UnknownIs {
                    value,
                    kind: UnknownKind::Null,
                },
                ty: bool_ty,
                span: self.span(binary.span.start, binary.span.end),
            });
            let present = self.unary_bool_expr(UnaryOp::Not, absent, binary.span, body);
            if matches!(
                binary.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            ) {
                return Ok(Some(self.unary_bool_expr(
                    UnaryOp::Not,
                    present,
                    binary.span,
                    body,
                )));
            }
            return Ok(Some(present));
        }
        if let Some(matches_kind) = self.static_typeof_match(value_ty, expected) {
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

    /// Return a static `typeof` comparison result when all runtime variants agree.
    fn static_typeof_match(&self, ty: smelt_hir::TypeId, expected: &str) -> Option<bool> {
        let resolved_ty = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(resolved_ty).cloned() {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Future(_)) => None,
            Some(Type::Class { name, .. })
                if self.ctx.krate.symbols.get(name) == Some("PropertyKey") =>
            {
                None
            }
            Some(Type::Union(items)) => {
                let mut matches = items
                    .into_iter()
                    .map(|item| self.static_typeof_match(item, expected))
                    .collect::<Option<Vec<_>>>()?
                    .into_iter();
                let first = matches.next()?;
                matches.all(|item| item == first).then_some(first)
            }
            Some(Type::Optional(inner)) => {
                let present = self.static_typeof_match(inner, expected)?;
                let absent = expected == "undefined";
                (present == absent).then_some(present)
            }
            Some(_) => Some(self.type_matches_typeof(resolved_ty, expected)),
            None => None,
        }
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
            | None => None,
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
        let Some(value_expr) = Self::nullish_comparison_value(&binary.left, &binary.right) else {
            return Ok(None);
        };
        let value = self.expression(value_expr, body)?;
        let Some(value_expression) = body
            .exprs
            .get(usize::try_from(value.0).unwrap_or(usize::MAX))
        else {
            return Ok(None);
        };
        let value = match &value_expression.kind {
            ExprKind::UnknownCast { value: erased, .. }
                if body
                    .exprs
                    .get(usize::try_from(erased.0).unwrap_or(usize::MAX))
                    .is_some_and(|erased_expr| matches!(erased_expr.kind, ExprKind::Local(local)
                        if self.ctx.krate.types.get(Self::local_ty(body, local)) == Some(&Type::Unknown))) =>
            {
                *erased
            }
            _ => value,
        };
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        if self.ctx.krate.types.get(Self::expr_ty(body, value)) != Some(&Type::Unknown) {
            let none = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: self.ctx.krate.types.intern(Type::None),
                span: self.span(binary.span.start, binary.span.end),
            });
            let op = if matches!(
                binary.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            ) {
                BinOp::NotEq
            } else {
                BinOp::Eq
            };
            return Ok(Some(self.comparison_expr(
                op,
                value,
                none,
                binary.span,
                body,
            )));
        }
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

    /// Return the non-nullish operand for `value == null/undefined` comparisons.
    fn nullish_comparison_value<'a>(
        left: &'a Expression<'a>,
        right: &'a Expression<'a>,
    ) -> Option<&'a Expression<'a>> {
        if Self::is_nullish_expression(left) {
            Some(right)
        } else if Self::is_nullish_expression(right) {
            Some(left)
        } else {
            None
        }
    }

    /// Return whether an expression is JavaScript `null` or `undefined`.
    fn is_nullish_expression(expression: &Expression<'_>) -> bool {
        matches!(expression, Expression::NullLiteral(_))
            || matches!(expression, Expression::Identifier(identifier) if identifier.name == "undefined")
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
        if self.concrete_type_requires_never_value(target)
            && !Self::is_empty_object_expression(expression)
        {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "type assertion cannot construct a never value",
            ));
        }
        if let Some(parsed) = self.json_parse_call_with_target(expression, target, span, body)? {
            return Ok(parsed);
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

    /// Return whether an expression is an empty object literal after TS-only wrappers.
    fn is_empty_object_expression(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::ObjectExpression(object) => object.properties.is_empty(),
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::is_empty_object_expression(&parenthesized.expression)
            }
            Expression::TSAsExpression(assertion) => {
                Self::is_empty_object_expression(&assertion.expression)
            }
            Expression::TSSatisfiesExpression(assertion) => {
                Self::is_empty_object_expression(&assertion.expression)
            }
            Expression::TSNonNullExpression(assertion) => {
                Self::is_empty_object_expression(&assertion.expression)
            }
            _ => false,
        }
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
            Argument::BigIntLiteral(lit) => {
                self.bigint_literal_expression(lit.value.as_str(), lit.span, body)
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
            Argument::RegExpLiteral(literal) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                let value = Self::regex_literal_pattern_text(literal);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(value)),
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                }))
            }
            Argument::Identifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            Argument::ThisExpression(this_expr) => {
                self.identifier_expression("this", this_expr.span.start, this_expr.span.end, body)
            }
            Argument::Super(super_expr) => {
                self.identifier_expression("this", super_expr.span.start, super_expr.span.end, body)
            }
            Argument::BinaryExpression(binary) => {
                if binary.operator == BinaryOperator::Instanceof {
                    return self.instanceof_expression(binary, body);
                }
                if binary.operator == BinaryOperator::In {
                    return self.in_expression(binary, body);
                }
                self.binary_expression(binary, body)
            }
            Argument::ConditionalExpression(conditional) => {
                self.conditional_expression(conditional, body, None)
            }
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
            Argument::TSNonNullExpression(non_null) => self.non_null_assertion_expression(
                &non_null.expression,
                self.span(non_null.span.start, non_null.span.end),
                body,
            ),
            Argument::NewExpression(new_expr) => {
                self.new_expression_with_hint(new_expr, body, None)
            }
            Argument::ComputedMemberExpression(member) => self.computed_member(member, body),
            Argument::StaticMemberExpression(member) => self.static_member(member, body),
            Argument::AwaitExpression(await_expr) => {
                if !self.current_async {
                    return Err(SmeltError::unsupported(
                        self.span(await_expr.span.start, await_expr.span.end),
                        "await expressions are only lowered inside async functions",
                    ));
                }
                let awaited = self.expression(&await_expr.argument, body)?;
                let awaited_ty = Self::expr_ty(body, awaited);
                let Some(ty) = self.future_inner_type(awaited_ty) else {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(await_expr.span.start, await_expr.span.end),
                    }));
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Await(awaited),
                    ty,
                    span: self.span(await_expr.span.start, await_expr.span.end),
                }))
            }
            Argument::ArrowFunctionExpression(arrow) => self.arrow_function_expression(arrow, body),
            Argument::FunctionExpression(function) => {
                if function.r#async {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(function.span.start, function.span.end),
                    }));
                }
                self.function_expression_value(function, None, function.span, body)
            }
            Argument::TSInstantiationExpression(instantiation) => {
                self.expression(&instantiation.expression, body)
            }
            Argument::SpreadElement(spread) => self.expression(&spread.argument, body),
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
            Argument::ArrowFunctionExpression(arrow) => {
                self.arrow_function_expression_with_hint(arrow, body, type_hint)
            }
            Argument::FunctionExpression(function) => {
                self.function_expression_value(function, type_hint, function.span, body)
            }
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
            Argument::TSNonNullExpression(non_null) => {
                let value = self.expression_with_hint(&non_null.expression, body, type_hint)?;
                Ok(self.non_null_assertion_value(
                    value,
                    self.span(non_null.span.start, non_null.span.end),
                    body,
                ))
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
        if member.property.name == "resolve" {
            if call.arguments.len() > 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Promise.resolve supports at most one value argument",
                ));
            }
            let inner_ty = if let Some(argument) = call.arguments.first() {
                let value = self.argument(argument, body)?;
                Self::expr_ty(body, value)
            } else {
                self.ctx.krate.types.intern(Type::None)
            };
            let duration = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: self.ctx.krate.types.intern(Type::Float),
                span: self.span(call.span.start, call.span.start),
            });
            let ty = self.ctx.krate.types.intern(Type::Future(inner_ty));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::AsyncOp {
                    op: AsyncOp::Sleep,
                    args: vec![duration],
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
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
        let (args, output_ty) = if let Argument::ArrayExpression(array) = first_argument {
            let args = self.promise_array_args(array, body)?;
            let output_ty = self.promise_literal_combinator_output(op, &args, array.span, body)?;
            (args, output_ty)
        } else {
            self.promise_list_combinator_args(op, first_argument, body)?
        };
        let ty = self.ctx.krate.types.intern(Type::Future(output_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp { op, args },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return output type for Promise combinators over a source array literal.
    fn promise_literal_combinator_output(
        &mut self,
        op: AsyncOp,
        args: &[smelt_hir::ExprId],
        span: oxc::span::Span,
        body: &Body,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match op {
            AsyncOp::All | AsyncOp::AllSettled => {
                let outputs = args
                    .iter()
                    .map(|arg| {
                        self.future_inner_type(Self::expr_ty(body, *arg))
                            .ok_or_else(|| {
                                SmeltError::unsupported(
                                    self.span(span.start, span.end),
                                    "Promise combinator entries must be Promise<T> values",
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.ctx.krate.types.intern(Type::Tuple(outputs)))
            }
            AsyncOp::Race => {
                let Some(first) = args.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(span.start, span.end),
                        "Promise.race requires at least one promise",
                    ));
                };
                self.future_inner_type(Self::expr_ty(body, *first))
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(span.start, span.end),
                            "Promise.race entries must be Promise<T> values",
                        )
                    })
            }
            AsyncOp::Sleep
            | AsyncOp::CreateTask
            | AsyncOp::WaitFor
            | AsyncOp::HttpGetText
            | AsyncOp::SetTimeout
            | AsyncOp::ClearTimeout => Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                format!("Promise.{op:?} is not lowered yet"),
            )),
        }
    }

    /// Lower Promise combinators over a non-literal list of homogeneous futures.
    fn promise_list_combinator_args(
        &mut self,
        op: AsyncOp,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<(Vec<smelt_hir::ExprId>, smelt_hir::TypeId), SmeltError> {
        if op == AsyncOp::Race {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "Promise.race over a non-literal array is not lowered yet",
            ));
        }
        let list = self.argument(argument, body)?;
        let list_ty = self.type_param_constraint_or_self(Self::expr_ty(body, list));
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty).cloned() else {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "Promise combinators require an array of Promise<T> values",
            ));
        };
        let (list, output_item_ty) = if let Some(output_item_ty) = self.future_inner_type(item_ty) {
            (list, output_item_ty)
        } else {
            let future_item_ty = self.ctx.krate.types.intern(Type::Future(item_ty));
            let future_list_ty = self.ctx.krate.types.intern(Type::List(future_item_ty));
            let list = body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: list,
                    target: future_list_ty,
                },
                ty: future_list_ty,
                span: self.span(argument.span().start, argument.span().end),
            });
            (list, item_ty)
        };
        let output_ty = self.ctx.krate.types.intern(Type::List(output_item_ty));
        Ok((vec![list], output_ty))
    }

    /// Lower supported `new Promise<T>(executor)` expressions to future values.
    ///
    /// Timer executors keep their timeout duration. Other executor forms are
    /// represented as zero-delay futures with the explicit `Promise<T>` output
    /// type so async batching helpers can keep their type surface.
    fn promise_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "Promise" {
            return Ok(None);
        }
        let [Argument::ArrowFunctionExpression(executor)] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Promise constructor lowering supports one arrow executor",
            ));
        };
        let output_ty = type_hint
            .and_then(|hint| self.future_inner_type(hint))
            .or_else(|| {
                self.promise_constructor_output_type(new_expr)
                    .ok()
                    .flatten()
            })
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::None));
        let ty = self.ctx.krate.types.intern(Type::Future(output_ty));
        let duration = if let Some(timer_call) = Self::promise_executor_timer_call(executor) {
            let Some(duration_argument) = timer_call.arguments.get(1) else {
                return Err(SmeltError::unsupported(
                    self.span(timer_call.span.start, timer_call.span.end),
                    "Promise timer executor must pass a duration argument",
                ));
            };
            self.argument(duration_argument, body)?
        } else {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: self.ctx.krate.types.intern(Type::Float),
                span: self.span(new_expr.span.start, new_expr.span.start),
            })
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op: AsyncOp::Sleep,
                args: vec![duration],
            },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// Return the explicit `Promise<T>` constructor output type when present.
    fn promise_constructor_output_type(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let Some(type_arguments) = &new_expr.type_arguments else {
            return Ok(None);
        };
        let [item] = type_arguments.params.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Promise construction supports exactly one type argument",
            ));
        };
        self.ts_type_to_hir(item).map(Some)
    }

    /// Return the `setTimeout` call inside a supported Promise executor.
    fn promise_executor_timer_call<'a>(
        executor: &'a oxc::ast::ast::ArrowFunctionExpression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        let [statement] = executor.body.statements.as_slice() else {
            return None;
        };
        let Statement::ExpressionStatement(expr_stmt) = statement else {
            return None;
        };
        let Expression::CallExpression(call) = &expr_stmt.expression else {
            return None;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        (callee.name == "setTimeout" && call.arguments.len() == 2).then_some(call)
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
        match callee.name.as_str() {
            "setTimeout" if call.arguments.len() == 1 => {
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
            "setTimeout" if call.arguments.len() == 2 => {
                let Some(callback) = call.arguments.first() else {
                    return Ok(None);
                };
                let Some(duration) = call.arguments.get(1) else {
                    return Ok(None);
                };
                let callback = self.argument(callback, body)?;
                let duration = self.argument(duration, body)?;
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::SetTimeout,
                        args: vec![callback, duration],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "clearTimeout" if call.arguments.len() == 1 => {
                let Some(timeout) = call.arguments.first() else {
                    return Ok(None);
                };
                let timeout = self.argument(timeout, body)?;
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::ClearTimeout,
                        args: vec![timeout],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "setTimeout" | "clearTimeout" => Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "timer lowering supports setTimeout(milliseconds), setTimeout(callback, milliseconds), and clearTimeout(id)",
            )),
            _ => Ok(None),
        }
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

    /// Lower TypeScript `fetch(url[, options])` into an async HTTP GET text operation.
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
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch lowering supports fetch(url[, options])",
            ));
        }
        let Some(url_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch lowering supports fetch(url[, options])",
            ));
        };
        let mut url = self.argument(url_argument, body)?;
        if let Some(options_argument) = call.arguments.get(1) {
            let _ = self.argument(options_argument, body)?;
        }
        let url_ty = Self::expr_ty(body, url);
        if self.ctx.krate.types.get(url_ty) != Some(&Type::String) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            if self.is_string_compatible_type(url_ty) || self.type_contains_unknown(url_ty) {
                url = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: url,
                        target: string_ty,
                    },
                    ty: string_ty,
                    span: self.span(url_argument.span().start, url_argument.span().end),
                });
            } else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "fetch requires a string-compatible URL argument",
                ));
            }
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
        let timestamp_ms = self.date_constructor_timestamp(new_expr, body)?;
        let date_name = self.intern_type_name("Date");
        let ty = self.ctx.krate.types.intern(Type::Class {
            name: date_name,
            args: Vec::new(),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::DateFromValue {
                value: timestamp_ms,
            },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Lower `new (date.constructor as DateCtor)(value)` while retaining Date identity.
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
        let value = self.argument(value, body)?;
        let date_name = self.intern_type_name("Date");
        let ty = self.ctx.krate.types.intern(Type::Class {
            name: date_name,
            args: Vec::new(),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DateFromValue { value },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// Lower guarded dynamic Date constructor identifiers such as `new constructor(0)`.
    fn dynamic_identifier_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "constructor" || !self.locals.contains_key(callee.name.as_str()) {
            return Ok(None);
        }
        let [value] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "dynamic Date constructor identifiers require exactly one value argument",
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
            let ty = self.ctx.krate.types.intern(Type::Int);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Int(0)),
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        };
        let timestamp_ms = self.argument(timestamp_arg, body)?;
        let timestamp_ty = Self::expr_ty(body, timestamp_ms);
        if matches!(
            self.ctx.krate.types.get(timestamp_ty),
            Some(Type::Int | Type::Float)
        ) {
            return Ok(timestamp_ms);
        }
        if self.is_date_constructor_arg_type(timestamp_ty) {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DateFromValue {
                    value: timestamp_ms,
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
            Some(Type::Union(items)) => items.iter().copied().all(|item| {
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
        if method == "toISOString" {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.toISOString() does not accept arguments",
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateToIsoString {
                    timestamp_ms: receiver,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if method == "getTime" {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.getTime() does not accept arguments",
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToFloat,
                    operand: receiver,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if method == "setTime" {
            if call.arguments.len() != 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.setTime() requires exactly one numeric argument",
                ));
            }
            let receiver = self.expression(&member.object, body)?;
            let receiver_ty = Self::expr_ty(body, receiver);
            if !self.is_date_constructor_arg_type(receiver_ty) {
                return Err(SmeltError::unsupported(
                    self.span(member.object.span().start, member.object.span().end),
                    "Date.setTime() receiver must be a timestamp or Date-like value",
                ));
            }
            let Some(argument) = call.arguments.first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.setTime() requires exactly one numeric argument",
                ));
            };
            let value = self.argument(argument, body)?;
            let value_ty = Self::expr_ty(body, value);
            let value = if self.is_date_constructor_arg_type(value_ty) {
                value
            } else if self
                .non_nullish_type(value_ty)
                .is_some_and(|ty| self.is_numeric_like_type(ty))
            {
                self.non_null_assertion_value(
                    value,
                    self.span(argument.span().start, argument.span().end),
                    body,
                )
            } else {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "Date.setTime() argument must be numeric",
                ));
            };
            if let Expression::Identifier(identifier) = &member.object
                && let Some(local) = self.locals.get(identifier.name.as_str()).copied()
            {
                let target = body.push_expr(Expr {
                    kind: ExprKind::Local(local),
                    ty: receiver_ty,
                    span: self.span(identifier.span.start, identifier.span.end),
                });
                if let Some(block) = self.current_statement_block {
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                } else {
                    body.push_stmt(Stmt::Assign { target, value });
                }
            }
            return Ok(Some(value));
        }
        if method == "getTimezoneOffset" {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Date.getTimezoneOffset() does not accept arguments",
                ));
            }
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateTimezoneOffset,
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let getter_part = match method {
            "getFullYear" | "getUTCFullYear" => Some(DatePart::FullYear),
            "getMonth" | "getUTCMonth" => Some(DatePart::Month),
            "getDate" | "getUTCDate" => Some(DatePart::Date),
            "getDay" | "getUTCDay" => Some(DatePart::Day),
            "getHours" | "getUTCHours" => Some(DatePart::Hour),
            "getMinutes" | "getUTCMinutes" => Some(DatePart::Minute),
            "getSeconds" | "getUTCSeconds" => Some(DatePart::Second),
            "getMilliseconds" | "getUTCMilliseconds" => Some(DatePart::Millisecond),
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
            "setFullYear" | "setUTCFullYear" => Some(DatePart::FullYear),
            "setMonth" | "setUTCMonth" => Some(DatePart::Month),
            "setDate" | "setUTCDate" => Some(DatePart::Date),
            "setHours" | "setUTCHours" => Some(DatePart::Hour),
            "setMinutes" | "setUTCMinutes" => Some(DatePart::Minute),
            "setSeconds" | "setUTCSeconds" => Some(DatePart::Second),
            "setMilliseconds" | "setUTCMilliseconds" => Some(DatePart::Millisecond),
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
        let mut values = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            let mut value = self.argument(argument, body)?;
            let value_ty = Self::expr_ty(body, value);
            let value_is_numeric = matches!(
                self.ctx.krate.types.get(value_ty),
                Some(Type::Int | Type::Float | Type::Unknown | Type::TypeParam { .. })
            );
            let narrowed_numeric = self
                .non_nullish_type(value_ty)
                .is_some_and(|ty| self.is_numeric_like_type(ty));
            if !value_is_numeric && !narrowed_numeric {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    format!("Date.{method}() arguments must be numeric"),
                ));
            }
            if !value_is_numeric {
                value = self.non_null_assertion_value(
                    value,
                    self.span(argument.span().start, argument.span().end),
                    body,
                );
            }
            values.push(value);
        }
        let ty = Self::expr_ty(body, receiver);
        let value = body.push_expr(Expr {
            kind: ExprKind::DateSetPart {
                part,
                timestamp_ms: receiver,
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
            if let Some(block) = self.current_statement_block {
                body.push_stmt_to_block(block, Stmt::Assign { target, value });
            } else {
                body.push_stmt(Stmt::Assign { target, value });
            }
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
        if matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Int | Type::Float)
        ) {
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
