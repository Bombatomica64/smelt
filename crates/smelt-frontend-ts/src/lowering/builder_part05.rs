impl ModuleBuilder<'_> {
    /// Lower a `describe` test-suite declaration and its inherited hooks.
    fn describe_declaration(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        inherited_setup: &[&Statement<'_>],
        inherited_before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        inherited_after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let name_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "describe calls require a string name",
            )
        })?;
        let group_name = self.test_title(name_arg)?;
        let body_arg = call.arguments.get(1).ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "describe calls require a callback",
            )
        })?;
        let arrow = self.test_arrow_callback(body_arg, "describe callbacks")?;
        self.describe_body_declarations(
            &arrow.body.statements,
            &group_name,
            inherited_setup.to_vec(),
            inherited_before_each.to_vec(),
            inherited_after_each.to_vec(),
            &[],
        )
    }

    /// Return the static title for a supported `describe(...)` call.
    fn describe_group_name(
        &self,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Result<String, SmeltError> {
        let name_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "describe calls require a string name",
            )
        })?;
        self.test_title(name_arg)
    }

    /// Lower the statements in a `describe`-like suite body.
    ///
    /// This recursively flattens nested suite organization into Rust test
    /// functions while preserving inherited setup statements, lifecycle hooks,
    /// and literal table bindings from enclosing `describe.each` rows.
    fn describe_body_declarations<'a>(
        &mut self,
        statements: &'a oxc::allocator::Vec<'a, Statement<'a>>,
        group_name: &str,
        inherited_setup: Vec<&'a Statement<'a>>,
        inherited_before_each: Vec<&'a oxc::ast::ast::ArrowFunctionExpression<'a>>,
        inherited_after_each: Vec<&'a oxc::ast::ast::ArrowFunctionExpression<'a>>,
        table_bindings: &[(&'a str, &'a ArrayExpressionElement<'a>)],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let mut items = Vec::new();
        let mut setup = inherited_setup;
        let mut before_each = inherited_before_each;
        let mut after_each = inherited_after_each;
        for statement in statements {
            if matches!(statement, Statement::VariableDeclaration(_)) {
                setup.push(statement);
                continue;
            }
            let Statement::ExpressionStatement(expr_stmt) = statement else {
                return Err(SmeltError::unsupported(
                    self.statement_span(statement),
                    "describe blocks only support direct it/test/describe calls for now",
                ));
            };
            if self.collect_lifecycle_hook(&expr_stmt.expression, &mut before_each, &mut after_each)
            {
                continue;
            }
            if let Some(table_call) = self.table_test_call(&expr_stmt.expression) {
                items.extend(self.table_test_declarations(
                    table_call,
                    Some(group_name),
                    &setup,
                    &before_each,
                    &after_each,
                    table_bindings,
                )?);
                continue;
            }
            if let Some(nested_describe) = self.describe_call(&expr_stmt.expression) {
                let nested_group_name = format!(
                    "{group_name} {}",
                    self.describe_group_name(nested_describe)?
                );
                items.extend(self.describe_declaration_with_name_and_bindings(
                    nested_describe,
                    &nested_group_name,
                    &setup,
                    &before_each,
                    &after_each,
                    table_bindings,
                )?);
                continue;
            }
            if self.dynamic_test_alias_call(&expr_stmt.expression) {
                continue;
            }
            if let Some(test_call) = self.test_case_call(&expr_stmt.expression) {
                items.push(self.test_case_declaration(
                    test_call,
                    Some(group_name),
                    &setup,
                    &before_each,
                    &after_each,
                    table_bindings,
                )?);
                continue;
            }
            setup.push(statement);
        }
        Ok(items)
    }

    /// Lower a nested `describe` while carrying inherited table bindings.
    fn describe_declaration_with_name_and_bindings<'a>(
        &mut self,
        call: &'a oxc::ast::ast::CallExpression<'a>,
        group_name: &str,
        inherited_setup: &[&'a Statement<'a>],
        inherited_before_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        inherited_after_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        table_bindings: &[(&'a str, &'a ArrayExpressionElement<'a>)],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let body_arg = call.arguments.get(1).ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "describe calls require a callback",
            )
        })?;
        let arrow = self.test_arrow_callback(body_arg, "describe callbacks")?;
        self.describe_body_declarations(
            &arrow.body.statements,
            group_name,
            inherited_setup.to_vec(),
            inherited_before_each.to_vec(),
            inherited_after_each.to_vec(),
            table_bindings,
        )
    }

    /// Return whether an expression is a dynamic test alias call.
    fn dynamic_test_alias_call(&self, expression: &Expression<'_>) -> bool {
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return false;
        };
        if self.test_builtins.contains(callee.name.as_str()) || call.arguments.len() < 2 {
            return false;
        }
        call.arguments
            .get(1)
            .is_some_and(|argument| self.test_arrow_callback(argument, "dynamic test alias").is_ok())
    }

    /// Lower a top-level Vitest `test` / `it` call into an HIR test function.
    fn test_case_declaration(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        group_name: Option<&str>,
        setup: &[&Statement<'_>],
        before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        table_bindings: &[(&str, &ArrayExpressionElement<'_>)],
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let name_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "test case calls require a string name",
            )
        })?;
        let body_arg = call.arguments.get(1).ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "test case calls require a callback",
            )
        })?;
        let test_name = self.test_case_name(name_arg, group_name)?;
        match body_arg {
            Argument::ArrowFunctionExpression(arrow) => self.test_function_from_arrow(
                &test_name,
                self.span(call.span.start, call.span.end),
                arrow,
                setup,
                before_each,
                after_each,
                table_bindings,
            ),
            Argument::FunctionExpression(function) => self.test_function_from_function(
                &test_name,
                self.span(call.span.start, call.span.end),
                function,
                setup,
                before_each,
                after_each,
                table_bindings,
            ),
            _ => Err(SmeltError::unsupported(
                self.span(body_arg.span().start, body_arg.span().end),
                "test case callbacks must be functions",
            )),
        }
    }

    /// Lower a prepared test callback into an HIR test function.
    fn test_function_from_arrow(
        &mut self,
        test_name: &str,
        span: Span,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        setup: &[&Statement<'_>],
        before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        table_bindings: &[(&str, &ArrayExpressionElement<'_>)],
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_async = self.current_async;
        self.current_async = arrow.r#async;
        let mut body = Body::new(None, self.span(arrow.body.span.start, arrow.body.span.end));
        let mut errors = Vec::new();
        for (name, value) in table_bindings {
            if let Err(error) = self.bind_table_value(name, value, &mut body) {
                errors.push(error);
            }
        }
        for statement in setup {
            if let Err(error) = self.test_case_statement(statement, &mut body) {
                errors.push(error);
            }
        }
        for hook in before_each {
            for statement in &hook.body.statements {
                if let Err(error) = self.test_case_statement(statement, &mut body) {
                    errors.push(error);
                }
            }
        }
        for statement in &arrow.body.statements {
            if let Err(error) = self.test_case_statement(statement, &mut body) {
                errors.push(error);
            }
        }
        for hook in after_each {
            for statement in &hook.body.statements {
                if let Err(error) = self.test_case_statement(statement, &mut body) {
                    errors.push(error);
                }
            }
        }
        self.locals = saved_locals;
        self.current_async = saved_async;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        if arrow.r#async {
            body.build_async_state_machine();
        }

        let name = self.intern_source_name(test_name);
        let body_id = self.ctx.krate.push_body(body);
        let none = self.ctx.krate.types.intern(Type::None);
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            params: Vec::new(),
            return_ty: none,
            is_async: arrow.r#async,
            is_test: true,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        self.items.insert(test_name.to_owned(), item);
        Ok(item)
    }

    /// Lower a prepared `function () { ... }` test callback into an HIR test function.
    fn test_function_from_function(
        &mut self,
        test_name: &str,
        span: Span,
        function: &oxc::ast::ast::Function<'_>,
        setup: &[&Statement<'_>],
        before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        table_bindings: &[(&str, &ArrayExpressionElement<'_>)],
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "test case function callbacks must have a body",
            ));
        };
        if !function.params.items.is_empty() || function.params.rest.is_some() {
            return Err(SmeltError::unsupported(
                self.span(function.params.span.start, function.params.span.end),
                "test case function callbacks with parameters are not lowered yet",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_async = self.current_async;
        self.current_async = function.r#async;
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut errors = Vec::new();
        for (name, value) in table_bindings {
            if let Err(error) = self.bind_table_value(name, value, &mut body) {
                errors.push(error);
            }
        }
        for statement in setup {
            if let Err(error) = self.test_case_statement(statement, &mut body) {
                errors.push(error);
            }
        }
        for hook in before_each {
            for statement in &hook.body.statements {
                if let Err(error) = self.test_case_statement(statement, &mut body) {
                    errors.push(error);
                }
            }
        }
        for statement in &function_body.statements {
            if let Err(error) = self.test_case_statement(statement, &mut body) {
                errors.push(error);
            }
        }
        for hook in after_each {
            for statement in &hook.body.statements {
                if let Err(error) = self.test_case_statement(statement, &mut body) {
                    errors.push(error);
                }
            }
        }
        self.locals = saved_locals;
        self.current_async = saved_async;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        if function.r#async {
            body.build_async_state_machine();
        }

        let name = self.intern_source_name(test_name);
        let body_id = self.ctx.krate.push_body(body);
        let none = self.ctx.krate.types.intern(Type::None);
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            params: Vec::new(),
            return_ty: none,
            is_async: function.r#async,
            is_test: true,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        self.items.insert(test_name.to_owned(), item);
        Ok(item)
    }

    /// Lower `test.each` / `it.each` table rows into one Rust test per row.
    fn table_test_declarations(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        group_name: Option<&str>,
        setup: &[&Statement<'_>],
        before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        inherited_bindings: &[(&str, &ArrayExpressionElement<'_>)],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let Expression::CallExpression(each_call) = &call.callee else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "table tests must call test.each(...) or describe.each(...)",
            ));
        };
        let Expression::StaticMemberExpression(each_member) = &each_call.callee else {
            return Err(SmeltError::unsupported(
                self.span(each_call.span.start, each_call.span.end),
                "table tests must call test.each(...) or describe.each(...)",
            ));
        };
        let Expression::Identifier(test_api) = &each_member.object else {
            return Err(SmeltError::unsupported(
                self.span(each_member.span.start, each_member.span.end),
                "table tests must be called on test, it, or describe",
            ));
        };
        let rows = self.table_rows(each_call)?;
        if test_api.name == "describe" {
            return self.describe_each_declarations(
                call,
                group_name,
                &rows,
                setup,
                before_each,
                after_each,
                inherited_bindings,
            );
        }
        let name_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "test.each calls require a string name",
            )
        })?;
        let body_arg = call.arguments.get(1).ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "test.each calls require a callback",
            )
        })?;
        let arrow = self.test_arrow_callback_with_params(body_arg, "test.each callbacks", true)?;
        let mut items = Vec::new();
        for (case_index, row) in rows.iter().enumerate() {
            let case_group = group_name.map(|name| format!("{name} case {case_index}"));
            let test_name = self.test_case_name(name_arg, case_group.as_deref())?;
            let mut bindings = inherited_bindings.to_vec();
            bindings.extend(self.table_bindings(arrow, row)?);
            items.push(self.test_function_from_arrow(
                &test_name,
                self.span(call.span.start, call.span.end),
                arrow,
                setup,
                before_each,
                after_each,
                &bindings,
            )?);
        }
        Ok(items)
    }

    /// Lower `describe.each` by flattening each row's nested suite body.
    fn describe_each_declarations<'a>(
        &mut self,
        call: &'a oxc::ast::ast::CallExpression<'a>,
        group_name: Option<&str>,
        rows: &[Vec<&'a ArrayExpressionElement<'a>>],
        setup: &[&Statement<'a>],
        before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'a>],
        after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'a>],
        inherited_bindings: &[(&'a str, &'a ArrayExpressionElement<'a>)],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let name_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "describe.each calls require a string name",
            )
        })?;
        let body_arg = call.arguments.get(1).ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "describe.each calls require a callback",
            )
        })?;
        let arrow =
            self.test_arrow_callback_with_params(body_arg, "describe.each callbacks", true)?;
        let mut items = Vec::new();
        for (case_index, row) in rows.iter().enumerate() {
            let row_group = self.test_title(name_arg)?;
            let row_group = group_name.map_or_else(
                || format!("{row_group} case {case_index}"),
                |parent| format!("{parent} {row_group} case {case_index}"),
            );
            let mut bindings = inherited_bindings.to_vec();
            bindings.extend(self.table_bindings(arrow, row)?);
            items.extend(self.describe_body_declarations(
                &arrow.body.statements,
                &row_group,
                setup.to_vec(),
                before_each.to_vec(),
                after_each.to_vec(),
                &bindings,
            )?);
        }
        Ok(items)
    }

    /// Parse the table literal from an `.each([...])` call.
    ///
    /// Nested array elements represent multi-argument rows. Scalar elements
    /// use Vitest's shorthand for a one-argument row.
    fn table_rows<'a>(
        &self,
        each_call: &'a oxc::ast::ast::CallExpression<'a>,
    ) -> Result<Vec<Vec<&'a ArrayExpressionElement<'a>>>, SmeltError> {
        let [Argument::ArrayExpression(table)] = each_call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(each_call.span.start, each_call.span.end),
                "table tests support only array literal tables",
            ));
        };
        let mut rows = Vec::new();
        for element in &table.elements {
            match element {
                ArrayExpressionElement::ArrayExpression(row) => {
                    rows.push(row.elements.iter().collect());
                }
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => {
                    return Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "table test rows do not support spreads or elisions",
                    ));
                }
                _ => rows.push(vec![element]),
            }
        }
        Ok(rows)
    }

    /// Pair callback parameter names with one table row.
    fn table_bindings<'a>(
        &self,
        arrow: &'a oxc::ast::ast::ArrowFunctionExpression<'a>,
        row: &[&'a ArrayExpressionElement<'a>],
    ) -> Result<Vec<(&'a str, &'a ArrayExpressionElement<'a>)>, SmeltError> {
        if arrow.params.items.len() != row.len() {
            return Err(SmeltError::unsupported(
                self.span(arrow.params.span.start, arrow.params.span.end),
                "table test callback parameter count must match row width",
            ));
        }
        arrow
            .params
            .items
            .iter()
            .zip(row)
            .map(|(param, value)| {
                let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                    return Err(SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "table test callback parameters must be identifiers",
                    ));
                };
                Ok((binding.name.as_str(), *value))
            })
            .collect()
    }

    /// Bind one `test.each` row value to a local used by the callback body.
    fn bind_table_value(
        &mut self,
        name: &str,
        value: &ArrayExpressionElement<'_>,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let expr = self.array_element(value, body)?;
        let ty = Self::expr_ty(body, expr);
        let symbol = self.intern_source_name(name);
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span: self.span(value.span().start, value.span().end),
        });
        self.locals.insert(name.to_owned(), local);
        let pat = body.push_pattern(Pattern::Binding(local));
        body.push_stmt(Stmt::Let {
            pat,
            ty,
            value: Some(expr),
        });
        Ok(())
    }

    /// Convert a test-case name argument into a stable Rust function name.
    fn test_case_name(
        &self,
        argument: &Argument<'_>,
        group_name: Option<&str>,
    ) -> Result<String, SmeltError> {
        let case_name = self.test_title(argument)?;
        let full_name = group_name.map_or_else(
            || case_name.clone(),
            |group_name| format!("{group_name} {case_name}"),
        );
        Ok(format!(
            "test_{}",
            sanitize_test_name(&full_name).unwrap_or_else(|| "case".to_owned())
        ))
    }

    /// Extract a string title from a test-framework name argument.
    fn test_title(&self, argument: &Argument<'_>) -> Result<String, SmeltError> {
        match argument {
            Argument::StringLiteral(name) => Ok(name.value.to_string()),
            Argument::Identifier(identifier) => Ok(identifier.name.to_string()),
            _ => Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "test case names must be string literals or identifiers",
            )),
        }
    }

    /// Extract and validate an arrow callback for supported test-framework calls.
    fn test_arrow_callback<'a>(
        &self,
        argument: &'a Argument<'a>,
        context: &str,
    ) -> Result<&'a oxc::ast::ast::ArrowFunctionExpression<'a>, SmeltError> {
        self.test_arrow_callback_with_params(argument, context, false)
    }

    /// Extract and validate an arrow callback, optionally allowing table-test parameters.
    fn test_arrow_callback_with_params<'a>(
        &self,
        argument: &'a Argument<'a>,
        context: &str,
        allow_params: bool,
    ) -> Result<&'a oxc::ast::ast::ArrowFunctionExpression<'a>, SmeltError> {
        let Argument::ArrowFunctionExpression(arrow) = argument else {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                format!("{context} must be arrow functions"),
            ));
        };
        if !allow_params && !arrow.params.items.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(arrow.params.span.start, arrow.params.span.end),
                format!("{context} with parameters are not lowered yet"),
            ));
        }
        Ok(arrow)
    }

    /// Lower one supported statement inside a test case callback.
    fn test_case_statement(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        if let Statement::ExpressionStatement(expr_stmt) = statement
            && let Expression::CallExpression(call) = &expr_stmt.expression
            && self.expect_matcher_statement(call, body)?
        {
            return Ok(());
        }
        if let Statement::ExpressionStatement(expr_stmt) = statement
            && let Expression::CallExpression(call) = &expr_stmt.expression
            && self.deep_strict_equal_statement(call, body)?
        {
            return Ok(());
        }
        self.statement(statement, body)
    }

    // Continued in the next split builder file.
}
