impl ModuleBuilder<'_> {
    fn string_split_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "split" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires exactly one separator argument",
            ));
        }
        let haystack = self.expression(&member.object, body)?;
        let Some(separator_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires exactly one separator argument",
            ));
        };
        let separator = self.argument(separator_argument, body)?;
        let haystack_ty = Self::expr_ty(body, haystack);
        let separator_ty = Self::expr_ty(body, separator);
        if self.ctx.krate.types.get(haystack_ty) != Some(&Type::String)
            || self.ctx.krate.types.get(separator_ty) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires string receiver and separator",
            ));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let ty = self.ctx.krate.types.intern(Type::List(string_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSplit {
                haystack,
                separator,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower array entries passed to a `Promise.*` combinator.
    fn promise_array_args(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        array
            .elements
            .iter()
            .map(|element| match element {
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => {
                    Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "Promise combinator arrays cannot use spread or elision",
                    ))
                }
                _ => self.array_element(element, body),
            })
            .collect()
    }

    /// Lower `new Set(...)` from an array literal or annotated empty constructor.
    fn set_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "Set" {
            return Ok(None);
        }
        let (items, ty) = match new_expr.arguments.as_slice() {
            [] => {
                let Some(hint) = type_hint else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "empty Set constructors require a Set<T> type annotation",
                    ));
                };
                if !matches!(self.ctx.krate.types.get(hint), Some(Type::Set(_))) {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "new Set() requires a Set<T> type annotation",
                    ));
                }
                (Vec::new(), hint)
            }
            [Argument::ArrayExpression(array)] => {
                let items = array
                    .elements
                    .iter()
                    .map(|element| self.array_element(element, body))
                    .collect::<Result<Vec<_>, _>>()?;
                let ty = if let Some(hint) = type_hint {
                    if !matches!(self.ctx.krate.types.get(hint), Some(Type::Set(_))) {
                        return Err(SmeltError::unsupported(
                            self.span(new_expr.span.start, new_expr.span.end),
                            "new Set([...]) requires a Set<T> type annotation when annotated",
                        ));
                    }
                    hint
                } else if let Some(first_item) = items.first().copied() {
                    let item_ty = Self::expr_ty(body, first_item);
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "empty Set array literals require a Set<T> type annotation",
                    ));
                };
                (items, ty)
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(new_expr.span.start, new_expr.span.end),
                    "new Set currently supports no arguments or one array literal argument",
                ));
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::SetLit(items),
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// Lower `new Map(...)` to a dictionary literal.
    fn map_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "Map" {
            return Ok(None);
        }
        let (entries, ty) = match new_expr.arguments.as_slice() {
            [] => {
                let Some(ty) = type_hint else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "empty Map constructors require a Map<K, V> type annotation",
                    ));
                };
                if !matches!(self.ctx.krate.types.get(ty), Some(Type::Dict(_, _))) {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "new Map() requires a Map<K, V> type annotation",
                    ));
                }
                (Vec::new(), ty)
            }
            [Argument::ArrayExpression(array)] => {
                let entries = self.map_constructor_entries(array, body)?;
                let ty = if let Some(hint) = type_hint {
                    let Some(Type::Dict(key_ty, value_ty)) = self.ctx.krate.types.get(hint) else {
                        return Err(SmeltError::unsupported(
                            self.span(new_expr.span.start, new_expr.span.end),
                            "new Map([...]) requires a Map<K, V> type annotation when annotated",
                        ));
                    };
                    for (key, value) in &entries {
                        if Self::expr_ty(body, *key) != *key_ty
                            || Self::expr_ty(body, *value) != *value_ty
                        {
                            return Err(SmeltError::unsupported(
                                self.span(new_expr.span.start, new_expr.span.end),
                                "new Map entry key and value types must match the Map<K, V> annotation",
                            ));
                        }
                    }
                    hint
                } else if let Some((key, value)) = entries.first().copied() {
                    let key_ty = Self::expr_ty(body, key);
                    let value_ty = Self::expr_ty(body, value);
                    for (entry_key, entry_value) in &entries {
                        if Self::expr_ty(body, *entry_key) != key_ty
                            || Self::expr_ty(body, *entry_value) != value_ty
                        {
                            return Err(SmeltError::unsupported(
                                self.span(new_expr.span.start, new_expr.span.end),
                                "new Map entry key and value types must be homogeneous",
                            ));
                        }
                    }
                    self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "empty Map array literals require a Map<K, V> type annotation",
                    ));
                };
                (entries, ty)
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(new_expr.span.start, new_expr.span.end),
                    "new Map currently supports no arguments or one array literal of [key, value] pairs",
                ));
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// Lower the entry array passed to `new Map([[key, value], ...])`.
    fn map_constructor_entries(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
    ) -> Result<Vec<(smelt_hir::ExprId, smelt_hir::ExprId)>, SmeltError> {
        let mut entries = Vec::new();
        for element in &array.elements {
            let ArrayExpressionElement::ArrayExpression(pair) = element else {
                return Err(SmeltError::unsupported(
                    self.span(element.span().start, element.span().end),
                    "new Map entries must be [key, value] array pairs",
                ));
            };
            let [key_element, value_element] = pair.elements.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(pair.span.start, pair.span.end),
                    "new Map entries must contain exactly key and value",
                ));
            };
            let key = self.array_element(key_element, body)?;
            let value = self.array_element(value_element, body)?;
            entries.push((key, value));
        }
        Ok(entries)
    }

    /// Lower an array element.
    fn array_element(
        &mut self,
        element: &ArrayExpressionElement<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match element {
            ArrayExpressionElement::NumericLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Float);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            ArrayExpressionElement::StringLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(lit.value.to_string())),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            ArrayExpressionElement::BooleanLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            ArrayExpressionElement::Identifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            ArrayExpressionElement::BinaryExpression(binary) => {
                self.binary_expression(binary, body)
            }
            ArrayExpressionElement::LogicalExpression(logical) => {
                self.logical_expression(logical, body)
            }
            ArrayExpressionElement::UnaryExpression(unary) => self.unary_expression(unary, body),
            ArrayExpressionElement::ArrayExpression(array) => {
                let mut items = Vec::new();
                for nested_element in &array.elements {
                    let item = match nested_element {
                        ArrayExpressionElement::SpreadElement(_) => {
                            return Err(SmeltError::unsupported(
                                self.span(nested_element.span().start, nested_element.span().end),
                                "array spread elements are not lowered yet",
                            ));
                        }
                        ArrayExpressionElement::Elision(_) => {
                            return Err(SmeltError::unsupported(
                                self.span(nested_element.span().start, nested_element.span().end),
                                "array elisions are not lowered",
                            ));
                        }
                        _ => self.array_element(nested_element, body)?,
                    };
                    items.push(item);
                }
                let Some(first) = items.first().copied() else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "empty nested arrays require an explicit type annotation",
                    ));
                };
                let item_ty = Self::expr_ty(body, first);
                let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                Ok(body.push_expr(Expr {
                    kind: ExprKind::ListLit(items),
                    ty,
                    span: self.span(array.span.start, array.span.end),
                }))
            }
            ArrayExpressionElement::CallExpression(call) => self.call_expression(call, body),
            ArrayExpressionElement::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            ArrayExpressionElement::StaticMemberExpression(member) => {
                self.static_member(member, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(element.span().start, element.span().end),
                format!("array element kind is not lowered yet: {element:?}"),
            )),
        }
    }

    /// Lower a binary expression.
    fn binary_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let op = match binary.operator {
            BinaryOperator::Addition => BinOp::Add,
            BinaryOperator::Subtraction => BinOp::Sub,
            BinaryOperator::Multiplication => BinOp::Mul,
            BinaryOperator::Division => BinOp::Div,
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
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(binary.span.start, binary.span.end),
                    format!("binary operator is not lowered yet: {:?}", binary.operator),
                ));
            }
        };
        let lhs = self.expression(&binary.left, body)?;
        let rhs = self.expression(&binary.right, body)?;
        let ty = match op {
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                self.ctx.krate.types.intern(Type::Bool)
            }
            _ => Self::expr_ty(body, lhs),
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty,
            span: self.span(binary.span.start, binary.span.end),
        }))
    }

    /// Lower a logical expression.
    fn logical_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if logical.operator == LogicalOperator::Or
            && matches!(logical.left, Expression::ChainExpression(_))
        {
            return self.expression(&logical.right, body);
        }
        let op = match logical.operator {
            LogicalOperator::And => BinOp::And,
            LogicalOperator::Or => BinOp::Or,
            LogicalOperator::Coalesce => {
                return Err(SmeltError::unsupported(
                    self.span(logical.span.start, logical.span.end),
                    "nullish coalescing is not lowered yet",
                ));
            }
        };
        let lhs = self.expression(&logical.left, body)?;
        let rhs = self.expression(&logical.right, body)?;
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty,
            span: self.span(logical.span.start, logical.span.end),
        }))
    }

    /// Lower a unary expression.
    fn unary_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let op = match unary.operator {
            UnaryOperator::LogicalNot => UnaryOp::Not,
            UnaryOperator::UnaryNegation => UnaryOp::Neg,
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(unary.span.start, unary.span.end),
                    format!("unary operator is not lowered yet: {:?}", unary.operator),
                ));
            }
        };
        let operand = self.expression(&unary.argument, body)?;
        let ty = match op {
            UnaryOp::Not => self.ctx.krate.types.intern(Type::Bool),
            UnaryOp::Neg => Self::expr_ty(body, operand),
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnaryOp { op, operand },
            ty,
            span: self.span(unary.span.start, unary.span.end),
        }))
    }

    /// Lower an array expression.
    fn array_expression(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut items = Vec::new();
        for element in &array.elements {
            if matches!(
                element,
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_)
            ) {
                return Err(SmeltError::unsupported(
                    self.span(element.span().start, element.span().end),
                    "array spread elements and elisions are not lowered",
                ));
            }
            items.push(self.array_element(element, body)?);
        }
        let ty = if let Some(hint) = type_hint {
            hint
        } else if let Some(first) = items.first() {
            let item_ty = Self::expr_ty(body, *first);
            self.ctx.krate.types.intern(Type::List(item_ty))
        } else {
            return Err(SmeltError::unsupported(
                self.span(array.span.start, array.span.end),
                "empty arrays require an explicit type annotation",
            ));
        };
        Ok(body.push_expr(Expr {
            kind: if matches!(self.ctx.krate.types.get(ty), Some(Type::Tuple(_))) {
                ExprKind::TupleLit(items)
            } else {
                ExprKind::ListLit(items)
            },
            ty,
            span: self.span(array.span.start, array.span.end),
        }))
    }

    /// Lower an object expression.
    fn object_expression(
        &mut self,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut entries = Vec::new();
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(property.span().start, property.span().end),
                    "object spread properties are not lowered yet",
                ));
            };
            if object_property.computed || object_property.method {
                return Err(SmeltError::unsupported(
                    self.span(object_property.span.start, object_property.span.end),
                    "computed object keys and object methods are not lowered yet",
                ));
            }
            let key_text = match &object_property.key {
                PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
                PropertyKey::StringLiteral(lit) => lit.value.to_string(),
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(
                            object_property.key.span().start,
                            object_property.key.span().end,
                        ),
                        "object literal keys must be static string keys",
                    ));
                }
            };
            let key_ty = self.ctx.krate.types.intern(Type::String);
            let key = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(key_text)),
                ty: key_ty,
                span: self.span(
                    object_property.key.span().start,
                    object_property.key.span().end,
                ),
            });
            let value = self.expression(&object_property.value, body)?;
            entries.push((key, value));
        }
        let ty = self.object_literal_type(&entries, type_hint, body);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty,
            span: self.span(object.span.start, object.span.end),
        }))
    }

    /// Infer the dictionary type used for a lowered object literal.
    fn object_literal_type(
        &mut self,
        entries: &[(smelt_hir::ExprId, smelt_hir::ExprId)],
        type_hint: Option<smelt_hir::TypeId>,
        body: &Body,
    ) -> smelt_hir::TypeId {
        if let Some(ty) = type_hint
            && matches!(self.ctx.krate.types.get(ty), Some(Type::Dict(_, _)))
        {
            return ty;
        }
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let first_value_ty = entries.first().map(|(_, value)| Self::expr_ty(body, *value));
        let value_ty = first_value_ty
            .filter(|first_ty| {
                entries
                    .iter()
                    .all(|(_, value)| Self::expr_ty(body, *value) == *first_ty)
            })
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
        self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
    }

    /// Lower a static member access expression.
    fn namespace_member_expression(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some((namespace, member_name)) = self.namespace_member_name(member) else {
            return Ok(None);
        };
        let span = self.span(member.span.start, member.span.end);
        if let Some(value) = self.const_literals.get(member_name).cloned() {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(value.literal),
                ty: value.ty,
                span,
            })));
        }
        let item = self
            .object_namespaces
            .get(namespace)
            .and_then(|members| members.get(member_name))
            .copied()
            .or_else(|| self.items.get(member_name).copied());
        let Some(item) = item else {
            return Err(SmeltError::unsupported(
                span,
                format!("namespace import has no exported member `{member_name}`"),
            ));
        };
        let ty = self.item_expr_type(item, span)?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Item(item),
            ty,
            span,
        })))
    }

    // Continued in the next split builder file.
}
