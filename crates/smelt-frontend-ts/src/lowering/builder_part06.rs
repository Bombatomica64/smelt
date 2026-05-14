impl ModuleBuilder<'_> {
    /// Lower a Node `assert.deepStrictEqual` call statement when one is present.
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

    /// Lower supported Node `assert` equality calls into runtime assertions.
    fn node_assert_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
        if self.deep_strict_equal_statement(call, body)? {
            return Ok(true);
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(false);
        };
        if member.property.name != "strictEqual" {
            return Ok(false);
        }
        let Expression::Identifier(object) = &member.object else {
            return Ok(false);
        };
        if object.name != "assert" {
            return Ok(false);
        }
        let [actual_arg, expected_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "assert.strictEqual(...) requires actual and expected values",
            ));
        };
        let actual = self.argument(actual_arg, body)?;
        let expected = self.argument(expected_arg, body)?;
        let failed = self.comparison_expr(BinOp::NotEq, actual, expected, call.span, body);
        self.push_test_failure_if(failed, "assert.strictEqual(...) failed", call.span, body);
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
        if matches!(
            member.property.name.as_str(),
            "toThrow" | "toThrowErrorMatchingInlineSnapshot"
        ) {
            return self.expect_to_throw_statement(call, member, body);
        }
        if member.property.name == "toBeUndefined" {
            return self.expect_to_be_none_statement(call, member, body, "toBeUndefined");
        }
        if member.property.name == "toBeNull" {
            return self.expect_to_be_none_statement(call, member, body, "toBeNull");
        }
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

    /// Lower nullish zero-argument matchers to a `None` equality check.
    fn expect_to_be_none_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
        matcher_name: &str,
    ) -> Result<bool, SmeltError> {
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("expect(...).{matcher_name}() does not take arguments"),
            ));
        }
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
                format!("expect(...).{matcher_name}() requires an actual value"),
            )
        })?;
        let actual = self.argument(actual_arg, body)?;
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let expected = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: none_ty,
            span: self.span(call.span.start, call.span.end),
        });
        let mut failed = self.comparison_expr(BinOp::NotEq, actual, expected, call.span, body);
        if inverted {
            failed = self.unary_bool_expr(UnaryOp::Not, failed, call.span, body);
        }
        self.push_test_failure_if(
            failed,
            &format!("expect(...).{matcher_name}() failed"),
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

    /// Lower `expect(() => ...).toThrow(...)` to native HIR exception flow.
    ///
    /// The initial lowering only needs to prove that the callback throws. The
    /// optional expected message argument is intentionally ignored until HIR has
    /// a first-class panic payload comparison path for TypeScript exceptions.
    fn expect_to_throw_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<bool, SmeltError> {
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
                "expect(...).toThrow(...) requires a callback",
            )
        })?;
        let Argument::ArrowFunctionExpression(arrow) = actual_arg else {
            return Err(SmeltError::unsupported(
                self.span(actual_arg.span().start, actual_arg.span().end),
                "expect(...).toThrow(...) supports arrow callbacks",
            ));
        };

        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let span = self.span(call.span.start, call.span.end);
        let did_throw_name = self.intern_source_name("did_throw");
        let did_throw = body.push_local(LocalDecl {
            name: Some(did_throw_name),
            ty: bool_ty,
            mutable: true,
            span,
        });
        let did_throw_pat = body.push_pattern(Pattern::Binding(did_throw));
        let false_expr = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span,
        });
        body.push_stmt(Stmt::Let {
            pat: did_throw_pat,
            ty: bool_ty,
            value: Some(false_expr),
        });

        let try_block = body.push_block(self.span(arrow.body.span.start, arrow.body.span.end));
        for statement in &arrow.body.statements {
            self.statement_in_block(statement, body, try_block)?;
        }
        let catch_block = body.push_block(span);
        let did_throw_target = body.push_expr(Expr {
            kind: ExprKind::Local(did_throw),
            ty: bool_ty,
            span,
        });
        let true_expr = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        body.push_stmt_to_block(
            catch_block,
            Stmt::Assign {
                target: did_throw_target,
                value: true_expr,
            },
        );
        body.push_stmt(Stmt::TryCatch {
            body: try_block,
            catch_binding: None,
            catch_body: Some(catch_block),
            finally_body: None,
        });

        let did_throw_check = body.push_expr(Expr {
            kind: ExprKind::Local(did_throw),
            ty: bool_ty,
            span,
        });
        let failed = if inverted {
            did_throw_check
        } else {
            self.unary_bool_expr(UnaryOp::Not, did_throw_check, call.span, body)
        };
        let message = if inverted {
            "expect(...).not.toThrow(...) failed"
        } else {
            "expect(...).toThrow(...) failed"
        };
        self.push_test_failure_if(failed, message, call.span, body);
        Ok(true)
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
        let operand_ty = Self::expr_ty(body, operand);
        match self.ctx.krate.types.get(operand_ty) {
            Some(Type::String | Type::List(_) | Type::Set(_) | Type::Dict(_, _) | Type::Tuple(_)) => {
                let int_ty = self.ctx.krate.types.intern(Type::Int);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Len { operand },
                    ty: int_ty,
                    span: self.span(span.start, span.end),
                }))
            }
            Some(Type::Unknown)
                if self.allow_unknown_index_access || self.type_contains_unknown(operand_ty) =>
            {
                let int_ty = self.ctx.krate.types.intern(Type::Int);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Len { operand },
                    ty: int_ty,
                    span: self.span(span.start, span.end),
                }))
            }
            _ if self.allow_unknown_index_access => {
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
        body: &Body,
    ) -> Option<HashMap<String, smelt_hir::TypeId>> {
        let mut out = HashMap::new();
        if let Expression::LogicalExpression(logical) = expression
            && logical.operator == LogicalOperator::And
        {
            if let Some(left) = self.guard_narrowing(&logical.left, body) {
                out.extend(left);
            }
            if let Some(right) = self.guard_narrowing(&logical.right, body) {
                out.extend(right);
            }
        } else if let Some((name, target)) = self.typeof_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.array_is_array_guard(expression) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.null_guard(expression) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.optional_some_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.truthy_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.predicate_call_guard(expression) {
            out.insert(name, target);
        }
        (!out.is_empty()).then_some(out)
    }

    /// Discover local type facts proven after a guard exits early.
    fn inverse_guard_narrowing(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<HashMap<String, smelt_hir::TypeId>> {
        let mut out = HashMap::new();
        if let Expression::LogicalExpression(logical) = expression
            && logical.operator == LogicalOperator::Or
        {
            if let Some(left) = self.inverse_guard_narrowing(&logical.left, body) {
                out.extend(left);
            }
            if let Some(right) = self.inverse_guard_narrowing(&logical.right, body) {
                out.extend(right);
            }
        } else if let Some((name, target)) = self.optional_none_inverse_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.typeof_inverse_guard(expression, body) {
            out.insert(name, target);
        } else if let Expression::UnaryExpression(unary) = expression
            && unary.operator == UnaryOperator::LogicalNot
            && let Some(narrowing) = self.guard_narrowing(&unary.argument, body)
        {
            out.extend(narrowing);
        }
        (!out.is_empty()).then_some(out)
    }

    /// Recognize `value !== undefined/null` guards.
    fn optional_some_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ) {
            return None;
        }
        let name = match (&binary.left, &binary.right) {
            (Expression::Identifier(identifier), Expression::Identifier(undefined))
                if undefined.name == "undefined" =>
            {
                identifier.name.as_str()
            }
            (Expression::Identifier(identifier), Expression::NullLiteral(_)) => {
                identifier.name.as_str()
            }
            _ => return None,
        };
        let local = self.locals.get(name).copied()?;
        let local_ty = self.narrowed_type(name).unwrap_or_else(|| Self::local_ty(body, local));
        match self.ctx.krate.types.get(local_ty).cloned() {
            Some(Type::Optional(inner)) => Some((name.to_owned(), inner)),
            Some(Type::Union(items)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let remaining = items
                    .into_iter()
                    .filter(|item| *item != none_ty)
                    .collect::<Vec<_>>();
                match remaining.as_slice() {
                    [single] => Some((name.to_owned(), *single)),
                    [] => None,
                    _ => Some((
                        name.to_owned(),
                        self.ctx.krate.types.intern(Type::Union(remaining)),
                    )),
                }
            }
            _ => None,
        }
    }

    /// Recognize `typeof value === "kind"` guards whose true branch exits.
    fn typeof_inverse_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
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
        let local = self.locals.get(identifier.name.as_str()).copied()?;
        let local_ty = self
            .narrowed_type(identifier.name.as_str())
            .unwrap_or_else(|| Self::local_ty(body, local));
        let remaining = self.remove_typeof_member(local_ty, kind.value.as_str())?;
        Some((identifier.name.to_string(), remaining))
    }

    /// Return a type with members matching a `typeof` kind removed.
    fn remove_typeof_member(
        &mut self,
        ty: smelt_hir::TypeId,
        kind: &str,
    ) -> Option<smelt_hir::TypeId> {
        let resolved_ty = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(resolved_ty).cloned() {
            Some(Type::Union(items)) => {
                let remaining = items
                    .into_iter()
                    .filter(|item| !self.type_matches_typeof(*item, kind))
                    .collect::<Vec<_>>();
                match remaining.as_slice() {
                    [] => None,
                    [single] => Some(*single),
                    _ => Some(self.ctx.krate.types.intern(Type::Union(remaining))),
                }
            }
            Some(Type::Optional(item)) if kind == "undefined" => Some(item),
            Some(_) if self.type_matches_typeof(resolved_ty, kind) => None,
            Some(_) => Some(resolved_ty),
            None => None,
        }
    }

    /// Return whether a HIR type corresponds to a JavaScript `typeof` result.
    fn type_matches_typeof(&self, ty: smelt_hir::TypeId, kind: &str) -> bool {
        let resolved_ty = self.type_param_constraint_or_self(ty);
        match (self.ctx.krate.types.get(resolved_ty), kind) {
            (Some(Type::Bool), "boolean")
            | (Some(Type::Float | Type::Int), "number")
            | (Some(Type::String), "string")
            | (Some(Type::Function(_)), "function")
            | (Some(Type::Optional(_)), "undefined")
            | (Some(Type::None), "undefined" | "object") => true,
            (Some(Type::Union(items)), _) => items
                .iter()
                .copied()
                .any(|item| self.type_matches_typeof(item, kind)),
            _ => false,
        }
    }

    /// Return whether two branch types can be represented by one callable shape.
    fn compatible_function_branch_types(
        &self,
        left: smelt_hir::TypeId,
        right: smelt_hir::TypeId,
    ) -> bool {
        let left = self.type_param_constraint_or_self(left);
        let right = self.type_param_constraint_or_self(right);
        let (Some(Type::Function(left_fn)), Some(Type::Function(right_fn))) =
            (self.ctx.krate.types.get(left), self.ctx.krate.types.get(right))
        else {
            return false;
        };
        left_fn.params.len() == right_fn.params.len() && left_fn.is_async == right_fn.is_async
    }

    /// Return the callable branch when the other branch is imprecise metadata.
    fn single_function_branch_type(
        &self,
        left: smelt_hir::TypeId,
        right: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let left = self.type_param_constraint_or_self(left);
        let right = self.type_param_constraint_or_self(right);
        match (self.ctx.krate.types.get(left), self.ctx.krate.types.get(right)) {
            (Some(Type::Function(_)), Some(Type::Unknown | Type::Class { .. })) => Some(left),
            (Some(Type::Unknown | Type::Class { .. }), Some(Type::Function(_))) => Some(right),
            _ => None,
        }
    }

    /// Recognize `value === undefined/null` guards whose true branch exits.
    fn optional_none_inverse_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality | BinaryOperator::Equality
        ) {
            return None;
        }
        let name = match (&binary.left, &binary.right) {
            (Expression::Identifier(identifier), Expression::Identifier(undefined))
                if undefined.name == "undefined" =>
            {
                identifier.name.as_str()
            }
            (Expression::Identifier(identifier), Expression::NullLiteral(_)) => {
                identifier.name.as_str()
            }
            _ => return None,
        };
        let local = self.locals.get(name).copied()?;
        let local_ty = self.narrowed_type(name).unwrap_or_else(|| Self::local_ty(body, local));
        match self.ctx.krate.types.get(local_ty).cloned() {
            Some(Type::Optional(inner)) => Some((name.to_owned(), inner)),
            Some(Type::Union(items)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let remaining = items
                    .into_iter()
                    .filter(|item| *item != none_ty)
                    .collect::<Vec<_>>();
                match remaining.as_slice() {
                    [single] => Some((name.to_owned(), *single)),
                    [] => None,
                    _ => Some((
                        name.to_owned(),
                        self.ctx.krate.types.intern(Type::Union(remaining)),
                    )),
                }
            }
            _ => None,
        }
    }

    /// Return whether a source statement always exits the current flow path.
    fn statement_must_exit(statement: &Statement<'_>) -> bool {
        match statement {
            Statement::ReturnStatement(_)
            | Statement::BreakStatement(_)
            | Statement::ContinueStatement(_)
            | Statement::ThrowStatement(_) => true,
            Statement::BlockStatement(block) => block
                .body
                .last()
                .is_some_and(Self::statement_must_exit),
            Statement::IfStatement(if_stmt) => if_stmt.alternate.as_ref().is_some_and(|alternate| {
                Self::statement_must_exit(&if_stmt.consequent)
                    && Self::statement_must_exit(alternate)
            }),
            _ => false,
        }
    }

    /// Recognize `typeof value === "kind"` guard expressions.
    fn typeof_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
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
            "function" => {
                let local_ty = self
                    .locals
                    .get(identifier.name.as_str())
                    .map(|local| Self::local_ty(body, *local));
                local_ty
                    .and_then(|ty| self.function_member_type(ty))
                    .unwrap_or_else(|| {
                        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                        self.ctx.krate.types.intern(Type::Function(FunctionType {
                            params: vec![unknown_ty],
                            return_ty: unknown_ty,
                            is_async: false,
                        }))
                    })
            }
            "object" => self.ctx.krate.types.intern(Type::Unknown),
            _ => return None,
        };
        Some((identifier.name.to_string(), ty))
    }

    /// Extract a callable member from a union or function type.
    fn function_member_type(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        let resolved_ty = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(resolved_ty) {
            Some(Type::Function(_)) => Some(resolved_ty),
            Some(Type::Optional(item)) => self.function_member_type(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .find_map(|item| self.function_member_type(item)),
            _ => None,
        }
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

    /// Recognize a call to a user-defined `value is T` predicate function.
    fn predicate_call_guard(&self, expression: &Expression<'_>) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        let predicate = self.predicate_functions.get(callee.name.as_str())?;
        let arg = call.arguments.get(predicate.param_index)?;
        let Argument::Identifier(identifier) = arg else {
            return None;
        };
        Some((identifier.name.to_string(), predicate.target))
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

    /// Recognize bare truthiness guards that remove `undefined`/`None` from locals.
    fn truthy_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::Identifier(identifier) = expression else {
            return None;
        };
        let name = identifier.name.as_str();
        let local = self.locals.get(name).copied()?;
        let local_ty = self.narrowed_type(name).unwrap_or_else(|| Self::local_ty(body, local));
        match self.ctx.krate.types.get(local_ty).cloned() {
            Some(Type::Optional(inner)) => Some((name.to_owned(), inner)),
            Some(Type::Union(items)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let remaining = items
                    .into_iter()
                    .filter(|item| *item != none_ty)
                    .collect::<Vec<_>>();
                match remaining.as_slice() {
                    [single] => Some((name.to_owned(), *single)),
                    [] => None,
                    _ => Some((
                        name.to_owned(),
                        self.ctx.krate.types.intern(Type::Union(remaining)),
                    )),
                }
            }
            _ => None,
        }
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
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init
            {
                let annotated_ty = declarator
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?;
                self.local_arrow_callback_declaration(
                    binding.name.as_str(),
                    binding.span.start,
                    binding.span.end,
                    arrow,
                    annotated_ty,
                    body,
                )?;
                continue;
            }
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
            self.binding_declaration(
                &declarator.id,
                value,
                annotated_ty,
                matches!(declarator.kind, oxc::ast::ast::VariableDeclarationKind::Let),
                body,
                block,
            )?;
        }
        Ok(())
    }

    /// Lower a local arrow function variable as a non-escaping closure value.
    fn local_arrow_callback_declaration(
        &mut self,
        name: &str,
        start: u32,
        end: u32,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        self.push_type_parameter_scope(arrow.type_parameters.as_deref())?;
        let result = (|| {
        let contextual_function = type_hint.and_then(|hint| {
            let function_hint = self.function_member_type(hint).unwrap_or(hint);
            if let Some(Type::Function(function)) = self.ctx.krate.types.get(function_hint) {
                Some(function.clone())
            } else {
                None
            }
        });
        let params = self.arrow_callback_param_types_with_hint(arrow, contextual_function.as_ref())?;
        let defaults = arrow
            .params
            .items
            .iter()
            .map(|param| {
                param.initializer
                    .as_ref()
                    .map(|default| self.expression_with_hint(default, body, None))
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let rest = arrow.params.rest.as_ref().and_then(|_rest| {
            let index = params.len().checked_sub(1)?;
            let param_ty = params.get(index).copied()?;
            let item_ty = match self.ctx.krate.types.get(param_ty) {
                Some(Type::List(item_ty)) => *item_ty,
                _ => param_ty,
            };
            Some(RestParam { index, item_ty })
        });
        let mut closure_defaults = defaults;
        if rest.is_some() {
            closure_defaults.push(None);
        }
        let return_ty = arrow
            .return_type
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?;
        let callback_result = match self.arrow_return_expression(arrow) {
            Ok(Expression::CallExpression(_)) => Err(SmeltError::unsupported(
                self.span(arrow.span.start, arrow.span.end),
                "call-bodied local arrows lower through closure bodies",
            )),
            _ => self.arrow_callback_from_params(arrow, &params, body),
        };
        let return_ty = return_ty
            .or_else(|| contextual_function.as_ref().map(|function| function.return_ty))
            .unwrap_or_else(|| {
            callback_result.as_ref().map_or_else(
                |_| self.ctx.krate.types.intern(Type::Unknown),
                |callback| callback.ty,
            )
        });
        let symbol = self.intern_source_name(name);
        let fn_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: params.clone(),
            return_ty,
            is_async: false,
        }));
        if let Ok(callback) = callback_result {
            let expected_callback_ty = match self.ctx.krate.types.get(return_ty) {
                Some(Type::Future(inner)) if arrow.r#async => *inner,
                _ => return_ty,
            };
            if !self.local_callback_return_type_compatible(callback.ty, expected_callback_ty) {
                return Err(SmeltError::unsupported(
                    self.span(arrow.span.start, arrow.span.end),
                    format!(
                        "local closure return type does not match its annotation: actual {:?}, expected {:?}",
                        self.ctx.krate.types.get(callback.ty),
                        self.ctx.krate.types.get(expected_callback_ty)
                    ),
                ));
            }
            let local = body.push_local(LocalDecl {
                name: Some(symbol),
                ty: fn_ty,
                mutable: false,
                span: self.span(start, end),
            });
            self.locals.insert(name.to_owned(), local);
            self.local_callbacks.insert(
                name.to_owned(),
                LocalCallback {
                    callback,
                    params,
                    defaults: closure_defaults,
                    rest,
                    return_ty,
                },
            );
            return Ok(());
        }
        let value = self.arrow_closure_body_expr(arrow, &params, return_ty, body)?;
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty: fn_ty,
            mutable: false,
            span: self.span(start, end),
        });
        self.locals.insert(name.to_owned(), local);
        let pat = body.push_pattern(Pattern::Binding(local));
        body.push_stmt(Stmt::Let {
            pat,
            ty: fn_ty,
            value: Some(value),
        });
        Ok(())
        })();
        self.pop_type_parameter_scope();
        result
    }

    /// Return whether a lowered local callback body satisfies its declared return type.
    fn local_callback_return_type_compatible(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        actual == expected
            || matches!(self.ctx.krate.types.get(expected), Some(Type::Class { .. }))
            || matches!(self.ctx.krate.types.get(expected), Some(Type::TypeParam { .. }))
            || matches!(
                self.ctx.krate.types.get(expected),
                Some(Type::Optional(inner)) if *inner == actual
            )
            || matches!(self.ctx.krate.types.get(actual), Some(Type::Unknown))
            || matches!(
                (self.ctx.krate.types.get(actual), self.ctx.krate.types.get(expected)),
                (Some(Type::Function(_)), Some(Type::Function(_)))
            )
    }

    /// Lower a binding pattern in a variable declaration.
    fn binding_declaration(
        &mut self,
        pattern: &BindingPattern<'_>,
        value: Option<smelt_hir::ExprId>,
        annotated_ty: Option<smelt_hir::TypeId>,
        mutable: bool,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                let ty = annotated_ty
                    .or_else(|| value.map(|expr_id| Self::expr_ty(body, expr_id)))
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::None));
                if annotated_ty.is_some()
                    && value.is_some()
                    && matches!(self.ctx.krate.types.get(ty), Some(Type::Never))
                {
                    return Err(SmeltError::unsupported(
                        self.span(binding.span.start, binding.span.end),
                        "variable annotation `never` requires a diverging initializer",
                    ));
                }
                let name = binding.name.as_str();
                let symbol = self.ctx.krate.symbols.intern(&camel_to_snake(name));
                self.ctx.krate.names.record(symbol, name);
                let local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty,
                    mutable,
                    span: self.span(binding.span.start, binding.span.end),
                });
                self.locals.insert(name.to_owned(), local);
                let pat = body.push_pattern(Pattern::Binding(local));
                body.push_stmt_to_block(block, Stmt::Let { pat, ty, value });
                Ok(())
            }
            BindingPattern::ObjectPattern(object) => {
                let Some(receiver) = value else {
                    return Err(SmeltError::unsupported(
                        self.span(object.span.start, object.span.end),
                        "object destructuring requires an initializer",
                    ));
                };
                let mut omitted_keys = Vec::new();
                for property in &object.properties {
                    let (ty, extracted, omitted_key) = if property.computed {
                        let index = self.property_key_index_expression(&property.key, body)?;
                        let ty = self.dynamic_field_type(Self::expr_ty(body, receiver));
                        (
                            ty,
                            body.push_expr(Expr {
                                kind: ExprKind::Index { receiver, index },
                                ty,
                                span: self.span(property.span.start, property.span.end),
                            }),
                            index,
                        )
                    } else {
                        let field = self.property_key_symbol(&property.key)?;
                        let omitted_key =
                            self.object_destructuring_static_key_expr(&property.key, body)?;
                        let ty = self.class_field_type(Self::expr_ty(body, receiver), field)?;
                        (
                            ty,
                            body.push_expr(Expr {
                                kind: ExprKind::Field { receiver, field },
                                ty,
                                span: self.span(property.span.start, property.span.end),
                            }),
                            omitted_key,
                        )
                    };
                    omitted_keys.push(omitted_key);
                    self.binding_declaration(
                        &property.value,
                        Some(extracted),
                        Some(ty),
                        mutable,
                        body,
                        block,
                    )?;
                }
                if let Some(rest) = &object.rest {
                    self.object_rest_binding_declaration(
                        &rest.argument,
                        receiver,
                        &omitted_keys,
                        body,
                        block,
                    )?;
                }
                Ok(())
            }
            BindingPattern::ArrayPattern(array) => {
                let Some(receiver) = value else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "array destructuring requires an initializer",
                    ));
                };
                let receiver_ty = Self::expr_ty(body, receiver);
                let tuple_items = match self.ctx.krate.types.get(receiver_ty).cloned() {
                    Some(Type::Tuple(items)) => Some(items),
                    _ => None,
                };
                let fallback_item_ty = if tuple_items.is_none() {
                    Some(self.index_type(receiver_ty)?)
                } else {
                    None
                };
                for (idx, element) in array.elements.iter().enumerate() {
                    let Some(element) = element else {
                        continue;
                    };
                    let item_ty = tuple_items
                        .as_ref()
                        .and_then(|items| items.get(idx).copied())
                        .or(fallback_item_ty)
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(element.span().start, element.span().end),
                                "array destructuring index is outside tuple type",
                            )
                        })?;
                    let idx = u32::try_from(idx).map_err(|err| {
                        SmeltError::unsupported(
                            self.span(array.span.start, array.span.end),
                            format!("array destructuring index is too large: {err}"),
                        )
                    })?;
                    let index_ty = self.ctx.krate.types.intern(Type::Float);
                    let index = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::Float(f64::from(idx))),
                        ty: index_ty,
                        span: self.span(array.span.start, array.span.end),
                    });
                    let extracted = body.push_expr(Expr {
                        kind: ExprKind::Index { receiver, index },
                        ty: item_ty,
                        span: self.span(element.span().start, element.span().end),
                    });
                    self.binding_declaration(
                        element,
                        Some(extracted),
                        Some(item_ty),
                        mutable,
                        body,
                        block,
                    )?;
                }
                if let Some(rest) = &array.rest {
                    let start = array.elements.len();
                    let (rest_ty, extracted) = if let Some(items) = &tuple_items {
                        let end = items.len();
                        let selected = if start <= end {
                            items.get(start..).unwrap_or_default().to_vec()
                        } else {
                            Vec::new()
                        };
                        let ty = self.ctx.krate.types.intern(Type::Tuple(selected));
                        (
                            ty,
                            body.push_expr(Expr {
                                kind: ExprKind::TupleSlice {
                                    tuple: receiver,
                                    start,
                                    end,
                                },
                                ty,
                                span: self.span(rest.span.start, rest.span.end),
                            }),
                        )
                    } else {
                        let item_ty = self.index_type(receiver_ty)?;
                        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                        let start_index = u32::try_from(start).map_err(|err| {
                            SmeltError::unsupported(
                                self.span(array.span.start, array.span.end),
                                format!("array destructuring rest index is too large: {err}"),
                            )
                        })?;
                        let index_ty = self.ctx.krate.types.intern(Type::Float);
                        let start_expr = body.push_expr(Expr {
                            kind: ExprKind::Literal(Literal::Float(f64::from(start_index))),
                            ty: index_ty,
                            span: self.span(rest.span.start, rest.span.end),
                        });
                        (
                            ty,
                            body.push_expr(Expr {
                                kind: ExprKind::ListSlice {
                                    list: receiver,
                                    start: Some(start_expr),
                                    end: None,
                                },
                                ty,
                                span: self.span(rest.span.start, rest.span.end),
                            }),
                        )
                    };
                    self.binding_declaration(
                        &rest.argument,
                        Some(extracted),
                        Some(rest_ty),
                        mutable,
                        body,
                        block,
                    )?;
                }
                Ok(())
            }
            BindingPattern::AssignmentPattern(assign) => self.binding_declaration(
                &assign.left,
                value,
                annotated_ty,
                mutable,
                body,
                block,
            ),
        }
    }

    /// Create a string key expression for a static object destructuring property.
    fn object_destructuring_static_key_expr(
        &mut self,
        key: &PropertyKey<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let (text, start, end) = match key {
            PropertyKey::StaticIdentifier(identifier) => (
                identifier.name.as_str().to_owned(),
                identifier.span.start,
                identifier.span.end,
            ),
            PropertyKey::PrivateIdentifier(identifier) => (
                identifier.name.as_str().to_owned(),
                identifier.span.start,
                identifier.span.end,
            ),
            PropertyKey::StringLiteral(literal) => (
                literal.value.to_string(),
                literal.span.start,
                literal.span.end,
            ),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(key.span().start, key.span().end),
                    "object destructuring rest keys must be static identifiers or string literals",
                ));
            }
        };
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(text)),
            ty,
            span: self.span(start, end),
        }))
    }

    /// Bind an object rest pattern by copying the receiver and deleting extracted keys.
    fn object_rest_binding_declaration(
        &mut self,
        pattern: &BindingPattern<'_>,
        receiver: smelt_hir::ExprId,
        omitted_keys: &[smelt_hir::ExprId],
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let rest_ty = if let Some(Type::Dict(_, _)) =
            self.ctx.krate.types.get(Self::expr_ty(body, receiver))
        {
            Self::expr_ty(body, receiver)
        } else {
            let key_ty = self.ctx.krate.types.intern(Type::String);
            let value_ty = self.ctx.krate.types.intern(Type::Unknown);
            self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
        };
        let receiver_span = Self::body_expr_span(body, receiver);
        let rest_value = body.push_expr(Expr {
            kind: ExprKind::DictCopy { dict: receiver },
            ty: rest_ty,
            span: receiver_span,
        });
        for key in omitted_keys {
            let key_span = Self::body_expr_span(body, *key);
            let removed = body.push_expr(Expr {
                kind: ExprKind::DictRemoveKey {
                    dict: rest_value,
                    key: *key,
                },
                ty: self.ctx.krate.types.intern(Type::Bool),
                span: key_span,
            });
            body.push_stmt_to_block(block, Stmt::Expr(removed));
        }
        self.binding_declaration(pattern, Some(rest_value), Some(rest_ty), false, body, block)
    }

    /// Return the span for an expression already inserted into a body.
    fn body_expr_span(body: &Body, expr: smelt_hir::ExprId) -> Span {
        let root_span = usize::try_from(body.root.0)
            .ok()
            .and_then(|index| body.blocks.get(index))
            .or_else(|| body.blocks.first())
            .map_or(Span::new(FileId(0), 0, 0), |block| block.span);
        usize::try_from(expr.0)
            .ok()
            .and_then(|index| body.exprs.get(index))
            .map_or(root_span, |expr| expr.span)
    }

    /// Collect source names introduced by a binding pattern.
    fn binding_pattern_names(pattern: &BindingPattern<'_>, names: &mut Vec<String>) {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                names.push(binding.name.as_str().to_owned());
            }
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    Self::binding_pattern_names(&property.value, names);
                }
                if let Some(rest) = &object.rest {
                    Self::binding_pattern_names(&rest.argument, names);
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for element in array.elements.iter().flatten() {
                    Self::binding_pattern_names(element, names);
                }
                if let Some(rest) = &array.rest {
                    Self::binding_pattern_names(&rest.argument, names);
                }
            }
            BindingPattern::AssignmentPattern(assign) => {
                Self::binding_pattern_names(&assign.left, names);
            }
        }
    }

    // Continued in the next split builder file.
}
