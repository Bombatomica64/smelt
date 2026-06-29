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
        if !matches!(source_arg, Argument::ObjectExpression(_)) {
            let source = self.argument(source_arg, body)?;
            let source_ty = self.type_param_constraint_or_self(Self::expr_ty(body, source));
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
            let list_ty = match self.ctx.krate.types.get(source_ty).cloned() {
                Some(Type::List(_)) if mapper_arg.is_none() => return Ok(Some(source)),
                Some(Type::List(item_ty)) => self.ctx.krate.types.intern(Type::List(item_ty)),
                Some(Type::Set(item_ty)) => {
                    let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::SetProjection {
                            op: SetProjectionOp::Values,
                            set: source,
                        },
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                }
                Some(Type::Dict(key_ty, _)) => {
                    let ty = self.ctx.krate.types.intern(Type::List(key_ty));
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::DictProjection {
                            op: DictProjectionOp::Keys,
                            dict: source,
                        },
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                }
                _ => self.ctx.krate.types.intern(Type::List(unknown_ty)),
            };
            if let Some(mapper_arg) = mapper_arg {
                let _ = self.argument(mapper_arg, body)?;
            }
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: source,
                    target: list_ty,
                },
                ty: list_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
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
        let callback = self.callback_argument_with_body_fallback(
            mapper_arg,
            &[unknown_ty, index_ty],
            index_ty,
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
        self.lower_array_construction(
            &new_expr.arguments,
            new_expr.type_arguments.as_deref(),
            new_expr.span.start,
            new_expr.span.end,
            body,
        )
    }

    /// Lower a bare `Array(length)` / `Array(element)` call as a value-returning
    /// constructor expression.
    ///
    /// In ECMAScript the `Array` global produces an identical array whether
    /// invoked as `Array(...)` or `new Array(...)` (the spec routes both through
    /// the same constructor behavior). Reusing the `new Array` lowering keeps the
    /// two spellings in lockstep instead of special-casing the call form. The
    /// es-toolkit corpus relies heavily on `Array(n)` to preallocate a list that
    /// is then filled by indexed writes.
    pub(super) fn array_constructor_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "Array" {
            return Ok(None);
        }
        self.lower_array_construction(
            &call.arguments,
            call.type_arguments.as_deref(),
            call.span.start,
            call.span.end,
            body,
        )
        .map(Some)
    }

    /// Shared core for `Array(...)` and `new Array(...)` construction.
    ///
    /// JavaScript creates a sparse array here; Smelt models the later indexed
    /// writes and only needs the list container type at construction time. A
    /// single array-literal argument (`Array([1, 2])`) builds that literal, a
    /// single numeric argument (`Array(3)`) preallocates a list, and an optional
    /// type argument supplies the element type.
    fn lower_array_construction(
        &mut self,
        arguments: &[Argument<'_>],
        type_arguments: Option<&oxc::ast::ast::TSTypeParameterInstantiation<'_>>,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(start, end),
                "Array(...) supports at most one length argument",
            ));
        }
        if let Some(Argument::ArrayExpression(array)) = arguments.first() {
            return self.array_expression(array, body, None);
        }
        if let Some(length) = arguments.first() {
            let length = self.argument(length, body)?;
            // The preallocation length is only used to size the (initially empty)
            // list, which Smelt models through later indexed writes, so the value
            // itself is discarded. Accept any numeric-like type plus the erased /
            // optional-numeric surfaces that flow from JS `number | undefined`
            // parameters; only reject clearly non-numeric arguments.
            let length_ty = Self::expr_ty(body, length);
            let numeric = self.is_numeric_like_type(length_ty)
                || matches!(
                    self.ctx.krate.types.get(length_ty),
                    Some(Type::Int | Type::Float)
                )
                || self.optional_numeric_surface(length_ty)
                || self.erased_or_union_surface(length_ty);
            if !numeric {
                return Err(SmeltError::unsupported(
                    self.span(start, end),
                    "Array(...) length must be numeric",
                ));
            }
        }
        let item_ty = if let Some(type_args) = type_arguments {
            let [item] = type_args.params.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(start, end),
                    "Array(...) supports exactly one type argument",
                ));
            };
            self.ts_type_to_hir(item)?
        } else {
            self.ctx.krate.types.intern(Type::Unknown)
        };
        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
        Ok(body.push_expr(Expr {
            kind: ExprKind::ListLit(Vec::new()),
            ty,
            span: self.span(start, end),
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
        let length = if let Some(length) = new_expr.arguments.first() {
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
            Some(length)
        } else {
            None
        };
        let item_ty = self.ctx.krate.types.intern(Type::Float);
        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let Some(length) = length else {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ListLit(Vec::new()),
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        };
        let zero = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(0.0)),
            ty: item_ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::ListRepeat {
                value: zero,
                count: length,
            },
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
        if let Expression::Identifier(object) = &member.object
            && (self.namespace_imports.contains(object.name.as_str())
                || self.value_imports.contains(object.name.as_str()))
        {
            if call.arguments.len() < 2 || call.arguments.len() > 3 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static string split requires value, separator, and optional limit arguments",
                ));
            }
            let Some(haystack_argument) = call.arguments.first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static string split requires value, separator, and optional limit arguments",
                ));
            };
            let Some(separator_argument) = call.arguments.get(1) else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static string split requires value, separator, and optional limit arguments",
                ));
            };
            let haystack = self.argument(haystack_argument, body)?;
            let separator = self.argument(separator_argument, body)?;
            let limit = call
                .arguments
                .get(2)
                .map(|argument| self.argument(argument, body))
                .transpose()?;
            return self.finish_string_split_call(call, haystack, separator, limit, body);
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
        self.finish_string_split_call(call, haystack, separator, limit, body)
    }

    /// Finish string split lowering after the receiver-style or helper-style arguments are known.
    fn finish_string_split_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        haystack: smelt_hir::ExprId,
        separator: smelt_hir::ExprId,
        limit: Option<smelt_hir::ExprId>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let haystack_ty = Self::expr_ty(body, haystack);
        let separator_ty = Self::expr_ty(body, separator);
        if !(self.is_string_compatible_type(haystack_ty)
            || self.type_contains_unknown(haystack_ty)
            || self.erased_or_union_surface(haystack_ty))
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
                _ => {
                    let value = self.array_element(element, body)?;
                    if matches!(
                        self.ctx.krate.types.get(Self::expr_ty(body, value)),
                        Some(Type::Future(_))
                    ) {
                        return Ok(value);
                    }
                    let duration = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::Float(0.0)),
                        ty: self.ctx.krate.types.intern(Type::Float),
                        span: self.span(element.span().start, element.span().start),
                    });
                    let ty = self
                        .ctx
                        .krate
                        .types
                        .intern(Type::Future(Self::expr_ty(body, value)));
                    Ok(body.push_expr(Expr {
                        kind: ExprKind::AsyncOp {
                            op: AsyncOp::Sleep,
                            args: vec![duration],
                        },
                        ty,
                        span: self.span(element.span().start, element.span().end),
                    }))
                }
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
        if !Self::is_ts_stdlib_class_name(callee.name.as_str(), smelt_stdlib::StdlibClass::Set) {
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
                let mut list = self.argument(argument, body)?;
                let raw_ty = Self::expr_ty(body, list);
                let list_ty = self.type_param_constraint_or_self(raw_ty);
                // `new Set(iterable)` accepts arrays directly. Optional arrays are
                // asserted to their inner list, an existing Set is already in set
                // shape, and erased/union surfaces (e.g. a generic helper return
                // typed `unknown`) are asserted to `List<Unknown>` so the
                // list-to-set conversion can proceed instead of being rejected.
                let item_ty = match self.ctx.krate.types.get(list_ty).cloned() {
                    Some(Type::List(item_ty)) => item_ty,
                    Some(Type::Optional(inner)) => {
                        if let Some(Type::List(item_ty)) =
                            self.ctx.krate.types.get(inner).cloned()
                        {
                            list = body.push_expr(Expr {
                                kind: ExprKind::TypeAssert { value: list },
                                ty: inner,
                                span: self.span(argument.span().start, argument.span().end),
                            });
                            item_ty
                        } else {
                            return Err(SmeltError::unsupported(
                                self.span(argument.span().start, argument.span().end),
                                "new Set(iterable) currently requires an array argument",
                            ));
                        }
                    }
                    Some(Type::Set(item_ty)) => {
                        // `new Set(otherSet)` copies an existing set: keep its
                        // element type and short-circuit (no list conversion).
                        let ty = if let Some(hint) = type_hint
                            && matches!(self.ctx.krate.types.get(hint), Some(Type::Set(_)))
                        {
                            hint
                        } else {
                            self.ctx.krate.types.intern(Type::Set(item_ty))
                        };
                        return Ok(Some(body.push_expr(Expr {
                            kind: ExprKind::TypeAssert { value: list },
                            ty,
                            span: self.span(new_expr.span.start, new_expr.span.end),
                        })));
                    }
                    _ if self.erased_or_union_surface(list_ty) => {
                        let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                        let asserted_list_ty =
                            self.ctx.krate.types.intern(Type::List(item_ty));
                        list = body.push_expr(Expr {
                            kind: ExprKind::TypeAssert { value: list },
                            ty: asserted_list_ty,
                            span: self.span(argument.span().start, argument.span().end),
                        });
                        item_ty
                    }
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(argument.span().start, argument.span().end),
                            "new Set(iterable) currently requires an array argument",
                        ));
                    }
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
                    self.ctx.krate.types.intern(Type::Set(item_ty))
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
        if !Self::is_ts_stdlib_class_name(callee.name.as_str(), smelt_stdlib::StdlibClass::Map) {
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
                    let unknown = self.ctx.krate.types.intern(Type::Unknown);
                    let dict_ty = self.ctx.krate.types.intern(Type::Dict(unknown, unknown));
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::DictLit(Vec::new()),
                        ty: dict_ty,
                        span: self.span(new_expr.span.start, new_expr.span.end),
                    })));
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
                for argument in &new_expr.arguments {
                    let _ = self.argument(argument, body)?;
                }
                let unknown = self.ctx.krate.types.intern(Type::Unknown);
                (
                    Vec::new(),
                    self.ctx.krate.types.intern(Type::Dict(unknown, unknown)),
                )
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// Lower `new Object()` / `Object(...)` to a concrete record value.
    ///
    /// JavaScript `Object()` is the plain-object constructor. Smelt models plain
    /// objects with the same concrete record representation as an object literal
    /// `{}` (a `Type::Dict` carrying `ExprKind::DictLit`), so no value is routed
    /// through `SmeltUnknown`:
    ///
    /// - `new Object()` / `Object()` / `Object(null)` / `Object(undefined)`
    ///   produce a fresh empty record, exactly like `{}`.
    /// - `Object(value)` where `value` is already an object/record (a `Dict`,
    ///   `Class`, or `unknown` surface) returns that value unchanged, matching
    ///   `Object(obj) === obj`.
    /// - Boxing a primitive (`Object(42)` -> a boxed `Number` object) has no
    ///   concrete Smelt model yet and is rejected as an unsupported lowering
    ///   rather than erased, so the boundary stays explicit.
    fn object_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let argument = match new_expr.arguments.as_slice() {
            [] => None,
            [Argument::NullLiteral(_)] => None,
            [Argument::Identifier(ident)] if ident.name == "undefined" => None,
            [argument] => Some(self.argument(argument, body)?),
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "Object constructor supports at most one argument",
                ));
            }
        };
        if let Some(value) = argument {
            let value_ty = self.type_param_constraint_or_self(Self::expr_ty(body, value));
            if matches!(
                self.ctx.krate.types.get(value_ty),
                Some(Type::Dict(_, _) | Type::Class { .. } | Type::Unknown)
            ) {
                return Ok(value);
            }
            return Err(SmeltError::unsupported(
                span,
                "Object(value) boxing of non-object values is not lowered yet",
            ));
        }
        let ty = self.object_literal_type(&[], type_hint, body);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty,
            span,
        }))
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
            ArrayExpressionElement::ConditionalExpression(conditional) => {
                self.conditional_expression(conditional, body, None)
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
                                span: self
                                    .span(nested_element.span().start, nested_element.span().end),
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
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let pattern = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(
                        Self::regex_literal_pattern_text_without_flags(literal),
                    )),
                    ty: string_ty,
                    span: self.span(literal.span.start, literal.span.end),
                });
                let flags = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(literal.regex.flags.to_string())),
                    ty: string_ty,
                    span: self.span(literal.span.start, literal.span.end),
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: self.intern_type_name("RegExp"),
                        args: vec![pattern, flags],
                    },
                    ty: self.regexp_type(),
                    span: self.span(literal.span.start, literal.span.end),
                }))
            }
            ArrayExpressionElement::TemplateLiteral(template) => {
                self.template_literal_expression(template, body)
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
            // `===`/`!==` keep JS reference semantics (`JsStrictEq`); `==`/`!=`
            // stay structural (`Eq`). See builder_part08's mapping.
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
            _ => {
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
        if let Some(expr) = self.logical_and_numeric_value_expression(logical, body)? {
            return Ok(expr);
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

    /// Lower JavaScript `left && numeric` expressions in numeric value contexts.
    ///
    /// JavaScript returns either the falsy left value or the right value. When
    /// the right side is numeric, generated Rust needs a numeric result instead
    /// of the boolean shape used for conditions, so falsy left values are
    /// represented by numeric zero.
    fn logical_and_numeric_value_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if logical.operator != LogicalOperator::And {
            return Ok(None);
        }
        let rhs = self.expression(&logical.right, body)?;
        let rhs_ty = Self::expr_ty(body, rhs);
        if !self.is_numeric_like_type(rhs_ty) {
            return Ok(None);
        }
        let cond = self.condition_expression(&logical.left, body)?;
        let zero = body.push_expr(Expr {
            kind: match self.ctx.krate.types.get(rhs_ty) {
                Some(Type::Int) => ExprKind::Literal(Literal::Int(0)),
                _ => ExprKind::Literal(Literal::Float(0.0)),
            },
            ty: rhs_ty,
            span: self.span(logical.left.span().start, logical.left.span().end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: rhs,
                else_expr: zero,
            },
            ty: rhs_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
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
        if let Expression::LogicalExpression(left_logical) =
            Self::unparenthesized_expression(&logical.left)
            && let Some(value) =
                self.logical_and_value_fallback_expression(logical, left_logical, body)?
        {
            return Ok(Some(value));
        }
        if let Expression::LogicalExpression(left_logical) =
            Self::unparenthesized_expression(&logical.left)
            && let Some(value) = self.logical_and_numeric_value_expression(left_logical, body)?
        {
            let value_ty = Self::expr_ty(body, value);
            if let Some(expr) =
                self.logical_or_numeric_fallback_expression(logical, body, value, value_ty)?
            {
                return Ok(Some(expr));
            }
        }
        let optional = self.expression(&logical.left, body)?;
        let optional_ty = Self::expr_ty(body, optional);
        if self.ctx.krate.types.get(optional_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(optional_ty)
        {
            let optional_receiver = self.optionalize_index_receiver(optional, body);
            let optional_receiver_ty = Self::expr_ty(body, optional_receiver);
            if self.is_nullishable_type(optional_receiver_ty) {
                let fallback =
                    self.expression_with_hint(&logical.right, body, Some(optional_receiver_ty))?;
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::OptionalCoalesce {
                        optional: optional_receiver,
                        fallback,
                    },
                    ty: self.ctx.krate.types.intern(Type::Unknown),
                    span: self.span(logical.span.start, logical.span.end),
                })));
            }
        }
        if !self.is_nullishable_type(optional_ty) {
            if let Some(expr) =
                self.logical_or_unknown_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_numeric_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_object_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_string_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_list_fallback_expression(logical, body, optional, optional_ty)?
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
        if matches!(self.ctx.krate.types.get(ty), Some(Type::Function(_))) {
            let fallback = self.expression(&logical.right, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::OptionalCoalesce { optional, fallback },
                ty: self.ctx.krate.types.intern(Type::Unknown),
                span: self.span(logical.span.start, logical.span.end),
            })));
        }
        let fallback = self.expression_with_hint(&logical.right, body, Some(ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        let ty = if fallback_ty == ty {
            ty
        } else if self.ctx.krate.types.get(ty) == Some(&Type::Unknown)
            || self.ctx.krate.types.get(fallback_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(ty)
            || self.type_contains_unknown(fallback_ty)
        {
            self.ctx.krate.types.intern(Type::Unknown)
        } else if self.is_structural_object_surface(ty) {
            // Object values are always truthy in JavaScript; keep the selected
            // runtime value when their fallback widens the expression surface.
            self.ctx.krate.types.intern(Type::Unknown)
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

    /// Lower JavaScript `left || right` for object-like values without string coercion.
    ///
    /// Some type aliases that include `null` are represented as object surfaces
    /// after TypeScript lowering. JavaScript still returns the selected operand
    /// for `||`, so object-like operands must branch on runtime truthiness before
    /// the string fallback path can treat classes as string-compatible values.
    fn logical_or_object_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_structural_object_surface(value_ty) {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&logical.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        if !self.is_structural_object_surface(fallback_ty) {
            return Ok(None);
        }
        let cond = body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToBool,
                operand: value,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.left.span().start, logical.left.span().end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: self.ctx.krate.types.intern(Type::Unknown),
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || right` for erased values without losing the selected operand.
    ///
    /// Dynamic interop and imported structural callbacks can surface as
    /// `unknown` even when the source expression returns an object-like value.
    /// JavaScript `||` returns one of the original operands, so erased values
    /// must branch on runtime truthiness instead of being coerced through a
    /// string or boolean fallback representation.
    fn logical_or_unknown_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.type_contains_unknown(value_ty) {
            return Ok(None);
        }
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let fallback = self.expression_with_hint(&logical.right, body, Some(unknown_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        if !self.type_contains_unknown(fallback_ty) {
            return Ok(None);
        }
        let cond = body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToBool,
                operand: value,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.left.span().start, logical.left.span().end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: unknown_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `(guard && value) || fallback` as a value fallback.
    ///
    /// The normal logical lowering produces booleans because `&&`/`||` are also
    /// used in conditions. In value positions, JavaScript preserves the selected
    /// operand. This shape appears in option-bag and locale lookup code where a
    /// guarded member access falls back to another member with the same value
    /// type.
    fn logical_and_value_fallback_expression(
        &mut self,
        outer: &oxc::ast::ast::LogicalExpression<'_>,
        left: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if left.operator != LogicalOperator::And {
            return Ok(None);
        }
        let value = self.expression(&left.right, body)?;
        let value_ty = Self::expr_ty(body, value);
        if self.is_numeric_like_type(value_ty) {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&outer.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        let Some(result_ty) = self.logical_fallback_result_type(value_ty, fallback_ty) else {
            return Ok(None);
        };
        let cond = self.condition_expression(&outer.left, body)?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: result_ty,
            span: self.span(outer.span.start, outer.span.end),
        })))
    }

    /// Return the common value type for JavaScript logical fallback operands.
    fn logical_fallback_result_type(
        &mut self,
        value_ty: smelt_hir::TypeId,
        fallback_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        if value_ty == fallback_ty {
            return Some(value_ty);
        }
        if self.is_string_compatible_type(value_ty) && self.is_string_compatible_type(fallback_ty) {
            return Some(self.ctx.krate.types.intern(Type::String));
        }
        if self.type_contains_unknown(value_ty) || self.type_contains_unknown(fallback_ty) {
            return Some(self.ctx.krate.types.intern(Type::Unknown));
        }
        match (
            self.ctx.krate.types.get(value_ty),
            self.ctx.krate.types.get(fallback_ty),
        ) {
            (Some(Type::Optional(value)), _) if *value == fallback_ty => Some(fallback_ty),
            (_, Some(Type::Optional(fallback))) if value_ty == *fallback => Some(value_ty),
            _ => None,
        }
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

    /// Lower JavaScript `left || right` fallback expressions selected by string truthiness.
    ///
    /// A numeric fallback remains an erased selected value because expressions
    /// such as `+(parts[index] || 0)` numerically coerce either branch after
    /// selection. Emitting a boolean result would discard the string value.
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
        let result_ty = if self.is_string_compatible_type(fallback_ty) {
            self.ctx.krate.types.intern(Type::String)
        } else if self.is_numeric_like_type(fallback_ty) {
            self.ctx.krate.types.intern(Type::Unknown)
        } else {
            return Ok(None);
        };
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
            ty: result_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || []` fallback expressions for array values.
    fn logical_or_list_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(item_ty) = self.list_fallback_item_ty(value_ty) else {
            return Ok(None);
        };
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let fallback = self.expression_with_hint(&logical.right, body, Some(list_ty))?;
        if !Self::is_empty_list_expr(body, fallback) && Self::expr_ty(body, fallback) != list_ty {
            return Ok(None);
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce {
                optional: value,
                fallback,
            },
            ty: list_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Return the item type for a value that can participate in an array fallback.
    fn list_fallback_item_ty(&mut self, value_ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(value_ty).cloned() {
            Some(Type::List(item_ty)) => Some(item_ty),
            Some(Type::Optional(inner_ty)) => self.list_fallback_item_ty(inner_ty),
            Some(Type::Union(items)) => items
                .into_iter()
                .find_map(|item| self.list_fallback_item_ty(item)),
            _ => None,
        }
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
        let right_hint = match &logical.right {
            Expression::LogicalExpression(right_logical)
                if right_logical.operator == LogicalOperator::Coalesce =>
            {
                type_hint
            }
            _ => Some(ty),
        };
        let mut fallback = self.expression_with_hint(&logical.right, body, right_hint)?;
        let fallback_ty = Self::expr_ty(body, fallback);
        let ty = if fallback_ty == ty
            || self.ctx.krate.types.get(ty) == Some(&Type::Unknown)
            || self.numeric_type_compatible(ty, fallback_ty)
        {
            ty
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && self.numeric_type_compatible(ty, fallback_inner)
        {
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && fallback_inner == ty
        {
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if matches!(self.ctx.krate.types.get(optional_ty), Some(Type::Optional(inner)) if *inner == ty)
            && self.erased_or_union_surface(fallback_ty)
        {
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && self.nullish_fallback_types_are_structurally_compatible(ty, fallback_inner)
        {
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty: smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty),
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if matches!(self.ctx.krate.types.get(ty), Some(Type::TypeParam { .. }))
            && matches!(
                self.ctx.krate.types.get(fallback_ty),
                Some(Type::TypeParam { .. })
            )
        {
            type_hint.unwrap_or_else(|| {
                self.ctx
                    .krate
                    .types
                    .intern(Type::Union(vec![ty, fallback_ty]))
            })
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
        } else if self.erased_or_union_surface(ty)
            || self.erased_or_union_surface(fallback_ty)
            || !self.concrete_type_requires_never_value(ty)
        {
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty,
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            ty
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
        let fallback_ty = self.non_nullish_type(fallback_ty).unwrap_or(fallback_ty);
        self.is_structural_object_surface(optional_inner)
            && self.is_structural_object_surface(fallback_ty)
    }

    /// Return whether a type behaves as a structural object surface.
    fn is_structural_object_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Class { .. } | Type::Dict(_, _) | Type::TypeParam { .. } | Type::Unknown,
            ) => true,
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
        smelt_hir::type_normalize::non_nullish_type(&mut self.ctx.krate.types, ty)
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

    /// Fold a `"<key>" in <global-alias>` feature probe to a literal.
    ///
    /// The receiver must be a recognized global alias (bare `globalThis` /
    /// `global` / `self`, or a local known to alias the global object) and the key
    /// must be a string literal — a dynamic key is on the erasure denylist and
    /// stays a runtime check. The presence answer is derived from the
    /// recognition registries via [`smelt_stdlib::global_member_presence`], so an
    /// unmodeled key (`Unknown`) is *not* folded: it returns `None` and falls
    /// through to ordinary lowering instead of guessing.
    fn global_contains_key_probe(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if !self.expr_is_global_alias(&binary.right) {
            return None;
        }
        let Expression::StringLiteral(key_lit) = &binary.left else {
            return None;
        };
        let presence = smelt_stdlib::global_member_presence(key_lit.value.as_str());
        let value = match presence {
            smelt_stdlib::GlobalPresence::Present => true,
            smelt_stdlib::GlobalPresence::Absent => false,
            // `Unknown` (and any future undecided presence) must not fold.
            _ => return None,
        };
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(value)),
            ty: bool_ty,
            span: self.span(binary.span.start, binary.span.end),
        }))
    }

    /// Lower JavaScript `key in object` checks for dictionaries and static objects.
    ///
    /// Static object constants are often erased to reusable metadata before a
    /// function body is lowered. For those, membership is a pure key-set test,
    /// so emitting string equality checks keeps the generated Rust independent
    /// from a runtime object allocation.
    fn in_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(binary.span.start, binary.span.end);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let string_ty = self.ctx.krate.types.intern(Type::String);

        if let Some(expr) = self.global_contains_key_probe(binary, body) {
            return Ok(expr);
        }

        // A `<key> in <global-alias>` membership test that the registry-derived
        // probe above could not fold (an unknown/undecided member, or a
        // non-literal key) must stay an honest blocker. The global object now
        // resolves to a marker host-object value (see
        // `global_object_value_expression`), so without this guard the test
        // would silently evaluate against the empty marker record and answer
        // `false` for members the real global actually has. Presence of the
        // global object as a value does not make its full key set known.
        if self.expr_is_global_alias(&binary.right) {
            return Err(SmeltError::unsupported(
                span,
                "`in` on the global object is only lowered for registry-decidable string-literal keys",
            ));
        }

        if let Expression::Identifier(receiver_ident) = &binary.right
            && let Some(object_const) = self
                .const_objects
                .get(receiver_ident.name.as_str())
                .cloned()
        {
            let mut key = self.expression(&binary.left, body)?;
            if Self::expr_ty(body, key) != string_ty {
                key = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: key },
                    ty: string_ty,
                    span: self.span(binary.left.span().start, binary.left.span().end),
                });
            }
            let mut condition = None;
            for entry in object_const.entries {
                let rhs = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(entry.key)),
                    ty: string_ty,
                    span,
                });
                let equals_key = body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Eq,
                        lhs: key,
                        rhs,
                    },
                    ty: bool_ty,
                    span,
                });
                condition = Some(condition.map_or(equals_key, |previous| {
                    body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Or,
                            lhs: previous,
                            rhs: equals_key,
                        },
                        ty: bool_ty,
                        span,
                    })
                }));
            }
            return Ok(condition.unwrap_or_else(|| {
                body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(false)),
                    ty: bool_ty,
                    span,
                })
            }));
        }

        let receiver = self.expression(&binary.right, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let mut key = self.expression(&binary.left, body)?;
        if matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::Optional(_)))
            && matches!(&binary.left, Expression::StringLiteral(value) if value.value == "done")
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty: bool_ty,
                span,
            }));
        }
        let Some(Type::Dict(receiver_key_ty, _)) = self.ctx.krate.types.get(receiver_ty) else {
            if self.ctx.krate.types.get(receiver_ty) == Some(&Type::Unknown)
                || self.erased_or_union_surface(receiver_ty)
                || matches!(
                    self.ctx.krate.types.get(receiver_ty),
                    Some(
                        Type::TypeParam { .. }
                            | Type::Class { .. }
                            | Type::List(_)
                            | Type::Tuple(_)
                            | Type::String
                    )
                )
            {
                let receiver = if self.ctx.krate.types.get(receiver_ty) == Some(&Type::Unknown) {
                    receiver
                } else {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: receiver },
                        ty,
                        span,
                    })
                };
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::DictContainsKey {
                        dict: receiver,
                        key,
                    },
                    ty: bool_ty,
                    span,
                }));
            }
            return Err(SmeltError::unsupported(
                span,
                "`in` checks require a static object, record, map, or unknown receiver",
            ));
        };
        let key_ty = *receiver_key_ty;
        if Self::expr_ty(body, key) != key_ty
            && self.is_string_compatible_type(Self::expr_ty(body, key))
        {
            key = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: key },
                ty: key_ty,
                span,
            });
        }
        if Self::expr_ty(body, key) != key_ty {
            return Err(SmeltError::unsupported(
                span,
                "`in` check key must match the record or map key type",
            ));
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictContainsKey {
                dict: receiver,
                key,
            },
            ty: bool_ty,
            span,
        }))
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
        if unary.operator == UnaryOperator::Typeof {
            return self.typeof_expression(unary, body);
        }
        if unary.operator == UnaryOperator::Void {
            let ty = self.ctx.krate.types.intern(Type::None);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Undefined),
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
                self.span(unary.argument.span().start, unary.argument.span().end),
                body,
            )
            .unwrap_or(operand)
        } else {
            operand
        };
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
        let (object, key) = match &unary.argument {
            Expression::ComputedMemberExpression(member) => {
                let object = self.expression(&member.object, body)?;
                let key = self.expression(&member.expression, body)?;
                (object, key)
            }
            Expression::StaticMemberExpression(member) => {
                let object = self.expression(&member.object, body)?;
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let key = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(member.property.name.to_string())),
                    ty: string_ty,
                    span: self.span(member.property.span.start, member.property.span.end),
                });
                (object, key)
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(unary.argument.span().start, unary.argument.span().end),
                    "delete is only lowered for object keys",
                ));
            }
        };
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictRemoveKey { dict: object, key },
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
            return self
                .ctx
                .krate
                .types
                .intern(Type::Optional(first_non_nullish));
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
                Some(
                    Type::Unknown
                    | Type::TypeParam { .. }
                    | Type::Class { .. }
                    | Type::Optional(_)
                    | Type::Union(_),
                )
                | None => self.ctx.krate.types.intern(Type::Unknown),
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
        // The operand's *own* declared type, BEFORE resolving a type parameter to
        // its constraint. An erased operand (`Type::TypeParam`/`Type::Unknown`)
        // whose constraint happens to be a list (e.g. Remeda's
        // `T extends IterableContainer = readonly unknown[]`) would otherwise hit
        // the `Type::List` arm below and be returned UNCHANGED — an alias that keeps
        // the erased type. That alias defeats typed list operations: a later
        // `[...items].sort(cmp)` stays dynamic and the sort result is discarded
        // (see blocker-logs/plan-sort-sortby-2026-06-23.md, Family 1, Option B).
        let raw_value_ty = Self::expr_ty(body, value);
        let value_ty = self.type_param_constraint_or_self(raw_value_ty);
        match self.ctx.krate.types.get(value_ty).cloned() {
            // A spread of an erased operand with a list constraint: construct a
            // FRESH `List`-typed value instead of returning the erased alias, so the
            // binding (`const ret = [...items]`) is a real `Vec` and downstream
            // typed list methods (e.g. in-place `sort`) fire. Reuse the verified
            // fresh-list idiom `ListConcat(value, [])`, which the multi-spread path
            // also uses; its emitter materializes a fresh `Vec` for erased operands.
            // A `[...list]` spread is a NEW array in JS, never an alias of its
            // source. Build it via the verified fresh-list idiom
            // `ListConcat(value, [])` (also used by the multi-spread path): it
            // coerces element types, materializes a fresh `Vec` for an erased
            // operand, and (via the empty-concat `fresh_copy`) mints a fresh
            // reference id for a concrete list — so the result never `===` source.
            Some(Type::List(_)) => {
                let empty = body.push_expr(Expr {
                    kind: ExprKind::ListLit(Vec::new()),
                    ty: list_ty,
                    span: self.span(span.start, span.end),
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::ListConcat {
                        left: value,
                        right: empty,
                    },
                    ty: list_ty,
                    span: self.span(span.start, span.end),
                }))
            }
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
            Some(Type::Class { .. } | Type::Optional(_) | Type::Union(_)) | None => Ok(body
                .push_expr(Expr {
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
            if object_property.kind == PropertyKind::Get {
                if Self::is_computed_symbol_key(object_property) {
                    continue;
                }
                let key = self.object_property_key_expr(object_property, body)?;
                let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                let getter =
                    if let Expression::FunctionExpression(function) = &object_property.value {
                        let getter_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                            params: Vec::new(),
                            rest: None,
                            required_params: None,
                    mutable_params: Vec::new(),
                            return_ty: unknown_ty,
                            is_async: false,
                            may_throw: false,
                        }));
                        self.function_expression_value(
                            function,
                            Some(getter_ty),
                            object_property.span,
                            body,
                        )?
                    } else {
                        self.object_property_value_expr(object_property, body, Some(unknown_ty))?
                    };
                let marker_key_ty = self.ctx.krate.types.intern(Type::String);
                let marker_key = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String("__smelt_get".to_owned())),
                    ty: marker_key_ty,
                    span: self.span(
                        object_property.key.span().start,
                        object_property.key.span().end,
                    ),
                });
                let marker_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Dict(marker_key_ty, unknown_ty));
                let value = body.push_expr(Expr {
                    kind: ExprKind::DictLit(vec![(marker_key, getter)]),
                    ty: marker_ty,
                    span: self.span(object_property.span.start, object_property.span.end),
                });
                entries.push((key, value));
                continue;
            }
            if object_property.method {
                if self.object_method_erases_to_iterable_marker(object_property)
                    && let Expression::FunctionExpression(function) = &object_property.value
                {
                    let key = self.object_property_key_expr(object_property, body)?;
                    let value =
                        self.function_expression_value(function, None, object_property.span, body)?;
                    entries.push((key, value));
                    continue;
                }
                let key = self.object_property_key_expr(object_property, body)?;
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                let value = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(object_property.span.start, object_property.span.end),
                });
                entries.push((key, value));
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
        let mut erased_spread_requires_unknown_record = false;

        for property in &object.properties {
            match property {
                ObjectPropertyKind::ObjectProperty(object_property) => {
                    if object_property.method {
                        if self.object_method_erases_to_iterable_marker(object_property)
                            && let Expression::FunctionExpression(function) = &object_property.value
                        {
                            let key = self.object_property_key_expr(object_property, body)?;
                            let value = self.function_expression_value(
                                function,
                                None,
                                object_property.span,
                                body,
                            )?;
                            pending_entries.push((key, value));
                            continue;
                        }
                        let key = self.object_property_key_expr(object_property, body)?;
                        let ty = self.ctx.krate.types.intern(Type::Unknown);
                        let value = body.push_expr(Expr {
                            kind: ExprKind::Literal(Literal::None),
                            ty,
                            span: self.span(object_property.span.start, object_property.span.end),
                        });
                        pending_entries.push((key, value));
                        continue;
                    }
                    let key = self.object_property_key_expr(object_property, body)?;
                    let value_hint = self.object_property_value_hint(object_property, record_ty);
                    let value =
                        self.object_property_value_expr(object_property, body, value_hint)?;
                    pending_entries.push((key, value));
                }
                ObjectPropertyKind::SpreadProperty(spread) => {
                    self.flush_object_spread_entries(
                        &mut pending_entries,
                        &mut sources,
                        &mut record_ty,
                        &mut erased_spread_requires_unknown_record,
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
                    let mut source =
                        self.expression_with_hint(&spread.argument, body, record_ty)?;
                    let source_ty = Self::expr_ty(body, source);
                    if self.object_spread_source_erases_to_empty(source_ty) {
                        let ty = record_ty.unwrap_or_else(|| {
                            let key_ty = self.ctx.krate.types.intern(Type::String);
                            let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                            self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
                        });
                        source = body.push_expr(Expr {
                            kind: ExprKind::DictLit(Vec::new()),
                            ty,
                            span: self.span(spread.span.start, spread.span.end),
                        });
                    } else if self
                        .accept_object_spread_source(source_ty, record_ty, spread.span)
                        .is_err()
                    {
                        let ty = record_ty.unwrap_or_else(|| {
                            let key_ty = self.ctx.krate.types.intern(Type::String);
                            let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                            self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
                        });
                        source = body.push_expr(Expr {
                            kind: ExprKind::DictLit(Vec::new()),
                            ty,
                            span: self.span(spread.span.start, spread.span.end),
                        });
                    }
                    let final_source_ty = Self::expr_ty(body, source);
                    if record_ty.is_none()
                        && matches!(
                            self.ctx.krate.types.get(final_source_ty),
                            Some(Type::Dict(_, _))
                        )
                    {
                        record_ty = Some(final_source_ty);
                    } else if self.object_spread_source_needs_unknown_record(final_source_ty) {
                        erased_spread_requires_unknown_record = true;
                    }
                    sources.push(source);
                }
            }
        }
        self.flush_object_spread_entries(
            &mut pending_entries,
            &mut sources,
            &mut record_ty,
            &mut erased_spread_requires_unknown_record,
            body,
            object.span,
        );

        let key_ty = self.ctx.krate.types.intern(Type::String);
        let fallback_value_ty = self.ctx.krate.types.intern(Type::Unknown);
        let record_ty = record_ty.unwrap_or_else(|| {
            self.ctx
                .krate
                .types
                .intern(Type::Dict(key_ty, fallback_value_ty))
        });
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
        let field_ty = self.class_field_type(hint, field).ok()?;
        if matches!(&property.value, Expression::ObjectExpression(_))
            && matches!(
                self.ctx.krate.types.get(field_ty),
                Some(Type::Class { .. } | Type::Optional(_))
            )
        {
            return None;
        }
        Some(field_ty)
    }

    /// Report whether a function body references the implicit `arguments`
    /// binding.
    ///
    /// Scans the body's source slice for the `arguments` identifier with
    /// surrounding identifier-boundary checks, mirroring the source-text probes
    /// already used for forward-callable detection. Used to decide whether a
    /// zero-parameter object-method function expression must be lowered as a
    /// real function value (which establishes the array-like `arguments`
    /// object) rather than collapsed into a getter return expression.
    fn function_body_references_arguments(
        &self,
        function_body: &oxc::ast::ast::FunctionBody<'_>,
    ) -> bool {
        let (Ok(start), Ok(end)) = (
            usize::try_from(function_body.span.start),
            usize::try_from(function_body.span.end),
        ) else {
            return false;
        };
        let Some(text) = self.source.get(start..end) else {
            return false;
        };
        Self::source_slice_mentions_identifier(text, "arguments")
    }

    /// Report whether `text` contains `identifier` as a standalone JavaScript
    /// identifier (not as a substring of a longer identifier such as a property
    /// name or a different variable).
    fn source_slice_mentions_identifier(text: &str, identifier: &str) -> bool {
        let bytes = text.as_bytes();
        let mut search_from = 0;
        while let Some(offset) = text.get(search_from..).and_then(|tail| tail.find(identifier)) {
            let match_start = search_from + offset;
            let match_end = match_start + identifier.len();
            let before_ok = match_start
                .checked_sub(1)
                .and_then(|index| bytes.get(index))
                .is_none_or(|byte| !Self::is_identifier_byte(*byte));
            let after_ok = bytes
                .get(match_end)
                .is_none_or(|byte| !Self::is_identifier_byte(*byte));
            if before_ok && after_ok {
                return true;
            }
            search_from = match_start + 1;
        }
        false
    }

    /// Report whether `byte` can appear inside a JavaScript identifier.
    fn is_identifier_byte(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
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
            // A zero-parameter `function` value that references its own
            // `arguments` binding is a real function, not a collapsible getter:
            // collapsing it to the bare return expression would lower
            // `arguments` against the enclosing scope (where it is unavailable).
            // Lower it as a genuine function-expression value instead, which
            // establishes the array-like `arguments` object for the body.
            if self.function_body_references_arguments(function_body) {
                return self.function_expression_value(function, type_hint, property.span, body);
            }
            let [Statement::ReturnStatement(statement)] = function_body.statements.as_slice()
            else {
                let ty = type_hint.unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(function.span.start, function.span.end),
                }));
            };
            let Some(argument) = &statement.argument else {
                return Err(SmeltError::unsupported(
                    self.span(statement.span.start, statement.span.end),
                    "object getter functions must return a value",
                ));
            };
            return self.expression_with_hint(argument, body, type_hint);
        }
        if matches!(&property.value, Expression::Identifier(identifier) if identifier.name == "undefined")
            && type_hint.is_none()
        {
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Undefined),
                ty,
                span: self.span(property.value.span().start, property.value.span().end),
            }));
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
            self.ctx.krate.types.intern(Type::Unknown)
        };

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        let saved_generator_yields = self.current_generator_yields;
        let saved_narrowed_locals = std::mem::take(&mut self.narrowed_locals);
        self.current_async = function.r#async;
        self.current_return_ty = Some(return_ty);
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut params = Vec::new();
        let mut param_names = HashSet::new();
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
                    self.ctx.krate.types.intern(Type::Unknown)
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
                param_names.insert(binding.name.to_string());
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
        // Lower an optional `...rest` parameter the same way top-level functions
        // and arrow expressions do: resolve its array element type, push a packed
        // list local/param, and record the rest index on the closure so codegen
        // collects the trailing source arguments into one list. Function
        // expressions appear as object property values, returned values, and call
        // arguments, so this keeps rest semantics for all of them.
        let mut rest = None;
        if let Some(rest_param) = &function.params.rest {
            let result = (|| {
                let BindingPattern::BindingIdentifier(binding) = &rest_param.rest.argument else {
                    return Err(SmeltError::unsupported(
                        self.span(rest_param.span.start, rest_param.span.end),
                        "function expression destructured rest parameters need rest binding lowering",
                    ));
                };
                let annotated_ty = rest_param
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?;
                let rest_index = params.len();
                let hint_rest_ty = hint_function.as_ref().and_then(|(_, function_ty)| {
                    function_ty
                        .rest
                        .filter(|index| *index == rest_index)
                        .and_then(|index| function_ty.params.get(index).copied())
                });
                let source_ty = annotated_ty
                    .or(hint_rest_ty)
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                let Ok((ty, item_ty)) = self.rest_param_array_type(source_ty) else {
                    return Err(SmeltError::unsupported(
                        self.span(rest_param.span.start, rest_param.span.end),
                        "function expression rest parameter type must be an array type",
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
                param_names.insert(binding.name.to_string());
                params.push(Param {
                    name: param_name,
                    local,
                    ty,
                    span: self.span(binding.span.start, binding.span.end),
                });
                rest = Some(RestParam {
                    index: rest_index,
                    item_ty,
                });
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(error);
            }
        }
        let required_params = function
            .params
            .items
            .iter()
            .position(|param| param.optional || Self::formal_parameter_has_default(param))
            .unwrap_or(function.params.items.len());
        let mut captures = Vec::new();
        if errors.is_empty() {
            let mut capture_names = Vec::new();
            let function_locals = self.locals.clone();
            self.locals = saved_locals.clone();
            for statement in &function_body.statements {
                self.collect_statement_capture_names(statement, &param_names, &mut capture_names);
            }
            self.locals = function_locals;
            capture_names.sort();
            capture_names.dedup();
            for name in capture_names {
                let Some(source_local) = saved_locals.get(name.as_str()).copied() else {
                    continue;
                };
                let Some(source_decl) = usize::try_from(source_local.0)
                    .ok()
                    .and_then(|index| outer_body.locals.get(index))
                else {
                    continue;
                };
                let symbol = source_decl
                    .name
                    .unwrap_or_else(|| self.ctx.krate.symbols.intern(name.as_str()));
                let body_local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty: source_decl.ty,
                    mutable: source_decl.mutable,
                    span: source_decl.span,
                });
                self.locals.insert(name, body_local);
                captures.push(ClosureCapture {
                    source_local,
                    body_local: Some(body_local),
                    symbol,
                    ty: source_decl.ty,
                    mode: CaptureMode::ByRef,
                });
            }
        }
        let generator_yields =
            function
                .generator
                .then(|| self.initialize_generator_yield_accumulator(function, &mut body));
        self.current_generator_yields = generator_yields;
        // A non-arrow `function` expression introduces its own `arguments`
        // binding, so make the array-like `arguments` object available while
        // lowering the body — mirroring the function-declaration and closure
        // lowering paths that also push the argument arity stack.
        self.current_arguments_arities
            .push(function.params.items.len());
        for statement in &function_body.statements {
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }
        if let Some(accumulator) = generator_yields {
            self.push_generator_return(accumulator, function, &mut body);
        }
        if function.r#async {
            body.build_async_state_machine();
        }
        self.current_arguments_arities.pop();
        self.locals = saved_locals;
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        self.current_generator_yields = saved_generator_yields;
        self.narrowed_locals = saved_narrowed_locals;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let body_id = self.ctx.krate.push_body(body);
        let rest_index = rest.as_ref().map(|rest| rest.index);
        let function_ty = hint_function.map_or_else(
            || {
                self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params: params.iter().map(|param| param.ty).collect(),
                    rest: rest_index,
                    required_params: Some(required_params),
                    mutable_params: Vec::new(),
                    return_ty,
                    is_async: function.r#async,
                    may_throw: false,
                }))
            },
            |(ty, _)| ty,
        );
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params,
                rest: rest_index,
                required_params: Some(required_params),
                return_ty,
                captures,
                body: body_id,
                function_item: None,
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
            return self.property_key_index_expression(&object_property.key, body);
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

    /// Return true for computed symbol keys that getter/method enumeration ignores.
    fn is_computed_symbol_key(object_property: &oxc::ast::ast::ObjectProperty<'_>) -> bool {
        if !object_property.computed {
            return false;
        }
        Self::is_direct_computed_symbol_call_key(object_property) || matches!(
            &object_property.key,
            PropertyKey::Identifier(identifier) if identifier.name.contains("SYMBOL")
        )
    }

    /// Return true when a computed key is a direct `Symbol(...)` expression.
    fn is_direct_computed_symbol_call_key(
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
    ) -> bool {
        object_property.computed
            && matches!(
                &object_property.key,
                PropertyKey::CallExpression(call)
                    if matches!(&call.callee, Expression::Identifier(callee) if callee.name == "Symbol")
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
        erased_spread_requires_unknown_record: &mut bool,
        body: &mut Body,
        span: oxc::span::Span,
    ) {
        if pending_entries.is_empty() {
            return;
        }
        let entries = std::mem::take(pending_entries);
        let force_unknown_record = record_ty.is_none()
            && *erased_spread_requires_unknown_record
            && !self.object_spread_entries_are_callable(&entries, body);
        let chunk_ty = if force_unknown_record {
            let key_ty = self.ctx.krate.types.intern(Type::String);
            let value_ty = self.ctx.krate.types.intern(Type::Unknown);
            self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
        } else {
            self.object_literal_type(&entries, *record_ty, body)
        };
        if record_ty.is_none() {
            *record_ty = Some(chunk_ty);
        }
        *erased_spread_requires_unknown_record = false;
        sources.push(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: record_ty.unwrap_or(chunk_ty),
            span: self.span(span.start, span.end),
        }));
    }

    /// Return whether every explicit property in a spread chunk is callable.
    fn object_spread_entries_are_callable(
        &self,
        entries: &[(smelt_hir::ExprId, smelt_hir::ExprId)],
        body: &Body,
    ) -> bool {
        !entries.is_empty()
            && entries.iter().all(|(_, value)| {
                matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, *value)),
                    Some(Type::Function(_))
                )
            })
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
                let Some(Type::Dict(record_key, record_value)) =
                    self.ctx.krate.types.get(record_ty).cloned()
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

    /// Return whether JavaScript object spread treats a source as an empty object.
    fn object_spread_source_erases_to_empty(&self, source_ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(source_ty),
            Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None)
        )
    }

    /// Return whether a spread source must keep later literal chunks erased.
    ///
    /// An unknown, generic, class, or optional object spread can carry
    /// heterogeneous property values. Without a contextual record type, later
    /// explicit properties must not force those copied fields into their own
    /// value type.
    fn object_spread_source_needs_unknown_record(&self, source_ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(source_ty) {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => true,
            Some(Type::Optional(inner)) => self.object_spread_source_needs_unknown_record(*inner),
            _ => false,
        }
    }

    /// Extract a dictionary type from a contextual object-literal type hint.
    fn dict_type_from_hint(
        &self,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
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

    /// Infer the storage type used for a lowered object literal.
    ///
    /// A fully compatible contextual record keeps nested typed fields, such as
    /// a locale option bag, from first erasing through `Record<string, unknown>`.
    /// Incomplete or incompatible contextual records remain dictionaries so
    /// ordinary structural adaptation can still occur at their use site.
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
        if let Some(ty) =
            type_hint.and_then(|ty| self.contextual_record_literal_type(ty, entries, body))
        {
            return ty;
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let key_ty = if entries
            .iter()
            .all(|(key, _)| Self::expr_ty(body, *key) == string_ty)
        {
            string_ty
        } else {
            self.ctx.krate.types.intern(Type::Unknown)
        };
        let first_value_ty = entries
            .first()
            .map(|(_, value)| Self::expr_ty(body, *value));
        let value_ty = first_value_ty
            .filter(|first_ty| {
                entries
                    .iter()
                    .all(|(_, value)| Self::expr_ty(body, *value) == *first_ty)
            })
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
        self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
    }

    /// Preserve a contextual interface type only when the literal can be
    /// constructed directly without inventing values for required fields.
    fn contextual_record_literal_type(
        &mut self,
        type_hint: smelt_hir::TypeId,
        entries: &[(smelt_hir::ExprId, smelt_hir::ExprId)],
        body: &Body,
    ) -> Option<smelt_hir::TypeId> {
        let candidate = match self.ctx.krate.types.get(type_hint) {
            Some(Type::Class { .. }) => type_hint,
            Some(Type::Optional(inner))
                if matches!(self.ctx.krate.types.get(*inner), Some(Type::Class { .. })) =>
            {
                *inner
            }
            _ => return None,
        };
        let fields = self.contextual_record_literal_fields(candidate)?;
        if fields.is_empty() || fields.iter().any(|field| !field.optional) {
            return None;
        }
        let mut needs_structural_adapter = false;
        for (key, value) in entries {
            let key_expr = body
                .exprs
                .get(usize::try_from(key.0).unwrap_or(usize::MAX))?;
            let ExprKind::Literal(Literal::String(field_key)) = &key_expr.kind else {
                return None;
            };
            let field = self.intern_source_name(field_key);
            let expected = self.class_field_type(candidate, field).ok()?;
            let actual = Self::expr_ty(body, *value);
            if !self.contextual_record_field_assignable(actual, expected) {
                return None;
            }
            needs_structural_adapter |=
                !self.contextual_record_field_directly_assignable(actual, expected);
        }
        needs_structural_adapter.then_some(candidate)
    }

    /// Return whether a contextual field can be assigned without record adaptation.
    fn contextual_record_field_directly_assignable(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        if self.type_assignable_to(actual, expected) {
            return true;
        }
        matches!(self.ctx.krate.types.get(expected), Some(Type::Optional(inner)) if self.contextual_record_field_directly_assignable(actual, *inner))
    }

    /// Return whether direct record emission can initialize one contextual field.
    ///
    /// Typed interface values may require the backend's established structural
    /// record adapter even when their nominal HIR names differ.
    fn contextual_record_field_assignable(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        if self.contextual_record_field_directly_assignable(actual, expected) {
            return true;
        }
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(expected) {
            return self.contextual_record_field_assignable(actual, *inner);
        }
        self.contextual_record_literal_fields(actual).is_some()
            && self.contextual_record_literal_fields(expected).is_some()
    }

    /// Collect fields that direct record-literal emission must initialize.
    ///
    /// Plain classes are deliberately excluded: constructor semantics are not
    /// equivalent to constructing a TypeScript options/interface literal.
    fn contextual_record_literal_fields(
        &self,
        candidate: smelt_hir::TypeId,
    ) -> Option<Vec<Field>> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(candidate) else {
            return None;
        };
        if let Some(interface) = self.find_interface(*name) {
            return self.contextual_interface_fields(interface.name, &mut HashSet::new());
        }
        self.type_alias_fields.get(name).cloned()
    }

    /// Collect inherited interface fields while rejecting recursive surfaces.
    fn contextual_interface_fields(
        &self,
        name: smelt_hir::Symbol,
        visited: &mut HashSet<smelt_hir::Symbol>,
    ) -> Option<Vec<Field>> {
        if !visited.insert(name) {
            return None;
        }
        let interface = self.find_interface(name)?;
        let mut fields = interface.fields.clone();
        for parent in &interface.extends {
            for field in self.contextual_interface_fields(parent.parent, visited)? {
                if !fields.iter().any(|existing| existing.name == field.name) {
                    fields.push(field);
                }
            }
        }
        Some(fields)
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
            return Ok(Some(self.object_const_expression(
                &value,
                member.span.start,
                member.span.end,
                body,
            )));
        }
        let item = self
            .object_namespaces
            .get(namespace)
            .and_then(|members| members.get(member_name))
            .copied()
            .or_else(|| self.items.get(member_name).copied());
        let Some(item) = item else {
            if self.namespace_imports.contains(namespace) {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span,
                })));
            }
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
