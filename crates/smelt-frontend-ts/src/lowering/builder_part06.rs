impl ModuleBuilder<'_> {
    fn deep_strict_equal_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        let is_deep_strict_equal = match &call.callee {
            Expression::Identifier(ident) => ident.name == "deepStrictEqual",
            Expression::StaticMemberExpression(member)
                if member.property.name == "deepStrictEqual" =>
            {
                matches!(
                    &member.object,
                    Expression::Identifier(object) if object.name == "U" || object.name == "assert"
                )
            }
            _ => false,
        };
        if !is_deep_strict_equal {
            return Ok(false);
        }
        let [actual_arg, expected_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "deepStrictEqual(...) requires actual and expected values",
            ));
        };
        let actual = self.argument(actual_arg, body)?;
        let expected = self.argument(expected_arg, body)?;
        let failed = self.comparison_expr(BinOp::NotEq, actual, expected, call.span, body);
        self.push_test_failure_if(failed, "deepStrictEqual(...) failed", call.span, body);
        Ok(true)
    }

    /// Lower supported `expect(actual).matcher(expected)` calls to failure paths.
    fn expect_matcher_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(false);
        };
        let Some(matcher) = TestMatcher::from_name(member.property.name.as_str()) else {
            return Ok(false);
        };
        let (expect_call, inverted) = self.expect_call_from_matcher_object(&member.object)?;
        let Expression::Identifier(expect_ident) = &expect_call.callee else {
            return Ok(false);
        };
        if !self.test_builtins.contains(expect_ident.name.as_str())
            || expect_ident.name.as_str() != "expect"
        {
            return Ok(false);
        }
        let actual_arg = expect_call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(expect_call.span.start, expect_call.span.end),
                format!(
                    "expect(...).{}(...) requires an actual value",
                    matcher.source_name()
                ),
            )
        })?;
        let expected_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "expect(...).{}(...) requires an expected value",
                    matcher.source_name()
                ),
            )
        })?;
        let actual = self.argument(actual_arg, body)?;
        let expected = self.argument(expected_arg, body)?;
        let mut failed =
            self.expect_matcher_failure_expr(matcher, actual, expected, call.span, body)?;
        if inverted {
            failed = self.unary_bool_expr(UnaryOp::Not, failed, call.span, body);
        }
        self.push_test_failure_if(
            failed,
            &format!("expect(...).{}(...) failed", matcher.source_name()),
            call.span,
            body,
        );
        Ok(true)
    }

    /// Extract `expect(...)` and whether `.not` was present from a matcher receiver.
    fn expect_call_from_matcher_object<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Result<(&'a oxc::ast::ast::CallExpression<'a>, bool), SmeltError> {
        if let Expression::CallExpression(expect_call) = expression {
            return Ok((expect_call, false));
        }
        let Expression::StaticMemberExpression(member) = expression else {
            return Err(SmeltError::unsupported(
                self.span(expression.span().start, expression.span().end),
                "expect matcher receiver must be expect(...) or expect(...).not",
            ));
        };
        if member.property.name != "not" {
            return Err(SmeltError::unsupported(
                self.span(member.span.start, member.span.end),
                "only expect(...).not matcher modifiers are supported",
            ));
        }
        let Expression::CallExpression(expect_call) = &member.object else {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "expect(...).not requires an expect(...) receiver",
            ));
        };
        Ok((expect_call, true))
    }

    /// Build the boolean expression that means a supported matcher has failed.
    fn expect_matcher_failure_expr(
        &mut self,
        matcher: TestMatcher,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match matcher {
            TestMatcher::Be | TestMatcher::Equal | TestMatcher::StrictEqual => {
                Ok(self.comparison_expr(BinOp::NotEq, actual, expected, span, body))
            }
            TestMatcher::Contain => {
                let contains = self.contains_expr(actual, expected, span, body)?;
                Ok(self.unary_bool_expr(UnaryOp::Not, contains, span, body))
            }
            TestMatcher::HaveLength => {
                let len = self.len_expr(actual, span, body)?;
                Ok(self.comparison_expr(BinOp::NotEq, len, expected, span, body))
            }
            TestMatcher::HaveProperty => {
                let contains = self.dict_contains_key_expr(actual, expected, span, body)?;
                Ok(self.unary_bool_expr(UnaryOp::Not, contains, span, body))
            }
        }
    }

    /// Push a throwing failure block guarded by a boolean condition.
    fn push_test_failure_if(
        &mut self,
        cond: smelt_hir::ExprId,
        message: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) {
        let failure_block = body.push_block(self.span(span.start, span.end));
        let message = self.string_literal_expr(message, span, body);
        body.push_stmt_to_block(failure_block, Stmt::Throw(message));
        body.push_stmt(Stmt::If {
            cond,
            then_block: failure_block,
            else_block: None,
        });
    }

    /// Create a boolean unary expression for synthesized test assertions.
    fn unary_bool_expr(
        &mut self,
        op: UnaryOp,
        operand: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        body.push_expr(Expr {
            kind: ExprKind::UnaryOp { op, operand },
            ty: bool_ty,
            span: self.span(span.start, span.end),
        })
    }

    /// Create a boolean comparison expression for synthesized test assertions.
    fn comparison_expr(
        &mut self,
        op: BinOp,
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        body.push_expr(Expr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty: bool_ty,
            span: self.span(span.start, span.end),
        })
    }

    /// Create a length expression for synthesized test assertions.
    fn len_expr(
        &mut self,
        operand: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match self.ctx.krate.types.get(Self::expr_ty(body, operand)) {
            Some(
                Type::String | Type::List(_) | Type::Set(_) | Type::Dict(_, _) | Type::Tuple(_),
            ) => {
                let int_ty = self.ctx.krate.types.intern(Type::Int);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Len { operand },
                    ty: int_ty,
                    span: self.span(span.start, span.end),
                }))
            }
            _ => Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "expect(...).toHaveLength(...) requires a string or collection actual value",
            )),
        }
    }

    /// Create a containment expression for synthesized test assertions.
    fn contains_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let kind = match self.ctx.krate.types.get(Self::expr_ty(body, actual)) {
            Some(Type::String)
                if self.ctx.krate.types.get(Self::expr_ty(body, expected))
                    == Some(&Type::String) =>
            {
                ExprKind::StringContains {
                    haystack: actual,
                    needle: expected,
                }
            }
            Some(Type::List(item_ty)) if Self::expr_ty(body, expected) == *item_ty => {
                ExprKind::ListContains {
                    list: actual,
                    item: expected,
                }
            }
            Some(Type::Set(item_ty)) if Self::expr_ty(body, expected) == *item_ty => {
                ExprKind::SetContains {
                    set: actual,
                    item: expected,
                }
            }
            Some(Type::Tuple(items)) if items.contains(&Self::expr_ty(body, expected)) => {
                ExprKind::TupleContains {
                    tuple: actual,
                    item: expected,
                }
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(span.start, span.end),
                    "expect(...).toContain(...) requires a string, array, set, or tuple actual value with a matching expected value",
                ));
            }
        };
        Ok(body.push_expr(Expr {
            kind,
            ty: bool_ty,
            span: self.span(span.start, span.end),
        }))
    }

    /// Create a dictionary key containment expression for `toHaveProperty`.
    fn dict_contains_key_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(Type::Dict(key_ty, _)) = self.ctx.krate.types.get(Self::expr_ty(body, actual))
        else {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "expect(...).toHaveProperty(...) requires an object or map actual value",
            ));
        };
        if Self::expr_ty(body, expected) != *key_ty {
            return Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "expect(...).toHaveProperty(...) key must match the object key type",
            ));
        }
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictContainsKey {
                dict: actual,
                key: expected,
            },
            ty: bool_ty,
            span: self.span(span.start, span.end),
        }))
    }

    /// Create a string literal expression for synthesized test diagnostics.
    fn string_literal_expr(
        &mut self,
        value: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let ty = self.ctx.krate.types.intern(Type::String);
        body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(value.to_owned())),
            ty,
            span: self.span(span.start, span.end),
        })
    }

    /// Create a block from a statement (wrapping if needed).
    fn block_from_statement(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::BlockId, SmeltError> {
        let span = self.statement_span(statement);
        let block = body.push_block(span);
        if let Statement::BlockStatement(block_stmt) = statement {
            for nested_statement in &block_stmt.body {
                self.statement_in_block(nested_statement, body, block)?;
            }
        } else {
            self.statement_in_block(statement, body, block)?;
        }
        Ok(block)
    }

    /// Create a HIR block from a JavaScript block statement.
    fn block_from_block_statement(
        &mut self,
        block_stmt: &oxc::ast::ast::BlockStatement<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::BlockId, SmeltError> {
        let block = body.push_block(self.span(block_stmt.span.start, block_stmt.span.end));
        for statement in &block_stmt.body {
            self.statement_in_block(statement, body, block)?;
        }
        Ok(block)
    }

    /// Apply a local narrowing in the current lexical lowering context.
    fn apply_narrowing(&mut self, name: String, target: smelt_hir::TypeId) {
        if let Some(scope) = self.narrowed_locals.last_mut() {
            scope.insert(name, target);
        } else {
            let mut scope = HashMap::new();
            scope.insert(name, target);
            self.narrowed_locals.push(scope);
        }
    }

    /// Return the active narrowed type for a source local, if any.
    fn narrowed_type(&self, name: &str) -> Option<smelt_hir::TypeId> {
        self.narrowed_locals
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    /// Discover the narrowing applied by a successful assertion call statement.
    fn assertion_call_narrowing(
        &self,
        expression: &Expression<'_>,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        let assertion = self.assertion_functions.get(callee.name.as_str())?;
        let arg = call.arguments.get(assertion.param_index)?;
        let Argument::Identifier(identifier) = arg else {
            return None;
        };
        Some((identifier.name.to_string(), assertion.target))
    }

    /// Discover local type facts proven by a boolean guard expression.
    fn guard_narrowing(
        &mut self,
        expression: &Expression<'_>,
    ) -> Option<HashMap<String, smelt_hir::TypeId>> {
        let mut out = HashMap::new();
        if let Some((name, target)) = self.typeof_guard(expression) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.array_is_array_guard(expression) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.null_guard(expression) {
            out.insert(name, target);
        }
        (!out.is_empty()).then_some(out)
    }

    /// Recognize `typeof value === "kind"` guard expressions.
    fn typeof_guard(&mut self, expression: &Expression<'_>) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality | BinaryOperator::Equality
        ) {
            return None;
        }
        let Expression::UnaryExpression(unary) = &binary.left else {
            return None;
        };
        if unary.operator != UnaryOperator::Typeof {
            return None;
        }
        let Expression::Identifier(identifier) = &unary.argument else {
            return None;
        };
        let Expression::StringLiteral(kind) = &binary.right else {
            return None;
        };
        let ty = match kind.value.as_str() {
            "boolean" => self.ctx.krate.types.intern(Type::Bool),
            "number" => self.ctx.krate.types.intern(Type::Float),
            "string" => self.ctx.krate.types.intern(Type::String),
            "object" => self.ctx.krate.types.intern(Type::Unknown),
            _ => return None,
        };
        Some((identifier.name.to_string(), ty))
    }

    /// Recognize `Array.isArray(value)` guard expressions.
    fn array_is_array_guard(
        &mut self,
        expression: &Expression<'_>,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Array" || member.property.name != "isArray" {
            return None;
        }
        let [Argument::Identifier(identifier)] = call.arguments.as_slice() else {
            return None;
        };
        let unknown = self.ctx.krate.types.intern(Type::Unknown);
        let ty = self.ctx.krate.types.intern(Type::List(unknown));
        Some((identifier.name.to_string(), ty))
    }

    /// Recognize `value === null` guard expressions.
    fn null_guard(&mut self, expression: &Expression<'_>) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality | BinaryOperator::Equality
        ) {
            return None;
        }
        let (Expression::Identifier(identifier), Expression::NullLiteral(_)) =
            (&binary.left, &binary.right)
        else {
            return None;
        };
        Some((
            identifier.name.to_string(),
            self.ctx.krate.types.intern(Type::None),
        ))
    }

    /// Lower a catch parameter to an optional HIR local binding.
    fn catch_binding(
        &mut self,
        param: &oxc::ast::ast::CatchParameter<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::LocalId, SmeltError> {
        let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
            return Err(SmeltError::unsupported(
                self.span(param.span.start, param.span.end),
                "destructured catch bindings are not lowered yet",
            ));
        };
        let ty = param
            .type_annotation
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::String));
        let name = binding.name.as_str();
        let symbol = self.intern_source_name(name);
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span: self.span(binding.span.start, binding.span.end),
        });
        self.locals.insert(name.to_owned(), local);
        Ok(local)
    }

    /// Lower a variable declaration statement.
    fn variable_declaration(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                return Err(SmeltError::unsupported(
                    self.span(declarator.span.start, declarator.span.end),
                    "destructuring declarations are not lowered yet",
                ));
            };

            let annotated_ty = declarator
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            let value = declarator
                .init
                .as_ref()
                .map(|init| self.expression_with_hint(init, body, annotated_ty))
                .transpose()?;
            let ty = annotated_ty
                .or_else(|| value.map(|expr_id| Self::expr_ty(body, expr_id)))
                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::None));
            let name = binding.name.as_str();
            let symbol = self.ctx.krate.symbols.intern(&camel_to_snake(name));
            self.ctx.krate.names.record(symbol, name);
            let local = body.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: matches!(declarator.kind, oxc::ast::ast::VariableDeclarationKind::Let),
                span: self.span(binding.span.start, binding.span.end),
            });
            self.locals.insert(name.to_owned(), local);
            let pat = body.push_pattern(Pattern::Binding(local));
            body.push_stmt_to_block(block, Stmt::Let { pat, ty, value });
        }
        Ok(())
    }

    // Continued in the next split builder file.
}
