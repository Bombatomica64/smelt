impl ModuleBuilder<'_> {
    /// Lower static `Array.from({ length }, mapper)` calls into indexed list construction.
    fn array_from_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if !matches!(&member.object, Expression::Identifier(object) if object.name == "Array")
            || member.property.name != "from"
        {
            return Ok(None);
        }
        let (source_arg, mapper_arg) = match call.arguments.as_slice() {
            [source_arg] => (source_arg, None),
            [source_arg, mapper_arg] => (source_arg, Some(mapper_arg)),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Array.from currently requires an array-like source and optional mapper callback",
                ));
            }
        };
        let length = self.array_from_length_argument(source_arg, body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, length)),
            Some(Type::Int | Type::Float)
        ) {
            return Err(SmeltError::unsupported(
                self.span(source_arg.span().start, source_arg.span().end),
                "Array.from({ length }, mapper) length must be numeric",
            ));
        }
        let Some(mapper_arg) = mapper_arg else {
            let item_ty = self.ctx.krate.types.intern(Type::Unknown);
            let ty = self.ctx.krate.types.intern(Type::List(item_ty));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ListFromLength { length },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        };
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let index_ty = self.ctx.krate.types.intern(Type::Float);
        let callback = self.callback_argument(
            mapper_arg,
            &[unknown_ty, index_ty],
            "Array.from mapper",
            body,
        )?;
        let ty = self.ctx.krate.types.intern(Type::List(callback.return_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListFromLengthMap {
                length,
                callback: callback.expr,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Extract the numeric `length` expression from `Array.from`'s source argument.
    fn array_from_length_argument(
        &mut self,
        source_arg: &Argument<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Argument::ObjectExpression(object) = source_arg else {
            return Err(SmeltError::unsupported(
                self.span(source_arg.span().start, source_arg.span().end),
                "Array.from currently supports object sources shaped as { length }",
            ));
        };
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(source_arg.span().start, source_arg.span().end),
                    "Array.from({ length }, mapper) does not support spread properties",
                ));
            };
            let key_text = match &property.key {
                PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str(),
                PropertyKey::StringLiteral(literal) => literal.value.as_str(),
                _ => continue,
            };
            if key_text == "length" {
                return self.object_property_value_expr(property, body, None);
            }
        }
        Err(SmeltError::unsupported(
            self.span(source_arg.span().start, source_arg.span().end),
            "Array.from object source must provide a length property",
        ))
    }

    /// Lower `new Array<T>(length)` to an empty list with item metadata.
    ///
    /// JavaScript creates a sparse array here; Smelt models the later indexed
    /// writes and only needs the list container type at construction time.
    fn array_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new Array(...) supports at most one length argument",
            ));
        }
        if let Some(Argument::ArrayExpression(array)) = new_expr.arguments.first() {
            return self.array_expression(array, body, None);
        }
        if let Some(length) = new_expr.arguments.first() {
            let length = self.argument(length, body)?;
            if !matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, length)),
                Some(Type::Int | Type::Float)
            ) {
                return Err(SmeltError::unsupported(
                    self.span(new_expr.span.start, new_expr.span.end),
                    "new Array(...) length must be numeric",
                ));
            }
        }
        let item_ty = self.ctx.krate.types.intern(Type::Unknown);
        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
        Ok(body.push_expr(Expr {
            kind: ExprKind::ListLit(Vec::new()),
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Lower `new TypedArray(length)` to a numeric list used by typed-array consumers.
    fn numeric_typed_array_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new TypedArray(...) supports at most one length argument",
            ));
        }
        if let Some(Argument::ArrayExpression(array)) = new_expr.arguments.first() {
            return self.array_expression(array, body, None);
        }
        if let Some(length) = new_expr.arguments.first() {
            let length = self.argument(length, body)?;
            if !matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, length)),
                Some(Type::Int | Type::Float)
            ) {
                return Err(SmeltError::unsupported(
                    self.span(new_expr.span.start, new_expr.span.end),
                    "new TypedArray(...) length must be numeric",
                ));
            }
        }
        let item_ty = self.ctx.krate.types.intern(Type::Float);
        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
        if new_expr.arguments.is_empty() {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ListLit(Vec::new()),
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        }
        let zero = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(0.0)),
            ty: item_ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::ListLit(vec![zero; 1024]),
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Lower supported string split calls into HIR string runtime calls.
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
        if call.arguments.is_empty() || call.arguments.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires a separator and optional limit argument",
            ));
        }
        let haystack = self.expression(&member.object, body)?;
        let Some(separator_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires a separator argument",
            ));
        };
        let separator = self.argument(separator_argument, body)?;
        let limit = call
            .arguments
            .get(1)
            .map(|argument| self.argument(argument, body))
            .transpose()?;
        let haystack_ty = Self::expr_ty(body, haystack);
        let separator_ty = Self::expr_ty(body, separator);
        if self.ctx.krate.types.get(haystack_ty) != Some(&Type::String)
            || !self.string_split_separator_type_is_supported(separator_ty)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires string receiver and separator",
            ));
        }
        if let Some(limit) = limit {
            let limit_ty = Self::expr_ty(body, limit);
            if !self.string_split_limit_type_is_supported(limit_ty) {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "string split limit must be numeric or undefined",
                ));
            }
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let ty = self.ctx.krate.types.intern(Type::List(string_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSplit {
                haystack,
                separator,
                limit,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether a type can act as a JavaScript string split separator.
    fn string_split_separator_type_is_supported(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::String | Type::Unknown | Type::TypeParam { .. }) => true,
            Some(Type::Class { name, .. }) => self
                .ctx
                .krate
                .symbols
                .get(*name)
                .is_some_and(|name| name == "RegExp"),
            Some(Type::Optional(item)) => self.string_split_separator_type_is_supported(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.string_split_separator_type_is_supported(item)),
            _ => false,
        }
    }

    /// Return whether a type can act as a JavaScript string split limit.
    fn string_split_limit_type_is_supported(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Int | Type::Float | Type::None | Type::Unknown | Type::TypeParam { .. }) => {
                true
            }
            Some(Type::Optional(item)) => self.string_split_limit_type_is_supported(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.string_split_limit_type_is_supported(item)),
            _ => false,
        }
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
                let ty = if let Some(type_args) = &new_expr.type_arguments
                    && let [item] = type_args.params.as_slice()
                {
                    let item_ty = self.ts_type_to_hir(item)?;
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                } else {
                    type_hint.unwrap_or_else(|| {
                        let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                        self.ctx.krate.types.intern(Type::Set(item_ty))
                    })
                };
                if !matches!(self.ctx.krate.types.get(ty), Some(Type::Set(_))) {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "new Set() requires a Set<T> type annotation",
                    ));
                }
                (Vec::new(), ty)
            }
            [Argument::ArrayExpression(array)] => {
                if array
                    .elements
                    .iter()
                    .any(|element| matches!(element, ArrayExpressionElement::SpreadElement(_)))
                {
                    let list = self.array_expression(array, body, None)?;
                    let list_ty = self.type_param_constraint_or_self(Self::expr_ty(body, list));
                    let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) else {
                        return Err(SmeltError::unsupported(
                            self.span(array.span.start, array.span.end),
                            "new Set([...spread]) requires an array literal argument",
                        ));
                    };
                    let ty = if let Some(hint) = type_hint {
                        if !matches!(self.ctx.krate.types.get(hint), Some(Type::Set(_))) {
                            return Err(SmeltError::unsupported(
                                self.span(new_expr.span.start, new_expr.span.end),
                                "new Set([...]) requires a Set<T> type annotation when annotated",
                            ));
                        }
                        hint
                    } else {
                        self.ctx.krate.types.intern(Type::Set(*item_ty))
                    };
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::ListToSet { list },
                        ty,
                        span: self.span(new_expr.span.start, new_expr.span.end),
                    })));
                }
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
                } else if let Some(type_args) = &new_expr.type_arguments
                    && let [item] = type_args.params.as_slice()
                {
                    let item_ty = self.ts_type_to_hir(item)?;
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                } else {
                    let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                };
                (items, ty)
            }
            [argument] => {
                let list = self.argument(argument, body)?;
                let list_ty = self.type_param_constraint_or_self(Self::expr_ty(body, list));
                let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) else {
                    return Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        "new Set(iterable) currently requires an array argument",
                    ));
                };
                let ty = if let Some(hint) = type_hint {
                    if !matches!(self.ctx.krate.types.get(hint), Some(Type::Set(_))) {
                        return Err(SmeltError::unsupported(
                            self.span(new_expr.span.start, new_expr.span.end),
                            "new Set(iterable) requires a Set<T> type annotation when annotated",
                        ));
                    }
                    hint
                } else {
                    self.ctx.krate.types.intern(Type::Set(*item_ty))
                };
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListToSet { list },
                    ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                })));
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(new_expr.span.start, new_expr.span.end),
                    "new Set currently supports no arguments or one array argument",
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
                let ty = if let Some(type_args) = &new_expr.type_arguments
                    && let [key, value] = type_args.params.as_slice()
                {
                    let key_ty = self.ts_type_to_hir(key)?;
                    let value_ty = self.ts_type_to_hir(value)?;
                    self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
                } else {
                    type_hint.unwrap_or_else(|| {
                        let unknown = self.ctx.krate.types.intern(Type::Unknown);
                        self.ctx.krate.types.intern(Type::Dict(unknown, unknown))
                    })
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
                    let unknown = self.ctx.krate.types.intern(Type::Unknown);
                    self.ctx.krate.types.intern(Type::Dict(unknown, unknown))
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
            ArrayExpressionElement::BigIntLiteral(lit) => {
                self.bigint_literal_expression(lit.value.as_str(), lit.span, body)
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
            ArrayExpressionElement::NullLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
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
            ArrayExpressionElement::TSAsExpression(as_expr) => {
                self.expression(&as_expr.expression, body)
            }
            ArrayExpressionElement::TSSatisfiesExpression(satisfies) => {
                self.expression(&satisfies.expression, body)
            }
            ArrayExpressionElement::TSNonNullExpression(non_null) => {
                self.expression(&non_null.expression, body)
            }
            ArrayExpressionElement::ArrayExpression(array) => {
                if let [ArrayExpressionElement::SpreadElement(spread)] = array.elements.as_slice() {
                    return self.expression(&spread.argument, body);
                }
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
                            let ty = self.ctx.krate.types.intern(Type::Unknown);
                            body.push_expr(Expr {
                                kind: ExprKind::Literal(Literal::None),
                                ty,
                                span: self.span(
                                    nested_element.span().start,
                                    nested_element.span().end,
                                ),
                            })
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
            ArrayExpressionElement::ObjectExpression(object) => {
                self.object_expression(object, body, None)
            }
            ArrayExpressionElement::RegExpLiteral(literal) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                let value = Self::regex_literal_pattern_text(literal);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(value)),
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                }))
            }
            ArrayExpressionElement::CallExpression(call) => self.call_expression(call, body),
            ArrayExpressionElement::NewExpression(new_expr) => {
                self.new_expression_with_hint(new_expr, body, None)
            }
            ArrayExpressionElement::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            ArrayExpressionElement::StaticMemberExpression(member) => {
                self.static_member(member, body)
            }
            ArrayExpressionElement::ArrowFunctionExpression(arrow) => {
                self.arrow_function_expression(arrow, body)
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
            BinaryOperator::Equality | BinaryOperator::Inequality => {
                return Err(SmeltError::unsupported(
                    self.span(binary.span.start, binary.span.end),
                    "coercive equality is not supported",
                ));
            }
            BinaryOperator::Addition => BinOp::Add,
            BinaryOperator::Subtraction => BinOp::Sub,
            BinaryOperator::Multiplication => BinOp::Mul,
            BinaryOperator::Division => BinOp::Div,
            BinaryOperator::Remainder => BinOp::Rem,
            BinaryOperator::StrictEquality => BinOp::Eq,
            BinaryOperator::StrictInequality => BinOp::NotEq,
            BinaryOperator::LessThan => BinOp::Lt,
            BinaryOperator::LessEqualThan => BinOp::Lte,
            BinaryOperator::GreaterThan => BinOp::Gt,
            BinaryOperator::GreaterEqualThan => BinOp::Gte,
            BinaryOperator::ShiftLeft => BinOp::Shl,
            BinaryOperator::ShiftRight => BinOp::Shr,
            BinaryOperator::ShiftRightZeroFill => BinOp::UShr,
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
        if let Some(expr) = self.logical_or_fallback_expression(logical, body)? {
            return Ok(expr);
        }
        if logical.operator == LogicalOperator::Coalesce {
            return self.nullish_coalesce_expression(logical, body, None);
        }
        let op = if logical.operator == LogicalOperator::And {
            BinOp::And
        } else {
            BinOp::Or
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

    /// Lower JavaScript `left || right` fallback expressions for optional values.
    ///
    /// Date-fns uses this for locale-width defaults. For optional left operands
    /// Smelt preserves the runtime value fallback with the same optional
    /// coalescing HIR shape used by `??`.
    fn logical_or_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if logical.operator != LogicalOperator::Or {
            return Ok(None);
        }
        let optional = self.expression(&logical.left, body)?;
        let optional_ty = Self::expr_ty(body, optional);
        if !self.is_nullishable_type(optional_ty) {
            if let Some(expr) =
                self.logical_or_numeric_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_string_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            return Ok(None);
        }
        let Some(ty) = self.non_nullish_type(optional_ty) else {
            if matches!(logical.left, Expression::ChainExpression(_)) {
                return self.expression(&logical.right, body).map(Some);
            }
            return Ok(None);
        };
        let fallback = self.expression_with_hint(&logical.right, body, Some(ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        let ty = if fallback_ty == ty {
            ty
        } else if self.is_string_compatible_type(ty) && self.is_string_compatible_type(fallback_ty)
        {
            self.ctx.krate.types.intern(Type::String)
        } else {
            return Ok(None);
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce { optional, fallback },
            ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower numeric JavaScript `left || right` value fallback expressions.
    ///
    /// Date-fns uses `numeric % 7 || 7` to replace zero with a default value.
    /// Lowering this as boolean `||` loses the numeric result type, so Smelt
    /// models the expression as `left != 0 ? left : right` for numeric operands.
    fn logical_or_numeric_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_numeric_like_type(value_ty) {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&logical.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        if !self.numeric_type_compatible(value_ty, fallback_ty) {
            return Ok(None);
        }
        let zero = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(0.0)),
            ty: self.ctx.krate.types.intern(Type::Float),
            span: self.span(logical.span.start, logical.span.end),
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: value,
                rhs: zero,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.span.start, logical.span.end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: value_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || right` fallback expressions for string values.
    fn logical_or_string_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_string_compatible_type(value_ty) {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&logical.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        if !self.is_string_compatible_type(fallback_ty) {
            return Ok(None);
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let empty = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(String::new())),
            ty: string_ty,
            span: self.span(logical.span.start, logical.span.end),
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: value,
                rhs: empty,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.span.start, logical.span.end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: string_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower TypeScript nullish coalescing while preserving falsey values.
    fn nullish_coalesce_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let optional = self.expression(&logical.left, body)?;
        let optional_ty = Self::expr_ty(body, optional);
        let Some(ty) = self.non_nullish_type(optional_ty) else {
            if self.ctx.krate.types.get(optional_ty) == Some(&Type::None) {
                let fallback = self.expression(&logical.right, body)?;
                return Ok(fallback);
            }
            return Ok(optional);
        };
        let mut fallback = self.expression_with_hint(&logical.right, body, Some(ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        let ty = if fallback_ty == ty {
            ty
        } else if self.ctx.krate.types.get(ty) == Some(&Type::Unknown) {
            fallback_ty
        } else if self.numeric_type_compatible(ty, fallback_ty) {
            ty
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && self.numeric_type_compatible(ty, fallback_inner)
        {
            self.ctx.krate.types.intern(Type::Optional(ty))
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && fallback_inner == ty
        {
            self.ctx.krate.types.intern(Type::Optional(ty))
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && self.nullish_fallback_types_are_structurally_compatible(ty, fallback_inner)
        {
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty: self.ctx.krate.types.intern(Type::Optional(ty)),
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            self.ctx.krate.types.intern(Type::Optional(ty))
        } else if matches!(self.ctx.krate.types.get(ty), Some(Type::TypeParam { .. }))
            && matches!(
                self.ctx.krate.types.get(fallback_ty),
                Some(Type::TypeParam { .. })
            )
        {
            type_hint.unwrap_or_else(|| self.ctx.krate.types.intern(Type::Union(vec![ty, fallback_ty])))
        } else if matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Union(items)) if items.contains(&fallback_ty)
        ) || self.allow_unknown_index_access
        {
            fallback_ty
        } else if self.nullish_fallback_matches_union_member(ty, fallback_ty) {
            ty
        } else if self.nullish_fallback_types_are_structurally_compatible(ty, fallback_ty) {
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty,
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            ty
        } else if let Some(hint) = type_hint
            && !self.concrete_type_requires_never_value(hint)
        {
            hint
        } else {
            return Err(SmeltError::unsupported(
                self.span(logical.span.start, logical.span.end),
                "nullish coalescing fallback must match the non-nullish value type",
            ));
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce { optional, fallback },
            ty,
            span: self.span(logical.span.start, logical.span.end),
        }))
    }

    /// Return whether a `??` fallback is covered by one member of the non-null union.
    fn nullish_fallback_matches_union_member(
        &self,
        non_nullish_ty: smelt_hir::TypeId,
        fallback_ty: smelt_hir::TypeId,
    ) -> bool {
        let Some(Type::Union(items)) = self.ctx.krate.types.get(non_nullish_ty) else {
            return false;
        };
        items
            .iter()
            .copied()
            .any(|item| item == fallback_ty || self.numeric_type_compatible(item, fallback_ty))
    }

    /// Return whether `??` may treat the fallback as the optional side's object surface.
    ///
    /// TypeScript uses structural object compatibility, so date-fns can coalesce
    /// an optional `Locale` interface with a concrete exported locale object.
    /// Smelt keeps the optional side's type and inserts a typed assertion around
    /// the fallback expression when both sides are object-like surfaces.
    fn nullish_fallback_types_are_structurally_compatible(
        &mut self,
        optional_inner: smelt_hir::TypeId,
        fallback_ty: smelt_hir::TypeId,
    ) -> bool {
        let fallback_ty = self
            .non_nullish_type(fallback_ty)
            .unwrap_or(fallback_ty);
        self.is_structural_object_surface(optional_inner)
            && self.is_structural_object_surface(fallback_ty)
    }

    /// Return whether a type behaves as a structural object surface.
    fn is_structural_object_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Class { .. } | Type::Dict(_, _) | Type::TypeParam { .. } | Type::Unknown) => {
                true
            }
            Some(Type::Optional(item)) => self.is_structural_object_surface(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.is_structural_object_surface(item)),
            _ => false,
        }
    }

    /// Return the type left after removing TypeScript nullish values.
    fn non_nullish_type(&mut self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::Optional(inner)) => Some(inner),
            Some(Type::Union(items)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let mut remaining = Vec::new();
                for item in items {
                    if item == none_ty {
                        continue;
                    }
                    let normalized = match self.ctx.krate.types.get(item).cloned() {
                        Some(Type::Optional(inner)) => inner,
                        _ => item,
                    };
                    if !remaining.contains(&normalized) {
                        remaining.push(normalized);
                    }
                }
                match remaining.as_slice() {
                    [single] => Some(*single),
                    [] => None,
                    _ => Some(self.ctx.krate.types.intern(Type::Union(remaining))),
                }
            }
            Some(Type::None) => None,
            _ => Some(ty),
        }
    }

    /// Lower a TypeScript non-null assertion while preserving the narrowed type.
    fn non_null_assertion_expression(
        &mut self,
        expression: &Expression<'_>,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(expression, body)?;
        Ok(self.non_null_assertion_value(value, span, body))
    }

    /// Apply non-null assertion narrowing to an already-lowered expression.
    fn non_null_assertion_value(
        &mut self,
        value: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let value_ty = Self::expr_ty(body, value);
        let Some(non_null_ty) = self.non_nullish_type(value_ty) else {
            return value;
        };
        if non_null_ty == value_ty {
            return value;
        }
        body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: non_null_ty,
            span,
        })
    }

    /// Lower a unary expression.
    fn unary_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if unary.operator == UnaryOperator::Delete {
            return self.delete_unary_expression(unary, body);
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
                if self.is_numeric_like_type(operand_ty) {
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

    /// Lower JavaScript `delete object[key]` to a dictionary key removal.
    fn delete_unary_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Expression::ComputedMemberExpression(member) = &unary.argument else {
            return Err(SmeltError::unsupported(
                self.span(unary.argument.span().start, unary.argument.span().end),
                "delete is only lowered for computed object keys",
            ));
        };
        let dict = self.expression(&member.object, body)?;
        let key = self.expression(&member.expression, body)?;
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictRemoveKey { dict, key },
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
        if array
            .elements
            .iter()
            .any(|element| matches!(element, ArrayExpressionElement::SpreadElement(_)))
        {
            return self.array_expression_with_spread(array, body, type_hint);
        }
        let mut items = Vec::new();
        let tuple_hints = type_hint.and_then(|hint| match self.ctx.krate.types.get(hint) {
            Some(Type::Tuple(tuple_items)) => Some(tuple_items.clone()),
            _ => None,
        });
        for (index, element) in array.elements.iter().enumerate() {
            if matches!(element, ArrayExpressionElement::SpreadElement(_)) {
                return Err(SmeltError::unsupported(
                    self.span(element.span().start, element.span().end),
                    "array spread elements are not lowered",
                ));
            }
            let element_hint = tuple_hints
                .as_ref()
                .and_then(|hints| hints.get(index).copied());
            let item = if let ArrayExpressionElement::Elision(elision) = element {
                let ty = element_hint.unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(elision.span.start, elision.span.end),
                })
            } else {
                self.array_element_with_hint(element, body, element_hint)?
            };
            items.push(item);
        }
        let ty = if let Some(hint) = type_hint {
            hint
        } else if !items.is_empty() {
            let item_ty = self.array_literal_item_type(&items, body);
            self.ctx.krate.types.intern(Type::List(item_ty))
        } else {
            let item_ty = self.ctx.krate.types.intern(Type::Unknown);
            self.ctx.krate.types.intern(Type::List(item_ty))
        };
        if self.array_literal_needs_never_value(ty, items.len()) {
            return Err(SmeltError::unsupported(
                self.span(array.span.start, array.span.end),
                "array or tuple literal cannot construct a never value",
            ));
        }
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

    /// Infer one item type for an array literal, preserving nullability when needed.
    fn array_literal_item_type(
        &mut self,
        items: &[smelt_hir::ExprId],
        body: &Body,
    ) -> smelt_hir::TypeId {
        let item_tys = items
            .iter()
            .map(|item| Self::expr_ty(body, *item))
            .collect::<Vec<_>>();
        let Some(first_ty) = item_tys.first().copied() else {
            return self.ctx.krate.types.intern(Type::Unknown);
        };
        if item_tys.iter().all(|item_ty| *item_ty == first_ty) {
            return first_ty;
        }
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let non_nullish = item_tys
            .iter()
            .copied()
            .filter(|item_ty| *item_ty != none_ty)
            .collect::<Vec<_>>();
        if let Some(first_non_nullish) = non_nullish.first().copied()
            && non_nullish
                .iter()
                .all(|item_ty| *item_ty == first_non_nullish)
            && item_tys.contains(&none_ty)
        {
            return self.ctx.krate.types.intern(Type::Optional(first_non_nullish));
        }
        self.ctx.krate.types.intern(Type::Unknown)
    }

    /// Lower an array literal element with contextual type information.
    fn array_element_with_hint(
        &mut self,
        element: &ArrayExpressionElement<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match element {
            ArrayExpressionElement::ArrayExpression(array) => {
                self.array_expression(array, body, type_hint)
            }
            ArrayExpressionElement::ObjectExpression(object) => {
                self.object_expression(object, body, type_hint)
            }
            _ => self.array_element(element, body),
        }
    }

    /// Lower an array literal that contains one or more spread elements.
    fn array_expression_with_spread(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if type_hint.is_none()
            && let [ArrayExpressionElement::SpreadElement(spread)] = array.elements.as_slice()
        {
            let spread_value = self.expression(&spread.argument, body)?;
            let value_ty = self.type_param_constraint_or_self(Self::expr_ty(body, spread_value));
            let item_ty = match self.ctx.krate.types.get(value_ty) {
                Some(Type::List(item_ty) | Type::Set(item_ty)) => *item_ty,
                Some(Type::String) => self.ctx.krate.types.intern(Type::String),
                Some(Type::Unknown | Type::TypeParam { .. }) => {
                    self.ctx.krate.types.intern(Type::Unknown)
                }
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(spread.span.start, spread.span.end),
                        "array spread operands must be arrays or sets",
                    ));
                }
            };
            let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
            return self.list_expr_from_spread_value(spread_value, list_ty, spread.span, body);
        }
        let item_ty = self.array_spread_item_type(array, type_hint);
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let mut packed = None;
        let mut current_items = Vec::new();
        for element in &array.elements {
            match element {
                ArrayExpressionElement::SpreadElement(spread) => {
                    if !current_items.is_empty() {
                        let right = body.push_expr(Expr {
                            kind: ExprKind::ListLit(std::mem::take(&mut current_items)),
                            ty: list_ty,
                            span: self.span(array.span.start, array.span.end),
                        });
                        packed = Some(packed.map_or(right, |left| {
                            body.push_expr(Expr {
                                kind: ExprKind::ListConcat { left, right },
                                ty: list_ty,
                                span: self.span(array.span.start, array.span.end),
                            })
                        }));
                    }
                    let spread_value =
                        self.expression_with_hint(&spread.argument, body, Some(list_ty))?;
                    let right =
                        self.list_expr_from_spread_value(spread_value, list_ty, spread.span, body)?;
                    packed = Some(packed.map_or(right, |left| {
                        body.push_expr(Expr {
                            kind: ExprKind::ListConcat { left, right },
                            ty: list_ty,
                            span: self.span(array.span.start, array.span.end),
                        })
                    }));
                }
                ArrayExpressionElement::Elision(_) => {
                    return Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "array elisions are not lowered",
                    ));
                }
                _ => current_items.push(self.array_element(element, body)?),
            }
        }
        if !current_items.is_empty() {
            let right = body.push_expr(Expr {
                kind: ExprKind::ListLit(current_items),
                ty: list_ty,
                span: self.span(array.span.start, array.span.end),
            });
            packed = Some(packed.map_or(right, |left| {
                body.push_expr(Expr {
                    kind: ExprKind::ListConcat { left, right },
                    ty: list_ty,
                    span: self.span(array.span.start, array.span.end),
                })
            }));
        }
        packed.ok_or_else(|| {
            SmeltError::unsupported(
                self.span(array.span.start, array.span.end),
                "array spread literal requires at least one element",
            )
        })
    }

    /// Infer the list item type for an array spread literal.
    fn array_spread_item_type(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        if let Some(hint) = type_hint
            && let Some(Type::List(item_ty)) = self.ctx.krate.types.get(hint)
        {
            return *item_ty;
        }
        for element in &array.elements {
            match element {
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => {}
                _ => {
                    return self.ctx.krate.types.intern(Type::Unknown);
                }
            }
        }
        self.ctx.krate.types.intern(Type::Unknown)
    }

    /// Convert an iterable spread operand into the list value required by list concatenation.
    fn list_expr_from_spread_value(
        &self,
        value: smelt_hir::ExprId,
        list_ty: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value_ty = self.type_param_constraint_or_self(Self::expr_ty(body, value));
        match self.ctx.krate.types.get(value_ty).cloned() {
            Some(Type::List(_)) => Ok(value),
            Some(Type::Set(_)) => Ok(body.push_expr(Expr {
                kind: ExprKind::SetProjection {
                    op: SetProjectionOp::Values,
                    set: value,
                },
                ty: list_ty,
                span: self.span(span.start, span.end),
            })),
            Some(Type::String) => Ok(body.push_expr(Expr {
                kind: ExprKind::StringChars { haystack: value },
                ty: list_ty,
                span: self.span(span.start, span.end),
            })),
            Some(Type::Unknown | Type::TypeParam { .. }) => Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: list_ty,
                span: self.span(span.start, span.end),
            })),
            _ => Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "array spread operands must be arrays or sets",
            )),
        }
    }

    /// Lower an object expression.
    fn object_expression(
        &mut self,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if object
            .properties
            .iter()
            .any(|property| matches!(property, ObjectPropertyKind::SpreadProperty(_)))
        {
            return self.object_expression_with_spread(object, body, type_hint);
        }

        let mut entries = Vec::new();
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(property.span().start, property.span().end),
                    "object spread properties are not lowered yet",
                ));
            };
            if object_property.method {
                if self.object_method_erases_to_iterable_marker(object_property) {
                    continue;
                }
                return Err(SmeltError::unsupported(
                    self.span(object_property.span.start, object_property.span.end),
                    "object methods are not lowered yet",
                ));
            }
            if Self::is_computed_symbol_key(object_property) {
                continue;
            }
            let key = self.object_property_key_expr(object_property, body)?;
            let value_hint = self.object_property_value_hint(object_property, type_hint);
            let value = self.object_property_value_expr(object_property, body, value_hint)?;
            entries.push((key, value));
        }
        let ty = self.object_literal_type(&entries, type_hint, body);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty,
            span: self.span(object.span.start, object.span.end),
        }))
    }

    /// Lower an object expression that uses JavaScript spread properties.
    ///
    /// The spread order is preserved by lowering each contiguous explicit
    /// property run into a dictionary literal and combining those chunks with
    /// spread sources through the ordered `DictAssign` operation.
    fn object_expression_with_spread(
        &mut self,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut record_ty = self.dict_type_from_hint(type_hint);
        let mut sources = Vec::new();
        let mut pending_entries = Vec::new();

        for property in &object.properties {
            match property {
                ObjectPropertyKind::ObjectProperty(object_property) => {
                    if object_property.method {
                        if self.object_method_erases_to_iterable_marker(object_property) {
                            continue;
                        }
                        return Err(SmeltError::unsupported(
                            self.span(object_property.span.start, object_property.span.end),
                            "object methods are not lowered yet",
                        ));
                    }
                    let key = self.object_property_key_expr(object_property, body)?;
                    let value_hint = self.object_property_value_hint(object_property, record_ty);
                    let value = self.object_property_value_expr(object_property, body, value_hint)?;
                    pending_entries.push((key, value));
                }
                ObjectPropertyKind::SpreadProperty(spread) => {
                    self.flush_object_spread_entries(
                        &mut pending_entries,
                        &mut sources,
                        &mut record_ty,
                        body,
                        object.span,
                    );
                    if let Some(source) = self.conditional_object_spread_source(
                        &spread.argument,
                        record_ty,
                        body,
                        spread.span,
                    )? {
                        let source_ty = Self::expr_ty(body, source);
                        if record_ty.is_none()
                            && matches!(self.ctx.krate.types.get(source_ty), Some(Type::Dict(_, _)))
                        {
                            record_ty = Some(source_ty);
                        }
                        sources.push(source);
                        continue;
                    }
                    let source = self.expression_with_hint(&spread.argument, body, record_ty)?;
                    let source_ty = Self::expr_ty(body, source);
                    self.accept_object_spread_source(source_ty, record_ty, spread.span)?;
                    if record_ty.is_none()
                        && matches!(self.ctx.krate.types.get(source_ty), Some(Type::Dict(_, _)))
                    {
                        record_ty = Some(source_ty);
                    }
                    sources.push(source);
                }
            }
        }
        self.flush_object_spread_entries(
            &mut pending_entries,
            &mut sources,
            &mut record_ty,
            body,
            object.span,
        );

        let key_ty = self.ctx.krate.types.intern(Type::String);
        let fallback_value_ty = self.ctx.krate.types.intern(Type::Unknown);
        let record_ty = record_ty
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Dict(key_ty, fallback_value_ty)));
        let target = body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty: record_ty,
            span: self.span(object.span.start, object.span.start),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictAssign { target, sources },
            ty: record_ty,
            span: self.span(object.span.start, object.span.end),
        }))
    }

    /// Return true for `[Symbol.iterator]()` methods that only mark an object as iterable.
    fn object_method_erases_to_iterable_marker(
        &self,
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
    ) -> bool {
        let Ok(start) = usize::try_from(object_property.span.start) else {
            return false;
        };
        let Ok(end) = usize::try_from(object_property.span.end) else {
            return false;
        };
        self.source
            .get(start..end)
            .is_some_and(|text| text.contains("[Symbol.iterator]"))
    }

    /// Lower `...(condition && { ... })` object spread sources to conditional records.
    ///
    /// JavaScript object spread treats falsey primitives as empty sources. The
    /// HIR spread operation expects object-like sources, so this helper keeps the
    /// object branch typed as a record and supplies an empty record for the
    /// false branch instead of exposing the boolean result of `&&`.
    fn conditional_object_spread_source(
        &mut self,
        argument: &Expression<'_>,
        record_ty: Option<smelt_hir::TypeId>,
        body: &mut Body,
        span: oxc::span::Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let argument = Self::object_spread_condition_source(argument);
        let Expression::LogicalExpression(logical) = argument else {
            return Ok(None);
        };
        if logical.operator != LogicalOperator::And
            || !matches!(&logical.right, Expression::ObjectExpression(_))
        {
            return Ok(None);
        }
        let cond = self.expression(&logical.left, body)?;
        let rhs_narrowing = self.guard_narrowing(&logical.left, body);
        if let Some(narrowing) = rhs_narrowing.clone() {
            self.narrowed_locals.push(narrowing);
        }
        let then_expr = self.expression_with_hint(&logical.right, body, record_ty)?;
        if rhs_narrowing.is_some() {
            self.narrowed_locals.pop();
        }
        let source_ty = Self::expr_ty(body, then_expr);
        self.accept_object_spread_source(source_ty, record_ty, span)?;
        let else_expr = body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty: source_ty,
            span: self.span(span.start, span.start),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            },
            ty: source_ty,
            span: self.span(span.start, span.end),
        })))
    }

    /// Strip transparent wrappers around an object-spread source condition.
    fn object_spread_condition_source<'a>(argument: &'a Expression<'a>) -> &'a Expression<'a> {
        match argument {
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::object_spread_condition_source(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::object_spread_condition_source(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::object_spread_condition_source(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::object_spread_condition_source(&non_null.expression)
            }
            _ => argument,
        }
    }

    /// Resolve a contextual field type for an object-literal property value.
    fn object_property_value_hint(
        &mut self,
        property: &oxc::ast::ast::ObjectProperty<'_>,
        object_hint: Option<smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
        let hint = object_hint?;
        if let Some(Type::Dict(_, value_ty)) = self.ctx.krate.types.get(hint) {
            return Some(*value_ty);
        }
        let field_name = match &property.key {
            PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str(),
            PropertyKey::StringLiteral(literal) => literal.value.as_str(),
            _ => return None,
        };
        let field = self.intern_source_name(field_name);
        self.class_field_type(hint, field).ok()
    }

    /// Lower an object property value, treating zero-argument getters as field values.
    fn object_property_value_expr(
        &mut self,
        property: &oxc::ast::ast::ObjectProperty<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Expression::FunctionExpression(function) = &property.value {
            if !function.params.items.is_empty() || function.params.rest.is_some() {
                return self.function_expression_value(function, type_hint, property.span, body);
            }
            let Some(function_body) = &function.body else {
                return Err(SmeltError::unsupported(
                    self.span(function.span.start, function.span.end),
                    "object getter functions must have a body",
                ));
            };
            let [Statement::ReturnStatement(statement)] = function_body.statements.as_slice()
            else {
                return Err(SmeltError::unsupported(
                    self.span(function_body.span.start, function_body.span.end),
                    "object getter functions must contain one return statement",
                ));
            };
            let Some(argument) = &statement.argument else {
                return Err(SmeltError::unsupported(
                    self.span(statement.span.start, statement.span.end),
                    "object getter functions must return a value",
                ));
            };
            return self.expression_with_hint(argument, body, type_hint);
        }
        self.expression_with_hint(&property.value, body, type_hint)
    }

    /// Lower a function-valued object property into a closure expression.
    ///
    /// Object tables such as date-fns `formatters` use `key: function (...) {}`
    /// entries. Contextual object types provide the function parameter and
    /// return types when the function expression omits annotations.
    fn function_expression_value(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        span: oxc::span::Span,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "function expressions must have a body",
            ));
        };
        let hint_function = type_hint.and_then(|ty| {
            let ty = self
                .function_member_type(ty)
                .unwrap_or_else(|| self.type_param_constraint_or_self(ty));
            match self.ctx.krate.types.get(ty) {
                Some(Type::Function(function_ty)) => Some((ty, function_ty.clone())),
                _ => None,
            }
        });
        let return_ty = if let Some(return_type) = &function.return_type {
            self.ts_type_to_hir(&return_type.type_annotation)?
        } else if let Some((_, function_ty)) = &hint_function {
            function_ty.return_ty
        } else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "function expressions without return annotations need a function type hint",
            ));
        };

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        self.current_async = function.r#async;
        self.current_return_ty = Some(return_ty);
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut params = Vec::new();
        let mut errors = Vec::new();
        for (index, param) in function.params.items.iter().enumerate() {
            let result = (|| {
                let ty = if let Some(annotation) = &param.type_annotation {
                    self.ts_type_to_hir(&annotation.type_annotation)?
                } else if let Some((_, function_ty)) = &hint_function {
                    function_ty.params.get(index).copied().ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(param.span.start, param.span.end),
                            "function expression has more parameters than its type hint",
                        )
                    })?
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "function expression parameters need annotations or a function type hint",
                    ));
                };
                let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                    return Err(SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "function expression parameters must be identifiers",
                    ));
                };
                let param_name = self.intern_source_name(binding.name.as_str());
                let local = body.push_local(LocalDecl {
                    name: Some(param_name),
                    ty,
                    mutable: false,
                    span: self.span(binding.span.start, binding.span.end),
                });
                body.params.push(local);
                self.locals.insert(binding.name.to_string(), local);
                params.push(Param {
                    name: param_name,
                    local,
                    ty,
                    span: self.span(binding.span.start, binding.span.end),
                });
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(error);
                break;
            }
        }
        if function.params.rest.is_some() {
            errors.push(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "function expression rest parameters are not lowered in object values yet",
            ));
        }
        for statement in &function_body.statements {
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }
        if function.r#async {
            body.build_async_state_machine();
        }
        self.locals = saved_locals;
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let body_id = self.ctx.krate.push_body(body);
        let function_ty = hint_function.map_or_else(
            || {
                self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params: params.iter().map(|param| param.ty).collect(),
                    return_ty,
                    is_async: function.r#async,
                }))
            },
            |(ty, _)| ty,
        );
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params,
                return_ty,
                captures: Vec::new(),
                body: body_id,
                callback_body: None,
                span: self.span(function.span.start, function.span.end),
            }),
            ty: function_ty,
            span: self.span(span.start, span.end),
        }))
    }

    /// Lower an object property key to a dictionary key expression.
    fn object_property_key_expr(
        &mut self,
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if object_property.computed {
            return match &object_property.key {
                PropertyKey::Identifier(identifier) => self.identifier_expression(
                    identifier.name.as_str(),
                    identifier.span.start,
                    identifier.span.end,
                    body,
                ),
                PropertyKey::StringLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::String);
                    Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::String(literal.value.to_string())),
                        ty,
                        span: self.span(literal.span.start, literal.span.end),
                    }))
                }
                PropertyKey::NumericLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::Float);
                    Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::Float(literal.value)),
                        ty,
                        span: self.span(literal.span.start, literal.span.end),
                    }))
                }
                _ => {
                    if let Some(key_text) = self.computed_string_literal_key(object_property) {
                        let ty = self.ctx.krate.types.intern(Type::String);
                        return Ok(body.push_expr(Expr {
                            kind: ExprKind::Literal(Literal::String(key_text)),
                            ty,
                            span: self.span(
                                object_property.key.span().start,
                                object_property.key.span().end,
                            ),
                        }));
                    }
                    Err(SmeltError::unsupported(
                        self.span(
                            object_property.key.span().start,
                            object_property.key.span().end,
                        ),
                        "computed object keys support identifiers and literal keys for now",
                    ))
                }
            };
        }

        let key_text = match &object_property.key {
            PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
            PropertyKey::StringLiteral(lit) => lit.value.to_string(),
            PropertyKey::NumericLiteral(lit) => lit.raw.as_ref().map_or_else(
                || {
                    if lit.value.fract() == 0.0_f64 {
                        format!("{:.0}", lit.value)
                    } else {
                        lit.value.to_string()
                    }
                },
                ToString::to_string,
            ),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(
                        object_property.key.span().start,
                        object_property.key.span().end,
                    ),
                    "object literal keys must be static string keys or computed expressions",
                ));
            }
        };
        let key_ty = self.ctx.krate.types.intern(Type::String);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(key_text)),
            ty: key_ty,
            span: self.span(
                object_property.key.span().start,
                object_property.key.span().end,
            ),
        }))
    }

    /// Return true for computed symbol keys that `Object.entries` ignores.
    fn is_computed_symbol_key(object_property: &oxc::ast::ast::ObjectProperty<'_>) -> bool {
        if !object_property.computed {
            return false;
        }
        matches!(
            &object_property.key,
            PropertyKey::CallExpression(call)
                if matches!(&call.callee, Expression::Identifier(callee) if callee.name == "Symbol")
        ) || matches!(
            &object_property.key,
            PropertyKey::Identifier(identifier) if identifier.name.contains("SYMBOL")
        )
    }

    /// Extract the source string from a computed string literal key with erased assertions.
    fn computed_string_literal_key(
        &self,
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
    ) -> Option<String> {
        let source = self
            .source
            .get(
                usize::try_from(object_property.key.span().start).ok()?
                    ..usize::try_from(object_property.key.span().end).ok()?,
            )?
            .trim();
        let quote = source.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let rest = &source[quote.len_utf8()..];
        let end = rest.find(quote)?;
        Some(rest[..end].to_owned())
    }

    /// Flush pending explicit properties into an ordered object-spread source.
    fn flush_object_spread_entries(
        &mut self,
        pending_entries: &mut Vec<(smelt_hir::ExprId, smelt_hir::ExprId)>,
        sources: &mut Vec<smelt_hir::ExprId>,
        record_ty: &mut Option<smelt_hir::TypeId>,
        body: &mut Body,
        span: oxc::span::Span,
    ) {
        if pending_entries.is_empty() {
            return;
        }
        let entries = std::mem::take(pending_entries);
        let chunk_ty = self.object_literal_type(&entries, *record_ty, body);
        if record_ty.is_none() {
            *record_ty = Some(chunk_ty);
        }
        sources.push(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: record_ty.unwrap_or(chunk_ty),
            span: self.span(span.start, span.end),
        }));
    }

    /// Validate a source expression used by an object spread property.
    fn accept_object_spread_source(
        &mut self,
        source_ty: smelt_hir::TypeId,
        record_ty: Option<smelt_hir::TypeId>,
        span: oxc::span::Span,
    ) -> Result<(), SmeltError> {
        match self.ctx.krate.types.get(source_ty) {
            Some(Type::Dict(_, _)) if record_ty.is_none() || record_ty == Some(source_ty) => Ok(()),
            Some(Type::Dict(source_key, source_value)) => {
                let Some(record_ty) = record_ty else {
                    return Ok(());
                };
                let Some(Type::Dict(record_key, record_value)) = self.ctx.krate.types.get(record_ty).cloned()
                else {
                    return Ok(());
                };
                if self.map_key_type_compatible(record_key, *source_key)
                    && (record_value == *source_value
                        || self.numeric_type_compatible(record_value, *source_value)
                        || self
                            .non_nullish_type(*source_value)
                            .is_some_and(|inner| self.numeric_type_compatible(record_value, inner)))
                {
                    Ok(())
                } else {
                    Err(SmeltError::unsupported(
                        self.span(span.start, span.end),
                        "object spread sources must be record, generic object, or unknown values",
                    ))
                }
            }
            Some(Type::Optional(inner)) => {
                self.accept_object_spread_source(*inner, record_ty, span)
            }
            Some(Type::Class { .. } | Type::TypeParam { .. } | Type::Unknown) => Ok(()),
            _ => Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "object spread sources must be record, generic object, or unknown values",
            )),
        }
    }

    /// Extract a dictionary type from a contextual object-literal type hint.
    fn dict_type_from_hint(&self, type_hint: Option<smelt_hir::TypeId>) -> Option<smelt_hir::TypeId> {
        let ty = type_hint?;
        match self.ctx.krate.types.get(ty) {
            Some(Type::Dict(_, _)) => Some(ty),
            Some(Type::Union(members)) => members
                .iter()
                .copied()
                .find(|member| matches!(self.ctx.krate.types.get(*member), Some(Type::Dict(_, _)))),
            _ => None,
        }
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
        if let Some(value) = self.const_objects.get(member_name).cloned() {
            return Ok(Some(self.object_const_expression(&value, member.span.start, member.span.end, body)));
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
