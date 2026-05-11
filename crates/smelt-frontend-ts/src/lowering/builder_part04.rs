impl ModuleBuilder<'_> {
    fn interface_declaration(
        &mut self,
        interface: &oxc::ast::ast::TSInterfaceDeclaration<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        if interface.type_parameters.is_some() {
            return Err(SmeltError::unsupported(
                self.span(interface.span.start, interface.span.end),
                "generic interfaces are not lowered yet",
            ));
        }
        let name_text = interface.id.name.as_str();
        let name = self.intern_type_name(name_text);
        let mut fields = Vec::new();
        let mut methods = Vec::new();

        for heritage in &interface.extends {
            let parent_name = self.interface_heritage_symbol(heritage)?;
            let parent = self.find_interface(parent_name).ok_or_else(|| {
                let parent_name_text = self
                    .ctx
                    .krate
                    .symbols
                    .get(parent_name)
                    .unwrap_or("<unknown>");
                SmeltError::unsupported(
                    self.span(heritage.span.start, heritage.span.end),
                    format!("extended interface `{parent_name_text}` is not declared"),
                )
            })?;
            fields.extend(parent.fields.clone());
            methods.extend(parent.methods.clone());
        }

        for sig in &interface.body.body {
            match sig {
                TSSignature::TSPropertySignature(prop) => {
                    if prop.computed && !is_static_property_key(&prop.key) {
                        return Err(SmeltError::unsupported(
                            self.span(prop.span.start, prop.span.end),
                            "dynamic computed interface property names are not lowered yet",
                        ));
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
                        || method.optional
                        || method.type_parameters.is_some()
                        || method.this_param.is_some()
                    {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "generic, optional, dynamic computed, and this-parameter interface methods are not lowered yet",
                        ));
                    }
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
                    for param in &method.params.items {
                        let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                            return Err(SmeltError::unsupported(
                                self.span(param.span.start, param.span.end),
                                "destructured interface method parameters are not lowered yet",
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
                                    "interface method parameters require explicit types",
                                )
                            })?;
                        params.push(ParamSig {
                            name: self.intern_source_name(binding.name.as_str()),
                            ty,
                            span: self.span(binding.span.start, binding.span.end),
                        });
                    }
                    methods.push(MethodSig {
                        name: self.property_key_symbol(&method.key)?,
                        params,
                        return_ty,
                        visibility: Visibility::Public,
                        is_async: matches!(
                            self.ctx.krate.types.get(return_ty),
                            Some(Type::Future(_))
                        ),
                        span: self.span(method.span.start, method.span.end),
                    });
                }
                TSSignature::TSCallSignatureDeclaration(_)
                | TSSignature::TSConstructSignatureDeclaration(_)
                | TSSignature::TSIndexSignature(_) => {
                    return Err(SmeltError::unsupported(
                        self.span(sig.span().start, sig.span().end),
                        "interface call, construct, and index signatures are not lowered yet",
                    ));
                }
            }
        }
        let item = self.ctx.krate.push_item(Item::Interface(Interface {
            name,
            span: self.span(interface.span.start, interface.span.end),
            fields,
            methods,
        }));
        self.interfaces.insert(name_text.to_owned(), item);
        Ok(item)
    }

    /// Lower a statement within a specific block.
    fn statement_in_block(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        match statement {
            Statement::VariableDeclaration(decl) => self.variable_declaration(decl, body, block),
            Statement::ExpressionStatement(expr_stmt) => {
                if self.is_test_framework_statement(&expr_stmt.expression) {
                    return Ok(());
                }
                if let Expression::AssignmentExpression(assign) = &expr_stmt.expression {
                    let (target, value) = self.assignment_parts(assign, body)?;
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                    return Ok(());
                }
                if let Expression::UpdateExpression(update) = &expr_stmt.expression {
                    let (target, value) = self.update_parts(update, body)?;
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                    return Ok(());
                }
                let assertion_narrowing = self.assertion_call_narrowing(&expr_stmt.expression);
                let expr = self.expression(&expr_stmt.expression, body)?;
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
                    .map(|argument| self.expression(argument, body))
                    .transpose()?;
                body.push_stmt_to_block(block, Stmt::Return(value));
                Ok(())
            }
            Statement::IfStatement(if_stmt) => {
                let cond = self.expression(&if_stmt.test, body)?;
                let then_narrowing = self.guard_narrowing(&if_stmt.test);
                if let Some(narrowing) = then_narrowing.clone() {
                    self.narrowed_locals.push(narrowing);
                }
                let then_block = self.block_from_statement(&if_stmt.consequent, body)?;
                if then_narrowing.is_some() {
                    self.narrowed_locals.pop();
                }
                let else_block = if_stmt
                    .alternate
                    .as_ref()
                    .map(|alternate| self.block_from_statement(alternate, body))
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
            Statement::WhileStatement(while_stmt) => {
                let cond = self.expression(&while_stmt.test, body)?;
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
            Statement::ForOfStatement(for_stmt) => {
                if for_stmt.r#await {
                    return Err(SmeltError::unsupported(
                        self.span(for_stmt.span.start, for_stmt.span.end),
                        "for await...of is async control flow and is not lowered yet",
                    ));
                }
                let iter = self.expression(&for_stmt.right, body)?;
                let iter = self.for_of_iterable(iter, &for_stmt.right, body);
                let pat = self.for_left_pattern(&for_stmt.left, body)?;
                let loop_body = self.block_from_statement(&for_stmt.body, body)?;
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

                for case in &switch_stmt.cases {
                    let case_block = body.push_block(self.span(case.span.start, case.span.end));
                    let mut saw_break = false;
                    for case_statement in &case.consequent {
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
                    if !saw_break && !case.consequent.iter().any(statement_terminates) {
                        return Err(SmeltError::unsupported(
                            self.span(case.span.start, case.span.end),
                            "switch fallthrough is not lowered yet; each case must break, return, or throw",
                        ));
                    }

                    if let Some(test) = &case.test {
                        arms.push(MatchArm {
                            label: self.literal_case_label(test)?,
                            body: case_block,
                        });
                    } else if default.replace(case_block).is_some() {
                        return Err(SmeltError::unsupported(
                            self.span(case.span.start, case.span.end),
                            "switch statements can only have one default case",
                        ));
                    }
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
                let expr = self.expression(&throw_stmt.argument, body)?;
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
        }
    }

    /// Return whether an expression is a top-level Vitest organization call.
    fn is_test_framework_statement(&self, expression: &Expression<'_>) -> bool {
        if self.table_test_call(expression).is_some() {
            return true;
        }
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        self.is_test_framework_callee(&call.callee)
    }

    /// Return a supported top-level test case call, if this expression is one.
    fn test_case_call<'a>(
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

    /// Collect a top-level `beforeEach` or `afterEach` callback.
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
        if !self.test_builtins.contains(name) || !matches!(name, "beforeEach" | "afterEach") {
            return false;
        }
        let Some(callback_arg) = call.arguments.first() else {
            return false;
        };
        let Ok(callback) = self.test_arrow_callback(callback_arg, "lifecycle callbacks") else {
            return false;
        };
        if name == "beforeEach" {
            before_each.push(callback);
        } else {
            after_each.push(callback);
        }
        true
    }

    /// Return whether a callee belongs to an imported test-framework API.
    fn is_test_framework_callee(&self, callee: &Expression<'_>) -> bool {
        match callee {
            Expression::Identifier(ident) => self.test_builtins.contains(ident.name.as_str()),
            Expression::StaticMemberExpression(member)
                if member.property.name == "concurrent"
                    || member.property.name == "each"
                    || member.property.name == "skip"
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
