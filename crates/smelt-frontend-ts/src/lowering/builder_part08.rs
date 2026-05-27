impl ModuleBuilder<'_> {
    /// Lower a `new ...` expression, including stdlib containers and class construction.
    fn new_expression_with_hint(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(expr) = self.set_constructor_expression(new_expr, body, type_hint)? {
            return Ok(expr);
        }
        if let Some(expr) = self.map_constructor_expression(new_expr, body, type_hint)? {
            return Ok(expr);
        }
        if let Some(expr) = self.promise_constructor_expression(new_expr, body, type_hint)? {
            return Ok(expr);
        }
        let Expression::Identifier(callee) = &new_expr.callee else {
            if let Some(expr) = self.intl_date_time_format_constructor_expression(new_expr, body)? {
                return Ok(expr);
            }
            if let Some(expr) = self.dynamic_date_constructor_expression(new_expr, body)? {
                return Ok(expr);
            }
            if let Expression::StaticMemberExpression(member) = &new_expr.callee {
                let class_name = self.intern_type_name(member.property.name.as_str());
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
        if callee.name == "String" {
            return self.string_constructor_expression(new_expr, body);
        }
        if Self::is_numeric_typed_array_constructor(callee.name.as_str()) {
            return self.numeric_typed_array_constructor_expression(new_expr, body);
        }
        if callee.name == "URLSearchParams" {
            return self.opaque_builtin_constructor_expression(new_expr, body, "URLSearchParams");
        }
        if matches!(callee.name.as_str(), "WeakMap" | "WeakSet") {
            return self.opaque_builtin_constructor_expression(new_expr, body, callee.name.as_str());
        }
        if matches!(callee.name.as_str(), "Error" | "TypeError" | "RangeError") {
            return self.error_constructor_expression(new_expr, body);
        }
        if let Some(expr) = self.dynamic_identifier_constructor_expression(new_expr, body)? {
            return Ok(expr);
        }
        if callee.name == "URL" {
            return self.url_constructor_expression(new_expr, body);
        }
        let Some(item) = self.classes.get(callee.name.as_str()).copied() else {
            if self.pending_class_names.contains(callee.name.as_str()) {
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
            if self.value_imports.contains(callee.name.as_str())
                || self.module_globals.contains_key(callee.name.as_str())
                || self.source_contains_class(callee.name.as_str())
            {
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
        let Item::Class(class) = self.item_ref(item).clone() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new expressions require a class item",
            ));
        };
        if matches!(class.kind, smelt_hir::ClassKind::Abstract) {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                format!("abstract class `{}` cannot be constructed", callee.name),
            ));
        }
        let class_name = class.name;
        let args = new_expr
            .arguments
            .iter()
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        let explicit_type_args = new_expr
            .type_arguments
            .as_ref()
            .map(|type_args| {
                type_args
                    .params
                    .iter()
                    .map(|arg| self.ts_type_to_hir(arg))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let class_args = if let Some(explicit_type_args) = explicit_type_args {
            let substitutions = self.type_argument_substitution(
                &class.type_params,
                &explicit_type_args,
                self.span(new_expr.span.start, new_expr.span.end),
            )?;
            class
                .type_params
                .iter()
                .map(|param| {
                    substitutions.get(&param.name).copied().unwrap_or_else(|| {
                        self.ctx
                            .krate
                            .types
                            .intern(Type::TypeParam { name: param.name })
                    })
                })
                .collect()
        } else {
            class
                .type_params
                .iter()
                .map(|param| {
                    param.default.unwrap_or_else(|| {
                        self.ctx
                            .krate
                            .types
                            .intern(Type::TypeParam { name: param.name })
                    })
                })
                .collect()
        };
        let ty = self.ctx.krate.types.intern(Type::Class {
            name: class_name,
            args: class_args,
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

    /// Lower `new URL(text)` to its full URL string for string-oriented URL APIs.
    fn url_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let [url_arg] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new URL() currently supports exactly one string URL argument",
            ));
        };
        let url = self.url_string_argument(url_arg, body, new_expr.span)?;
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(body.push_expr(Expr {
            kind: ExprKind::UrlField {
                field: UrlField::Href,
                url,
            },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Lower boxed `new String(value)` as its primitive string payload.
    fn string_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new String(...) supports at most one argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        let Some(argument) = new_expr.arguments.first() else {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        };
        let value = self.argument(argument, body)?;
        if Self::expr_ty(body, value) == ty {
            return Ok(value);
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Return whether a global constructor creates a numeric typed array.
    fn is_numeric_typed_array_constructor(name: &str) -> bool {
        matches!(
            name,
            "Int8Array"
                | "Uint8Array"
                | "Uint8ClampedArray"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float32Array"
                | "Float64Array"
        )
    }

    /// Lower a supported opaque builtin constructor to an unknown object value.
    fn opaque_builtin_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        class_text: &str,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let _class_name = self.intern_type_name(class_text);
        let _args = new_expr
            .arguments
            .iter()
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let value = body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty: dict_ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

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

    /// Lower `Error(message)`-style calls to the message value used by HIR throws.
    fn error_function_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if call.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Error function lowering supports at most one message argument",
            ));
        }
        let Some(message_arg) = call.arguments.first() else {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("Error".to_owned())),
                ty,
                span: self.span(call.span.start, call.span.end),
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
            "Error function message must be a string",
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
            Expression::ObjectExpression(object) => {
                self.object_expression(object, body, type_hint)
            }
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
                    BinaryOperator::StrictEquality | BinaryOperator::Equality => BinOp::Eq,
                    BinaryOperator::StrictInequality | BinaryOperator::Inequality => BinOp::NotEq,
                    BinaryOperator::LessThan => BinOp::Lt,
                    BinaryOperator::LessEqualThan => BinOp::Lte,
                    BinaryOperator::GreaterThan => BinOp::Gt,
                    BinaryOperator::GreaterEqualThan => BinOp::Gte,
                    BinaryOperator::ShiftLeft => BinOp::Shl,
                    BinaryOperator::ShiftRight => BinOp::Shr,
                    BinaryOperator::ShiftRightZeroFill => BinOp::UShr,
                    BinaryOperator::Exponential | BinaryOperator::BitwiseOR
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
                let lhs_ty = Self::expr_ty(body, lhs);
                let rhs_ty = Self::expr_ty(body, rhs);
                let ty = self.binary_result_type(op, lhs_ty, rhs_ty);
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
                if let Some(expr) = self.logical_and_numeric_value_expression(logical, body)? {
                    return Ok(expr);
                }
                let cond = self.condition_expression(&logical.left, body)?;
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
                let identity = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(
                        logical.operator == LogicalOperator::Or,
                    )),
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
                } else if self.date_runtime_float_matches_type_param(then_ty, else_ty) {
                    else_ty
                } else if self.date_runtime_float_matches_type_param(else_ty, then_ty) {
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
                } else if matches!(self.ctx.krate.types.get(then_ty), Some(Type::Union(items)) if items.contains(&else_ty)) {
                    then_ty
                } else if matches!(self.ctx.krate.types.get(else_ty), Some(Type::Union(items)) if items.contains(&then_ty)) {
                    else_ty
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
                if unary.operator == UnaryOperator::Void {
                    let ty = self.ctx.krate.types.intern(Type::None);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(unary.span.start, unary.span.end),
                    }));
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
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(unary.span.start, unary.span.end),
                            format!("unary operator is not lowered yet: {:?}", unary.operator),
                        ));
                    }
                };
                let operand = self.expression(&unary.argument, body)?;
                let operand = if matches!(op, UnaryOp::Not) {
                    self.optional_known_date_presence_condition(
                        operand,
                        self.expression_span(&unary.argument),
                        body,
                    )
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
            Expression::AssignmentExpression(assign) => {
                let (_target, value) = self.assignment_parts(assign, body)?;
                Ok(value)
            }
            Expression::YieldExpression(yield_expr) => {
                if let Some(argument) = &yield_expr.argument {
                    self.expression(argument, body)
                } else {
                    let ty = self.ctx.krate.types.intern(Type::None);
                    Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(yield_expr.span.start, yield_expr.span.end),
                    }))
                }
            }
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
                self.new_expression_with_hint(new_expr, body, type_hint)
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
        } else if self.date_runtime_float_matches_type_param(then_ty, else_ty) {
            Ok(else_ty)
        } else if self.date_runtime_float_matches_type_param(else_ty, then_ty) {
            Ok(then_ty)
        } else if self.ctx.krate.types.get(then_ty) == Some(&Type::None) {
            Ok(self.ctx.krate.types.intern(Type::Optional(else_ty)))
        } else if self.ctx.krate.types.get(else_ty) == Some(&Type::None) {
            Ok(self.ctx.krate.types.intern(Type::Optional(then_ty)))
        } else if self.compatible_function_branch_types(then_ty, else_ty) {
            Ok(then_ty)
        } else if let Some(function_ty) = self.single_function_branch_type(then_ty, else_ty) {
            Ok(function_ty)
        } else if matches!(self.ctx.krate.types.get(then_ty), Some(Type::Union(items)) if items.contains(&else_ty)) {
            Ok(then_ty)
        } else if matches!(self.ctx.krate.types.get(else_ty), Some(Type::Union(items)) if items.contains(&then_ty)) {
            Ok(else_ty)
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

    /// Return whether a timestamp-backed Date branch can flow into a generic date type.
    fn date_runtime_float_matches_type_param(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        self.ctx.krate.types.get(actual) == Some(&Type::Float)
            && matches!(
                self.ctx.krate.types.get(expected),
                Some(Type::TypeParam { .. })
            )
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
        self.lowered_condition_expression(cond, self.expression_span(expression), body)
    }

    /// Coerce an already lowered JavaScript value into its boolean truthiness result.
    ///
    /// Assignment operators such as `||=` already lower their target as a
    /// writable place. Reusing the resulting expression here avoids lowering a
    /// computed receiver solely to form the condition that selects its value.
    fn lowered_condition_expression(
        &mut self,
        cond: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
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
                span,
            }));
        }
        if self.ctx.krate.types.get(cond_ty) == Some(&Type::String) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            let empty = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: string_ty,
                span,
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: cond,
                    rhs: empty,
                },
                ty: bool_ty,
                span,
            }));
        }
        if matches!(self.ctx.krate.types.get(cond_ty), Some(Type::Int | Type::Float)) {
            let zero = body.push_expr(Expr {
                kind: match self.ctx.krate.types.get(cond_ty) {
                    Some(Type::Int) => ExprKind::Literal(Literal::Int(0)),
                    _ => ExprKind::Literal(Literal::Float(0.0)),
                },
                ty: cond_ty,
                span,
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: cond,
                    rhs: zero,
                },
                ty: bool_ty,
                span,
            }));
        }
        if let Some(condition) = self.optional_known_date_presence_condition(cond, span, body) {
            return Ok(condition);
        }
        if self
            .non_nullish_type(cond_ty)
            .is_some_and(|inner_ty| self.type_is_always_truthy_object_surface(inner_ty))
        {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            let none = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: none_ty,
                span,
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: cond,
                    rhs: none,
                },
                ty: bool_ty,
                span,
            }));
        }
        if self.is_nullishable_type(cond_ty) || self.type_is_truthy_condition_surface(cond_ty) {
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToBool,
                    operand: cond,
                },
                ty: bool_ty,
                span,
            }));
        }
        Err(SmeltError::unsupported(
            span,
            format!(
                "condition expression must be boolean or optional (got {:?})",
                self.ctx.krate.types.get(cond_ty)
            ),
        ))
    }

    /// Lower truthiness for optional Date values as object presence.
    ///
    /// Date instances are represented by timestamps in Rust, but source
    /// truthiness depends on the Date object existing, not on its timestamp.
    fn optional_known_date_presence_condition(
        &mut self,
        value: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let value_ty = Self::expr_ty(body, value);
        if !self.is_nullishable_type(value_ty)
            || !self.expression_is_known_date_value(value, body)
        {
            return None;
        }
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let none = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: none_ty,
            span,
        });
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: value,
                rhs: none,
            },
            ty: bool_ty,
            span,
        }))
    }

    /// Return whether a present optional value is always truthy in JavaScript.
    fn type_is_always_truthy_object_surface(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(ty)),
            Some(
                Type::Class { .. }
                    | Type::Function(_)
                    | Type::List(_)
                    | Type::Set(_)
                    | Type::Dict(_, _)
                    | Type::Tuple(_)
                    | Type::Future(_)
            )
        )
    }

    /// Return whether a non-boolean type can appear in a JavaScript truthiness guard.
    fn type_is_truthy_condition_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Function(_)
                | Type::Class { .. }
                | Type::TypeParam { .. }
                | Type::Unknown
                | Type::Never,
            ) => true,
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| {
                    matches!(
                        self.ctx.krate.types.get(item),
                        Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None)
                    ) || self.type_is_truthy_condition_surface(item)
                }),
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
