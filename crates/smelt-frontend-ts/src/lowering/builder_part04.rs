impl ModuleBuilder<'_> {
    /// Prefix a local type declaration with the active TypeScript namespace path.
    fn qualified_type_declaration_name(&self, name: &str) -> String {
        if self.type_namespace_prefix.is_empty() {
            return name.to_owned();
        }
        format!("{}.{}", self.type_namespace_prefix.join("."), name)
    }

    /// Lower a TypeScript type alias declaration to HIR.
    fn type_alias_declaration(
        &mut self,
        alias: &oxc::ast::ast::TSTypeAliasDeclaration<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let local_name_text = alias.id.name.as_str();
        let name_text = self.qualified_type_declaration_name(local_name_text);
        let name = self.intern_type_name(&name_text);
        let type_params = self.push_type_parameter_scope(alias.type_parameters.as_deref())?;
        let result = self.ts_type_to_hir(&alias.type_annotation);
        let fields = self.type_fields_from_ts(&alias.type_annotation).ok();
        let is_callable_object = Self::ts_type_is_callable_object_surface(&alias.type_annotation);
        self.pop_type_parameter_scope();
        let ty = result?;
        if is_callable_object {
            self.callable_object_aliases.insert(name);
            self.ctx.callable_object_aliases.insert(name);
        }
        if let Some(fields) = fields
            && !fields.is_empty()
        {
            self.type_alias_fields.insert(name, fields.clone());
            self.ctx.type_alias_fields.insert(name, fields);
        }
        let item = self.ctx.krate.push_item(Item::TypeAlias(smelt_hir::TypeAlias {
            name,
            type_params,
            ty,
            span: self.span(alias.span.start, alias.span.end),
        }));
        self.items.insert(name_text, item);
        Ok(item)
    }

    /// Lower a TypeScript interface declaration to HIR.
    fn interface_declaration(
        &mut self,
        interface: &oxc::ast::ast::TSInterfaceDeclaration<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let local_name_text = interface.id.name.as_str();
        let name_text = self.qualified_type_declaration_name(local_name_text);
        let name = self.intern_type_name(&name_text);
        let type_params = self.push_type_parameter_scope(interface.type_parameters.as_deref())?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut call_signatures = Vec::new();
        let mut index_value_ty = None;

        let mut heritage_refs = Vec::new();
        let result = (|| {
            for heritage in &interface.extends {
                let (parent_name, parent_args) = self.interface_heritage(heritage)?;
                if self.ctx.krate.symbols.get(parent_name) == Some("Date") {
                    continue;
                }
                heritage_refs.push(InterfaceHeritageRef {
                    parent: parent_name,
                    args: parent_args.clone(),
                });
                let Some(parent) = self.find_interface(parent_name).cloned() else {
                    // An extended name that is not a lowerable user interface
                    // resolves instead to a type alias, a `typeof`/namespace or
                    // value import, a dotted/qualified library type, or a global
                    // ambient lib type such as `Array`/`ArrayLike`. None of these
                    // can contribute structural fields here, but TypeScript has
                    // already validated the heritage, so the child interface
                    // keeps its own members and the parent is treated as an
                    // opaque base rather than blocking the whole file.
                    continue;
                };
                let substitutions = self.type_argument_substitution(
                    &parent.type_params,
                    &parent_args,
                    self.span(heritage.span.start, heritage.span.end),
                )?;
                fields.extend(self.substituted_fields(&parent.fields, &substitutions));
                methods.extend(self.substituted_methods(&parent.methods, &substitutions));
            }

            for sig in &interface.body.body {
                match sig {
                    TSSignature::TSPropertySignature(prop) => {
                        if prop.computed && !is_static_property_key(&prop.key) {
                            continue;
                        }
                        let ty = prop
                            .type_annotation
                            .as_ref()
                            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                            .transpose()?
                            .ok_or_else(|| {
                                SmeltError::unsupported(
                                    self.span(prop.span.start, prop.span.end),
                                    "interface fields require explicit type annotations",
                                )
                            })?;
                        let field_ty = if prop.optional {
                            self.ctx.krate.types.intern(Type::Optional(ty))
                        } else {
                            ty
                        };
                        fields.push(Field {
                            name: self.property_key_symbol(&prop.key)?,
                            ty: field_ty,
                            visibility: Visibility::Public,
                            optional: prop.optional,
                            span: self.span(prop.span.start, prop.span.end),
                        });
                    }
                    TSSignature::TSMethodSignature(method) => {
                        if (method.computed && !is_static_property_key(&method.key))
                            || method.this_param.is_some()
                        {
                            return Err(SmeltError::unsupported(
                                self.span(method.span.start, method.span.end),
                                "dynamic computed and this-parameter interface methods are not lowered yet",
                            ));
                        }
                        let _method_type_params =
                            self.push_type_parameter_scope(method.type_parameters.as_deref())?;
                        let result = (|| {
                            let return_ty = method
                                .return_type
                                .as_ref()
                                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                                .transpose()?
                                .ok_or_else(|| {
                                    SmeltError::unsupported(
                                        self.span(method.span.start, method.span.end),
                                        "interface methods require explicit return types",
                                    )
                                })?;
                            let mut params = Vec::new();
                            for (index, param) in method.params.items.iter().enumerate() {
                                let ty = param
                                    .type_annotation
                                    .as_ref()
                                    .map(|annotation| {
                                        self.ts_type_to_hir(&annotation.type_annotation)
                                    })
                                    .transpose()?
                                    .ok_or_else(|| {
                                        SmeltError::unsupported(
                                            self.span(param.span.start, param.span.end),
                                            "interface method parameters require explicit types",
                                        )
                                    })?;
                                let (param_name, param_span) =
                                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                                        (
                                            self.intern_source_name(binding.name.as_str()),
                                            self.span(binding.span.start, binding.span.end),
                                        )
                                    } else {
                                        (
                                            self.synthetic_param_symbol(index),
                                            self.span(param.span.start, param.span.end),
                                        )
                                    };
                                params.push(ParamSig {
                                    name: param_name,
                                    ty,
                                    span: param_span,
                                });
                            }
                            Ok((return_ty, params))
                        })();
                        self.pop_type_parameter_scope();
                        let (return_ty, params) = result?;
                        if method.optional {
                            let param_tys = params.iter().map(|param| param.ty).collect::<Vec<_>>();
                            let mutable_params =
                                self.mutable_params_from_returned_tuple_state(&param_tys, return_ty);
                            let function_ty = self.ctx.krate.types.intern(Type::Function(
                                FunctionType {
                                    params: param_tys,
                                    rest: None,
                                    required_params: None,
                                    mutable_params,
                                    return_ty,
                                    is_async: matches!(
                                        self.ctx.krate.types.get(return_ty),
                                        Some(Type::Future(_))
                                    ),
                                    may_throw: false,
                                },
                            ));
                            fields.push(Field {
                                name: self.property_key_symbol(&method.key)?,
                                ty: function_ty,
                                visibility: Visibility::Public,
                                optional: true,
                                span: self.span(method.span.start, method.span.end),
                            });
                            continue;
                        }
                        methods.push(MethodSig {
                            name: self.property_key_symbol(&method.key)?,
                            params,
            rest: None,
                            required_params: None,
return_ty,
                            visibility: Visibility::Public,
                            is_async: matches!(
                                self.ctx.krate.types.get(return_ty),
                                Some(Type::Future(_))
                            ),
                            span: self.span(method.span.start, method.span.end),
                        });
                    }
                    TSSignature::TSCallSignatureDeclaration(signature) => {
                        let return_ty = signature
                            .return_type
                            .as_ref()
                            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                            .transpose()?
                            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                        let mut params = Vec::new();
                        for param in &signature.params.items {
                            let ty = param
                                .type_annotation
                                .as_ref()
                                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                                .transpose()?
                                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                            params.push(ty);
                        }
                        call_signatures.push(FunctionType {
                            mutable_params: self
                                .mutable_params_from_returned_tuple_state(&params, return_ty),
                            params,
                            rest: None,
                            required_params: None,
                            return_ty,
                            is_async: matches!(
                                self.ctx.krate.types.get(return_ty),
                                Some(Type::Future(_))
                            ),
                            may_throw: false,
                        });
                    }
                    TSSignature::TSIndexSignature(index) => {
                        index_value_ty =
                            Some(self.ts_type_to_hir(&index.type_annotation.type_annotation)?);
                    }
                    TSSignature::TSConstructSignatureDeclaration(_) => {}
                }
            }
            Ok(())
        })();
        self.pop_type_parameter_scope();
        result?;
        let item = self.ctx.krate.push_item(Item::Interface(Interface {
            name,
            span: self.span(interface.span.start, interface.span.end),
            type_params,
            extends: heritage_refs
                .iter()
                .map(|heritage| smelt_hir::InterfaceHeritage {
                    parent: heritage.parent,
                    args: heritage.args.clone(),
                })
                .collect(),
            fields,
            methods,
        }));
        self.interface_extends
            .insert(name, heritage_refs.clone());
        self.ctx.interface_extends.insert(name, heritage_refs);
        self.interface_call_signatures
            .insert(name, call_signatures.clone());
        self.ctx
            .interface_call_signatures
            .insert(name, call_signatures);
        if let Some(index_value_ty) = index_value_ty {
            self.interface_index_values.insert(name, index_value_ty);
            self.ctx.interface_index_values.insert(name, index_value_ty);
        }
        self.interfaces.insert(name_text, item);
        Ok(item)
    }

    /// Lower TypeScript namespace declarations that contain exported type declarations.
    fn type_namespace_declaration(
        &mut self,
        module_decl: &oxc::ast::ast::TSModuleDeclaration<'_>,
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let Some(namespace_name) = Self::type_namespace_name(&module_decl.id) else {
            return Ok(Vec::new());
        };
        self.type_namespace_prefix.push(namespace_name);
        let result = self.type_namespace_body(module_decl.body.as_ref());
        self.type_namespace_prefix.pop();
        result
    }

    /// Return the source namespace identifier for namespace declarations.
    fn type_namespace_name(name: &TSModuleDeclarationName<'_>) -> Option<String> {
        match name {
            TSModuleDeclarationName::Identifier(ident) => Some(ident.name.to_string()),
            TSModuleDeclarationName::StringLiteral(_) => None,
        }
    }

    /// Lower exported type declarations from a namespace body.
    fn type_namespace_body(
        &mut self,
        body: Option<&TSModuleDeclarationBody<'_>>,
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let Some(body) = body else {
            return Ok(Vec::new());
        };
        match body {
            TSModuleDeclarationBody::TSModuleDeclaration(module_decl) => {
                self.type_namespace_declaration(module_decl)
            }
            TSModuleDeclarationBody::TSModuleBlock(block) => {
                let mut items = Vec::new();
                for statement in &block.body {
                    match statement {
                        Statement::TSTypeAliasDeclaration(alias) => {
                            items.push(self.type_alias_declaration(alias)?);
                        }
                        Statement::TSInterfaceDeclaration(interface) => {
                            items.push(self.interface_declaration(interface)?);
                        }
                        Statement::TSModuleDeclaration(module_decl) => {
                            items.extend(self.type_namespace_declaration(module_decl)?);
                        }
                        Statement::ExportNamedDeclaration(export) => {
                            let Some(decl) = &export.declaration else {
                                continue;
                            };
                            match decl {
                                Declaration::TSTypeAliasDeclaration(alias) => {
                                    items.push(self.type_alias_declaration(alias)?);
                                }
                                Declaration::TSInterfaceDeclaration(interface) => {
                                    items.push(self.interface_declaration(interface)?);
                                }
                                Declaration::TSModuleDeclaration(module_decl) => {
                                    items.extend(self.type_namespace_declaration(module_decl)?);
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                Ok(items)
            }
        }
    }

    /// Lower a statement within a specific block.
    fn statement_in_block(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let previous_statement_block = self.current_statement_block.replace(block);
        let result = match statement {
            Statement::VariableDeclaration(decl) => self.variable_declaration(decl, body, block),
            Statement::FunctionDeclaration(function) => {
                self.local_function_declaration(function, body, block)
            }
            Statement::ClassDeclaration(class) => {
                self.class_declaration(class)?;
                Ok(())
            }
            Statement::TSTypeAliasDeclaration(alias) => {
                self.type_alias_declaration(alias)?;
                Ok(())
            }
            Statement::TSInterfaceDeclaration(interface) => {
                self.interface_declaration(interface)?;
                Ok(())
            }
            Statement::TSModuleDeclaration(_) => Ok(()),
            Statement::ExpressionStatement(expr_stmt) => {
                // `Foo.prototype.x = …` assignments for a constructor function
                // were folded into the synthesized class during the block
                // prepass, so the assignment statement itself is dropped.
                if self.is_synthesized_prototype_assignment(&expr_stmt.expression) {
                    return Ok(());
                }
                if self.inline_runtime_lifecycle_setup(&expr_stmt.expression, body, block)? {
                    return Ok(());
                }
                if self.is_test_framework_statement(&expr_stmt.expression) {
                    return Ok(());
                }
                if Self::is_vitest_mock_statement(&expr_stmt.expression)
                    || Self::is_top_level_dynamic_import_await(&expr_stmt.expression)
                {
                    return Ok(());
                }
                if let Expression::CallExpression(call) = &expr_stmt.expression
                    && self.for_each_statement(call, body, block)?
                {
                    return Ok(());
                }
                if let Expression::CallExpression(call) = &expr_stmt.expression
                    && self.expect_matcher_statement(call, body)?
                {
                    return Ok(());
                }
                if let Expression::CallExpression(call) = &expr_stmt.expression
                    && self.node_assert_statement(call, body)?
                {
                    return Ok(());
                }
                if let Expression::AssignmentExpression(assign) = &expr_stmt.expression {
                    if block == body.root
                        && self.module_global_assignment_statement(assign, body, block)?
                    {
                        return Ok(());
                    }
                    if self.array_destructuring_assignment_statement(assign, body, block)? {
                        return Ok(());
                    }
                    let (target, value) = self.assignment_parts(assign, body)?;
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                    return Ok(());
                }
                if let Expression::UpdateExpression(update) = &expr_stmt.expression {
                    let (target, value) = self.update_parts(update, body)?;
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                    return Ok(());
                }
                if let Expression::YieldExpression(yield_expr) = &expr_stmt.expression
                    && self.generator_yield_statement(yield_expr, body, block)?
                {
                    return Ok(());
                }
                let assertion_narrowing = self.assertion_call_narrowing(&expr_stmt.expression);
                let expr = self.expression(&expr_stmt.expression, body)?;
                let expr = if matches!(self.ctx.krate.types.get(Self::expr_ty(body, expr)), Some(Type::Future(_))) {
                    let none_ty = self.ctx.krate.types.intern(Type::None);
                    body.push_expr(Expr {
                        kind: ExprKind::AsyncOp {
                            op: AsyncOp::SpawnLocal,
                            args: vec![expr],
                        },
                        ty: none_ty,
                        span: self.span(expr_stmt.span.start, expr_stmt.span.end),
                    })
                } else {
                    expr
                };
                body.push_stmt_to_block(block, Stmt::Expr(expr));
                if let Some((name, target)) = assertion_narrowing {
                    self.apply_narrowing(name, target);
                }
                Ok(())
            }
            Statement::ReturnStatement(return_stmt) => {
                let value = return_stmt
                    .argument
                    .as_ref()
                    .map(|argument| self.expression_with_hint(argument, body, self.current_return_ty))
                    .transpose()?;
                body.push_stmt_to_block(block, Stmt::Return(value));
                Ok(())
            }
            Statement::IfStatement(if_stmt) => {
                let cond = self.condition_expression(&if_stmt.test, body)?;
                let then_narrowing = self.guard_narrowing(&if_stmt.test, body);
                // Assignments performed only within this branch are flow facts
                // for the branch, not for statements reached from either path.
                self.narrowed_locals
                    .push(then_narrowing.unwrap_or_default());
                let then_block = self.block_from_statement(&if_stmt.consequent, body)?;
                self.narrowed_locals.pop();
                let else_narrowing = self.inverse_guard_narrowing(&if_stmt.test, body);
                let else_block = if let Some(alternate) = &if_stmt.alternate {
                    self.narrowed_locals
                        .push(else_narrowing.unwrap_or_default());
                    let else_block = self.block_from_statement(alternate, body)?;
                    self.narrowed_locals.pop();
                    Some(else_block)
                } else {
                    None
                };
                body.push_stmt_to_block(
                    block,
                    Stmt::If {
                        cond,
                        then_block,
                        else_block,
                    },
                );
                if if_stmt.alternate.is_none()
                    && Self::statement_must_exit(&if_stmt.consequent)
                    && let Some(narrowing) = self.inverse_guard_narrowing(&if_stmt.test, body)
                {
                    for (name, target) in narrowing {
                        self.apply_narrowing(name, target);
                    }
                }
                Ok(())
            }
            Statement::WhileStatement(while_stmt) => {
                if let Some((cond, loop_body, update_target, update_value)) =
                    self.while_assignment_condition_body(while_stmt, body, block)?
                {
                    body.push_stmt_to_block(
                        block,
                        Stmt::WhileUpdate {
                            cond,
                            body: loop_body,
                            update_target,
                            update_value,
                        },
                    );
                    return Ok(());
                }
                let cond = self.condition_expression(&while_stmt.test, body)?;
                let loop_body = self.block_from_statement(&while_stmt.body, body)?;
                body.push_stmt_to_block(
                    block,
                    Stmt::While {
                        cond,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::DoWhileStatement(do_while) => {
                let loop_body = self.block_from_statement(&do_while.body, body)?;
                let cond = self.condition_expression(&do_while.test, body)?;
                let negated = body.push_expr(Expr {
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::Not,
                        operand: cond,
                    },
                    ty: self.ctx.krate.types.intern(Type::Bool),
                    span: self.span(do_while.test.span().start, do_while.test.span().end),
                });
                let break_block = body.push_block(self.span(do_while.span.start, do_while.span.end));
                body.push_stmt_to_block(break_block, Stmt::Break);
                body.push_stmt_to_block(
                    loop_body,
                    Stmt::If {
                        cond: negated,
                        then_block: break_block,
                        else_block: None,
                    },
                );
                let true_expr = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(true)),
                    ty: self.ctx.krate.types.intern(Type::Bool),
                    span: self.span(do_while.span.start, do_while.span.end),
                });
                body.push_stmt_to_block(
                    block,
                    Stmt::While {
                        cond: true_expr,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::ForOfStatement(for_stmt) => {
                let saved_locals = self.locals.clone();
                let mut iter = self.expression(&for_stmt.right, body)?;
                if for_stmt.r#await
                    && let Some(Type::Future(inner)) =
                        self.ctx.krate.types.get(Self::expr_ty(body, iter)).cloned()
                {
                    iter = body.push_expr(Expr {
                        kind: ExprKind::Await(iter),
                        ty: inner,
                        span: self.span(for_stmt.right.span().start, for_stmt.right.span().end),
                    });
                }
                let iter = self.for_of_iterable(iter, &for_stmt.right, body);
                let destructured =
                    self.for_left_destructuring(&for_stmt.left, Self::expr_ty(body, iter), body)?;
                let (pat, loop_body) =
                    if let Some((pat, value, binding, annotated_ty, mutable)) = destructured {
                        let loop_body = body.push_block(self.statement_span(&for_stmt.body));
                        self.binding_declaration(
                            binding,
                            Some(value),
                            annotated_ty,
                            mutable,
                            body,
                            loop_body,
                        )?;
                        if let Statement::BlockStatement(block_stmt) = &for_stmt.body {
                            for nested_statement in &block_stmt.body {
                                self.statement_in_block(nested_statement, body, loop_body)?;
                            }
                        } else {
                            self.statement_in_block(&for_stmt.body, body, loop_body)?;
                        }
                        (pat, loop_body)
                    } else {
                        (
                            self.for_left_pattern(
                                &for_stmt.left,
                                Self::expr_ty(body, iter),
                                body,
                            )?,
                            self.block_from_statement(&for_stmt.body, body)?,
                        )
                    };
                self.locals = saved_locals;
                body.push_stmt_to_block(
                    block,
                    Stmt::For {
                        pat,
                        iter,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::ForInStatement(for_stmt) => {
                let saved_locals = self.locals.clone();
                let iter = self.for_in_iterable(&for_stmt.right, body)?;
                let destructured =
                    self.for_left_destructuring(&for_stmt.left, Self::expr_ty(body, iter), body)?;
                let (pat, loop_body) =
                    if let Some((pat, value, binding, annotated_ty, mutable)) = destructured {
                        let loop_body = body.push_block(self.statement_span(&for_stmt.body));
                        self.binding_declaration(
                            binding,
                            Some(value),
                            annotated_ty,
                            mutable,
                            body,
                            loop_body,
                        )?;
                        if let Statement::BlockStatement(block_stmt) = &for_stmt.body {
                            for nested_statement in &block_stmt.body {
                                self.statement_in_block(nested_statement, body, loop_body)?;
                            }
                        } else {
                            self.statement_in_block(&for_stmt.body, body, loop_body)?;
                        }
                        (pat, loop_body)
                    } else {
                        (
                            self.for_left_pattern(
                                &for_stmt.left,
                                Self::expr_ty(body, iter),
                                body,
                            )?,
                            self.block_from_statement(&for_stmt.body, body)?,
                        )
                    };
                self.locals = saved_locals;
                body.push_stmt_to_block(
                    block,
                    Stmt::For {
                        pat,
                        iter,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::ForStatement(for_stmt) => self.c_for_statement(for_stmt, body, block),
            Statement::SwitchStatement(switch_stmt) => {
                let scrutinee = self.expression(&switch_stmt.discriminant, body)?;
                let mut arms = Vec::new();
                let mut default = None;
                let mut pending_empty_labels = Vec::new();

                let case_count = switch_stmt.cases.len();
                for (case_index, case) in switch_stmt.cases.iter().enumerate() {
                    if case.consequent.is_empty() {
                        if let Some(test) = &case.test {
                            pending_empty_labels.push(self.literal_case_label(test)?);
                            continue;
                        }
                    }
                    let case_block = body.push_block(self.span(case.span.start, case.span.end));
                    let mut saw_break = false;
                    for case_statement in &case.consequent {
                        if let Statement::BlockStatement(block_stmt) = case_statement {
                            for nested_statement in &block_stmt.body {
                                if matches!(nested_statement, Statement::ContinueStatement(_)) {
                                    return Err(SmeltError::unsupported(
                                        self.statement_span(nested_statement),
                                        "switch continue lowering is not implemented yet",
                                    ));
                                }
                                if matches!(nested_statement, Statement::BreakStatement(_)) {
                                    saw_break = true;
                                    break;
                                }
                                self.statement_in_block(nested_statement, body, case_block)?;
                            }
                            if saw_break {
                                break;
                            }
                            continue;
                        }
                        if matches!(case_statement, Statement::ContinueStatement(_)) {
                            return Err(SmeltError::unsupported(
                                self.statement_span(case_statement),
                                "switch continue lowering is not implemented yet",
                            ));
                        }
                        if matches!(case_statement, Statement::BreakStatement(_)) {
                            saw_break = true;
                            break;
                        }
                        self.statement_in_block(case_statement, body, case_block)?;
                    }
                    let is_last_case = case_index + 1 == case_count;
                    if !saw_break
                        && !is_last_case
                        && !case.consequent.iter().any(statement_terminates)
                    {
                        return Err(SmeltError::unsupported(
                            self.span(case.span.start, case.span.end),
                            "switch fallthrough is not lowered yet; each case must break, return, or throw",
                        ));
                    }

                    if let Some(test) = &case.test {
                        for label in std::mem::take(&mut pending_empty_labels) {
                            arms.push(MatchArm {
                                label,
                                body: case_block,
                            });
                        }
                        arms.push(MatchArm {
                            label: self.literal_case_label(test)?,
                            body: case_block,
                        });
                    } else if default.replace(case_block).is_some() {
                        return Err(SmeltError::unsupported(
                            self.span(case.span.start, case.span.end),
                            "switch statements can only have one default case",
                        ));
                    } else {
                        for label in std::mem::take(&mut pending_empty_labels) {
                            arms.push(MatchArm {
                                label,
                                body: case_block,
                            });
                        }
                    }
                }
                if !pending_empty_labels.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(switch_stmt.span.start, switch_stmt.span.end),
                        "switch fallthrough is not lowered yet; each case must break, return, or throw",
                    ));
                }

                body.push_stmt_to_block(
                    block,
                    Stmt::Match {
                        scrutinee,
                        arms,
                        default,
                    },
                );
                Ok(())
            }
            Statement::BreakStatement(break_stmt) => {
                if break_stmt.label.is_some() {
                    return Err(SmeltError::unsupported(
                        self.span(break_stmt.span.start, break_stmt.span.end),
                        "labeled break is not lowered yet",
                    ));
                }
                body.push_stmt_to_block(block, Stmt::Break);
                Ok(())
            }
            Statement::ContinueStatement(continue_stmt) => {
                if continue_stmt.label.is_some() {
                    return Err(SmeltError::unsupported(
                        self.span(continue_stmt.span.start, continue_stmt.span.end),
                        "labeled continue is not lowered yet",
                    ));
                }
                body.push_stmt_to_block(block, Stmt::Continue);
                Ok(())
            }
            Statement::ThrowStatement(throw_stmt) => {
                let expr = self.throw_message_expression(&throw_stmt.argument, body)?;
                body.push_stmt_to_block(block, Stmt::Throw(expr));
                Ok(())
            }
            Statement::TryStatement(try_stmt) => {
                let try_body = self.block_from_block_statement(&try_stmt.block, body)?;
                let (catch_binding, catch_body) = if let Some(handler) = &try_stmt.handler {
                    let previous_locals = self.locals.clone();
                    let catch_binding = handler
                        .param
                        .as_ref()
                        .map(|param| self.catch_binding(param, body))
                        .transpose()?;
                    let catch_body = self.block_from_block_statement(&handler.body, body)?;
                    self.locals = previous_locals;
                    (catch_binding, Some(catch_body))
                } else {
                    (None, None)
                };
                let finally_body = try_stmt
                    .finalizer
                    .as_ref()
                    .map(|finalizer| self.block_from_block_statement(finalizer, body))
                    .transpose()?;

                body.push_stmt_to_block(
                    block,
                    Stmt::TryCatch {
                        body: try_body,
                        catch_binding,
                        catch_body,
                        finally_body,
                    },
                );
                Ok(())
            }
            Statement::BlockStatement(block_stmt) => {
                for child in &block_stmt.body {
                    self.statement_in_block(child, body, block)?;
                }
                Ok(())
            }
            _ => Err(SmeltError::unsupported(
                self.statement_span(statement),
                format!("statement kind is not lowered yet: {statement:?}"),
            )),
        };
        self.current_statement_block = previous_statement_block;
        result
    }

    /// Lower side-effecting `array.forEach((item) => { ... })` as a normal loop.
    fn for_each_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(false);
        };
        if member.property.name != "forEach" {
            return Ok(false);
        }
        let [Argument::ArrowFunctionExpression(arrow)] = call.arguments.as_slice() else {
            return Ok(false);
        };
        let Some(item_param) = arrow.params.items.first() else {
            // A `forEach` whose callback has no fixed item parameter — a bare
            // `() => ...` side effect or a rest-only `(...args) => ...` collector
            // — is not modeled by this statement-loop shortcut. Decline so the
            // general callback lowering (which supports rest parameters through
            // the closure-body path) handles it instead of failing here.
            return Ok(false);
        };
        let mut iter = self.expression(&member.object, body)?;
        let iter_ty = Self::expr_ty(body, iter);
        let item_ty = match self.ctx.krate.types.get(iter_ty).cloned() {
            Some(Type::List(item_ty)) => item_ty,
            Some(Type::Dict(_, value_ty)) => {
                let list_ty = self.ctx.krate.types.intern(Type::List(value_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::DictProjection {
                        op: DictProjectionOp::Values,
                        dict: iter,
                    },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                value_ty
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: iter },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                item_ty
            }
            Some(Type::Union(items))
                if items.iter().any(|item| {
                    matches!(
                        self.ctx.krate.types.get(*item),
                        Some(Type::List(_) | Type::Unknown | Type::TypeParam { .. } | Type::Class { .. })
                    )
                }) =>
            {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: iter },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                item_ty
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(member.object.span().start, member.object.span().end),
                    "array forEach statement receiver must be an array",
                ));
            }
        };
        let item_binding = match &item_param.pattern {
            BindingPattern::BindingIdentifier(binding) => Some(binding),
            _ => None,
        };
        let item_symbol = if let Some(binding) = item_binding {
            self.intern_source_name(binding.name.as_str())
        } else {
            self.ctx.krate.symbols.intern("__for_each_item")
        };
        let item_local = body.push_local(LocalDecl {
            name: Some(item_symbol),
            ty: item_ty,
            mutable: false,
            span: self.span(item_param.span.start, item_param.span.end),
        });
        let saved_item_local = item_binding
            .map(|binding| self.locals.insert(binding.name.to_string(), item_local));
        let item_pat = body.push_pattern(Pattern::Binding(item_local));
        let index_binding = arrow
            .params
            .items
            .get(1)
            .map(|index_param| {
                let BindingPattern::BindingIdentifier(index_binding) = &index_param.pattern else {
                    return Err(SmeltError::unsupported(
                        self.span(index_param.span.start, index_param.span.end),
                        "array forEach statement index parameters must be identifiers",
                    ));
                };
                Ok(index_binding)
            })
            .transpose()?;
        let index_ty = self.ctx.krate.types.intern(Type::Float);
        let index_counter = index_binding.is_some().then(|| {
            let counter_symbol = self.ctx.krate.symbols.intern("__for_each_index");
            let counter_local = body.push_local(LocalDecl {
                name: Some(counter_symbol),
                ty: index_ty,
                mutable: true,
                span: self.span(call.span.start, call.span.end),
            });
            let zero = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: index_ty,
                span: self.span(call.span.start, call.span.end),
            });
            let counter_pat = body.push_pattern(Pattern::Binding(counter_local));
            body.push_stmt_to_block(block, Stmt::Let {
                pat: counter_pat,
                ty: index_ty,
                value: Some(zero),
            });
            counter_local
        });
        let loop_body = body.push_block(self.span(arrow.body.span.start, arrow.body.span.end));
        if item_binding.is_none() {
            let item_value = body.push_expr(Expr {
                kind: ExprKind::Local(item_local),
                ty: item_ty,
                span: self.span(item_param.span.start, item_param.span.end),
            });
            self.binding_declaration(
                &item_param.pattern,
                Some(item_value),
                Some(item_ty),
                false,
                body,
                loop_body,
            )?;
        }
        let saved_index_local =
            if let (Some(index_binding), Some(counter)) = (index_binding, index_counter) {
                let index_symbol = self.intern_source_name(index_binding.name.as_str());
                let index_local = body.push_local(LocalDecl {
                    name: Some(index_symbol),
                    ty: index_ty,
                    mutable: false,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let counter_value = body.push_expr(Expr {
                    kind: ExprKind::Local(counter),
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let index_pat = body.push_pattern(Pattern::Binding(index_local));
                body.push_stmt_to_block(loop_body, Stmt::Let {
                    pat: index_pat,
                    ty: index_ty,
                    value: Some(counter_value),
                });
                let one = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(1.0)),
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let next = body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Add,
                        lhs: counter_value,
                        rhs: one,
                    },
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let target = body.push_expr(Expr {
                    kind: ExprKind::Local(counter),
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                body.push_stmt_to_block(loop_body, Stmt::Assign {
                    target,
                    value: next,
                });
                self.locals
                    .insert(index_binding.name.to_string(), index_local)
            } else {
                None
            };
        for statement in &arrow.body.statements {
            self.for_each_callback_statement(statement, body, loop_body)?;
        }
        if let Some(index_binding) = index_binding {
            if let Some(prior) = saved_index_local {
                self.locals.insert(index_binding.name.to_string(), prior);
            } else {
                self.locals.remove(index_binding.name.as_str());
            }
        }
        if let Some(item_binding) = item_binding {
            if let Some(Some(prior)) = saved_item_local {
                self.locals.insert(item_binding.name.to_string(), prior);
            } else {
                self.locals.remove(item_binding.name.as_str());
            }
        }
        body.push_stmt_to_block(
            block,
            Stmt::For {
                pat: item_pat,
                iter,
                body: loop_body,
            },
        );
        Ok(true)
    }

    /// Lower a statement inside a `forEach` callback body.
    fn for_each_callback_statement(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let previous_statement_block = self.current_statement_block.replace(block);
        let result = match statement {
            Statement::ExpressionStatement(expr_stmt) => {
                if let Expression::AssignmentExpression(assign) = &expr_stmt.expression {
                    if self.array_destructuring_assignment_statement(assign, body, block)? {
                        return Ok(());
                    }
                    let (target, value) = self.assignment_parts(assign, body)?;
                    if let Some(local_decl) = usize::try_from(target.0)
                        .ok()
                        .and_then(|index| body.exprs.get(index))
                        .and_then(|expr| match expr.kind {
                            ExprKind::Local(local) => usize::try_from(local.0)
                                .ok()
                                .and_then(|index| body.locals.get(index)),
                            _ => None,
                        })
                        && !local_decl.mutable
                    {
                        return Err(SmeltError::unsupported(
                            self.span(assign.span.start, assign.span.end),
                            "callback assignment to captured const local is not supported",
                        ));
                    }
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                    Ok(())
                } else {
                    self.statement_in_block(statement, body, block)
                }
            }
            Statement::ReturnStatement(_) => {
                body.push_stmt_to_block(block, Stmt::Continue);
                Ok(())
            }
            Statement::BlockStatement(block_stmt) => {
                for child in &block_stmt.body {
                    self.for_each_callback_statement(child, body, block)?;
                }
                Ok(())
            }
            Statement::IfStatement(if_stmt) => {
                let cond = self.expression(&if_stmt.test, body)?;
                let then_block = body.push_block(self.statement_span(&if_stmt.consequent));
                self.for_each_callback_statement(&if_stmt.consequent, body, then_block)?;
                let else_block = if_stmt
                    .alternate
                    .as_ref()
                    .map(|alternate| {
                        let else_block = body.push_block(self.statement_span(alternate));
                        self.for_each_callback_statement(alternate, body, else_block)?;
                        Ok(else_block)
                    })
                    .transpose()?;
                body.push_stmt_to_block(
                    block,
                    Stmt::If {
                        cond,
                        then_block,
                        else_block,
                    },
                );
                Ok(())
            }
            _ => self.statement_in_block(statement, body, block),
        };
        self.current_statement_block = previous_statement_block;
        result
    }

    /// Append a `yield` statement value to the active generator accumulator.
    fn generator_yield_statement(
        &mut self,
        yield_expr: &oxc::ast::ast::YieldExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Some(accumulator) = self.current_generator_yields else {
            return Ok(false);
        };
        if yield_expr.delegate {
            return Err(SmeltError::unsupported(
                self.span(yield_expr.span.start, yield_expr.span.end),
                "yield* generator delegation is not lowered yet",
            ));
        }
        let value = if let Some(argument) = &yield_expr.argument {
            self.expression(argument, body)?
        } else {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: none_ty,
                span: self.span(yield_expr.span.start, yield_expr.span.end),
            })
        };
        let item = body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value,
                target: accumulator.item_ty,
            },
            ty: accumulator.item_ty,
            span: self.span(yield_expr.span.start, yield_expr.span.end),
        });
        let list = body.push_expr(Expr {
            kind: ExprKind::Local(accumulator.local),
            ty: accumulator.list_ty,
            span: self.span(yield_expr.span.start, yield_expr.span.end),
        });
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        let push = body.push_expr(Expr {
            kind: ExprKind::ListPush { list, item },
            ty: number_ty,
            span: self.span(yield_expr.span.start, yield_expr.span.end),
        });
        body.push_stmt_to_block(block, Stmt::Expr(push));
        Ok(true)
    }

    /// Lower writes to known module-level variables without requiring a local target.
    fn module_global_assignment_statement(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left else {
            return Ok(false);
        };
        if assign.operator != AssignmentOperator::Assign {
            return Ok(false);
        }
        if !self.module_globals.contains_key(target.name.as_str()) {
            return Ok(false);
        }
        let value = self.expression(&assign.right, body)?;
        body.push_stmt_to_block(block, Stmt::Expr(value));
        Ok(true)
    }

    /// Lower `while ((target = value) !== null)` without dropping the assignment.
    fn while_assignment_condition_body(
        &mut self,
        while_stmt: &oxc::ast::ast::WhileStatement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<
        Option<(
            smelt_hir::ExprId,
            smelt_hir::BlockId,
            smelt_hir::ExprId,
            smelt_hir::ExprId,
        )>,
        SmeltError,
    > {
        let test = Self::unparenthesized_expression(&while_stmt.test);
        let Expression::BinaryExpression(binary) = test else {
            return Ok(None);
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ) || !matches!(&binary.right, Expression::NullLiteral(_))
        {
            return Ok(None);
        }
        let left = Self::unparenthesized_expression(&binary.left);
        let Expression::AssignmentExpression(assign) = left else {
            return Ok(None);
        };
        let (target, value) = self.assignment_parts(assign, body)?;
        body.push_stmt_to_block(block, Stmt::Assign { target, value });
        let null_expr = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: self.ctx.krate.types.intern(Type::None),
            span: self.span(binary.right.span().start, binary.right.span().end),
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: target,
                rhs: null_expr,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(binary.span.start, binary.span.end),
        });
        let loop_body = body.push_block(self.statement_span(&while_stmt.body));
        if let Statement::BlockStatement(block_stmt) = &while_stmt.body {
            for statement in &block_stmt.body {
                self.statement_in_block(statement, body, loop_body)?;
            }
        } else {
            self.statement_in_block(&while_stmt.body, body, loop_body)?;
        }
        let (update_target, update_value) = self.assignment_parts(assign, body)?;
        Ok(Some((cond, loop_body, update_target, update_value)))
    }

    /// Strip transparent parentheses from a TypeScript expression.
    fn unparenthesized_expression<'a>(expression: &'a Expression<'a>) -> &'a Expression<'a> {
        let mut current = expression;
        while let Expression::ParenthesizedExpression(parenthesized) = current {
            current = &parenthesized.expression;
        }
        current
    }

    /// Return whether an expression is a top-level Vitest organization call.
    fn is_test_framework_statement(&self, expression: &Expression<'_>) -> bool {
        if self.table_test_call(expression).is_some() {
            return true;
        }
        if self.property_test_call(expression).is_some() {
            return true;
        }
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        self.is_test_framework_callee(&call.callee)
    }

    /// Return whether this is a top-level `vi.mock(...)` registration.
    fn is_vitest_mock_statement(expression: &Expression<'_>) -> bool {
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return false;
        };
        member.property.name == "mock"
            && matches!(&member.object, Expression::Identifier(object) if object.name == "vi")
    }

    /// Return whether this is a top-level `await import("...")` side-effect load.
    fn is_top_level_dynamic_import_await(expression: &Expression<'_>) -> bool {
        let Expression::AwaitExpression(await_expr) = expression else {
            return false;
        };
        matches!(&await_expr.argument, Expression::ImportExpression(_))
    }

    /// Return a supported top-level test case call, if this expression is one.
    pub(super) fn test_case_call<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        let name = callee.name.as_str();
        (self.test_builtins.contains(name) && matches!(name, "it" | "test")).then_some(call)
    }

    /// Return whether an expression is a skipped Vitest test case.
    pub(super) fn skipped_test_case_call(&self, expression: &Expression<'_>) -> bool {
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return false;
        };
        if member.property.name != "skip" {
            return false;
        }
        matches!(
            &member.object,
            Expression::Identifier(object)
                if self.test_builtins.contains(object.name.as_str())
                    && matches!(object.name.as_str(), "it" | "test")
        )
    }

    /// Return whether a suite-level condition is known false for native Rust tests.
    pub(super) fn describe_condition_is_native_false(expression: &Expression<'_>) -> bool {
        Self::typeof_window_undefined_comparison(expression, false)
    }

    /// Return whether a suite-level condition is known true for native Rust tests.
    pub(super) fn describe_condition_is_native_true(expression: &Expression<'_>) -> bool {
        Self::typeof_window_undefined_comparison(expression, true)
    }

    /// Evaluate `typeof window ===/!== "undefined"` for the Rust test target.
    fn typeof_window_undefined_comparison(
        expression: &Expression<'_>,
        want_equal: bool,
    ) -> bool {
        let Expression::BinaryExpression(binary) = expression else {
            return false;
        };
        let is_equal = match binary.operator {
            BinaryOperator::StrictEquality | BinaryOperator::Equality => true,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality => false,
            _ => return false,
        };
        if is_equal != want_equal {
            return false;
        }
        let left_typeof_window = Self::is_typeof_window(&binary.left);
        let right_typeof_window = Self::is_typeof_window(&binary.right);
        matches!(
            (left_typeof_window, &binary.right, right_typeof_window, &binary.left),
            (true, Expression::StringLiteral(value), _, _) if value.value == "undefined"
        ) || matches!(
            (left_typeof_window, &binary.right, right_typeof_window, &binary.left),
            (_, _, true, Expression::StringLiteral(value)) if value.value == "undefined"
        )
    }

    /// Return whether an expression is `typeof window`.
    fn is_typeof_window(expression: &Expression<'_>) -> bool {
        let Expression::UnaryExpression(unary) = expression else {
            return false;
        };
        if unary.operator != UnaryOperator::Typeof {
            return false;
        }
        matches!(&unary.argument, Expression::Identifier(identifier) if identifier.name == "window")
    }

    /// Return a supported `test.each(...)` or `describe.each(...)` outer call.
    fn table_test_call<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        let call = match expression {
            Expression::CallExpression(call) => call,
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::CallExpression(call) => call,
                _ => return None,
            },
            _ => return None,
        };
        self.table_each_callee(&call.callee).then_some(call)
    }

    /// Return whether a callee is the invoked result of `.each(...)`.
    fn table_each_callee(&self, callee: &Expression<'_>) -> bool {
        let Expression::CallExpression(each_call) = callee else {
            return false;
        };
        let Expression::StaticMemberExpression(member) = &each_call.callee else {
            return false;
        };
        if member.property.name != "each" {
            return false;
        }
        matches!(
            &member.object,
            Expression::Identifier(object)
                if self.test_builtins.contains(object.name.as_str())
                    && matches!(object.name.as_str(), "test" | "it" | "describe")
        )
    }

    /// Return a supported `test.prop(...)` or `it.prop(...)` property-test call.
    fn property_test_call<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        let Expression::CallExpression(prop_call) = &call.callee else {
            return None;
        };
        let Expression::StaticMemberExpression(member) = &prop_call.callee else {
            return None;
        };
        if member.property.name != "prop" {
            return None;
        }
        matches!(
            &member.object,
            Expression::Identifier(object)
                if self.test_builtins.contains(object.name.as_str())
                    && matches!(object.name.as_str(), "test" | "it")
        )
        .then_some(call)
    }

    /// Return a supported top-level `describe` call, if this expression is one.
    fn describe_call<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        self.is_describe_callee(&call.callee).then_some(call)
    }

    /// Collect a test lifecycle callback that should wrap flattened tests.
    ///
    /// Smelt emits one Rust test per supported Vitest test case, so suite-level
    /// hooks are inherited into each flattened test body in declaration order.
    fn collect_lifecycle_hook<'a>(
        &self,
        expression: &'a Expression<'a>,
        before_each: &mut Vec<&'a oxc::ast::ast::ArrowFunctionExpression<'a>>,
        after_each: &mut Vec<&'a oxc::ast::ast::ArrowFunctionExpression<'a>>,
    ) -> bool {
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return false;
        };
        let name = callee.name.as_str();
        if !self.test_builtins.contains(name)
            || !matches!(name, "beforeAll" | "beforeEach" | "afterAll" | "afterEach")
        {
            return false;
        }
        let Some(callback_arg) = call.arguments.first() else {
            return false;
        };
        let Ok(callback) = self.test_arrow_callback(callback_arg, "lifecycle callbacks") else {
            return false;
        };
        if matches!(name, "beforeAll" | "beforeEach") {
            before_each.push(callback);
        } else {
            after_each.push(callback);
        }
        true
    }

    /// Inline setup hooks that are invoked from suite setup helpers.
    ///
    /// Suite expressions are copied into every native Rust test body. If such a
    /// helper registers `beforeEach` or `beforeAll`, executing its callback at
    /// that copied call site gives the same setup ordering for the current test.
    /// Teardown hooks remain handled by ordinary suite collection or by the
    /// per-test reset emitted for mutable Vitest runtime state.
    fn inline_runtime_lifecycle_setup(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Expression::CallExpression(call) = expression else {
            return Ok(false);
        };
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(false);
        };
        if !self.test_builtins.contains(callee.name.as_str())
            || !matches!(callee.name.as_str(), "beforeAll" | "beforeEach")
        {
            return Ok(false);
        }
        let Some(callback_arg) = call.arguments.first() else {
            return Ok(false);
        };
        let callback = self.test_arrow_callback(callback_arg, "lifecycle callbacks")?;
        for statement in &callback.body.statements {
            self.statement_in_block(statement, body, block)?;
        }
        Ok(true)
    }

    /// Return whether a callee belongs to an imported test-framework API.
    fn is_test_framework_callee(&self, callee: &Expression<'_>) -> bool {
        match callee {
            Expression::Identifier(ident) => self.test_builtins.contains(ident.name.as_str()),
            Expression::CallExpression(call) => self.is_test_framework_callee(&call.callee),
            Expression::StaticMemberExpression(member)
                if member.property.name == "concurrent"
                    || member.property.name == "each"
                    || member.property.name == "skip"
                    || member.property.name == "skipIf"
                    || member.property.name == "only" =>
            {
                matches!(
                    &member.object,
                    Expression::Identifier(object)
                        if self.test_builtins.contains(object.name.as_str())
                )
            }
            _ => false,
        }
    }

    /// Return whether a callee is `describe` or `describe.concurrent`.
    fn is_describe_callee(&self, callee: &Expression<'_>) -> bool {
        match callee {
            Expression::Identifier(ident) => {
                ident.name == "describe" && self.test_builtins.contains("describe")
            }
            Expression::StaticMemberExpression(member) if member.property.name == "concurrent" => {
                matches!(
                    &member.object,
                    Expression::Identifier(object)
                        if object.name == "describe" && self.test_builtins.contains("describe")
                )
            }
            _ => false,
        }
    }

    // Continued in the next split builder file.
}
