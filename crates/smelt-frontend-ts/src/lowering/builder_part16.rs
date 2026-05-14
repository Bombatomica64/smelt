impl ModuleBuilder<'_> {
    /// Lower a TypeScript type annotation into HIR type metadata.
    ///
    /// `never` is preserved as a bottom marker instead of being converted to
    /// `None` or `Unknown`; callers that need executable values must reject it.
    fn ts_type_to_hir(&mut self, ty: &TSType<'_>) -> Result<smelt_hir::TypeId, SmeltError> {
        match ty {
            TSType::TSAnyKeyword(_) | TSType::TSUnknownKeyword(_) | TSType::TSObjectKeyword(_) => {
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
            TSType::TSTypeQuery(query) => Ok(self.type_query_to_hir(query)),
            TSType::TSNumberKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Float)),
            TSType::TSBigIntKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Int)),
            TSType::TSStringKeyword(_) | TSType::TSTemplateLiteralType(_) => {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            TSType::TSBooleanKeyword(_) | TSType::TSTypePredicate(_) => {
                Ok(self.ctx.krate.types.intern(Type::Bool))
            }
            TSType::TSSymbolKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Unknown)),
            TSType::TSNeverKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Never)),
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
                oxc::ast::ast::TSLiteral::UnaryExpression(unary)
                    if unary.operator == UnaryOperator::UnaryNegation
                        && matches!(
                            unary.argument,
                            Expression::NumericLiteral(_)
                        ) =>
                {
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
                let mut saw_never = false;
                for member in &union.types {
                    let member_ty = self.ts_type_to_hir(member)?;
                    if matches!(self.ctx.krate.types.get(member_ty), Some(Type::Never)) {
                        saw_never = true;
                    } else if matches!(self.ctx.krate.types.get(member_ty), Some(Type::None)) {
                        nullish.push(member_ty);
                    } else if !lowered.contains(&member_ty) {
                        lowered.push(member_ty);
                    }
                }
                if let Some(list_ty) = self.list_union_type(&lowered) {
                    if nullish.is_empty() {
                        return Ok(list_ty);
                    }
                    lowered = vec![list_ty];
                }
                if lowered.len() == 1 && !nullish.is_empty() {
                    let single = lowered.remove(0);
                    Ok(self.ctx.krate.types.intern(Type::Optional(single)))
                } else if lowered.len() == 1 {
                    let single = lowered.remove(0);
                    Ok(single)
                } else if lowered.is_empty() && nullish.is_empty() && saw_never {
                    Ok(self.ctx.krate.types.intern(Type::Never))
                } else {
                    lowered.extend(nullish);
                    Ok(self.ctx.krate.types.intern(Type::Union(lowered)))
                }
            }
            TSType::TSIntersectionType(intersection) => {
                let mut meaningful = Vec::new();
                let mut fields = Vec::new();
                for member in &intersection.types {
                    if matches!(member, TSType::TSTypeLiteral(lit) if lit.members.is_empty()) {
                        continue;
                    }
                    if let Ok(member_fields) = self.type_fields_from_ts(member) {
                        fields.extend(member_fields);
                    }
                    meaningful.push(self.ts_type_to_hir(member)?);
                }
                match meaningful.as_slice() {
                    [] => Ok(self.ctx.krate.types.intern(Type::None)),
                    [single] => Ok(*single),
                    _ if meaningful.iter().any(|member_ty| {
                        matches!(self.ctx.krate.types.get(*member_ty), Some(Type::Function(_)))
                    }) =>
                    {
                        let function_ty = *meaningful
                            .iter()
                            .find(|member_ty| {
                                matches!(
                                    self.ctx.krate.types.get(**member_ty),
                                    Some(Type::Function(_))
                                )
                            })
                            .expect("function member should exist");
                        if !fields.is_empty() {
                            self.callable_fields.insert(function_ty, fields.clone());
                            self.ctx.callable_fields.insert(function_ty, fields);
                        }
                        Ok(function_ty)
                    }
                    _ if meaningful.iter().all(|member_ty| {
                        matches!(
                            self.ctx.krate.types.get(*member_ty),
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
            TSType::TSParenthesizedType(parenthesized) => {
                self.ts_type_to_hir(&parenthesized.type_annotation)
            }
            TSType::TSTupleType(tuple) => {
                if let Some(rest_ty) = self.homogeneous_tuple_rest_type(tuple)? {
                    return Ok(rest_ty);
                }
                if let Some(rest_ty) = self.tuple_rest_list_type(tuple)? {
                    return Ok(rest_ty);
                }
                let mut items = Vec::new();
                for item in &tuple.element_types {
                    if self.tuple_rest_type_erases_to_empty(item)? {
                        continue;
                    }
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
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Unique =>
            {
                self.ts_type_to_hir(&operator.type_annotation)
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Keyof =>
            {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            TSType::TSNamedTupleMember(named) => {
                self.tuple_element_type_to_hir(&named.element_type)
            }
            TSType::TSTypeReference(reference) => self.type_reference_to_hir(reference),
            TSType::TSIndexedAccessType(indexed) => self.indexed_access_type_to_hir(indexed),
            TSType::TSTypeLiteral(literal) => self.type_literal_to_hir(literal),
            TSType::TSMappedType(mapped) => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = mapped
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(annotation))
                    .transpose()?
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                Ok(self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty)))
            }
            TSType::TSFunctionType(function) => self.function_type_to_hir(function),
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

    /// Lower `typeof name` type queries for already-known function values.
    fn type_query_to_hir(
        &mut self,
        query: &oxc::ast::ast::TSTypeQuery<'_>,
    ) -> smelt_hir::TypeId {
        if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name
            && let Some(item) = self.items.get(ident.name.as_str()).copied()
            && let Item::Function(function) = self.item_ref(item)
        {
            let params = function.params.iter().map(|param| param.ty).collect();
            return self.ctx.krate.types.intern(Type::Function(FunctionType {
                params,
                return_ty: function.return_ty,
                is_async: function.is_async,
            }));
        }
        self.ctx.krate.types.intern(Type::Unknown)
    }

    /// Convert a function-type rest parameter annotation into its list type.
    fn function_type_rest_param_to_hir(
        &mut self,
        ty: &TSType<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if matches!(ty, TSType::TSAnyKeyword(_) | TSType::TSUnknownKeyword(_)) {
            let item_ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok(self.ctx.krate.types.intern(Type::List(item_ty)));
        }
        self.ts_type_to_hir(ty)
    }

    /// Convert an inline TypeScript object type into Smelt's record-like dict type.
    fn type_literal_to_hir(
        &mut self,
        literal: &oxc::ast::ast::TSTypeLiteral<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let mut value_tys = Vec::new();
        for member in &literal.members {
            if let TSSignature::TSIndexSignature(index) = member {
                let value_ty = self.ts_type_to_hir(&index.type_annotation.type_annotation)?;
                if !value_tys.contains(&value_ty) {
                    value_tys.push(value_ty);
                }
            }
        }
        for field in self.type_literal_fields(literal)? {
            let value_ty = if field.optional {
                self.ctx.krate.types.intern(Type::Optional(field.ty))
            } else {
                field.ty
            };
            if !value_tys.contains(&value_ty) {
                value_tys.push(value_ty);
            }
        }
        let value_ty = match value_tys.as_slice() {
            [] => self.ctx.krate.types.intern(Type::Unknown),
            [single] => *single,
            _ => self.ctx.krate.types.intern(Type::Union(value_tys)),
        };
        Ok(self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty)))
    }

    /// Extract named fields from a TypeScript structural type.
    fn type_fields_from_ts(&mut self, ty: &TSType<'_>) -> Result<Vec<Field>, SmeltError> {
        match ty {
            TSType::TSTypeLiteral(literal) => self.type_literal_fields(literal),
            TSType::TSIntersectionType(intersection) => {
                let mut fields = Vec::new();
                for member in &intersection.types {
                    fields.extend(self.type_fields_from_ts(member)?);
                }
                Ok(fields)
            }
            TSType::TSUnionType(union) => self.union_type_fields(union),
            TSType::TSTypeReference(reference) => {
                let TSTypeName::IdentifierReference(name) = &reference.type_name else {
                    return Ok(Vec::new());
                };
                self.type_reference_fields(reference, name.name.as_str())
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Extract structural fields from a named type reference.
    ///
    /// This mirrors `type_reference_to_hir` for field metadata: aliases and
    /// interfaces carry structural fields independently from their erased HIR
    /// representation, so references need generic substitutions applied before
    /// callers use the fields for member access.
    fn type_reference_fields(
        &mut self,
        reference: &oxc::ast::ast::TSTypeReference<'_>,
        name_text: &str,
    ) -> Result<Vec<Field>, SmeltError> {
        let args = reference
            .type_arguments
            .as_ref()
            .map(|args| args.params.iter().collect::<Vec<_>>())
            .unwrap_or_default();
        if name_text == "Pick" {
            let [base, keys] = args.as_slice() else {
                return Ok(Vec::new());
            };
            let mut fields = self.type_fields_from_ts(base)?;
            if let Some(selected) = Self::static_pick_keys(keys) {
                fields.retain(|field| {
                    self.ctx
                        .krate
                        .symbols
                        .get(field.name)
                        .is_some_and(|name| selected.iter().any(|key| key == name))
                });
            }
            return Ok(fields);
        }

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
            return Ok(self.substituted_fields(&interface.fields, &substitutions));
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
            if let Some(fields) = self.type_alias_fields.get(&symbol).cloned() {
                return Ok(self.substituted_fields(&fields, &substitutions));
            }
        }
        Ok(self
            .type_alias_fields
            .get(&symbol)
            .cloned()
            .unwrap_or_default())
    }

    /// Return static string keys from the key side of `Pick<T, K>`.
    fn static_pick_keys(ty: &TSType<'_>) -> Option<Vec<String>> {
        match ty {
            TSType::TSLiteralType(literal) => match &literal.literal {
                oxc::ast::ast::TSLiteral::StringLiteral(value) => {
                    Some(vec![value.value.to_string()])
                }
                _ => None,
            },
            TSType::TSUnionType(union) => {
                let mut keys = Vec::new();
                for item in &union.types {
                    keys.extend(Self::static_pick_keys(item)?);
                }
                Some(keys)
            }
            _ => None,
        }
    }

    /// Extract fields common to a union of object-like type surfaces.
    fn union_type_fields(
        &mut self,
        union: &oxc::ast::ast::TSUnionType<'_>,
    ) -> Result<Vec<Field>, SmeltError> {
        let variants = union
            .types
            .iter()
            .map(|member| self.type_fields_from_ts(member))
            .collect::<Result<Vec<_>, _>>()?;
        let mut grouped: HashMap<smelt_hir::Symbol, Vec<Field>> = HashMap::new();
        for fields in &variants {
            for field in fields {
                grouped.entry(field.name).or_default().push(field.clone());
            }
        }
        let mut out = Vec::new();
        for (name, fields) in grouped {
            let mut value_tys = Vec::new();
            let mut optional = fields.len() != variants.len();
            let span = fields
                .first()
                .map_or_else(|| self.span(union.span.start, union.span.end), |field| field.span);
            for field in fields {
                optional |= field.optional;
                if !value_tys.contains(&field.ty) {
                    value_tys.push(field.ty);
                }
            }
            let ty = match value_tys.as_slice() {
                [single] => *single,
                [] => self.ctx.krate.types.intern(Type::Unknown),
                _ => self.ctx.krate.types.intern(Type::Union(value_tys)),
            };
            out.push(Field {
                name,
                ty,
                visibility: Visibility::Public,
                optional,
                span,
            });
        }
        Ok(out)
    }

    /// Extract named fields from a TypeScript type literal.
    fn type_literal_fields(
        &mut self,
        literal: &oxc::ast::ast::TSTypeLiteral<'_>,
    ) -> Result<Vec<Field>, SmeltError> {
        let mut fields = Vec::new();
        for member in &literal.members {
            let TSSignature::TSPropertySignature(prop) = member else {
                continue;
            };
            if prop.computed && !is_static_property_key(&prop.key) {
                continue;
            }
            let name = self.property_key_symbol(&prop.key)?;
            let ty = prop
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
            fields.push(Field {
                name,
                ty,
                visibility: Visibility::Public,
                optional: prop.optional,
                span: self.span(prop.span.start, prop.span.end),
            });
        }
        Ok(fields)
    }

    /// Convert an indexed access type such as `Parameters<Fn>[0]` to an item type.
    fn indexed_access_type_to_hir(
        &mut self,
        indexed: &oxc::ast::ast::TSIndexedAccessType<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let object_ty = self.ts_type_to_hir(&indexed.object_type)?;
        match self.ctx.krate.types.get(object_ty) {
            Some(Type::Tuple(items)) => {
                if let Some(index) = Self::numeric_literal_type_index(&indexed.index_type) {
                    return Ok(items
                        .get(index)
                        .copied()
                        .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown)));
                }
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
            Some(Type::List(item) | Type::Set(item)) => Ok(*item),
            Some(Type::Dict(_, value)) => Ok(*value),
            _ => Ok(self.ctx.krate.types.intern(Type::Unknown)),
        }
    }

    /// Extract a non-negative integer from a numeric literal type.
    fn numeric_literal_type_index(ty: &TSType<'_>) -> Option<usize> {
        let TSType::TSLiteralType(literal) = ty else {
            return None;
        };
        let oxc::ast::ast::TSLiteral::NumericLiteral(number) = &literal.literal else {
            return None;
        };
        let text = number.raw.as_ref()?;
        (number.value.fract() == 0.0 && number.value >= 0.0)
            .then(|| text.parse::<usize>().ok())
            .flatten()
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

    /// Extract `value is T` metadata from a TypeScript return annotation.
    fn predicate_return_type(
        &mut self,
        ty: &TSType<'_>,
    ) -> Option<Result<(String, smelt_hir::TypeId), SmeltError>> {
        let TSType::TSTypePredicate(predicate) = ty else {
            return None;
        };
        if predicate.asserts {
            return None;
        }
        let oxc::ast::ast::TSTypePredicateName::Identifier(parameter) = &predicate.parameter_name
        else {
            return Some(Err(SmeltError::unsupported(
                self.span(predicate.span.start, predicate.span.end),
                "predicate functions on `this` are not lowered yet",
            )));
        };
        let Some(annotation) = &predicate.type_annotation else {
            return Some(Err(SmeltError::unsupported(
                self.span(predicate.span.start, predicate.span.end),
                "predicate functions must use `value is T`",
            )));
        };
        Some(
            self.ts_type_to_hir(&annotation.type_annotation)
                .map(|target| (parameter.name.to_string(), target)),
        )
    }

    /// Lower `[T, ...T[]]`-style tuple aliases to homogeneous list metadata.
    ///
    /// TypeScript uses this spelling to model non-empty arrays. HIR does not
    /// currently track non-empty length refinements, so the useful runtime
    /// shape is the list element type.
    fn homogeneous_tuple_rest_type(
        &mut self,
        tuple: &oxc::ast::ast::TSTupleType<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let Some(TSTupleElement::TSRestType(rest)) = tuple.element_types.last() else {
            return Ok(None);
        };
        let rest_ty = self.ts_type_to_hir(&rest.type_annotation)?;
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(rest_ty).cloned() else {
            return Ok(None);
        };
        if matches!(self.ctx.krate.types.get(element_ty), Some(Type::Never)) {
            return Ok(None);
        }
        if tuple.element_types.len() <= 1 {
            return Ok(Some(self.ctx.krate.types.intern(Type::List(element_ty))));
        }
        let Some(fixed_items) = tuple.element_types.get(..tuple.element_types.len() - 1) else {
            return Ok(None);
        };
        for item in fixed_items {
            if matches!(item, TSTupleElement::TSRestType(_)) {
                return Ok(None);
            }
            if self.tuple_element_type_to_hir(item)? != element_ty {
                return Ok(None);
            }
        }
        Ok(Some(self.ctx.krate.types.intern(Type::List(element_ty))))
    }

    /// Lower tuple aliases with a non-`never` rest tail to list metadata.
    fn tuple_rest_list_type(
        &mut self,
        tuple: &oxc::ast::ast::TSTupleType<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let Some(TSTupleElement::TSRestType(rest)) = tuple.element_types.last() else {
            return Ok(None);
        };
        let rest_ty = self.ts_type_to_hir(&rest.type_annotation)?;
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(rest_ty).cloned() else {
            return Ok(None);
        };
        if matches!(self.ctx.krate.types.get(element_ty), Some(Type::Never)) {
            return Ok(None);
        }
        Ok(Some(self.ctx.krate.types.intern(Type::List(element_ty))))
    }

    /// Convert tuple element type to HIR type.
    fn tuple_element_type_to_hir(
        &mut self,
        item: &TSTupleElement<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match item {
            TSTupleElement::TSNumberKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Float)),
            TSTupleElement::TSStringKeyword(_) | TSTupleElement::TSTemplateLiteralType(_) => {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            TSTupleElement::TSBooleanKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Bool)),
            TSTupleElement::TSUnknownKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Unknown)),
            TSTupleElement::TSNeverKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Never)),
            TSTupleElement::TSNullKeyword(_)
            | TSTupleElement::TSUndefinedKeyword(_)
            | TSTupleElement::TSVoidKeyword(_) => Ok(self.ctx.krate.types.intern(Type::None)),
            TSTupleElement::TSArrayType(array) => {
                let element_ty = self.ts_type_to_hir(&array.element_type)?;
                Ok(self.ctx.krate.types.intern(Type::List(element_ty)))
            }
            TSTupleElement::TSTupleType(tuple) => {
                if let Some(ty) = self.homogeneous_tuple_rest_type(tuple)? {
                    return Ok(ty);
                }
                if let Some(ty) = self.tuple_rest_list_type(tuple)? {
                    return Ok(ty);
                }
                let mut items = Vec::new();
                for tuple_item in &tuple.element_types {
                    if self.tuple_rest_type_erases_to_empty(tuple_item)? {
                        continue;
                    }
                    items.push(self.tuple_element_type_to_hir(tuple_item)?);
                }
                Ok(self.ctx.krate.types.intern(Type::Tuple(items)))
            }
            TSTupleElement::TSTypeReference(reference) => self.type_reference_to_hir(reference),
            TSTupleElement::TSLiteralType(literal) => match &literal.literal {
                oxc::ast::ast::TSLiteral::StringLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::String))
                }
                oxc::ast::ast::TSLiteral::NumericLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Float))
                }
                oxc::ast::ast::TSLiteral::BooleanLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Bool))
                }
                _ => Ok(self.ctx.krate.types.intern(Type::Unknown)),
            },
            TSTupleElement::TSIndexedAccessType(_) => {
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
            TSTupleElement::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Readonly =>
            {
                self.ts_type_to_hir(&operator.type_annotation)
            }
            TSTupleElement::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Keyof =>
            {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            TSTupleElement::TSFunctionType(function) => self.function_type_to_hir(function),
            TSTupleElement::TSUnionType(union) => {
                let mut lowered = Vec::new();
                let mut nullish = Vec::new();
                let mut saw_never = false;
                for member in &union.types {
                    let member_ty = self.ts_type_to_hir(member)?;
                    if matches!(self.ctx.krate.types.get(member_ty), Some(Type::Never)) {
                        saw_never = true;
                    } else if matches!(self.ctx.krate.types.get(member_ty), Some(Type::None)) {
                        nullish.push(member_ty);
                    } else if !lowered.contains(&member_ty) {
                        lowered.push(member_ty);
                    }
                }
                if let Some(list_ty) = self.list_union_type(&lowered) {
                    if nullish.is_empty() {
                        return Ok(list_ty);
                    }
                    lowered = vec![list_ty];
                }
                if lowered.len() == 1 && !nullish.is_empty() {
                    let single = lowered.remove(0);
                    Ok(self.ctx.krate.types.intern(Type::Optional(single)))
                } else if lowered.len() == 1 {
                    let single = lowered.remove(0);
                    Ok(single)
                } else if lowered.is_empty() && nullish.is_empty() && saw_never {
                    Ok(self.ctx.krate.types.intern(Type::Never))
                } else {
                    lowered.extend(nullish);
                    Ok(self.ctx.krate.types.intern(Type::Union(lowered)))
                }
            }
            TSTupleElement::TSOptionalType(optional) => {
                let inner = self.ts_type_to_hir(&optional.type_annotation)?;
                Ok(self.ctx.krate.types.intern(Type::Optional(inner)))
            }
            TSTupleElement::TSRestType(rest) => Err(SmeltError::unsupported(
                self.span(rest.span.start, rest.span.end),
                "tuple rest types are only lowered for never tails",
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

    /// Collapse unions that only differ by an empty tuple branch into a list type.
    fn list_union_type(&mut self, items: &[smelt_hir::TypeId]) -> Option<smelt_hir::TypeId> {
        let mut element_tys = Vec::new();
        for item in items {
            match self.ctx.krate.types.get(*item).cloned() {
                Some(Type::List(element_ty)) if !element_tys.contains(&element_ty) => {
                    element_tys.push(element_ty);
                }
                Some(Type::List(_)) => {}
                Some(Type::Tuple(tuple_items)) if tuple_items.is_empty() => {}
                _ => return None,
            }
        }
        if element_tys.is_empty() {
            return None;
        }
        let element_ty = match element_tys.as_slice() {
            [single] => *single,
            _ => self.ctx.krate.types.intern(Type::Union(element_tys)),
        };
        Some(self.ctx.krate.types.intern(Type::List(element_ty)))
    }

    /// Convert a TypeScript function type node into HIR callable metadata.
    fn function_type_to_hir(
        &mut self,
        function: &oxc::ast::ast::TSFunctionType<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if function.this_param.is_some() {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "this-parameter function types are not lowered yet",
            ));
        }
        self.push_type_parameter_scope(function.type_parameters.as_deref())?;
        let mut params = Vec::new();
        let result = (|| {
            for param in &function.params.items {
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
                let param_ty = if param.optional {
                    self.ctx.krate.types.intern(Type::Optional(param_ty))
                } else {
                    param_ty
                };
                params.push(param_ty);
            }
            if let Some(rest) = &function.params.rest {
                let rest_ty = rest
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.function_type_rest_param_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(rest.span.start, rest.span.end),
                            "rest function type parameters require explicit array types",
                        )
                    })?;
                params.push(rest_ty);
            }
            let return_ty = self.ts_type_to_hir(&function.return_type.type_annotation)?;
            Ok(self.ctx.krate.types.intern(Type::Function(FunctionType {
                params,
                return_ty,
                is_async: false,
            })))
        })();
        self.pop_type_parameter_scope();
        result
    }

    /// Return whether a tuple rest element can be erased from HIR tuple metadata.
    ///
    /// TypeScript libraries use `[...never[]]` and occasionally `...never` as
    /// type-level variadic filters. Those tails cannot contribute runtime tuple
    /// elements, so Smelt erases them to an empty metadata tail. Generic tuple
    /// accumulators constrained to arrays are also erased because HIR has no
    /// open tuple-tail type and frontend type checking has already validated
    /// the source-level spread shape.
    fn tuple_rest_type_erases_to_empty(
        &mut self,
        item: &TSTupleElement<'_>,
    ) -> Result<bool, SmeltError> {
        let TSTupleElement::TSRestType(rest) = item else {
            return Ok(false);
        };
        let _ = self.ts_type_to_hir(&rest.type_annotation)?;
        Ok(true)
    }

    /// Return whether a concrete value of this type would contain a `never` value.
    fn concrete_type_requires_never_value(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Never) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
                self.concrete_type_requires_never_value(*item)
            }
            Some(Type::Dict(key, value)) => {
                self.concrete_type_requires_never_value(*key)
                    || self.concrete_type_requires_never_value(*value)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .any(|item| self.concrete_type_requires_never_value(*item)),
            Some(Type::Union(items)) => items
                .iter()
                .all(|item| self.concrete_type_requires_never_value(*item)),
            Some(
                Type::Bool
                | Type::Int
                | Type::Float
                | Type::String
                | Type::Unknown
                | Type::TypeParam { .. }
                | Type::Class { .. }
                | Type::Function(_)
                | Type::None,
            )
            | None => false,
        }
    }

    /// Return whether an array or tuple literal would need to materialize `never`.
    fn array_literal_needs_never_value(&self, ty: smelt_hir::TypeId, item_count: usize) -> bool {
        if item_count == 0 {
            return false;
        }
        match self.ctx.krate.types.get(ty) {
            Some(Type::List(item) | Type::Set(item)) => {
                self.concrete_type_requires_never_value(*item)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .take(item_count)
                .any(|item| self.concrete_type_requires_never_value(*item)),
            _ => false,
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
            ("Capitalize" | "Uncapitalize" | "Uppercase" | "Lowercase", [_]) => {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            (
                "Array"
                | "ArrayLike"
                | "Iterable"
                | "ReadonlyArray"
                | "NonEmptyArray"
                | "IterableContainer",
                [item],
            ) => {
                let lowered_item = self.ts_type_to_hir(item)?;
                Ok(self.ctx.krate.types.intern(Type::List(lowered_item)))
            }
            ("IterableContainer", []) => {
                let item = self.ctx.krate.types.intern(Type::Unknown);
                Ok(self.ctx.krate.types.intern(Type::List(item)))
            }
            ("Readonly", [item]) => self.ts_type_to_hir(item),
            ("Pick", [base, _keys]) => self.ts_type_to_hir(base),
            ("Set" | "ReadonlySet", [item]) => {
                let lowered_item = self.ts_type_to_hir(item)?;
                Ok(self.ctx.krate.types.intern(Type::Set(lowered_item)))
            }
            ("Record", [key, value]) => {
                let lowered_value = self.ts_type_to_hir(value)?;
                let lowered_key = match self.record_key_type_to_hir(key, reference.span) {
                    Ok(key) => key,
                    Err(error)
                        if matches!(self.ctx.krate.types.get(lowered_value), Some(Type::Never)) =>
                    {
                        drop(error);
                        self.ctx.krate.types.intern(Type::String)
                    }
                    Err(error) => return Err(error),
                };
                Ok(self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Dict(lowered_key, lowered_value)))
            }
            ("Map" | "ReadonlyMap", [key, value]) => {
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
            ("Parameters", [function_arg]) => {
                let function_ty = self.ts_type_to_hir(function_arg)?;
                if let Some(Type::Function(function_ty_data)) = self.ctx.krate.types.get(function_ty)
                {
                    Ok(self
                        .ctx
                        .krate
                        .types
                        .intern(Type::Tuple(function_ty_data.params.clone())))
                } else {
                    let item = self.ctx.krate.types.intern(Type::Unknown);
                    Ok(self.ctx.krate.types.intern(Type::List(item)))
                }
            }
            ("ReturnType", [function_arg]) => {
                let function_ty = self.ts_type_to_hir(function_arg)?;
                if let Some(Type::Function(function_ty_data)) = self.ctx.krate.types.get(function_ty)
                {
                    Ok(function_ty_data.return_ty)
                } else {
                    Ok(self.ctx.krate.types.intern(Type::Unknown))
                }
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
                    let alias_is_union = matches!(
                        self.ctx.krate.types.get(alias.ty),
                        Some(Type::Union(_))
                    );
                    let substituted_alias_ty = self.substitute_type_params(alias.ty, &substitutions);
                    if !alias_is_union
                        && let Some(function_ty) = self.function_member_type(substituted_alias_ty)
                    {
                        if let Some(fields) = self.type_alias_fields.get(&symbol).cloned()
                            && !fields.is_empty()
                        {
                            let fields = self.substituted_fields(&fields, &substitutions);
                            self.callable_fields.insert(function_ty, fields.clone());
                            self.ctx.callable_fields.insert(function_ty, fields);
                        }
                        return Ok(function_ty);
                    }
                    if self
                        .type_alias_fields
                        .get(&symbol)
                        .is_some_and(|fields| !fields.is_empty())
                        && self.function_member_type(substituted_alias_ty).is_none()
                    {
                        let instantiated_args = alias
                            .type_params
                            .iter()
                            .map(|param| {
                                substitutions.get(&param.name).copied().unwrap_or_else(|| {
                                    self.ctx.krate.types.intern(Type::TypeParam {
                                        name: param.name,
                                    })
                                })
                            })
                            .collect();
                        return Ok(self.ctx.krate.types.intern(Type::Class {
                            name: symbol,
                            args: instantiated_args,
                        }));
                    }
                    return Ok(substituted_alias_ty);
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

    /// Lower a TypeScript `Record<K, V>` key type for Smelt's object model.
    ///
    /// JavaScript object records expose stringified own keys for normal object
    /// operations, so concrete `string`, `number`, literal, `keyof`, and
    /// `PropertyKey`-like key surfaces normalize to `string`. A direct generic
    /// key such as `Record<K, V>` is preserved because later alias substitution
    /// still needs to thread that type parameter through generic helper returns.
    fn record_key_type_to_hir(
        &mut self,
        key: &TSType<'_>,
        span: oxc::span::Span,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if let Some(param_ty) = self.direct_record_key_type_param(key)? {
            return Ok(param_ty);
        }
        if self.record_key_surface_is_lowerable(key)? {
            return Ok(self.ctx.krate.types.intern(Type::String));
        }
        Err(SmeltError::unsupported(
            self.span(span.start, span.end),
            "Record keys must lower to TypeScript object property keys",
        ))
    }

    /// Return a direct generic key for `Record<K, V>` without normalizing it.
    ///
    /// Preserving the exact type parameter keeps generic object helpers
    /// instantiable after type aliases are substituted. Composite keys such as
    /// `K | string` use Smelt's string-key object representation instead.
    fn direct_record_key_type_param(
        &mut self,
        key: &TSType<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        match key {
            TSType::TSParenthesizedType(parenthesized) => {
                self.direct_record_key_type_param(&parenthesized.type_annotation)
            }
            TSType::TSTypeOperatorType(operator)
                if matches!(
                    operator.operator,
                    oxc::ast::ast::TSTypeOperatorOperator::Readonly
                        | oxc::ast::ast::TSTypeOperatorOperator::Unique
                ) =>
            {
                self.direct_record_key_type_param(&operator.type_annotation)
            }
            TSType::TSTypeReference(reference) => {
                let TSTypeName::IdentifierReference(name) = &reference.type_name else {
                    return Ok(None);
                };
                if reference.type_arguments.is_some() {
                    return Ok(None);
                }
                Ok(self.type_parameter_type(name.name.as_str()))
            }
            _ => Ok(None),
        }
    }

    /// Return whether a `Record` key surface is valid for string-key object lowering.
    ///
    /// TypeScript's checker already enforces that `Record<K, V>` keys are
    /// assignable to `PropertyKey`. This helper keeps Smelt from rejecting
    /// common type-level spellings whose concrete runtime representation is a
    /// JavaScript object with stringified ordinary keys.
    fn record_key_surface_is_lowerable(&mut self, key: &TSType<'_>) -> Result<bool, SmeltError> {
        match key {
            TSType::TSStringKeyword(_)
            | TSType::TSNumberKeyword(_)
            | TSType::TSBigIntKeyword(_)
            | TSType::TSSymbolKeyword(_)
            | TSType::TSAnyKeyword(_)
            | TSType::TSUnknownKeyword(_)
            | TSType::TSObjectKeyword(_)
            | TSType::TSTemplateLiteralType(_)
            | TSType::TSNeverKeyword(_) => Ok(true),
            TSType::TSLiteralType(literal) => Ok(matches!(
                literal.literal,
                oxc::ast::ast::TSLiteral::StringLiteral(_)
                    | oxc::ast::ast::TSLiteral::NumericLiteral(_)
                    | oxc::ast::ast::TSLiteral::UnaryExpression(_)
                    | oxc::ast::ast::TSLiteral::BooleanLiteral(_)
            )),
            TSType::TSParenthesizedType(parenthesized) => {
                self.record_key_surface_is_lowerable(&parenthesized.type_annotation)
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Keyof =>
            {
                Ok(true)
            }
            TSType::TSTypeOperatorType(operator)
                if matches!(
                    operator.operator,
                    oxc::ast::ast::TSTypeOperatorOperator::Readonly
                        | oxc::ast::ast::TSTypeOperatorOperator::Unique
                ) =>
            {
                self.record_key_surface_is_lowerable(&operator.type_annotation)
            }
            TSType::TSUnionType(union) => union
                .types
                .iter()
                .map(|member| self.record_key_surface_is_lowerable(member))
                .try_fold(true, |all_lowerable, member| {
                    member.map(|member_lowerable| all_lowerable && member_lowerable)
                }),
            TSType::TSIntersectionType(intersection) => intersection
                .types
                .iter()
                .map(|member| self.record_key_surface_is_lowerable(member))
                .try_fold(true, |any_lowerable, member| {
                    member.map(|member_lowerable| any_lowerable || member_lowerable)
                }),
            TSType::TSConditionalType(conditional) => {
                Ok(self.record_key_surface_is_lowerable(&conditional.true_type)?
                    && self.record_key_surface_is_lowerable(&conditional.false_type)?)
            }
            TSType::TSIndexedAccessType(_)
            | TSType::TSMappedType(_)
            | TSType::TSTypeReference(_) => {
                let lowered_key = self.ts_type_to_hir(key)?;
                Ok(self.lowered_record_key_is_string_representable(lowered_key))
            }
            _ => Ok(false),
        }
    }

    /// Return whether an already-lowered key can be represented as object keys.
    fn lowered_record_key_is_string_representable(&self, key: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(key)) {
            Some(
                Type::String
                | Type::Int
                | Type::Float
                | Type::Unknown
                | Type::Never
                | Type::TypeParam { .. },
            ) => true,
            Some(Type::Class { name, .. }) => self.ctx.krate.symbols.get(*name).is_some(),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.lowered_record_key_is_string_representable(item)),
            _ => false,
        }
    }

    /// Resolve the type of a class field.
    fn class_field_type(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        field: smelt_hir::Symbol,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let Some(receiver_type) = self.ctx.krate.types.get(receiver_ty).cloned() else {
            return Err(SmeltError::unsupported(
                self.span(0, 0),
                format!(
                    "field access is only lowered for Record<string, T>, class, and interface values for now (unknown receiver type id {receiver_ty:?})"
                ),
            ));
        };
        match receiver_type {
            Type::Dict(_, value) => Ok(value),
            Type::Unknown | Type::String if self.allow_unknown_index_access => {
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
            Type::TypeParam { .. } => Ok(self.ctx.krate.types.intern(Type::Unknown)),
            Type::Function(_) => {
                if let Some(field) = self
                    .callable_fields
                    .get(&receiver_ty)
                    .and_then(|fields| fields.iter().find(|item| item.name == field))
                {
                    return Ok(if field.optional {
                        self.ctx.krate.types.intern(Type::Optional(field.ty))
                    } else {
                        field.ty
                    });
                }
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
            Type::Union(items) => {
                let matches = items
                    .iter()
                    .filter_map(|item| self.class_field_type(*item, field).ok())
                    .collect::<Vec<_>>();
                match matches.as_slice() {
                    [single] => Ok(*single),
                    [] => Err(SmeltError::unsupported(
                        self.span(0, 0),
                        format!(
                            "field access is only lowered for Record<string, T>, class, and interface values for now (union receiver: {items:?})"
                        ),
                    )),
                    [first, rest @ ..] if rest.iter().all(|item| item == first) => Ok(*first),
                    _ => {
                        let mut union_items = Vec::new();
                        for item in matches {
                            if !union_items.contains(&item) {
                                union_items.push(item);
                            }
                        }
                        Ok(self.ctx.krate.types.intern(Type::Union(union_items)))
                    }
                }
            }
            Type::Class { name, args } => {
                let class_field = self.class_by_symbol(name).and_then(|class| {
                    class
                        .fields
                        .iter()
                        .find(|item| item.name == field)
                        .map(|item| item.ty)
                });
                let class_name = self
                    .ctx
                    .krate
                    .names
                    .get(name)
                    .or_else(|| self.ctx.krate.symbols.get(name))
                    .map(str::to_owned);
                let sidecar_field = class_name.as_deref().and_then(|class_name| {
                    self.class_fields.get(class_name).and_then(|fields| {
                        fields
                            .iter()
                            .find(|item| item.name == field)
                            .map(|item| item.ty)
                    })
                });
                let interface = self.find_interface(name).cloned();
                let interface_exists = interface.is_some();
                let interface_field = if let Some(interface) = interface {
                    let substitutions = self.type_argument_substitution(
                        &interface.type_params,
                        &args,
                        self.span(0, 0),
                    )?;
                    let field_data = interface
                        .fields
                        .iter()
                        .find(|item| item.name == field)
                        .map(|item| (item.ty, item.optional));
                    field_data.map(|(ty, optional)| {
                        self.instantiate_field_type(ty, optional, &substitutions)
                    })
                } else {
                    None
                };
                let alias_fields = self
                    .type_alias_fields
                    .get(&name)
                    .cloned()
                    .unwrap_or_default();
                let alias_field = if let Some(alias) = self.find_type_alias(name).cloned() {
                    let substitutions = self.type_argument_substitution(
                        &alias.type_params,
                        &args,
                        self.span(0, 0),
                    )?;
                    let field_data = alias_fields
                        .iter()
                        .find(|item| item.name == field)
                        .map(|item| (item.ty, item.optional));
                    field_data.map(|(ty, optional)| {
                        self.instantiate_field_type(ty, optional, &substitutions)
                    })
                } else {
                    let field_data = alias_fields
                        .iter()
                        .find(|item| item.name == field)
                        .map(|item| (item.ty, item.optional));
                    field_data.map(|(ty, optional)| self.field_type_with_optional(ty, optional))
                };
                if let Some(ty) = class_field
                    .or(sidecar_field)
                    .or(interface_field)
                    .or(alias_field)
                {
                    return Ok(ty);
                }
                let mut visited = HashSet::new();
                if let Some(ty) =
                    self.inherited_interface_field_type(name, &args, field, &mut visited)?
                {
                    return Ok(ty);
                }
                if self.class_by_symbol(name).is_none()
                    && class_name.as_deref().is_none_or(|sidecar_name| {
                        !self.class_fields.contains_key(sidecar_name)
                    })
                    && !interface_exists
                {
                    return Ok(self.ctx.krate.types.intern(Type::Unknown));
                }
                let field_name = self.ctx.krate.symbols.get(field).unwrap_or("<unknown>");
                Err(SmeltError::unsupported(
                    self.span(0, 0),
                    format!("unknown class or interface field `{field_name}`"),
                ))
            }
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                format!(
                    "field access is only lowered for Record<string, T>, class, and interface values for now (receiver: {receiver_type:?})"
                ),
            )),
        }
    }

    /// Resolve a field through stored interface heritage edges.
    ///
    /// Cyclic type-only imports can lower an interface before its parents are
    /// available. Keeping heritage edges lets member access retry inheritance
    /// against the latest declarations after the cycle has been loaded.
    fn inherited_interface_field_type(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
        field: smelt_hir::Symbol,
        visited: &mut HashSet<smelt_hir::Symbol>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        if !visited.insert(name) {
            return Ok(None);
        }
        let Some(parents) = self.interface_extends.get(&name).cloned() else {
            return Ok(None);
        };
        let child_params = self
            .find_interface(name)
            .map(|interface| interface.type_params.clone())
            .unwrap_or_default();
        let child_substitutions =
            self.type_argument_substitution(&child_params, args, self.span(0, 0))?;
        for parent in parents {
            let parent_args = parent
                .args
                .into_iter()
                .map(|arg| self.substitute_type_params(arg, &child_substitutions))
                .collect::<Vec<_>>();
            if let Some(ty) = self.interface_declared_field_type(parent.parent, &parent_args, field)?
            {
                return Ok(Some(ty));
            }
            if let Some(ty) =
                self.inherited_interface_field_type(parent.parent, &parent_args, field, visited)?
            {
                return Ok(Some(ty));
            }
        }
        Ok(None)
    }

    /// Resolve a directly declared interface field with generic arguments applied.
    fn interface_declared_field_type(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
        field: smelt_hir::Symbol,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let Some(interface) = self.find_interface(name).cloned() else {
            return Ok(None);
        };
        let substitutions =
            self.type_argument_substitution(&interface.type_params, args, self.span(0, 0))?;
        let field_data = interface
            .fields
            .iter()
            .find(|item| item.name == field)
            .map(|item| (item.ty, item.optional));
        Ok(field_data.map(|(ty, optional)| {
            self.instantiate_field_type(ty, optional, &substitutions)
        }))
    }

    /// Apply generic substitutions and optional wrapping for a structural field.
    fn instantiate_field_type(
        &mut self,
        ty: smelt_hir::TypeId,
        optional: bool,
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        let ty = self.substitute_type_params(ty, substitutions);
        self.field_type_with_optional(ty, optional)
    }

    /// Wrap optional fields without producing nested `Optional` types.
    fn field_type_with_optional(
        &mut self,
        ty: smelt_hir::TypeId,
        optional: bool,
    ) -> smelt_hir::TypeId {
        if optional && !matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
            self.ctx.krate.types.intern(Type::Optional(ty))
        } else {
            ty
        }
    }

    /// Resolve the value type for dynamic object field extraction.
    fn dynamic_field_type(&mut self, receiver_ty: smelt_hir::TypeId) -> smelt_hir::TypeId {
        let receiver_ty = self.type_param_constraint_or_self(receiver_ty);
        match self.ctx.krate.types.get(receiver_ty).cloned() {
            Some(Type::Dict(_, value)) => value,
            Some(Type::Unknown) if self.allow_unknown_index_access => {
                self.ctx.krate.types.intern(Type::Unknown)
            }
            Some(Type::Class { name, .. }) => {
                let fields = self
                    .type_alias_fields
                    .get(&name)
                    .cloned()
                    .unwrap_or_default();
                let mut value_tys = Vec::new();
                for field in fields {
                    let ty = if field.optional {
                        self.ctx.krate.types.intern(Type::Optional(field.ty))
                    } else {
                        field.ty
                    };
                    if !value_tys.contains(&ty) {
                        value_tys.push(ty);
                    }
                }
                match value_tys.as_slice() {
                    [single] => *single,
                    [] => self.ctx.krate.types.intern(Type::Unknown),
                    _ => self.ctx.krate.types.intern(Type::Union(value_tys)),
                }
            }
            Some(Type::Union(items)) => {
                let mut value_tys = Vec::new();
                for item in items {
                    let ty = self.dynamic_field_type(item);
                    if !value_tys.contains(&ty) {
                        value_tys.push(ty);
                    }
                }
                match value_tys.as_slice() {
                    [single] => *single,
                    [] => self.ctx.krate.types.intern(Type::Unknown),
                    _ => self.ctx.krate.types.intern(Type::Union(value_tys)),
                }
            }
            _ => self.ctx.krate.types.intern(Type::Unknown),
        }
    }

    /// Return the inner type for an optional receiver, or the original type otherwise.
    fn optional_receiver_inner_type(&self, ty: smelt_hir::TypeId) -> smelt_hir::TypeId {
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(ty) {
            *inner
        } else {
            ty
        }
    }

    /// Wrap a result type in `Optional`, avoiding nested optionals from optional fields.
    fn optional_chain_result_type(&mut self, ty: smelt_hir::TypeId) -> smelt_hir::TypeId {
        if matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
            ty
        } else {
            self.ctx.krate.types.intern(Type::Optional(ty))
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
        span: oxc::span::Span,
    ) -> Result<(smelt_hir::TypeId, smelt_hir::ItemId), SmeltError> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(receiver_ty) else {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
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
        let receiver_ty = self.type_param_constraint_or_self(receiver_ty);
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(item)) => Ok(*item),
            Some(Type::String) => Ok(self.ctx.krate.types.intern(Type::String)),
            Some(Type::Dict(_, value)) => Ok(*value),
            Some(Type::Unknown) if self.allow_unknown_index_access => {
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
            Some(Type::TypeParam { .. } | Type::Class { .. }) => {
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
            Some(Type::Tuple(items)) => {
                let mut union_items = Vec::new();
                for item in items.clone() {
                    if !union_items.contains(&item) {
                        union_items.push(item);
                    }
                }
                match union_items.as_slice() {
                    [single] => Ok(*single),
                    [] => Ok(self.ctx.krate.types.intern(Type::Unknown)),
                    _ => Ok(self.ctx.krate.types.intern(Type::Union(union_items))),
                }
            }
            Some(Type::Union(items)) => {
                let mut union_items = Vec::new();
                for item in items.clone() {
                    if let Ok(item_ty) = self.index_type(item)
                        && !union_items.contains(&item_ty)
                    {
                        union_items.push(item_ty);
                    }
                }
                match union_items.as_slice() {
                    [single] => Ok(*single),
                    [] => Ok(self.ctx.krate.types.intern(Type::Unknown)),
                    _ => Ok(self.ctx.krate.types.intern(Type::Union(union_items))),
                }
            }
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                format!(
                    "index access is only lowered for arrays, strings, and records for now (receiver: {:?})",
                    self.ctx.krate.types.get(receiver_ty)
                ),
            )),
        }
    }

    /// Return a type parameter's active constraint when expression lowering needs its shape.
    fn type_param_constraint_or_self(
        &self,
        ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        match self.ctx.krate.types.get(ty) {
            Some(Type::TypeParam { name }) => self.type_parameter_constraint(*name).unwrap_or(ty),
            _ => ty,
        }
    }

    /// Return whether a type can be represented as a string at runtime.
    fn is_string_compatible_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::String | Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => true,
            Some(Type::Optional(item)) => self.is_string_compatible_type(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.is_string_compatible_type(item)),
            _ => false,
        }
    }

    /// Return whether a type contains `unknown` after unwrapping simple containers.
    fn type_contains_unknown(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Unknown) => true,
            Some(Type::Optional(item)) => self.type_contains_unknown(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.type_contains_unknown(item)),
            _ => false,
        }
    }

    /// Return whether a type can hold a TypeScript nullish value at runtime.
    fn is_nullishable_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(_) | Type::None) => true,
            Some(Type::Union(items)) => items.iter().copied().any(|item| {
                self.ctx.krate.types.get(item) == Some(&Type::None)
                    || self.is_nullishable_type(item)
            }),
            _ => false,
        }
    }

    /// Return whether a union contains at least one string-compatible branch.
    fn union_has_string_compatible_member(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::String) => true,
            Some(Type::Optional(item)) => self.union_has_string_compatible_member(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.union_has_string_compatible_member(item)),
            _ => false,
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
        if name == "RegExp" {
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DictLit(Vec::new()),
                ty,
                span: self.span(start, end),
            }));
        }
        if name == "lazyImplementation" {
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DictLit(Vec::new()),
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
            if let Some(value) = self.const_objects.get(name).cloned() {
                return Ok(self.object_const_expression(&value, start, end, body));
            }
            if let Some(item) = self.items.get(name).copied()
                && matches!(self.item_ref(item), Item::Function(_))
            {
                return self.item_function_closure_expression(item, start, end, body);
            }
            if let Some(ty) = self.module_globals.get(name).copied() {
                return self.module_global_expression(ty, start, end, body);
            }
            if self.value_imports.contains(name) {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return self.module_global_expression(ty, start, end, body);
            }
            if self.allow_unknown_index_access {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(start, end),
                }));
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

    /// Wrap a function item in a first-class closure value for argument positions.
    fn item_function_closure_expression(
        &mut self,
        item: smelt_hir::ItemId,
        start: u32,
        end: u32,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Item::Function(function) = self.item_ref(item).clone() else {
            return Err(SmeltError::unsupported(
                self.span(start, end),
                "only function items can be wrapped as closure values",
            ));
        };
        let span = self.span(start, end);
        let mut closure_body = Body::new(None, span);
        let mut closure_params = Vec::new();
        let mut call_args = Vec::new();
        for param in &function.params {
            let local = closure_body.push_local(LocalDecl {
                name: Some(param.name),
                ty: param.ty,
                mutable: false,
                span: param.span,
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name: param.name,
                local,
                ty: param.ty,
                span: param.span,
            });
            call_args.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty: param.ty,
                span: param.span,
            }));
        }
        let callee = closure_body.push_expr(Expr {
            kind: ExprKind::Item(item),
            ty: self.item_expr_type(item, span)?,
            span,
        });
        let call = closure_body.push_expr(Expr {
            kind: ExprKind::Call {
                callee,
                args: call_args,
            },
            ty: function.return_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(call)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: function.params.iter().map(|param| param.ty).collect(),
            return_ty: function.return_ty,
            is_async: function.is_async,
        }));
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                return_ty: function.return_ty,
                captures: Vec::new(),
                body: body_id,
                callback_body: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Synthesize a read value for a known module-level variable.
    fn module_global_expression(
        &mut self,
        ty: smelt_hir::TypeId,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(Type::Function(function)) = self.ctx.krate.types.get(ty).cloned() {
            return self.module_global_function_expression(&function, ty, start, end, body);
        }
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
            Some(Type::List(_)) => ExprKind::ListLit(Vec::new()),
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

    /// Synthesize a callable value for a known module-level function constant.
    ///
    /// Date-fns stores helper callbacks in non-exported top-level `const`
    /// bindings and then reads those bindings while building exported locale
    /// objects. The module body owns the real closure, while exported constant
    /// lowering only needs a callable value with the same public type.
    fn module_global_function_expression(
        &mut self,
        function: &FunctionType,
        ty: smelt_hir::TypeId,
        start: u32,
        end: u32,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(start, end);
        let mut body = Body::new(None, span);
        let mut params = Vec::new();
        for (index, param_ty) in function.params.iter().copied().enumerate() {
            let name = self.intern_source_name(&format!("arg_{index}"));
            let local = body.push_local(LocalDecl {
                name: Some(name),
                ty: param_ty,
                mutable: false,
                span,
            });
            body.params.push(local);
            params.push(Param {
                name,
                local,
                ty: param_ty,
                span,
            });
        }
        let value = self.default_module_global_value(function.return_ty, span, &mut body)?;
        body.push_stmt(Stmt::Return(Some(value)));
        let body_id = self.ctx.krate.push_body(body);
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params,
                return_ty: function.return_ty,
                captures: Vec::new(),
                body: body_id,
                callback_body: None,
                span,
            }),
            ty,
            span,
        }))
    }

    /// Build a conservative default expression for synthesized module globals.
    fn default_module_global_value(
        &mut self,
        ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let kind = match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::None) => ExprKind::Literal(Literal::None),
            Some(Type::Bool) => ExprKind::Literal(Literal::Bool(false)),
            Some(Type::Int) => ExprKind::Literal(Literal::Int(0)),
            Some(Type::Float) => ExprKind::Literal(Literal::Float(0.0)),
            Some(Type::String) => ExprKind::Literal(Literal::String(String::new())),
            Some(Type::List(_)) => ExprKind::ListLit(Vec::new()),
            Some(Type::Dict(_, _)) => ExprKind::DictLit(Vec::new()),
            Some(Type::Optional(_)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: none_ty,
                    span,
                }));
            }
            Some(Type::Unknown | Type::Class { .. } | Type::TypeParam { .. }) => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let dict_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                let value = body.push_expr(Expr {
                    kind: ExprKind::DictLit(Vec::new()),
                    ty: dict_ty,
                    span,
                });
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::UnknownCast { value, target: ty },
                    ty,
                    span,
                }));
            }
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "module-level function return type needs a supported default value",
                ));
            }
        };
        Ok(body.push_expr(Expr { kind, ty, span }))
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
