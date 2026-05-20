impl ModuleBuilder<'_> {
    /// Lower a TypeScript function declaration into a HIR function item.
    fn function_declaration(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let id = function.id.as_ref().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "anonymous function declarations are not lowered yet",
            )
        })?;
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "declare functions are not lowered yet",
            ));
        };
        let name_text = id.name.as_str();
        let name = self.intern_source_name(name_text);
        let _type_params = self.push_type_parameter_scope(function.type_parameters.as_deref())?;
        let assertion_return = match function
            .return_type
            .as_ref()
            .and_then(|annotation| self.assertion_return_type(&annotation.type_annotation))
            .transpose()
        {
            Ok(value) => value,
            Err(error) => {
                self.pop_type_parameter_scope();
                return Err(error);
            }
        };
        let predicate_return = match function
            .return_type
            .as_ref()
            .and_then(|annotation| self.predicate_return_type(&annotation.type_annotation))
            .transpose()
        {
            Ok(value) => value,
            Err(error) => {
                self.pop_type_parameter_scope();
                return Err(error);
            }
        };
        let declared_return_ty = if assertion_return.is_some() {
            Some(self.ctx.krate.types.intern(Type::None))
        } else {
            match self.function_return_type_annotation_or_overload(function, name_text) {
                Ok(value) => value,
                Err(error) => {
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
            }
        };
        if function.r#async
            && declared_return_ty.is_some()
            && !matches!(
                declared_return_ty.and_then(|ty| self.ctx.krate.types.get(ty)),
                Some(Type::Future(_))
            )
        {
            self.pop_type_parameter_scope();
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "async functions must declare a Promise<T> return type",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_narrowed_locals = std::mem::take(&mut self.narrowed_locals);
        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        self.current_async = function.r#async;
        self.current_return_ty = declared_return_ty;
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut params = Vec::new();

        let mut destructured_params = Vec::new();
        for (index, param) in function.params.items.iter().enumerate() {
            let ty = match self.function_parameter_type(param) {
                Ok(value) => value,
                Err(error) => {
                    self.locals = saved_locals;
                    self.narrowed_locals = saved_narrowed_locals;
                    self.current_async = saved_async;
                    self.current_return_ty = saved_return_ty;
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
            };
            let (param_name, span, source_name) =
                if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                    (
                        self.intern_source_name(binding.name.as_str()),
                        self.span(binding.span.start, binding.span.end),
                        Some(binding.name.to_string()),
                    )
                } else {
                    let synthetic_name = format!("__param{index}");
                    (
                        self.intern_source_name(&synthetic_name),
                        self.span(param.span.start, param.span.end),
                        None,
                    )
                };
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span,
            });
            body.params.push(local);
            if let Some(source_name) = source_name {
                self.locals.insert(source_name, local);
            } else {
                destructured_params.push((&param.pattern, local, ty));
            }
            params.push(Param {
                name: param_name,
                local,
                ty,
                span,
            });
        }
        let rest = if let Some(rest) = &function.params.rest {
            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                self.locals = saved_locals;
                self.narrowed_locals = saved_narrowed_locals;
                self.current_async = saved_async;
                self.current_return_ty = saved_return_ty;
                self.pop_type_parameter_scope();
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest parameters need rest binding lowering",
                ));
            };
            let ty = match rest
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()
                .and_then(|value| {
                    value.ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(rest.span.start, rest.span.end),
                            "rest function parameters must have explicit array type annotations",
                        )
                    })
                }) {
                Ok(value) => value,
                Err(error) => {
                    self.locals = saved_locals;
                    self.narrowed_locals = saved_narrowed_locals;
                    self.current_async = saved_async;
                    self.current_return_ty = saved_return_ty;
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
            };
            let Ok((ty, item_ty)) = self.rest_param_array_type(ty) else {
                self.locals = saved_locals;
                self.narrowed_locals = saved_narrowed_locals;
                self.current_async = saved_async;
                self.current_return_ty = saved_return_ty;
                self.pop_type_parameter_scope();
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest function parameter type must be an array type",
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
            let index = params.len();
            params.push(Param {
                name: param_name,
                local,
                ty,
                span: self.span(binding.span.start, binding.span.end),
            });
            Some(RestParam { index, item_ty })
        } else {
            None
        };

        let mut errors = Vec::new();
        if let Err(error) = self.predeclare_local_arrow_callbacks(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        for (pattern, local, ty) in destructured_params {
            let root = body.root;
            let value = body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span: self.span(function.span.start, function.span.end),
            });
            if let Err(error) = self.binding_declaration(
                pattern,
                Some(value),
                Some(ty),
                false,
                &mut body,
                root,
            ) {
                errors.push(error);
            }
        }
        if let Err(error) = self.predeclare_local_arrow_callbacks(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        for statement in &function_body.statements {
            if self.is_super_call_statement(statement) {
                continue;
            }
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }
        if function.r#async {
            body.build_async_state_machine();
        }
        self.locals = saved_locals;
        self.narrowed_locals = saved_narrowed_locals;
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        self.pop_type_parameter_scope();

        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let mut return_ty = declared_return_ty
            .or_else(|| self.last_return_type(&body))
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::None));
        if function.r#async && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
        {
            return_ty = self.ctx.krate.types.intern(Type::Future(return_ty));
        }
        let body_id = self.ctx.krate.push_body(body);
        let function_item = Function {
            name,
            span: self.span(function.span.start, function.span.end),
            params,
            return_ty,
            is_async: function.r#async,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        };
        let item = if let Some(item) = self.local_function_items.get(name_text).copied() {
            let index = usize::try_from(item.0).unwrap_or(usize::MAX);
            if let Some(slot) = self.ctx.krate.items.get_mut(index) {
                *slot = Item::Function(function_item);
            }
            item
        } else {
            self.ctx.krate.push_item(Item::Function(function_item))
        };
        self.items.insert(name_text.to_owned(), item);
        if let Some(rest) = rest {
            self.function_rests.insert(name_text.to_owned(), rest);
        }
        if let Some((parameter_name, target)) = assertion_return
            && let Some(param_index) = function.params.items.iter().position(|param| {
                matches!(
                    &param.pattern,
                    BindingPattern::BindingIdentifier(binding)
                        if binding.name.as_str() == parameter_name
                )
            })
        {
            self.assertion_functions.insert(
                name_text.to_owned(),
                AssertionNarrowing {
                    param_index,
                    target,
                },
            );
        }
        if let Some((parameter_name, target)) = predicate_return
            && let Some(param_index) = function.params.items.iter().position(|param| {
                matches!(
                    &param.pattern,
                    BindingPattern::BindingIdentifier(binding)
                        if binding.name.as_str() == parameter_name
                )
            })
        {
            self.predicate_functions.insert(
                name_text.to_owned(),
                AssertionNarrowing {
                    param_index,
                    target,
                },
            );
        }
        Ok(item)
    }

    /// Resolve the HIR type for a function declaration parameter.
    ///
    /// Explicit TypeScript annotations remain the primary source of parameter
    /// types. For unannotated parameters with default initializers, TypeScript
    /// infers the in-body parameter type from the default expression, so Smelt
    /// mirrors that narrow case without weakening arbitrary untyped functions.
    fn function_parameter_type(
        &mut self,
        param: &oxc::ast::ast::FormalParameter<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let ty = if let Some(annotation) = &param.type_annotation {
            self.ts_type_to_hir(&annotation.type_annotation)?
        } else if let Some(initializer) = &param.initializer {
            self.infer_module_global_initializer_type(initializer)?
        } else {
            return Err(SmeltError::unsupported(
                self.span(param.span.start, param.span.end),
                "function parameters must have explicit type annotations or default initializers",
            ));
        };
        if param.optional && !matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
            Ok(self.ctx.krate.types.intern(Type::Optional(ty)))
        } else {
            Ok(ty)
        }
    }

    /// Lower a function expression into a module-owned HIR function item.
    ///
    /// Exported object namespace constants can contain method syntax, as in
    /// date-fns formatter tables. Each method is represented as a private
    /// module function and referenced from the namespace metadata.
    fn function_expression_item(
        &mut self,
        name_text: &str,
        function: &oxc::ast::ast::Function<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "function expressions must have a body",
            ));
        };
        if function.params.rest.is_some() {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "object namespace method rest parameters are not lowered yet",
            ));
        }

        let name = self.intern_source_name(name_text);
        let _type_params = self.push_type_parameter_scope(function.type_parameters.as_deref())?;
        let hinted_function = type_hint.and_then(|ty| match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::Function(function_ty)) => Some(function_ty),
            _ => None,
        });
        let return_ty = if let Some(return_type) = &function.return_type {
            match self.ts_type_to_hir(&return_type.type_annotation) {
                Ok(value) => value,
                Err(error) => {
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
            }
        } else if let Some(function_ty) = &hinted_function {
            function_ty.return_ty
        } else {
            match self.function_return_type_or_overload(function, name_text) {
                Ok(value) => value,
                Err(error) => {
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
            }
        };
        if function.r#async && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
        {
            self.pop_type_parameter_scope();
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "async functions must declare a Promise<T> return type",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_narrowed_locals = std::mem::take(&mut self.narrowed_locals);
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
                } else if let Some(function_ty) = &hinted_function {
                    function_ty.params.get(index).copied().ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(param.span.start, param.span.end),
                            "function expression parameter is missing from contextual callable type",
                        )
                    })?
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "function expression parameters must have explicit type annotations",
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
                debug_assert_eq!(
                    usize::try_from(local.0).unwrap_or(usize::MAX),
                    index,
                    "function expression parameters should be added in source order",
                );
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(error);
                break;
            }
        }
        if errors.is_empty()
            && let Some(function_ty) = &hinted_function
            && function_ty.params.len() > params.len()
        {
            for (index, ty) in function_ty.params.iter().copied().enumerate().skip(params.len()) {
                let param_name = self.intern_source_name(&format!("__unused{index}"));
                let local = body.push_local(LocalDecl {
                    name: Some(param_name),
                    ty,
                    mutable: false,
                    span: self.span(function.span.start, function.span.end),
                });
                body.params.push(local);
                params.push(Param {
                    name: param_name,
                    local,
                    ty,
                    span: self.span(function.span.start, function.span.end),
                });
            }
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
        self.narrowed_locals = saved_narrowed_locals;
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        self.pop_type_parameter_scope();
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let body_id = self.ctx.krate.push_body(body);
        Ok(self.ctx.krate.push_item(Item::Function(Function {
            name,
            span: self.span(function.span.start, function.span.end),
            params,
            return_ty,
            is_async: function.r#async,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        })))
    }

    /// Lower a class declaration to HIR.
    fn class_declaration(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let id = class.id.as_ref().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "anonymous classes are not lowered yet",
            )
        })?;
        if !class.decorators.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "decorators are not lowered yet",
            ));
        }
        let class_text = id.name.as_str();
        let class_name = self.intern_type_name(class_text);
        let type_params = self.push_type_parameter_scope(class.type_parameters.as_deref())?;
        let class_type_args = type_params
            .iter()
            .map(|param| {
                self.ctx
                    .krate
                    .types
                    .intern(Type::TypeParam { name: param.name })
            })
            .collect::<Vec<_>>();
        let class_ty = self.ctx.krate.types.intern(Type::Class {
            name: class_name,
            args: class_type_args,
        });
        let (base, base_args) = self.class_extends_clause(class)?;
        if let Some(base_name) = base {
            self.class_bases
                .insert(class_text.to_owned(), (base_name, base_args.clone()));
        }
        let mut fields = Vec::new();
        let mut constructor = None;
        let mut methods = Vec::new();
        let mut abstract_methods = Vec::new();

        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(property) => {
                    if property.computed && !is_static_property_key(&property.key) {
                        return Err(SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "dynamic computed property names are not lowered yet",
                        ));
                    }
                    if property.r#static {
                        return Err(SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "static fields are not lowered yet",
                        ));
                    }
                    let name = self.property_key_symbol(&property.key)?;
                    let mut ty = if let Some(annotation) = &property.type_annotation {
                        self.ts_type_to_hir(&annotation.type_annotation)?
                    } else if let Some(value) = &property.value {
                        let mut field_body =
                            Body::new(None, self.span(value.span().start, value.span().end));
                        let value = self.expression(value, &mut field_body)?;
                        Self::expr_ty(&field_body, value)
                    } else {
                        return Err(SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "class fields require explicit type annotations",
                        ));
                    };
                    if property.optional {
                        ty = self.field_type_with_optional(ty, true);
                    }
                    fields.push(Field {
                        name,
                        ty,
                        visibility: visibility(property.accessibility),
                        optional: property.optional,
                        span: self.span(property.span.start, property.span.end),
                    });
                }
                ClassElement::MethodDefinition(method)
                    if method.kind == MethodDefinitionKind::Get =>
                {
                    if !method.decorators.is_empty() {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "method decorators are not lowered yet",
                        ));
                    }
                    if method.computed && !is_static_property_key(&method.key) {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "dynamic computed method names are not lowered yet",
                        ));
                    }
                    if method.r#static {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "static methods are not lowered yet",
                        ));
                    }
                    let name = self.property_key_symbol(&method.key)?;
                    let ty = method
                        .value
                        .return_type
                        .as_ref()
                        .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                        .transpose()?
                        .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                    fields.push(Field {
                        name,
                        ty,
                        visibility: visibility(method.accessibility),
                        optional: false,
                        span: self.span(method.span.start, method.span.end),
                    });
                }
                _ => {}
            }
        }
        self.class_fields
            .insert(class_text.to_owned(), fields.clone());
        let method_sigs = self.class_method_signatures(&class.body.body)?;
        self.class_methods
            .insert(class_text.to_owned(), method_sigs);

        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(_) => {}
                ClassElement::MethodDefinition(method) => {
                    if method.kind == MethodDefinitionKind::Get {
                        continue;
                    }
                    if !method.decorators.is_empty() {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "method decorators are not lowered yet",
                        ));
                    }
                    if method.computed && !is_static_property_key(&method.key) {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "dynamic computed method names are not lowered yet",
                        ));
                    }
                    if method.r#static {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "static methods are not lowered yet",
                        ));
                    }
                    if method.r#type == MethodDefinitionType::TSAbstractMethodDefinition {
                        if !matches!(method.kind, MethodDefinitionKind::Method) {
                            return Err(SmeltError::unsupported(
                                self.span(method.span.start, method.span.end),
                                "abstract constructors are not lowered yet",
                            ));
                        }
                        let sig = self.abstract_class_method_sig(method)?;
                        abstract_methods.push(sig);
                        continue;
                    }
                    if !matches!(
                        method.kind,
                        MethodDefinitionKind::Constructor | MethodDefinitionKind::Method
                    ) {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "getters and setters are not lowered yet",
                        ));
                    }
                    let item = if method.kind == MethodDefinitionKind::Constructor {
                        if constructor.is_some() {
                            return Err(SmeltError::unsupported(
                                self.span(method.span.start, method.span.end),
                                "duplicate constructors are not allowed",
                            ));
                        }
                        let item =
                            self.class_function(class_text, class_name, class_ty, method, true)?;
                        constructor = Some(item);
                        item
                    } else {
                        let item =
                            self.class_function(class_text, class_name, class_ty, method, false)?;
                        methods.push(item);
                        item
                    };
                    let _ = item;
                }
                ClassElement::AccessorProperty(accessor) => {
                    return Err(SmeltError::unsupported(
                        self.span(accessor.span.start, accessor.span.end),
                        "accessor properties are not lowered yet",
                    ));
                }
                ClassElement::StaticBlock(block) => {
                    return Err(SmeltError::unsupported(
                        self.span(block.span.start, block.span.end),
                        "static blocks are not lowered yet",
                    ));
                }
                ClassElement::TSIndexSignature(sig) => {
                    return Err(SmeltError::unsupported(
                        self.span(sig.span.start, sig.span.end),
                        "class index signatures are not lowered yet",
                    ));
                }
            }
        }

        if constructor.is_none() {
            constructor = Some(self.synthesize_default_class_constructor(
                class_name,
                class_ty,
                self.span(class.span.start, class.span.end),
            ));
        }

        let implements = class
            .implements
            .iter()
            .filter_map(|imp| self.implements_symbol(imp).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        let item = self.ctx.krate.push_item(Item::Class(Class {
            name: class_name,
            span: self.span(class.span.start, class.span.end),
            kind: if class.r#abstract {
                smelt_hir::ClassKind::Abstract
            } else {
                smelt_hir::ClassKind::Plain
            },
            type_params,
            base,
            base_args,
            fields,
            constructor,
            methods,
            abstract_methods,
            implements,
        }));
        self.pop_type_parameter_scope();
        self.classes.insert(class_text.to_owned(), item);
        self.validate_implements(item)?;
        Ok(item)
    }

    /// Collect class method signatures before lowering method bodies.
    ///
    /// The final class item is emitted after method bodies are lowered, so
    /// same-class calls need this metadata to resolve return types during the
    /// class lowering pass. The collected signatures are metadata only; method
    /// bodies still lower into regular HIR function items.
    fn class_method_signatures(
        &mut self,
        elements: &[ClassElement<'_>],
    ) -> Result<Vec<MethodSig>, SmeltError> {
        let mut methods = Vec::new();
        for element in elements {
            let ClassElement::MethodDefinition(method) = element else {
                continue;
            };
            if method.kind != MethodDefinitionKind::Method {
                continue;
            }
            methods.push(self.abstract_class_method_sig(method)?);
        }
        Ok(methods)
    }

    /// Synthesize the implicit zero-argument constructor for a TypeScript class.
    fn synthesize_default_class_constructor(
        &mut self,
        class_name: smelt_hir::Symbol,
        class_ty: smelt_hir::TypeId,
        span: Span,
    ) -> smelt_hir::ItemId {
        let mut body = Body::new(None, span);
        let this_symbol = self.ctx.krate.symbols.intern("this");
        body.push_local(LocalDecl {
            name: Some(this_symbol),
            ty: class_ty,
            mutable: true,
            span,
        });
        let body_id = self.ctx.krate.push_body(body);
        let name = self.ctx.krate.symbols.intern("new");
        self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            params: Vec::new(),
            return_ty: class_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Constructor { class: class_name },
        }))
    }

    /// Lower the single supported TypeScript `extends` shape for class declarations.
    fn class_extends_clause(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
    ) -> Result<(Option<smelt_hir::Symbol>, Vec<smelt_hir::TypeId>), SmeltError> {
        let Some(super_class) = &class.super_class else {
            return Ok((None, Vec::new()));
        };
        let Expression::Identifier(identifier) = super_class else {
            return Err(SmeltError::unsupported(
                self.span(super_class.span().start, super_class.span().end),
                "class extends currently requires a direct base class identifier",
            ));
        };
        let name = identifier.name.as_str();
        let base = self.intern_type_name(name);
        let allowed_builtin = matches!(
            name,
            "Date"
                | "Error"
                | "EvalError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "TypeError"
                | "URIError"
                | "AggregateError"
        );
        if !allowed_builtin && !self.classes.contains_key(name) && !self.value_imports.contains(name)
        {
            return Err(SmeltError::unsupported(
                self.span(super_class.span().start, super_class.span().end),
                format!("base class `{name}` is not declared"),
            ));
        }
        let args = class
            .super_type_arguments
            .as_ref()
            .map(|type_args| {
                type_args
                    .params
                    .iter()
                    .map(|arg| self.ts_type_to_hir(arg))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok((Some(base), args))
    }

    /// Lower an abstract TypeScript method declaration to a HIR method signature.
    fn abstract_class_method_sig(
        &mut self,
        method: &oxc::ast::ast::MethodDefinition<'_>,
    ) -> Result<MethodSig, SmeltError> {
        if method.value.this_param.is_some() {
            return Err(SmeltError::unsupported(
                self.span(method.span.start, method.span.end),
                "this-parameter abstract methods are not lowered yet",
            ));
        }
        let _type_params = self.push_type_parameter_scope(method.value.type_parameters.as_deref())?;
        let return_ty = method
            .value
            .return_type
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .unwrap_or_else(|| {
                let unknown = self.ctx.krate.types.intern(Type::Unknown);
                if method.value.r#async {
                    self.ctx.krate.types.intern(Type::Future(unknown))
                } else {
                    unknown
                }
            });
        let mut params = Vec::new();
        for (index, param) in method.value.params.items.iter().enumerate() {
            let ty = param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .or_else(|| {
                    param
                        .initializer
                        .as_ref()
                        .and_then(|initializer| self.infer_module_global_initializer_type(initializer).ok())
                })
                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
            let (name, span) = if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                (
                    self.intern_source_name(binding.name.as_str()),
                    self.span(binding.span.start, binding.span.end),
                )
            } else {
                (
                    self.intern_source_name(&format!("__param{index}")),
                    self.span(param.span.start, param.span.end),
                )
            };
            params.push(ParamSig {
                name,
                ty,
                span,
            });
        }
        let sig = MethodSig {
            name: self.property_key_symbol(&method.key)?,
            params,
            return_ty,
            visibility: visibility(method.accessibility),
            is_async: matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_))),
            span: self.span(method.span.start, method.span.end),
        };
        self.pop_type_parameter_scope();
        Ok(sig)
    }

    /// Lower a class method or constructor to HIR.
    fn class_function(
        &mut self,
        class_text: &str,
        class_name: smelt_hir::Symbol,
        class_ty: smelt_hir::TypeId,
        method: &oxc::ast::ast::MethodDefinition<'_>,
        is_constructor: bool,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let Some(function_body) = &method.value.body else {
            return Err(SmeltError::unsupported(
                self.span(method.span.start, method.span.end),
                "declare methods are not lowered yet",
            ));
        };
        let method_name = if is_constructor {
            self.ctx.krate.symbols.intern("new")
        } else {
            self.property_key_symbol(&method.key)?
        };
        let _method_type_params =
            self.push_type_parameter_scope(method.value.type_parameters.as_deref())?;
        let return_ty = if is_constructor {
            if method.value.return_type.is_some() {
                return Err(SmeltError::unsupported(
                    self.span(method.span.start, method.span.end),
                    "constructors cannot declare return types",
                ));
            }
            class_ty
        } else {
            method
                .value
                .return_type
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .unwrap_or_else(|| {
                    let unknown = self.ctx.krate.types.intern(Type::Unknown);
                    if method.value.r#async {
                        self.ctx.krate.types.intern(Type::Future(unknown))
                    } else {
                        unknown
                    }
                })
        };
        if method.value.r#async
            && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
        {
            return Err(SmeltError::unsupported(
                self.span(method.span.start, method.span.end),
                "async methods must declare a Promise<T> return type",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_class = self.current_class.replace(class_text.to_owned());
        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        self.current_async = method.value.r#async;
        self.current_return_ty = Some(return_ty);
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut params = Vec::new();
        let this_symbol = self.ctx.krate.symbols.intern("this");
        let this_local = body.push_local(LocalDecl {
            name: Some(this_symbol),
            ty: class_ty,
            mutable: true,
            span: self.span(method.span.start, method.span.start),
        });
        self.locals.insert("this".to_owned(), this_local);
        if !is_constructor {
            body.params.push(this_local);
            params.push(Param {
                name: this_symbol,
                local: this_local,
                ty: class_ty,
                span: self.span(method.span.start, method.span.start),
            });
        }

        let mut destructured_params = Vec::new();
        for (index, param) in method.value.params.items.iter().enumerate() {
            let ty = if let Some(annotation) = &param.type_annotation {
                self.ts_type_to_hir(&annotation.type_annotation)?
            } else if let Some(default) = &param.initializer {
                let default = self.expression(default, &mut body)?;
                Self::expr_ty(&body, default)
            } else {
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "method parameters must have explicit type annotations",
                ));
            };
            let (param_name, span, source_name) =
                if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                    (
                        self.intern_source_name(binding.name.as_str()),
                        self.span(binding.span.start, binding.span.end),
                        Some(binding.name.to_string()),
                    )
                } else {
                    let synthetic_name = format!("__param{index}");
                    (
                        self.intern_source_name(&synthetic_name),
                        self.span(param.span.start, param.span.end),
                        None,
                    )
                };
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span,
            });
            body.params.push(local);
            if let Some(source_name) = source_name {
                self.locals.insert(source_name, local);
            } else {
                destructured_params.push((&param.pattern, local, ty));
            }
            params.push(Param {
                name: param_name,
                local,
                ty,
                span,
            });
            if is_constructor && param.accessibility.is_some() {
                let field = Field {
                    name: param_name,
                    ty,
                    visibility: visibility(param.accessibility),
                    optional: false,
                    span,
                };
                self.class_fields
                    .entry(class_text.to_owned())
                    .or_default()
                    .push(field);
            }
        }

        let mut errors = Vec::new();
        if let Err(error) = self.predeclare_local_arrow_callbacks(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        for (pattern, local, ty) in destructured_params {
            let root = body.root;
            let value = body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span: self.span(method.span.start, method.span.end),
            });
            if let Err(error) =
                self.binding_declaration(pattern, Some(value), Some(ty), false, &mut body, root)
            {
                errors.push(error);
            }
        }
        for statement in &function_body.statements {
            if self.is_super_call_statement(statement) {
                continue;
            }
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }
        if method.value.r#async {
            body.build_async_state_machine();
        }
        self.locals = saved_locals;
        self.current_class = saved_class;
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        self.pop_type_parameter_scope();
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        let body_id = self.ctx.krate.push_body(body);
        Ok(self.ctx.krate.push_item(Item::Function(Function {
            name: method_name,
            span: self.span(method.span.start, method.span.end),
            params,
            return_ty,
            is_async: method.value.r#async,
            is_test: false,
            body: Some(body_id),
            owner: if is_constructor {
                FunctionOwner::Constructor { class: class_name }
            } else {
                FunctionOwner::ClassMethod {
                    class: class_name,
                    method: method_name,
                }
            },
        })))
    }

    /// Return whether a constructor statement is a bare `super(...)` call.
    fn is_super_call_statement(&self, statement: &Statement<'_>) -> bool {
        let Statement::ExpressionStatement(statement) = statement else {
            return false;
        };
        if self
            .source
            .get(
                usize::try_from(statement.span.start).unwrap_or(usize::MAX)
                    ..usize::try_from(statement.span.end).unwrap_or(usize::MAX),
            )
            .is_some_and(|text| text.trim_start().starts_with("super("))
        {
            return true;
        }
        let Expression::CallExpression(call) = &statement.expression else {
            return false;
        };
        matches!(call.callee, Expression::Super(_))
    }

    /// Lower a statement in the current scope.
    fn statement(&mut self, statement: &Statement<'_>, body: &mut Body) -> Result<(), SmeltError> {
        self.statement_in_block(statement, body, body.root)
    }

    // Continued in the next split builder file.
}
