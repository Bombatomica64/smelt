impl ModuleBuilder<'_> {
    fn ts_type_to_hir(&mut self, ty: &TSType<'_>) -> Result<smelt_hir::TypeId, SmeltError> {
        match ty {
            TSType::TSNumberKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Float)),
            TSType::TSStringKeyword(_) => Ok(self.ctx.krate.types.intern(Type::String)),
            TSType::TSBooleanKeyword(_) | TSType::TSTypePredicate(_) => {
                Ok(self.ctx.krate.types.intern(Type::Bool))
            }
            TSType::TSUnknownKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Unknown)),
            TSType::TSVoidKeyword(_) | TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => {
                Ok(self.ctx.krate.types.intern(Type::None))
            }
            TSType::TSLiteralType(literal) => match &literal.literal {
                oxc::ast::ast::TSLiteral::StringLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::String))
                }
                oxc::ast::ast::TSLiteral::NumericLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Float))
                }
                oxc::ast::ast::TSLiteral::BooleanLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Bool))
                }
                _ => Err(SmeltError::unsupported(
                    self.span(ty.span().start, ty.span().end),
                    format!("literal type annotation is not lowered yet: {ty:?}"),
                )),
            },
            TSType::TSUnionType(union) => {
                let mut lowered = Vec::new();
                let mut nullish = Vec::new();
                for member in &union.types {
                    let member_ty = self.ts_type_to_hir(member)?;
                    if matches!(self.ctx.krate.types.get(member_ty), Some(Type::None)) {
                        nullish.push(member_ty);
                    } else if !lowered.contains(&member_ty) {
                        lowered.push(member_ty);
                    }
                }
                if lowered.len() == 1 && !nullish.is_empty() {
                    let single = lowered.remove(0);
                    Ok(self.ctx.krate.types.intern(Type::Optional(single)))
                } else if lowered.len() == 1 {
                    let single = lowered.remove(0);
                    Ok(single)
                } else {
                    lowered.extend(nullish);
                    Ok(self.ctx.krate.types.intern(Type::Union(lowered)))
                }
            }
            TSType::TSIntersectionType(intersection) => {
                let mut meaningful = Vec::new();
                for member in &intersection.types {
                    if matches!(member, TSType::TSTypeLiteral(lit) if lit.members.is_empty()) {
                        continue;
                    }
                    meaningful.push(self.ts_type_to_hir(member)?);
                }
                match meaningful.as_slice() {
                    [] => Ok(self.ctx.krate.types.intern(Type::None)),
                    [single] => Ok(*single),
                    _ if meaningful.iter().all(|ty| {
                        matches!(
                            self.ctx.krate.types.get(*ty),
                            Some(Type::Class { .. } | Type::Dict(_, _))
                        )
                    }) =>
                    {
                        let key_ty = self.ctx.krate.types.intern(Type::String);
                        let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                        Ok(self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty)))
                    }
                    _ => Ok(self.ctx.krate.types.intern(Type::Union(meaningful))),
                }
            }
            TSType::TSConditionalType(conditional) => {
                let true_ty = self.ts_type_to_hir(&conditional.true_type)?;
                let false_ty = self.ts_type_to_hir(&conditional.false_type)?;
                if true_ty == false_ty {
                    Ok(true_ty)
                } else {
                    Ok(self.ctx.krate.types.intern(Type::Union(vec![true_ty, false_ty])))
                }
            }
            TSType::TSArrayType(array) => {
                let element_ty = self.ts_type_to_hir(&array.element_type)?;
                Ok(self.ctx.krate.types.intern(Type::List(element_ty)))
            }
            TSType::TSTupleType(tuple) => {
                let mut items = Vec::new();
                for item in &tuple.element_types {
                    items.push(self.tuple_element_type_to_hir(item)?);
                }
                Ok(self.ctx.krate.types.intern(Type::Tuple(items)))
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Readonly =>
            {
                self.ts_type_to_hir(&operator.type_annotation)
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Keyof =>
            {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            TSType::TSTypeReference(reference) => self.type_reference_to_hir(reference),
            TSType::TSFunctionType(function) => {
                if function.type_parameters.is_some() || function.this_param.is_some() {
                    return Err(SmeltError::unsupported(
                        self.span(function.span.start, function.span.end),
                        "generic and this-parameter function types are not lowered yet",
                    ));
                }
                let mut params = Vec::new();
                for param in &function.params.items {
                    if param.optional {
                        return Err(SmeltError::unsupported(
                            self.span(param.span.start, param.span.end),
                            "optional function type parameters are not lowered yet",
                        ));
                    }
                    let param_ty = param
                        .type_annotation
                        .as_ref()
                        .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                        .transpose()?
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(param.span.start, param.span.end),
                                "function type parameters require explicit type annotations",
                            )
                        })?;
                    params.push(param_ty);
                }
                if function.params.rest.is_some() {
                    return Err(SmeltError::unsupported(
                        self.span(function.params.span.start, function.params.span.end),
                        "rest function type parameters are not lowered yet",
                    ));
                }
                let return_ty = self.ts_type_to_hir(&function.return_type.type_annotation)?;
                Ok(self.ctx.krate.types.intern(Type::Function(smelt_hir::FunctionType {
                    params,
                    return_ty,
                    is_async: false,
                })))
            }
            TSType::TSThisType(this_ty) => {
                let Some(class_name) = &self.current_class else {
                    return Err(SmeltError::unsupported(
                        self.span(this_ty.span.start, this_ty.span.end),
                        "this types outside classes are not lowered yet",
                    ));
                };
                let Some(class_item) = self.classes.get(class_name).copied() else {
                    return Err(SmeltError::unsupported(
                        self.span(this_ty.span.start, this_ty.span.end),
                        "this class type is not resolvable yet",
                    ));
                };
                let Item::Class(class) = self.item_ref(class_item) else {
                    return Err(SmeltError::unsupported(
                        self.span(this_ty.span.start, this_ty.span.end),
                        "this class type is not resolvable yet",
                    ));
                };
                Ok(self.ctx.krate.types.intern(Type::Class {
                    name: class.name,
                    args: Vec::new(),
                }))
            }
            _ => Err(SmeltError::unsupported(
                self.span(ty.span().start, ty.span().end),
                format!("type annotation is not lowered yet: {ty:?}"),
            )),
        }
    }

    /// Extract `asserts value is T` metadata from a TypeScript return annotation.
    fn assertion_return_type(
        &mut self,
        ty: &TSType<'_>,
    ) -> Option<Result<(String, smelt_hir::TypeId), SmeltError>> {
        let TSType::TSTypePredicate(predicate) = ty else {
            return None;
        };
        if !predicate.asserts {
            return None;
        }
        let oxc::ast::ast::TSTypePredicateName::Identifier(parameter) = &predicate.parameter_name
        else {
            return Some(Err(SmeltError::unsupported(
                self.span(predicate.span.start, predicate.span.end),
                "assertion functions on `this` are not lowered yet",
            )));
        };
        let Some(annotation) = &predicate.type_annotation else {
            return Some(Err(SmeltError::unsupported(
                self.span(predicate.span.start, predicate.span.end),
                "assertion functions must use `asserts value is T`",
            )));
        };
        Some(
            self.ts_type_to_hir(&annotation.type_annotation)
                .map(|target| (parameter.name.to_string(), target)),
        )
    }

    /// Convert tuple element type to HIR type.
    fn tuple_element_type_to_hir(
        &mut self,
        item: &TSTupleElement<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match item {
            TSTupleElement::TSNumberKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Float)),
            TSTupleElement::TSStringKeyword(_) => Ok(self.ctx.krate.types.intern(Type::String)),
            TSTupleElement::TSBooleanKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Bool)),
            TSTupleElement::TSUnknownKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Unknown)),
            TSTupleElement::TSNullKeyword(_)
            | TSTupleElement::TSUndefinedKeyword(_)
            | TSTupleElement::TSVoidKeyword(_) => Ok(self.ctx.krate.types.intern(Type::None)),
            TSTupleElement::TSArrayType(array) => {
                let element_ty = self.ts_type_to_hir(&array.element_type)?;
                Ok(self.ctx.krate.types.intern(Type::List(element_ty)))
            }
            TSTupleElement::TSTupleType(tuple) => {
                let mut items = Vec::new();
                for tuple_item in &tuple.element_types {
                    items.push(self.tuple_element_type_to_hir(tuple_item)?);
                }
                Ok(self.ctx.krate.types.intern(Type::Tuple(items)))
            }
            TSTupleElement::TSTypeReference(reference) => self.type_reference_to_hir(reference),
            TSTupleElement::TSOptionalType(optional) => {
                let inner = self.ts_type_to_hir(&optional.type_annotation)?;
                Ok(self.ctx.krate.types.intern(Type::Optional(inner)))
            }
            TSTupleElement::TSRestType(rest) => Err(SmeltError::unsupported(
                self.span(rest.span.start, rest.span.end),
                "tuple rest types are not lowered yet",
            )),
            TSTupleElement::TSNamedTupleMember(named) => {
                self.tuple_element_type_to_hir(&named.element_type)
            }
            _ => Err(SmeltError::unsupported(
                self.span(item.span().start, item.span().end),
                format!("tuple element type is not lowered yet: {item:?}"),
            )),
        }
    }

    /// Convert type reference to HIR type.
    fn type_reference_to_hir(
        &mut self,
        reference: &oxc::ast::ast::TSTypeReference<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let TSTypeName::IdentifierReference(name) = &reference.type_name else {
            return Err(SmeltError::unsupported(
                self.span(reference.span.start, reference.span.end),
                "qualified type references are not lowered yet",
            ));
        };
        let name_text = name.name.as_str();
        if let Some(param_ty) = self.type_parameter_type(name_text) {
            if reference.type_arguments.is_some() {
                return Err(SmeltError::unsupported(
                    self.span(reference.span.start, reference.span.end),
                    "type parameters cannot be used with type arguments",
                ));
            }
            return Ok(param_ty);
        }
        let args = reference
            .type_arguments
            .as_ref()
            .map(|args| args.params.iter().collect::<Vec<_>>())
            .unwrap_or_default();
        match (name_text, args.as_slice()) {
            ("Array", [item]) => {
                let lowered_item = self.ts_type_to_hir(item)?;
                Ok(self.ctx.krate.types.intern(Type::List(lowered_item)))
            }
            ("Set", [item]) => {
                let lowered_item = self.ts_type_to_hir(item)?;
                Ok(self.ctx.krate.types.intern(Type::Set(lowered_item)))
            }
            ("Record", [key, value]) => {
                let lowered_key = self.ts_type_to_hir(key)?;
                if self.ctx.krate.types.get(lowered_key) != Some(&Type::String) {
                    return Err(SmeltError::unsupported(
                        self.span(reference.span.start, reference.span.end),
                        "only Record<string, T> is lowered for now",
                    ));
                }
                let lowered_value = self.ts_type_to_hir(value)?;
                Ok(self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Dict(lowered_key, lowered_value)))
            }
            ("Map", [key, value]) => {
                let lowered_key = self.ts_type_to_hir(key)?;
                let lowered_value = self.ts_type_to_hir(value)?;
                Ok(self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Dict(lowered_key, lowered_value)))
            }
            ("Promise", [item]) => {
                let lowered_item = self.ts_type_to_hir(item)?;
                Ok(self.ctx.krate.types.intern(Type::Future(lowered_item)))
            }
            _ => {
                let symbol = self.intern_type_name(name_text);
                if let Some(interface) = self.find_interface(symbol).cloned() {
                    let lowered_args = args
                        .iter()
                        .map(|arg| self.ts_type_to_hir(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    let substitutions = self.type_argument_substitution(
                        &interface.type_params,
                        &lowered_args,
                        self.span(reference.span.start, reference.span.end),
                    )?;
                    let instantiated_args = interface
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
                        .collect();
                    return Ok(self.ctx.krate.types.intern(Type::Class {
                        name: symbol,
                        args: instantiated_args,
                    }));
                }
                if let Some(alias) = self.find_type_alias(symbol).cloned() {
                    let lowered_args = args
                        .iter()
                        .map(|arg| self.ts_type_to_hir(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    let substitutions = self.type_argument_substitution(
                        &alias.type_params,
                        &lowered_args,
                        self.span(reference.span.start, reference.span.end),
                    )?;
                    return Ok(self.substitute_type_params(alias.ty, &substitutions));
                }
                let lowered_args = args
                    .iter()
                    .map(|arg| self.ts_type_to_hir(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.ctx.krate.types.intern(Type::Class {
                    name: symbol,
                    args: lowered_args,
                }))
            }
        }
    }

    /// Resolve the type of a class field.
    fn class_field_type(
        &self,
        receiver_ty: smelt_hir::TypeId,
        field: smelt_hir::Symbol,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::Dict(_, value)) => Ok(*value),
            Some(Type::Class { name, .. }) => self
                .class_by_symbol(*name)
                .and_then(|class| {
                    class
                        .fields
                        .iter()
                        .find(|item| item.name == field)
                        .map(|item| item.ty)
                })
                .or_else(|| {
                    let class_name = self
                        .ctx
                        .krate
                        .names
                        .get(*name)
                        .or_else(|| self.ctx.krate.symbols.get(*name))?;
                    self.class_fields.get(class_name).and_then(|fields| {
                        fields
                            .iter()
                            .find(|item| item.name == field)
                            .map(|item| item.ty)
                    })
                })
                .ok_or_else(|| {
                    let field_name = self.ctx.krate.symbols.get(field).unwrap_or("<unknown>");
                    SmeltError::unsupported(
                        self.span(0, 0),
                        format!("unknown class field `{field_name}`"),
                    )
                }),
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                "field access is only lowered for Record<string, T> and class values for now",
            )),
        }
    }

    /// Look up a class by its symbol.
    fn class_by_symbol(&self, name: smelt_hir::Symbol) -> Option<&Class> {
        self.ctx.krate.items.iter().find_map(|item| {
            if let Item::Class(class) = item {
                if class.name == name {
                    return Some(class);
                }
            }
            None
        })
    }

    /// Resolve a method call on a type.
    fn resolve_method(
        &self,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
    ) -> Result<(smelt_hir::TypeId, smelt_hir::ItemId), SmeltError> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(receiver_ty) else {
            return Err(SmeltError::unsupported(
                self.span(0, 0),
                "method calls are only lowered for class values for now",
            ));
        };
        let Some(class) = self.class_by_symbol(*name) else {
            return Err(SmeltError::unsupported(
                self.span(0, 0),
                "method receiver class is unknown",
            ));
        };
        for item in &class.methods {
            if let Item::Function(function) = self.item_ref(*item)
                && function.name == method
            {
                return Ok((function.return_ty, *item));
            }
        }
        let method_name = self.ctx.krate.symbols.get(method).unwrap_or("<unknown>");
        Err(SmeltError::unsupported(
            self.span(0, 0),
            format!("unknown class method `{method_name}`"),
        ))
    }

    /// Get the element type of an indexable type.
    fn index_type(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(item)) => Ok(*item),
            Some(Type::String) => Ok(self.ctx.krate.types.intern(Type::String)),
            Some(Type::Dict(_, value)) => Ok(*value),
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                "index access is only lowered for arrays, strings, and records for now",
            )),
        }
    }

    /// Resolve the static numeric index required for TypeScript tuple indexing.
    fn static_tuple_index(
        &self,
        index_expr_id: smelt_hir::ExprId,
        body: &Body,
        len: usize,
        span: oxc::span::Span,
    ) -> Result<usize, SmeltError> {
        let Some(index_expr) = usize::try_from(index_expr_id.0)
            .ok()
            .and_then(|index| body.exprs.get(index))
        else {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "tuple index expression is invalid",
            ));
        };
        let ExprKind::Literal(Literal::Float(value)) = &index_expr.kind else {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "tuple indexing requires a static non-negative integer index",
            ));
        };
        let index_value = *value;
        if index_value < 0.0_f64 {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "negative tuple bracket indexes are JavaScript property lookups; use .at(...) when supported for negative element indexing",
            ));
        }
        if index_value.fract() != 0.0_f64 {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "tuple indexing requires an integer index",
            ));
        }
        let resolved_index = index_value.to_string().parse::<usize>().map_err(|_err| {
            SmeltError::unsupported(
                self.span(span.start, span.end),
                "tuple indexing requires a representable usize index",
            )
        })?;
        if resolved_index >= len {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "tuple index is out of bounds",
            ));
        }
        Ok(resolved_index)
    }

    /// Reject negative TypeScript bracket indexes before they reach Python-style HIR indexing.
    fn reject_negative_bracket_index(
        &self,
        receiver_ty: smelt_hir::TypeId,
        index: smelt_hir::ExprId,
        body: &Body,
        span: oxc::span::Span,
    ) -> Result<(), SmeltError> {
        let uses_sequence_indexing = matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::List(_) | Type::String | Type::Tuple(_))
        );
        if uses_sequence_indexing && Self::is_negative_numeric_expr(body, index) {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "negative array/string bracket indexes are JavaScript property lookups; use .at(...) for negative element indexing",
            ));
        }
        Ok(())
    }

    /// Returns whether a lowered expression is a negative numeric literal.
    fn is_negative_numeric_expr(body: &Body, expr_id: smelt_hir::ExprId) -> bool {
        let Ok(expr_index) = usize::try_from(expr_id.0) else {
            return false;
        };
        let Some(candidate) = body.exprs.get(expr_index) else {
            return false;
        };
        match &candidate.kind {
            ExprKind::Literal(Literal::Float(value)) => *value < 0.0,
            ExprKind::UnaryOp {
                op: UnaryOp::Neg,
                operand,
            } => usize::try_from(operand.0)
                .ok()
                .and_then(|operand_index| body.exprs.get(operand_index))
                .is_some_and(|operand_expr| {
                    matches!(operand_expr.kind, ExprKind::Literal(Literal::Float(_)))
                }),
            _ => false,
        }
    }

    /// Returns true when TypeScript `.length` can lower directly to Rust `.len()`.
    fn supports_stdlib_length(&self, receiver_ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::List(_) | Type::String | Type::Tuple(_))
        )
    }

    /// Returns true when TypeScript `.size` can lower directly to Rust `.len()`.
    fn supports_stdlib_size(&self, receiver_ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Dict(_, _) | Type::Set(_))
        )
    }

    /// Return the inner item type for a `Promise<T>` / `Future<T>` value.
    fn future_inner_type(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Future(inner)) => Some(*inner),
            _ => None,
        }
    }

    /// Intern a source identifier name and convert from `camelCase` to `snake_case`.
    fn intern_source_name(&mut self, name: &str) -> smelt_hir::Symbol {
        let symbol = self.ctx.krate.symbols.intern(&camel_to_snake(name));
        self.ctx.krate.names.record(symbol, name);
        symbol
    }

    /// Intern a type name symbol.
    fn intern_type_name(&mut self, name: &str) -> smelt_hir::Symbol {
        let symbol = self.ctx.krate.symbols.intern(name);
        self.ctx.krate.names.record(symbol, name);
        symbol
    }

    /// Create an identifier expression from a local variable.
    fn identifier_expression(
        &mut self,
        name: &str,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if name == "Infinity" {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(f64::INFINITY)),
                ty,
                span: self.span(start, end),
            }));
        }
        if name == "NaN" {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(f64::NAN)),
                ty,
                span: self.span(start, end),
            }));
        }
        if name == "undefined" {
            let ty = self.ctx.krate.types.intern(Type::None);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(start, end),
            }));
        }
        if let Some(callback) = self.local_callbacks.get(name).cloned() {
            return Ok(self.callback_expr_to_closure_with_return_ty(
                callback.return_ty,
                callback.callback,
                &callback.params,
                self.span(start, end),
                body,
            ));
        }
        let Some(local) = self.locals.get(name).copied() else {
            if let Some(value) = self.const_literals.get(name) {
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(value.literal.clone()),
                    ty: value.ty,
                    span: self.span(start, end),
                }));
            }
            if let Some(ty) = self.module_globals.get(name).copied() {
                return self.module_global_expression(ty, start, end, body);
            }
            return Err(SmeltError::unsupported(
                self.span(start, end),
                format!("unresolved identifier `{name}`"),
            ));
        };
        let base_ty = Self::local_ty(body, local);
        let ty = self.narrowed_type(name).unwrap_or(base_ty);
        let local_expr = body.push_expr(Expr {
            kind: ExprKind::Local(local),
            ty,
            span: self.span(start, end),
        });
        if self.ctx.krate.types.get(base_ty) == Some(&Type::Unknown)
            && ty != base_ty
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: local_expr,
                    target: ty,
                },
                ty,
                span: self.span(start, end),
            }));
        }
        Ok(local_expr)
    }

    /// Synthesize a read value for a known module-level variable.
    fn module_global_expression(
        &mut self,
        ty: smelt_hir::TypeId,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Class { .. } | Type::Unknown | Type::TypeParam { .. })
        ) {
            let key_ty = self.ctx.krate.types.intern(Type::String);
            let value_ty = self.ctx.krate.types.intern(Type::Unknown);
            let dict_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
            let value = body.push_expr(Expr {
                kind: ExprKind::DictLit(Vec::new()),
                ty: dict_ty,
                span: self.span(start, end),
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast { value, target: ty },
                ty,
                span: self.span(start, end),
            }));
        }
        let kind = match self.ctx.krate.types.get(ty) {
            Some(Type::Dict(_, _)) => ExprKind::DictLit(Vec::new()),
            Some(Type::None | Type::Optional(_)) => ExprKind::Literal(Literal::None),
            Some(Type::Bool) => ExprKind::Literal(Literal::Bool(false)),
            Some(Type::Int) => ExprKind::Literal(Literal::Int(0)),
            Some(Type::Float) => ExprKind::Literal(Literal::Float(0.0)),
            Some(Type::String) => ExprKind::Literal(Literal::String(String::new())),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(start, end),
                    "module-level variable type is not lowered for function reads yet",
                ));
            }
        };
        Ok(body.push_expr(Expr {
            kind,
            ty,
            span: self.span(start, end),
        }))
    }

    /// Ensure a console.log item exists in the HIR.
    fn ensure_console_log_item(&mut self, span: Span) -> smelt_hir::ItemId {
        let name = self.ctx.krate.symbols.intern(smelt_hir::CONSOLE_LOG_SYMBOL);
        let none = self.ctx.krate.types.intern(Type::None);
        self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            params: Vec::new(),
            return_ty: none,
            is_async: false,
            is_test: false,
            body: None,
            owner: FunctionOwner::Module,
        }))
    }

    /// Create a Span from byte offsets.
    fn span(&self, start: u32, end: u32) -> Span {
        Span::new(self.file_id, start, end)
    }

    /// Get the span of a statement.
    fn statement_span(&self, statement: &Statement<'_>) -> Span {
        let span = statement.span();
        self.span(span.start, span.end)
    }

    /// Get the span of an expression.
    fn expression_span(&self, expression: &Expression<'_>) -> Span {
        let span = expression.span();
        self.span(span.start, span.end)
    }

    // Continued in the next split builder file.
}
