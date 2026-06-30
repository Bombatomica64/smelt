/// TypeScript type lowering, structural type inspection, and HIR type resolution.
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
                oxc::ast::ast::TSLiteral::BigIntLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Int))
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
                    let mut member_ty = self.ts_type_to_hir(member)?;
                    if matches!(self.ctx.krate.types.get(member_ty), Some(Type::Function(_)))
                        && self.ts_type_is_known_callable_object_surface(member)
                    {
                        member_ty = self.ctx.krate.types.intern(Type::Unknown);
                    }
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
                if Self::ts_type_is_callable_object_surface(ty)
                    || Self::ts_type_is_opaque_object_intersection_surface(ty)
                {
                    return Ok(self.ctx.krate.types.intern(Type::Unknown));
                }
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
                    Ok(self.ctx.krate.types.intern(Type::Unknown))
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
                if let Some(list_ty) = self.leading_rest_tuple_list_type(tuple)? {
                    return Ok(list_ty);
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
                let value_ty = mapped
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(annotation))
                    .transpose()?
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                if let TSType::TSTypeOperatorType(operator) = &mapped.constraint
                    && operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Keyof
                {
                    let source_ty = self.ts_type_to_hir(&operator.type_annotation)?;
                    let source_ty = self.type_param_constraint_or_self(source_ty);
                    if self.mapped_key_source_is_iterable(source_ty) {
                        return Ok(self.ctx.krate.types.intern(Type::List(value_ty)));
                    }
                }
                let key_ty = self.ctx.krate.types.intern(Type::String);
                Ok(self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty)))
            }
            TSType::TSFunctionType(function) => self.function_type_to_hir(function),
            TSType::TSThisType(this_ty) => {
                let Some(class_name) = &self.current_class else {
                    return Ok(self.ctx.krate.types.intern(Type::Unknown));
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
                rest: function.rest,
                required_params: None,
                    mutable_params: Vec::new(),
return_ty: function.return_ty,
                is_async: function.is_async,
                            may_throw: false,
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
        if matches!(ty, TSType::TSNeverKeyword(_)) {
            let item_ty = self.ctx.krate.types.intern(Type::Never);
            return Ok(self.ctx.krate.types.intern(Type::List(item_ty)));
        }
        let ty = self.ts_type_to_hir(ty)?;
        let (list_ty, _) = self.rest_param_array_type(ty)?;
        Ok(list_ty)
    }

    /// Resolve a rest-parameter annotation to the runtime array binding type and item type.
    fn rest_param_array_type(
        &mut self,
        ty: smelt_hir::TypeId,
    ) -> Result<(smelt_hir::TypeId, smelt_hir::TypeId), SmeltError> {
        let ty = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::List(item_ty)) => Ok((ty, item_ty)),
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                Ok((self.ctx.krate.types.intern(Type::List(item_ty)), item_ty))
            }
            Some(Type::Tuple(items)) => {
                let item_ty = self.union_of_types_or_unknown(items);
                Ok((self.ctx.krate.types.intern(Type::List(item_ty)), item_ty))
            }
            Some(Type::Union(items)) => {
                let mut item_tys = Vec::new();
                for item in items {
                    if matches!(self.ctx.krate.types.get(item), Some(Type::Never)) {
                        continue;
                    }
                    let (_, item_ty) = self.rest_param_array_type(item)?;
                    if !item_tys.contains(&item_ty) {
                        item_tys.push(item_ty);
                    }
                }
                let item_ty = self.union_of_types_or_unknown(item_tys);
                Ok((self.ctx.krate.types.intern(Type::List(item_ty)), item_ty))
            }
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                "rest parameter type must resolve to an array type",
            )),
        }
    }

    /// Build a union type from unique item types, falling back to `unknown` for an empty set.
    fn union_of_types_or_unknown(
        &mut self,
        items: Vec<smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        let unknown = self.ctx.krate.types.intern(Type::Unknown);
        if items.contains(&unknown) {
            return unknown;
        }
        match items.as_slice() {
            [] => unknown,
            [single] => *single,
            _ => self.ctx.krate.types.intern(Type::Union(items)),
        }
    }

    /// Return whether a source type is a callable value with object fields.
    fn ts_type_is_known_callable_object_surface(&mut self, ty: &TSType<'_>) -> bool {
        if Self::ts_type_is_callable_object_surface(ty) {
            return true;
        }
        let TSType::TSTypeReference(reference) = ty else {
            return false;
        };
        let TSTypeName::IdentifierReference(name) = &reference.type_name else {
            return false;
        };
        let symbol = self.intern_type_name(name.name.as_str());
        self.callable_object_aliases.contains(&symbol)
    }

    /// Return whether a type's syntax is an intersection of callable and object surfaces.
    fn ts_type_is_callable_object_surface(ty: &TSType<'_>) -> bool {
        let TSType::TSIntersectionType(intersection) = ty else {
            return false;
        };
        let mut has_callable = false;
        let mut has_object = false;
        for member in &intersection.types {
            match member {
                TSType::TSFunctionType(_) => has_callable = true,
                TSType::TSTypeLiteral(literal) => {
                    has_object |= !literal.members.is_empty();
                }
                TSType::TSTypeReference(_) => has_object = true,
                TSType::TSParenthesizedType(parenthesized) => {
                    if matches!(
                        &parenthesized.type_annotation,
                        TSType::TSFunctionType(_) | TSType::TSConstructorType(_)
                    ) {
                        has_callable = true;
                    } else {
                        has_object = true;
                    }
                }
                _ => {}
            }
        }
        has_callable && has_object
    }

    /// Return whether an intersection mixes an opaque reference with object fields.
    ///
    /// Smelt cannot currently preserve both halves of `Alias & { field: ... }`
    /// unless the alias resolves locally to a concrete class or dict. Erasing the
    /// surface to `unknown` keeps runtime field access and callable-object values
    /// intact instead of collapsing the value to only the object-literal fields.
    fn ts_type_is_opaque_object_intersection_surface(ty: &TSType<'_>) -> bool {
        let TSType::TSIntersectionType(intersection) = ty else {
            return false;
        };
        let mut has_reference = false;
        let mut has_object_literal_fields = false;
        for member in &intersection.types {
            match member {
                TSType::TSTypeReference(_) => has_reference = true,
                TSType::TSTypeLiteral(literal) => {
                    has_object_literal_fields |= !literal.members.is_empty();
                }
                TSType::TSParenthesizedType(parenthesized) => {
                    if matches!(&parenthesized.type_annotation, TSType::TSTypeReference(_)) {
                        has_reference = true;
                    }
                }
                _ => {}
            }
        }
        has_reference && has_object_literal_fields
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
            let Ok(name) = self.property_key_symbol(&prop.key) else {
                continue;
            };
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

    /// Extract assertion-signature metadata from a TypeScript return annotation.
    ///
    /// Two assertion forms are recognized:
    /// - `asserts value is T` carries a narrowing target `T` and yields
    ///   `Some(Ok((value, Some(T))))`.
    /// - `asserts value` (a bare condition assertion, e.g. `invariant`) has no
    ///   structural narrowing target and yields `Some(Ok((value, None)))`.
    ///
    /// In both cases the function's runtime return type is void; the `Option`
    /// target only drives optional parameter narrowing at call sites.
    fn assertion_return_type(
        &mut self,
        ty: &TSType<'_>,
    ) -> Option<Result<(String, Option<smelt_hir::TypeId>), SmeltError>> {
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
            return Some(Ok((parameter.name.to_string(), None)));
        };
        Some(
            self.ts_type_to_hir(&annotation.type_annotation)
                .map(|target| (parameter.name.to_string(), Some(target))),
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

    /// Return how many leading arguments a rest parameter's source type requires.
    ///
    /// HIR lowers both `...rest: T[]` and `...rest: NonEmptyArray<T>` (i.e.
    /// `[T, ...T[]]`) to the same `Type::List(T)`, erasing the required-prefix
    /// arity. Overload selection still needs that arity so a `NonEmptyArray`
    /// rest does not match an empty rest tail vacuously, so this inspects the
    /// *source* annotation:
    ///
    /// * `NonEmptyArray<T>` (by name) and a required-prefix tuple `[A, B, ...T[]]`
    ///   return the number of required leading elements (`>= 1`).
    /// * Plain `T[]`, `Array<T>`, `ReadonlyArray<T>`, and all-rest tuples
    ///   `[...T[]]` accept an empty tail and return `0`.
    ///
    /// `Readonly`/`Required`/parenthesized wrappers are transparent, matching how
    /// `ts_type_to_hir` already unwraps them.
    fn rest_parameter_min_arity(ty: &TSType<'_>) -> usize {
        match ty {
            TSType::TSParenthesizedType(parenthesized) => {
                Self::rest_parameter_min_arity(&parenthesized.type_annotation)
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == oxc::ast::ast::TSTypeOperatorOperator::Readonly =>
            {
                Self::rest_parameter_min_arity(&operator.type_annotation)
            }
            TSType::TSTypeReference(reference) => match &reference.type_name {
                // `NonEmptyArray<T>` is `[T, ...T[]]`: at least one element.
                TSTypeName::IdentifierReference(name) if name.name.as_str() == "NonEmptyArray" => 1,
                // `Readonly<…>`/`Required<…>` are transparent wrappers.
                TSTypeName::IdentifierReference(name)
                    if matches!(name.name.as_str(), "Readonly" | "Required") =>
                {
                    reference
                        .type_arguments
                        .as_ref()
                        .and_then(|args| args.params.first())
                        .map_or(0, Self::rest_parameter_min_arity)
                }
                _ => 0,
            },
            // A tuple type's required-prefix length, e.g. `[A, B, ...T[]]` => 2.
            // All-rest tuples (`[...T[]]`) and fixed tuples without a rest tail
            // are not required-prefix rests for this purpose, so they yield 0.
            TSType::TSTupleType(tuple) => {
                let has_rest_tail = matches!(
                    tuple.element_types.last(),
                    Some(TSTupleElement::TSRestType(_))
                );
                if !has_rest_tail {
                    return 0;
                }
                tuple
                    .element_types
                    .iter()
                    .take_while(|item| !matches!(item, TSTupleElement::TSRestType(_)))
                    .count()
            }
            _ => 0,
        }
    }

    /// Lower tuple aliases with a non-`never` rest tail to list metadata.
    fn tuple_rest_list_type(
        &mut self,
        tuple: &oxc::ast::ast::TSTupleType<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        if !tuple.element_types.is_empty()
            && tuple
                .element_types
                .iter()
                .all(|item| matches!(item, TSTupleElement::TSRestType(_)))
        {
            let mut list_item_ty = None;
            for item in &tuple.element_types {
                let TSTupleElement::TSRestType(rest) = item else {
                    continue;
                };
                let rest_ty = self.ts_type_to_hir(&rest.type_annotation)?;
                let rest_ty = self.type_param_constraint_or_self(rest_ty);
                let item_ty = match self.ctx.krate.types.get(rest_ty).cloned() {
                    Some(Type::List(item_ty)) => item_ty,
                    Some(Type::Never) => return Ok(None),
                    _ => self.ctx.krate.types.intern(Type::Unknown),
                };
                list_item_ty = Some(match list_item_ty {
                    Some(previous) if previous == item_ty => previous,
                    Some(_) => self.ctx.krate.types.intern(Type::Unknown),
                    None => item_ty,
                });
            }
            if let Some(item_ty) = list_item_ty {
                return Ok(Some(self.ctx.krate.types.intern(Type::List(item_ty))));
            }
        }

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

    /// Lower a tuple whose single rest element is NOT last (e.g. `[...T[], U]`)
    /// to a homogeneous list.
    ///
    /// TypeScript uses leading/middle-rest tuples to model arrays whose leading
    /// portion is variadic with a fixed tail. HIR has no leading-rest tuple
    /// shape, and modelling it as a fixed tuple would silently drop the rest
    /// element (and truncate any literal) because `tuple_rest_type_erases_to_empty`
    /// discards every `TSRestType`. The useful runtime shape is a list of the
    /// union of the rest element type and the fixed (non-rest) element types, so
    /// indexing any position stays well-typed. Trailing-rest and all-rest tuples
    /// are handled earlier by `homogeneous_tuple_rest_type` / `tuple_rest_list_type`,
    /// so this only fires for the otherwise-truncated leading/middle-rest shape.
    fn leading_rest_tuple_list_type(
        &mut self,
        tuple: &oxc::ast::ast::TSTupleType<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let rest_positions: Vec<usize> = tuple
            .element_types
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, TSTupleElement::TSRestType(_)))
            .map(|(idx, _)| idx)
            .collect();
        if rest_positions.len() != 1 {
            return Ok(None);
        }
        let rest_index = rest_positions[0];
        if rest_index == tuple.element_types.len() - 1 {
            return Ok(None); // trailing rest: handled by tuple_rest_list_type
        }
        let TSTupleElement::TSRestType(rest) = &tuple.element_types[rest_index] else {
            return Ok(None);
        };
        let rest_ty = self.ts_type_to_hir(&rest.type_annotation)?;
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(rest_ty).cloned() else {
            return Ok(None);
        };
        if matches!(self.ctx.krate.types.get(element_ty), Some(Type::Never)) {
            return Ok(None);
        }
        let mut members = vec![element_ty];
        for (idx, item) in tuple.element_types.iter().enumerate() {
            if idx == rest_index {
                continue;
            }
            let item_ty = self.tuple_element_type_to_hir(item)?;
            if !members.contains(&item_ty) {
                members.push(item_ty);
            }
        }
        let element = if members.len() == 1 {
            members[0]
        } else {
            self.ctx.krate.types.intern(Type::Union(members))
        };
        Ok(Some(self.ctx.krate.types.intern(Type::List(element))))
    }

    /// Convert tuple element type to HIR type.
    fn tuple_element_type_to_hir(
        &mut self,
        item: &TSTupleElement<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match item {
            TSTupleElement::TSNumberKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Float)),
            TSTupleElement::TSBigIntKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Int)),
            TSTupleElement::TSStringKeyword(_) | TSTupleElement::TSTemplateLiteralType(_) => {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            TSTupleElement::TSBooleanKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Bool)),
            TSTupleElement::TSAnyKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Unknown)),
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
                if let Some(ty) = self.leading_rest_tuple_list_type(tuple)? {
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
            TSTupleElement::TSTypeLiteral(literal) => self.type_literal_to_hir(literal),
            TSTupleElement::TSLiteralType(literal) => match &literal.literal {
                oxc::ast::ast::TSLiteral::StringLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::String))
                }
                oxc::ast::ast::TSLiteral::NumericLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Float))
                }
                oxc::ast::ast::TSLiteral::BigIntLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Int))
                }
                oxc::ast::ast::TSLiteral::BooleanLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Bool))
                }
                _ => Ok(self.ctx.krate.types.intern(Type::Unknown)),
            },
            TSTupleElement::TSIndexedAccessType(_) => {
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
            TSTupleElement::TSConditionalType(conditional) => {
                let true_ty = self.ts_type_to_hir(&conditional.true_type)?;
                let false_ty = self.ts_type_to_hir(&conditional.false_type)?;
                if true_ty == false_ty {
                    Ok(true_ty)
                } else {
                    Ok(self.ctx.krate.types.intern(Type::Union(vec![
                        true_ty, false_ty,
                    ])))
                }
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
            TSTupleElement::TSParenthesizedType(parenthesized) => {
                self.ts_type_to_hir(&parenthesized.type_annotation)
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

    /// Infer function parameters that represent returned structural state.
    ///
    /// Some TypeScript APIs model mutation by returning either a value or a
    /// tuple containing the updated state object. When a function field has
    /// that shape, generated Rust can pass the state parameter by `&mut`
    /// instead of cloning it through an adapter and losing identity.
    fn mutable_params_from_returned_tuple_state(
        &self,
        params: &[smelt_hir::TypeId],
        return_ty: smelt_hir::TypeId,
    ) -> Vec<usize> {
        params
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                (self.mutable_abi_param_type(*param)
                    && self.return_type_tuple_contains(return_ty, *param))
                .then_some(index)
            })
            .collect()
    }

    /// Return whether a parameter type can use Rust mutable-reference ABI.
    fn mutable_abi_param_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Class { .. } | Type::List(_) | Type::Set(_) | Type::Dict(_, _)) => true,
            Some(Type::Optional(inner)) => self.mutable_abi_param_type(*inner),
            _ => false,
        }
    }

    /// Return whether a return type has a tuple branch containing `target`.
    fn return_type_tuple_contains(
        &self,
        ty: smelt_hir::TypeId,
        target: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Tuple(items)) => items.contains(&target),
            Some(Type::Union(items)) => items
                .iter()
                .any(|item| self.return_type_tuple_contains(*item, target)),
            Some(Type::Optional(inner) | Type::Future(inner)) => {
                self.return_type_tuple_contains(*inner, target)
            }
            _ => false,
        }
    }

    /// Convert a TypeScript function type node into HIR callable metadata.
    fn function_type_to_hir(
        &mut self,
        function: &oxc::ast::ast::TSFunctionType<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        // An explicit `this` parameter in a function *type* is purely type-level:
        // JavaScript drops the bound receiver at call sites, so the runtime signature
        // is just the declared `params`. oxc keeps `this_param` separate from
        // `params.items`, so erasing it here needs no further filtering.
        let _ = &function.this_param;
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
            let mut rest_index = None;
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
                rest_index = Some(params.len());
                params.push(rest_ty);
            }
            let return_ty = self.ts_type_to_hir(&function.return_type.type_annotation)?;
            let mutable_params = self.mutable_params_from_returned_tuple_state(&params, return_ty);
            Ok(self.ctx.krate.types.intern(Type::Function(FunctionType {
                params,
                rest: rest_index,
                required_params: None,
                mutable_params,
                return_ty,
                is_async: false,
                may_throw: false,
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
        let name_text = match &reference.type_name {
            TSTypeName::IdentifierReference(name) => name.name.to_string(),
            TSTypeName::QualifiedName(qualified) => Self::qualified_type_name_text(qualified),
            TSTypeName::ThisExpression(_) => {
                return Err(SmeltError::unsupported(
                    self.span(reference.span.start, reference.span.end),
                    "unsupported type reference name",
                ));
            }
        };
        if let Some(param_ty) = self.type_parameter_type(&name_text) {
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
        match (name_text.as_str(), args.as_slice()) {
            ("RegExp", []) => Ok(self.regexp_type()),
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
            ("TuplePrefix" | "RemovePrefix" | "TupleSuffix" | "RemoveSuffix", _)
            | ("IterableContainer", []) => {
                let item = self.ctx.krate.types.intern(Type::Unknown);
                Ok(self.ctx.krate.types.intern(Type::List(item)))
            }
            ("Partial" | "Readonly" | "Required", [item]) => self.ts_type_to_hir(item),
            ("Extract", [_base, extracted]) => self.ts_type_to_hir(extracted),
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
                let symbol = self.resolve_type_reference_symbol(&name_text);
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
                    let alias_has_fields = self
                        .type_alias_fields
                        .get(&symbol)
                        .is_some_and(|fields| !fields.is_empty());
                    if alias_has_fields && self.function_member_type(substituted_alias_ty).is_none()
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

    /// Return the full source path for a qualified type name.
    fn qualified_type_name_text(qualified: &oxc::ast::ast::TSQualifiedName<'_>) -> String {
        let left = match &qualified.left {
            TSTypeName::IdentifierReference(left) => left.name.to_string(),
            TSTypeName::QualifiedName(left) => Self::qualified_type_name_text(left),
            TSTypeName::ThisExpression(_) => "<unknown>".to_owned(),
        };
        format!("{left}.{}", qualified.right.name)
    }

    /// Resolve a source type reference through local import aliases to its declared symbol.
    fn resolve_type_reference_symbol(&mut self, name_text: &str) -> smelt_hir::Symbol {
        if let Some(item) = self.items.get(name_text).copied() {
            match self.item_ref(item) {
                Item::TypeAlias(alias) => return alias.name,
                Item::Interface(interface) => return interface.name,
                Item::Class(class) => return class.name,
                _ => {}
            }
        }
        self.intern_type_name(name_text)
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
            | TSType::TSTypeQuery(_)
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
            Type::Optional(inner) => {
                let inner_field = self.class_field_type(inner, field)?;
                Ok(self.optional_chain_result_type(inner_field))
            }
            Type::Unknown | Type::TypeParam { .. } => Ok(self.ctx.krate.types.intern(Type::Unknown)),
            Type::Tuple(_) => Ok(self.ctx.krate.types.intern(Type::Unknown)),
            Type::Bool => {
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
            Type::Float | Type::Int
                if self.ctx.krate.symbols.get(field) == Some("constructor") =>
            {
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
            Type::String if self.ctx.krate.symbols.get(field) == Some("length") => {
                Ok(self.ctx.krate.types.intern(Type::Float))
            }
            Type::String if self.ctx.krate.symbols.get(field) == Some("message") => {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            Type::String if self.ctx.krate.symbols.get(field) == Some("name") => {
                Ok(self.ctx.krate.types.intern(Type::String))
            }
            Type::String if self.ctx.krate.symbols.get(field) == Some("prototype") => {
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
            Type::String if self.allow_unknown_index_access => {
                Ok(self.ctx.krate.types.intern(Type::Unknown))
            }
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
                    [] => Ok(self.ctx.krate.types.intern(Type::Unknown)),
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
                if self.ctx.krate.symbols.get(field) == Some("constructor") {
                    return Ok(self.ctx.krate.types.intern(Type::Unknown));
                }
                let class_item = self.class_by_symbol(name).cloned();
                if class_item.is_none()
                    && let Some(ty) = self.in_progress_class_method_member_type(name, field)
                {
                    return Ok(ty);
                }
                let class_field = if let Some(class) = &class_item {
                    let substitutions =
                        self.type_argument_substitution(&class.type_params, &args, self.span(0, 0))?;
                    class
                        .fields
                        .iter()
                        .find(|item| item.name == field)
                        .map(|item| self.substitute_type_params(item.ty, &substitutions))
                } else {
                    None
                };
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
                let sidecar_method = class_name.as_deref().and_then(|class_name| {
                    let method = self
                        .class_methods
                        .get(class_name)?
                        .iter()
                        .find(|item| item.name == field)?;
                    let params = method.params.iter().map(|param| param.ty).collect::<Vec<_>>();
                    let mutable_params =
                        self.mutable_params_from_returned_tuple_state(&params, method.return_ty);
                    Some(self.ctx.krate.types.intern(Type::Function(FunctionType {
                        params,
                        rest: None,
                        required_params: None,
                        mutable_params,
                        return_ty: method.return_ty,
                        is_async: method.is_async,
                        may_throw: false,
                    })))
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
                    }).or_else(|| self.interface_index_values.get(&name).copied())
                } else {
                    None
                };
                let interface_method = self.find_interface(name).cloned().and_then(|iface| {
                    let substitutions = self
                        .type_argument_substitution(&iface.type_params, &args, self.span(0, 0))
                        .ok()?;
                    let method = iface.methods.iter().find(|item| item.name == field)?;
                    let params = method
                        .params
                        .iter()
                        .map(|param| self.substitute_type_params(param.ty, &substitutions))
                        .collect::<Vec<_>>();
                    let return_ty = self.substitute_type_params(method.return_ty, &substitutions);
                    let mutable_params =
                        self.mutable_params_from_returned_tuple_state(&params, return_ty);
                    Some(self.ctx.krate.types.intern(Type::Function(FunctionType {
                        params,
                        rest: None,
                        required_params: None,
                        mutable_params,
                        return_ty,
                        is_async: method.is_async,
                        may_throw: false,
                    })))
                });
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
                    .or(sidecar_method)
                    .or(interface_field)
                    .or(interface_method)
                    .or(alias_field)
                {
                    return Ok(ty);
                }
                if let Some(ty) = self.builtin_class_field_type(name, field) {
                    return Ok(ty);
                }
                if let Some((base, base_args)) =
                    class_item.and_then(|class| class.base.map(|base| (base, class.base_args)))
                    .or_else(|| {
                        class_name
                            .as_deref()
                            .and_then(|class_name| self.class_bases.get(class_name).cloned())
                    })
                {
                    let substitutions = self.type_argument_substitution(
                        &self
                            .class_by_symbol(name)
                            .map(|class| class.type_params.clone())
                            .unwrap_or_default(),
                        &args,
                        self.span(0, 0),
                    )?;
                    let base_args = base_args
                        .into_iter()
                        .map(|arg| self.substitute_type_params(arg, &substitutions))
                        .collect();
                    let base_ty = self.ctx.krate.types.intern(Type::Class {
                        name: base,
                        args: base_args,
                    });
                    if let Ok(ty) = self.class_field_type(base_ty, field) {
                        return Ok(ty);
                    }
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
                if self
                    .class_by_symbol(name)
                    .is_some_and(|class| class.base.is_some())
                    || self
                        .ctx
                        .krate
                        .symbols
                        .get(name)
                        .is_some_and(|base_lookup_name| {
                            self.class_bases.contains_key(base_lookup_name)
                        })
                {
                    return Ok(self.ctx.krate.types.intern(Type::Unknown));
                }
                let field_name = self.ctx.krate.symbols.get(field).unwrap_or("<unknown>");
                if field_name == "id" {
                    return Ok(self.ctx.krate.types.intern(Type::Unknown));
                }
                if interface_exists {
                    return Ok(self.ctx.krate.types.intern(Type::Unknown));
                }
                let receiver_name = self.ctx.krate.symbols.get(name).unwrap_or("<unknown>");
                Err(SmeltError::unsupported(
                    self.span(0, 0),
                    format!("unknown class or interface field `{field_name}` on `{receiver_name}`"),
                ))
            }
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                format!(
                    "field access is only lowered for Record<string, T>, class, and interface values for now (receiver: {receiver_type:?}, field: {})",
                    self.ctx.krate.symbols.get(field).unwrap_or("<unknown>")
                ),
            )),
        }
    }

    /// Resolve an in-progress class method reference as a function-valued member type.
    fn in_progress_class_method_member_type(
        &mut self,
        class_name: smelt_hir::Symbol,
        method_name: smelt_hir::Symbol,
    ) -> Option<smelt_hir::TypeId> {
        let class_text = self
            .ctx
            .krate
            .names
            .get(class_name)
            .or_else(|| self.ctx.krate.symbols.get(class_name))
            .map(str::to_owned)?;
        let methods = self.class_methods.get(&class_text).cloned()?;
        let method = methods.into_iter().find(|item| item.name == method_name)?;
        let params = method.params.iter().map(|param| param.ty).collect::<Vec<_>>();
        let mutable_params =
            self.mutable_params_from_returned_tuple_state(&params, method.return_ty);
        Some(self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest: None,
            required_params: None,
            mutable_params,
            return_ty: method.return_ty,
            is_async: method.is_async,
            may_throw: false,
        })))
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
            if self
                .ctx
                .krate
                .symbols
                .get(parent.parent)
                .is_some_and(|parent_name| parent_name.contains('.'))
            {
                return Ok(Some(self.ctx.krate.types.intern(Type::Unknown)));
            }
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
        Ok(field_data
            .map(|(ty, optional)| self.instantiate_field_type(ty, optional, &substitutions))
            .or_else(|| self.interface_index_values.get(&name).copied()))
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
        smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
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

    /// Return declared fields for built-in classes that Smelt does not import from lib.d.ts.
    fn builtin_class_field_type(
        &mut self,
        class: smelt_hir::Symbol,
        field: smelt_hir::Symbol,
    ) -> Option<smelt_hir::TypeId> {
        let class_name = self
            .ctx
            .krate
            .names
            .get(class)
            .or_else(|| self.ctx.krate.symbols.get(class))?;
        if !matches!(
            class_name,
            "Error"
                | "EvalError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "TypeError"
                | "URIError"
                | "AggregateError"
        ) {
            return None;
        }
        let field_name = self.ctx.krate.symbols.get(field)?;
        match field_name {
            "name" | "message" => Some(self.ctx.krate.types.intern(Type::String)),
            "stack" => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                Some(self.ctx.krate.types.intern(Type::Optional(string_ty)))
            }
            _ => None,
        }
    }

    /// Resolve a method call on a type.
    fn resolve_method(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
        span: oxc::span::Span,
    ) -> Result<(smelt_hir::TypeId, smelt_hir::ItemId), SmeltError> {
        let Some(Type::Class { name, args }) = self.ctx.krate.types.get(receiver_ty).cloned() else {
            if matches!(
                self.ctx.krate.types.get(receiver_ty),
                Some(
                    Type::Unknown
                        | Type::TypeParam { .. }
                        | Type::Union(_)
                        | Type::Dict(_, _)
                        | Type::List(_)
                        | Type::Set(_)
                        | Type::Function(_)
                        | Type::Future(_)
                )
            ) {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return Ok((ty, smelt_hir::ItemId(u32::MAX)));
            }
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "method calls are only lowered for class values for now",
            ));
        };
        let Some(class) = self.class_by_symbol(name).cloned() else {
            if let Some(return_ty) =
                self.in_progress_class_method_return(name, &args, method, span)?
            {
                return Ok((return_ty, smelt_hir::ItemId(u32::MAX)));
            }
            return Ok((receiver_ty, smelt_hir::ItemId(u32::MAX)));
        };
        for item in &class.methods {
            if let Item::Function(function) = self.item_ref(*item)
                && function.name == method
            {
                return Ok((function.return_ty, *item));
            }
        }
        for abstract_method in &class.abstract_methods {
            if abstract_method.name == method {
                return Ok((abstract_method.return_ty, smelt_hir::ItemId(u32::MAX)));
            }
        }
        if let Some(base) = class.base {
            let substitutions = self.type_argument_substitution(
                &class.type_params,
                &args,
                self.span(span.start, span.end),
            )?;
            let base_args = class
                .base_args
                .clone()
                .into_iter()
                .map(|arg| self.substitute_type_params(arg, &substitutions))
                .collect();
            let base_ty = self.ctx.krate.types.intern(Type::Class {
                name: base,
                args: base_args,
            });
            return self.resolve_method(base_ty, method, span);
        }
        if self
            .ctx
            .krate
            .symbols
            .get(method)
            .is_some_and(|method_text| matches!(method_text, "call" | "apply" | "bind" | "flush"))
        {
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok((ty, smelt_hir::ItemId(u32::MAX)));
        }
        let method_name = self.ctx.krate.symbols.get(method).unwrap_or("<unknown>");
        Err(SmeltError::unsupported(
            self.span(0, 0),
            format!("unknown class method `{method_name}`"),
        ))
    }

    /// Resolve a method return type from metadata collected before a class item exists.
    fn in_progress_class_method_return(
        &self,
        class: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
        method: smelt_hir::Symbol,
        span: oxc::span::Span,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let Some(class_name) = self
            .ctx
            .krate
            .names
            .get(class)
            .or_else(|| self.ctx.krate.symbols.get(class))
            .map(str::to_owned)
        else {
            return Ok(None);
        };
        let Some(methods) = self.class_methods.get(&class_name).cloned() else {
            return Ok(None);
        };
        let Some(signature) = methods.into_iter().find(|item| item.name == method) else {
            return Ok(None);
        };
        let _ = (args, span);
        Ok(Some(signature.return_ty))
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
            Some(Type::Unknown | Type::None) => Ok(self.ctx.krate.types.intern(Type::Unknown)),
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
            None => Ok(self.ctx.krate.types.intern(Type::Unknown)),
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

    /// Return whether a `keyof` mapped-type source is entirely array-like.
    fn mapped_key_source_is_iterable(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::List(_) | Type::Tuple(_) | Type::Set(_)) => true,
            Some(Type::Union(items)) => {
                !items.is_empty()
                    && items
                        .iter()
                        .copied()
                        .all(|item| self.mapped_key_source_is_iterable(item))
            }
            _ => false,
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

    /// Infer the HIR result type for a lowered JavaScript binary operation.
    fn binary_result_type(
        &mut self,
        op: BinOp,
        lhs_ty: smelt_hir::TypeId,
        rhs_ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        if matches!(
            op,
            BinOp::Eq
                | BinOp::NotEq
                | BinOp::StrictEq
                | BinOp::StrictNotEq
                | BinOp::JsStrictEq
                | BinOp::JsStrictNotEq
                | BinOp::Lt
                | BinOp::Lte
                | BinOp::Gt
                | BinOp::Gte
        ) {
            return self.ctx.krate.types.intern(Type::Bool);
        }
        if op == BinOp::Add && (self.has_static_string_type(lhs_ty) || self.has_static_string_type(rhs_ty)) {
            return self.ctx.krate.types.intern(Type::String);
        }
        if matches!(
            op,
            BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Rem
                | BinOp::Shl
                | BinOp::Shr
                | BinOp::UShr
        ) && (self.is_erased_numeric_operand(lhs_ty)
            || self.is_erased_numeric_operand(rhs_ty)
            || self.is_optional_numeric_operand(lhs_ty)
            || self.is_optional_numeric_operand(rhs_ty))
        {
            return self.ctx.krate.types.intern(Type::Float);
        }
        lhs_ty
    }

    /// Return whether arithmetic should coerce an optional numeric operand to a number.
    fn is_optional_numeric_operand(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::Optional(inner)) => self.is_numeric_like_type(*inner),
            _ => false,
        }
    }

    /// Return whether an operand should use erased numeric arithmetic.
    fn is_erased_numeric_operand(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        )
    }

    /// Return whether a type is known to contain a string operand for `+`.
    fn has_static_string_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::String) => true,
            Some(Type::Optional(item)) => self.has_static_string_type(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.has_static_string_type(item)),
            _ => false,
        }
    }

    /// Return the HIR type used for JavaScript `RegExp` values.
    fn regexp_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("RegExp");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
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
            Some(Type::List(_) | Type::String | Type::Tuple(_) | Type::TypeParam { .. })
        )
            || matches!(
                self.ctx.krate.types.get(receiver_ty),
                Some(Type::Class { name, .. })
                    if self
                        .ctx
                        .krate
                        .symbols
                        .get(*name)
                        .is_some_and(|name| !self.classes.contains_key(name) && !self.interfaces.contains_key(name))
            )
    }

    /// Returns true when TypeScript `.size` can lower directly to Rust `.len()`.
    fn supports_stdlib_size(&self, receiver_ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Dict(_, _) | Type::Set(_) | Type::TypeParam { .. })
        )
    }

    /// Return the inner item type for a `Promise<T>` / `Future<T>` value.
    pub(super) fn future_inner_type(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Future(inner)) => Some(*inner),
            _ => None,
        }
    }

    /// Return the resolved item type for any awaitable actual value.
    ///
    /// A statically-typed `Promise<T>` / `Future<T>` yields its inner `T`. An
    /// erased actual (`Type::Unknown`) is a `SmeltUnknown::Promise` thunk — for
    /// example an `async` call across an import boundary whose generic return
    /// type was erased — and resolves to an erased `Unknown` value once awaited.
    /// Anything else is not awaitable and returns `None`. This keeps the Vitest
    /// `resolves`/`rejects` matchers general over both concrete and erased
    /// promise actuals instead of demanding a statically-spelled `Promise<T>`.
    pub(super) fn awaitable_inner_type(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Future(inner)) => Some(*inner),
            // An erased actual is itself the `Unknown` type id; awaiting it
            // yields another erased value, so reuse the same id rather than
            // re-interning.
            Some(Type::Unknown) => Some(ty),
            _ => None,
        }
    }
}
