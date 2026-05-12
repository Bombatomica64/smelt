impl ModuleBuilder<'_> {
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
        let return_ty = if assertion_return.is_some() {
            self.ctx.krate.types.intern(Type::None)
        } else {
            match function
                .return_type
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()
                .and_then(|value| {
                    value.ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(function.span.start, function.span.end),
                            "function declarations must have an explicit return type",
                        )
                    })
                })
            {
                Ok(value) => value,
                Err(error) => {
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
            }
        };
        if function.return_type.is_none() {
            self.pop_type_parameter_scope();
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "function declarations must have an explicit return type",
            ));
        }
        if function.r#async && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
        {
            self.pop_type_parameter_scope();
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "async functions must declare a Promise<T> return type",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_async = self.current_async;
        self.current_async = function.r#async;
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut params = Vec::new();

        for param in &function.params.items {
            let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                self.locals = saved_locals;
                self.current_async = saved_async;
                self.pop_type_parameter_scope();
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "destructured parameters are not lowered yet",
                ));
            };
            let ty = match param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()
                .and_then(|value| {
                    value.ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(param.span.start, param.span.end),
                            "function parameters must have explicit type annotations",
                        )
                    })
                })
            {
                Ok(value) => value,
                Err(error) => {
                    self.locals = saved_locals;
                    self.current_async = saved_async;
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
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
        }

        let mut errors = Vec::new();
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
        self.pop_type_parameter_scope();

        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let body_id = self.ctx.krate.push_body(body);
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name,
            span: self.span(function.span.start, function.span.end),
            params,
            return_ty,
            is_async: function.r#async,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        self.items.insert(name_text.to_owned(), item);
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
        Ok(item)
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
        if class.r#abstract {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "abstract classes are not lowered yet",
            ));
        }
        if class.type_parameters.is_some() {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "generic classes are not lowered yet",
            ));
        }
        if class.super_class.is_some() {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "class extends is not lowered yet",
            ));
        }

        let class_text = id.name.as_str();
        let class_name = self.intern_type_name(class_text);
        let class_ty = self.ctx.krate.types.intern(Type::Class {
            name: class_name,
            args: Vec::new(),
        });
        let mut fields = Vec::new();
        let mut constructor = None;
        let mut methods = Vec::new();

        for element in &class.body.body {
            if let ClassElement::PropertyDefinition(property) = element {
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
                if property.value.is_some() {
                    return Err(SmeltError::unsupported(
                        self.span(property.span.start, property.span.end),
                        "field initializers are not lowered yet",
                    ));
                }
                if property.optional {
                    return Err(SmeltError::unsupported(
                        self.span(property.span.start, property.span.end),
                        "optional class fields are not lowered yet",
                    ));
                }
                let name = self.property_key_symbol(&property.key)?;
                let ty = property
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "class fields require explicit type annotations",
                        )
                    })?;
                fields.push(Field {
                    name,
                    ty,
                    visibility: visibility(property.accessibility),
                    optional: false,
                    span: self.span(property.span.start, property.span.end),
                });
            }
        }
        self.class_fields
            .insert(class_text.to_owned(), fields.clone());

        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(_) => {}
                ClassElement::MethodDefinition(method) => {
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

        if !fields.is_empty() && constructor.is_none() {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "classes with required fields must declare a constructor",
            ));
        }

        let implements = class
            .implements
            .iter()
            .map(|imp| self.implements_symbol(imp))
            .collect::<Result<Vec<_>, _>>()?;
        let item = self.ctx.krate.push_item(Item::Class(Class {
            name: class_name,
            span: self.span(class.span.start, class.span.end),
            kind: smelt_hir::ClassKind::Plain,
            base: None,
            fields,
            constructor,
            methods,
            implements,
        }));
        self.classes.insert(class_text.to_owned(), item);
        self.validate_implements(item)?;
        Ok(item)
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
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(method.span.start, method.span.end),
                        "methods require explicit return types",
                    )
                })?
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
        self.current_async = method.value.r#async;
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

        for param in &method.value.params.items {
            let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                self.locals = saved_locals;
                self.current_class = saved_class;
                self.current_async = saved_async;
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "destructured parameters are not lowered yet",
                ));
            };
            let ty = param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "method parameters must have explicit type annotations",
                    )
                })?;
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
        }

        let mut errors = Vec::new();
        for statement in &function_body.statements {
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

    /// Lower a statement in the current scope.
    fn statement(&mut self, statement: &Statement<'_>, body: &mut Body) -> Result<(), SmeltError> {
        self.statement_in_block(statement, body, body.root)
    }

    // Continued in the next split builder file.
}
