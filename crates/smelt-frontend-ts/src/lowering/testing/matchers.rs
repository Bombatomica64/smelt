//! `ModuleBuilder` lowering methods (part 06): assertion/expectation lowering
//! and related HIR construction helpers split out of `lowering.rs`.

use crate::lowering::{LocalCallback, LocalCallbackDefault, ModuleBuilder, RestParam, TestMatcher};
use crate::{SmeltError, camel_to_snake};
use oxc::ast::ast::{Argument, BindingPattern, Expression, PropertyKey, Statement};
use oxc::span::GetSpan;
use oxc::syntax::operator::{BinaryOperator, LogicalOperator, UnaryOperator};
use smelt_hir::{
    BinOp, Body, CallbackExpr, CallbackExprKind, CaptureMode, ClosureCapture, Expr, ExprKind,
    FileId, FunctionType, Literal, LocalDecl, Param, Pattern, Span, Stmt, Type, UnaryOp,
};
use std::collections::{HashMap, HashSet};

impl ModuleBuilder<'_> {
    /// Lower a Node `assert.deepStrictEqual` call statement when one is present.
    pub(in crate::lowering) fn deep_strict_equal_statement(
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
    pub(in crate::lowering) fn node_assert_statement(
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
    pub(in crate::lowering) fn expect_matcher_statement(
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
        if member.property.name == "toHaveBeenCalledTimes" {
            let (expect_call, _) = self.expect_call_from_matcher_object(&member.object)?;
            let Expression::Identifier(expect_ident) = &expect_call.callee else {
                return Ok(false);
            };
            if !self.test_builtins.contains(expect_ident.name.as_str())
                || expect_ident.name.as_str() != "expect"
            {
                return Ok(false);
            }
            if let Some(actual_arg) = expect_call.arguments.first() {
                let _actual = self.argument(actual_arg, body)?;
            }
            if let Some(expected_arg) = call.arguments.first() {
                let _expected = self.argument(expected_arg, body)?;
            }
            return Ok(true);
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
        // Contextually type the expected value from the actual's type for
        // deep-equality matchers. An expected literal such as
        // `[[1, 'a'], [2, 'b']]` in `expect(zip(...)).toEqual([...])` would
        // otherwise infer as `SmeltList<SmeltList<SmeltUnknown>>` and fail to
        // compare against the actual's `SmeltList<(f64, String)>` (E0308).
        // `array_expression`'s arity guard ignores tuple hints whose arity does
        // not match the literal, so ragged expected values are unaffected.
        let expected = if matches!(matcher, TestMatcher::Equal | TestMatcher::StrictEqual) {
            let actual_ty = Self::expr_ty(body, actual);
            self.argument_with_hint(expected_arg, body, Some(actual_ty))?
        } else {
            self.argument(expected_arg, body)?
        };
        let use_strict_identity = matcher == TestMatcher::Be
            && self.test_to_be_needs_strict_identity(actual, expected, body);
        let mut failed = if use_strict_identity {
            let op = if inverted {
                BinOp::StrictEq
            } else {
                BinOp::StrictNotEq
            };
            self.comparison_expr(op, actual, expected, call.span, body)
        } else {
            self.expect_matcher_failure_expr(matcher, actual, expected, call.span, body)?
        };
        if use_strict_identity {
            self.push_test_failure_if(
                failed,
                &format!("expect(...).{}(...) failed", matcher.source_name()),
                call.span,
                body,
            );
            return Ok(true);
        }
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

    /// Return whether Vitest `toBe` needs JavaScript `SameValue` semantics.
    pub(in crate::lowering) fn test_to_be_needs_strict_identity(
        &self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        body: &Body,
    ) -> bool {
        if Self::test_to_be_nan_literal(actual, body)
            || Self::test_to_be_nan_literal(expected, body)
        {
            return true;
        }
        let actual_ty = self.type_param_constraint_or_self(Self::expr_ty(body, actual));
        let expected_ty = self.type_param_constraint_or_self(Self::expr_ty(body, expected));
        if matches!(
            self.ctx.krate.types.get(actual_ty),
            Some(Type::Int | Type::Float)
        ) && matches!(
            self.ctx.krate.types.get(expected_ty),
            Some(Type::Int | Type::Float)
        ) {
            return true;
        }
        let actual_ref = self.test_to_be_identity_type(actual_ty);
        let expected_ref = self.test_to_be_identity_type(expected_ty);
        if actual_ref || expected_ref {
            return true;
        }
        self.test_to_be_erased_type(actual_ty) && self.test_to_be_erased_type(expected_ty)
    }

    /// Return whether an assertion operand is the JavaScript `NaN` literal.
    pub(in crate::lowering) fn test_to_be_nan_literal(
        value: smelt_hir::ExprId,
        body: &Body,
    ) -> bool {
        matches!(
            usize::try_from(value.0)
                .ok()
                .and_then(|index| body.exprs.get(index)),
            Some(Expr {
                kind: ExprKind::Literal(Literal::Float(number)),
                ..
            }) if number.is_nan()
        )
    }

    /// Return whether a type has reference identity under JavaScript `toBe`.
    pub(in crate::lowering) fn test_to_be_identity_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => self.test_to_be_identity_type(*inner),
            Some(
                Type::List(_)
                | Type::Dict(_, _)
                | Type::Set(_)
                | Type::Tuple(_)
                | Type::Class { .. }
                | Type::Function(_),
            ) => true,
            _ => false,
        }
    }

    /// Return whether a type is erased enough that `toBe` must defer to runtime.
    pub(in crate::lowering) fn test_to_be_erased_type(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        )
    }

    /// Lower nullish zero-argument matchers to strict singleton checks.
    pub(in crate::lowering) fn expect_to_be_none_statement(
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
        let mut actual = self.argument(actual_arg, body)?;
        let actual_ty = Self::expr_ty(body, actual);
        if !matches!(self.ctx.krate.types.get(actual_ty), Some(Type::Optional(_)))
            && self.assertion_type_contains_unknown(actual_ty)
        {
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
            actual = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: actual },
                ty: unknown_ty,
                span: self.span(actual_arg.span().start, actual_arg.span().end),
            });
        }
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let literal = if matcher_name == "toBeUndefined" {
            Literal::Undefined
        } else {
            Literal::None
        };
        let expected = body.push_expr(Expr {
            kind: ExprKind::Literal(literal),
            ty: none_ty,
            span: self.span(call.span.start, call.span.end),
        });
        let mut failed =
            self.comparison_expr(BinOp::JsStrictNotEq, actual, expected, call.span, body);
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
    pub(in crate::lowering) fn expect_call_from_matcher_object<'a>(
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
    pub(in crate::lowering) fn expect_to_throw_statement(
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
        let did_throw_stmt = Stmt::Let {
            pat: did_throw_pat,
            ty: bool_ty,
            value: Some(false_expr),
        };
        if let Some(block) = self.current_statement_block {
            body.push_stmt_to_block(block, did_throw_stmt);
        } else {
            body.push_stmt(did_throw_stmt);
        }

        let try_block = if let Argument::ArrowFunctionExpression(arrow) = actual_arg {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            let mut callee = self.arrow_closure_body_expr(arrow, &[], none_ty, body)?;
            let function = FunctionType {
                params: Vec::new(),
                rest: None,
                required_params: Some(0),
                mutable_params: Vec::new(),
                return_ty: none_ty,
                is_async: false,
                may_throw: true,
            };
            let throwing_function_ty = self.ctx.krate.types.intern(Type::Function(function));
            callee = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: throwing_function_ty,
                span: self.span(arrow.span.start, arrow.span.end),
            });
            let try_block = body.push_block(self.span(arrow.body.span.start, arrow.body.span.end));
            let call_expr = body.push_expr(Expr {
                kind: ExprKind::ClosureCall {
                    callee,
                    args: Vec::new(),
                },
                ty: none_ty,
                span: self.span(arrow.span.start, arrow.span.end),
            });
            body.push_stmt_to_block(try_block, Stmt::Expr(call_expr));
            try_block
        } else {
            let mut callee = self.argument(actual_arg, body)?;
            let callee_ty = self
                .ctx
                .krate
                .types
                .get(Self::expr_ty(body, callee))
                .cloned();
            let mut function = match callee_ty {
                Some(Type::Function(function)) => function,
                // `expect(value).toThrow()` may name a callable whose static
                // shape is erased here (a cross-module helper such as
                // `once(...)` resolves to `Unknown` under single-file lowering).
                // It is still a zero-argument callable observed only for whether
                // it throws, so adapt it through a synthesized throwing signature
                // rather than rejecting the matcher.
                Some(Type::Unknown) => FunctionType {
                    params: Vec::new(),
                    rest: None,
                    required_params: Some(0),
                    mutable_params: Vec::new(),
                    return_ty: self.ctx.krate.types.intern(Type::None),
                    is_async: false,
                    may_throw: true,
                },
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(actual_arg.span().start, actual_arg.span().end),
                        "expect(...).toThrow(...) requires a zero-argument callback",
                    ));
                }
            };
            if Self::is_bind_call_argument(actual_arg) {
                function.params.clear();
            }
            // `toThrow` observes the callable's error result. At this call site a
            // thrown value must flow to the synthesized catch block even when the
            // callable's source signature omitted a throwing annotation.
            function.may_throw = true;
            let throwing_function_ty = self
                .ctx
                .krate
                .types
                .intern(Type::Function(function.clone()));
            callee = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: throwing_function_ty,
                span: self.span(actual_arg.span().start, actual_arg.span().end),
            });
            let try_block =
                body.push_block(self.span(actual_arg.span().start, actual_arg.span().end));
            let mut args = Vec::new();
            for param_ty in function.params {
                if !matches!(self.ctx.krate.types.get(param_ty), Some(Type::Optional(_))) {
                    return Err(SmeltError::unsupported(
                        self.span(actual_arg.span().start, actual_arg.span().end),
                        "expect(...).toThrow(...) requires a zero-argument callback",
                    ));
                }
                args.push(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: param_ty,
                    span: self.span(actual_arg.span().start, actual_arg.span().end),
                }));
            }
            let call_expr = body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: function.return_ty,
                span: self.span(actual_arg.span().start, actual_arg.span().end),
            });
            body.push_stmt_to_block(try_block, Stmt::Expr(call_expr));
            try_block
        };
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
        let try_stmt = Stmt::TryCatch {
            body: try_block,
            catch_binding: None,
            catch_body: Some(catch_block),
            finally_body: None,
        };
        if let Some(block) = self.current_statement_block {
            body.push_stmt_to_block(block, try_stmt);
        } else {
            body.push_stmt(try_stmt);
        }

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

    /// Return whether an `expect(...).toThrow()` argument is a bound function call.
    pub(in crate::lowering) fn is_bind_call_argument(argument: &Argument<'_>) -> bool {
        let Some(Expression::CallExpression(call)) = argument.as_expression() else {
            return false;
        };
        matches!(
            &call.callee,
            Expression::StaticMemberExpression(member) if member.property.name == "bind"
        )
    }

    /// Build the boolean expression that means a supported matcher has failed.
    pub(in crate::lowering) fn expect_matcher_failure_expr(
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
            TestMatcher::BeInstanceOf => {
                let _ = expected;
                let bool_ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(false)),
                    ty: bool_ty,
                    span: self.span(span.start, span.end),
                }))
            }
        }
    }

    /// Push a throwing failure block guarded by a boolean condition.
    pub(in crate::lowering) fn push_test_failure_if(
        &mut self,
        cond: smelt_hir::ExprId,
        message: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) {
        let failure_block = body.push_block(self.span(span.start, span.end));
        let message = self.string_literal_expr(message, span, body);
        body.push_stmt_to_block(failure_block, Stmt::Throw(message));
        let failure_stmt = Stmt::If {
            cond,
            then_block: failure_block,
            else_block: None,
        };
        if let Some(block) = self.current_statement_block {
            body.push_stmt_to_block(block, failure_stmt);
        } else {
            body.push_stmt(failure_stmt);
        }
    }

    /// Create a boolean unary expression for synthesized test assertions.
    pub(in crate::lowering) fn unary_bool_expr(
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
    pub(in crate::lowering) fn comparison_expr(
        &mut self,
        op: BinOp,
        lhs: smelt_hir::ExprId,
        mut rhs: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let lhs_ty = Self::expr_ty(body, lhs);
        let rhs_ty = Self::expr_ty(body, rhs);
        let compares_optional_to_none = matches!(
            (
                self.ctx.krate.types.get(lhs_ty),
                self.ctx.krate.types.get(rhs_ty)
            ),
            (Some(Type::Optional(_)), Some(Type::None))
        );
        if lhs_ty != rhs_ty
            && self.assertion_type_contains_unknown(lhs_ty)
            && !compares_optional_to_none
        {
            rhs = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: rhs },
                ty: lhs_ty,
                span: self.span(span.start, span.end),
            });
        }
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        body.push_expr(Expr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty: bool_ty,
            span: self.span(span.start, span.end),
        })
    }

    /// Return whether an assertion actual type contains erased unknown values.
    ///
    /// This is intentionally local to test assertions. The broader lowering
    /// pipeline still uses the narrower `type_contains_unknown` helper because
    /// treating every `Array<unknown>` as unknown-like changes overload choices
    /// in normal library code.
    pub(in crate::lowering) fn assertion_type_contains_unknown(
        &self,
        ty: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Unknown | Type::TypeParam { .. }) => true,
            Some(
                Type::Optional(item) | Type::List(item) | Type::Set(item) | Type::Future(item),
            ) => self.assertion_type_contains_unknown(*item),
            Some(Type::Dict(key, value)) => {
                self.assertion_type_contains_unknown(*key)
                    || self.assertion_type_contains_unknown(*value)
            }
            Some(Type::Tuple(items) | Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.assertion_type_contains_unknown(item)),
            _ => false,
        }
    }

    /// Create a length expression for synthesized test assertions.
    pub(in crate::lowering) fn len_expr(
        &mut self,
        operand: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let operand_ty = Self::expr_ty(body, operand);
        match self.ctx.krate.types.get(operand_ty) {
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
    pub(in crate::lowering) fn contains_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let expected_ty = Self::expr_ty(body, expected);
        // The expected value may be erased (`Unknown`) when it comes from a
        // cross-module helper, as in `expect(array).toContain(sample(array))`.
        // A concrete collection actual still supports the runtime containment
        // check against such a value, so an erased expected matches any item
        // type rather than forcing a static element-type equality.
        let expected_is_erased =
            matches!(self.ctx.krate.types.get(expected_ty), Some(Type::Unknown));
        // A `sample(...)`-style helper returns `T | undefined`, so the expected
        // needle is commonly an `Optional(T)` while the actual collection holds
        // `T` (`expect(array).toContain(sample(array))`). JavaScript containment
        // compares the needle against each element regardless of the needle's
        // nullability, so an optional expected whose inner type matches the
        // element type (or is itself erased) is accepted here; the emitter guards
        // the optional at runtime (a `None`/`undefined` needle is never contained
        // in a collection of non-optional elements).
        // Unwrap an optional expected to its inner type; a `None`/`undefined`
        // needle simply never matches, and the emitter guards it at runtime. The
        // containment is supported when the inner type is exactly the element
        // type, or when the inner type is erased (`unknown`/leaked type param):
        // an erased needle is compared against each element after the element is
        // itself erased to the runtime value (JS `includes`/`has` semantics),
        // exactly like a bare erased expected.
        let expected_inner = match self.ctx.krate.types.get(expected_ty) {
            Some(Type::Optional(inner)) => Some(*inner),
            _ => None,
        };
        let expected_inner_is_erased = expected_inner.is_some_and(|inner| {
            matches!(
                self.ctx.krate.types.get(inner),
                Some(Type::Unknown | Type::TypeParam { .. })
            )
        });
        let item_matches = |item_ty: smelt_hir::TypeId| {
            expected_ty == item_ty
                || expected_is_erased
                || expected_inner == Some(item_ty)
                || expected_inner_is_erased
        };
        let kind = match self.ctx.krate.types.get(Self::expr_ty(body, actual)) {
            Some(Type::String)
                if self.ctx.krate.types.get(expected_ty) == Some(&Type::String)
                    || expected_is_erased =>
            {
                ExprKind::StringContains {
                    haystack: actual,
                    needle: expected,
                    from_index: None,
                }
            }
            Some(Type::List(item_ty)) if item_matches(*item_ty) => ExprKind::ListContains {
                list: actual,
                item: expected,
            },
            Some(Type::Set(item_ty)) if item_matches(*item_ty) => ExprKind::SetContains {
                set: actual,
                item: expected,
            },
            Some(Type::Tuple(items)) if items.iter().any(|item| item_matches(*item)) => {
                ExprKind::TupleContains {
                    tuple: actual,
                    item: expected,
                }
            }
            // The actual may itself be erased (`Unknown`/leaked type param) when
            // it comes from a cross-module helper whose return type does not
            // resolve in this lowering unit (`expect(keysIn(buffer)).toContain(k)`).
            // JavaScript containment inspects the live value, so project the
            // erased actual to an erased list and erase the needle; the emitted
            // runtime projection panics if the value is not an array, matching
            // how other matchers treat erased actuals.
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
                let matcher_span = self.span(span.start, span.end);
                let list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: actual },
                    ty: list_ty,
                    span: matcher_span,
                });
                let item = if expected_ty == unknown_ty {
                    expected
                } else {
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: expected },
                        ty: unknown_ty,
                        span: matcher_span,
                    })
                };
                ExprKind::ListContains { list, item }
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
    ///
    /// A statically-typed record/map (`Type::Dict`) checks key membership
    /// directly and requires the key type to match. An erased actual
    /// (`Unknown`/`Union`/unconstrained type param, or a class-shaped type
    /// with no local declaration such as the ambient `IArguments` interface —
    /// see [`Self::class_type_erases_to_unknown`]) is a runtime JavaScript
    /// value — for example the erased return of an imported helper — so the
    /// emitted `DictContainsKey` inspects the live `SmeltUnknown::Object` at
    /// runtime; the key may be any string-convertible value there, so no
    /// static key-type match is demanded. This keeps `toHaveProperty` general
    /// over both concrete records and erased object actuals.
    pub(in crate::lowering) fn dict_contains_key_expr(
        &mut self,
        actual: smelt_hir::ExprId,
        expected: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let actual_ty = Self::expr_ty(body, actual);
        match self.ctx.krate.types.get(actual_ty) {
            Some(Type::Dict(key_ty, _)) => {
                if Self::expr_ty(body, expected) != *key_ty {
                    return Err(SmeltError::unsupported(
                        self.span(span.start, span.end),
                        "expect(...).toHaveProperty(...) key must match the object key type",
                    ));
                }
            }
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {}
            _ if self.class_type_erases_to_unknown(actual_ty) => {}
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(span.start, span.end),
                    format!(
                        "expect(...).toHaveProperty(...) requires an object or map actual value (actual: {:?})",
                        self.ctx.krate.types.get(actual_ty)
                    ),
                ));
            }
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
    pub(in crate::lowering) fn string_literal_expr(
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
    pub(in crate::lowering) fn block_from_statement(
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
    pub(in crate::lowering) fn block_from_block_statement(
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
    pub(in crate::lowering) fn apply_narrowing(&mut self, name: String, target: smelt_hir::TypeId) {
        if let Some(scope) = self.narrowed_locals.last_mut() {
            scope.insert(name, target);
        } else {
            let mut scope = HashMap::new();
            scope.insert(name, target);
            self.narrowed_locals.push(scope);
        }
    }

    /// Push a fresh narrowing scope carrying a single local fact.
    ///
    /// Unlike [`apply_narrowing`], which mutates the current scope, this always
    /// pushes a new scope so callers can pop exactly the fact they added. It is
    /// used for scoped branch facts (such as switch-case discriminant narrowing)
    /// whose lifetime must not outlive the branch body.
    pub(in crate::lowering) fn apply_narrowing_scope(
        &mut self,
        name: String,
        target: smelt_hir::TypeId,
    ) {
        let mut scope = HashMap::new();
        scope.insert(name, target);
        self.narrowed_locals.push(scope);
    }

    /// Return the active narrowed type for a source local, if any.
    pub(in crate::lowering) fn narrowed_type(&self, name: &str) -> Option<smelt_hir::TypeId> {
        self.narrowed_locals
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    /// Discover the narrowing applied by a successful assertion call statement.
    pub(in crate::lowering) fn assertion_call_narrowing(
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
    pub(in crate::lowering) fn guard_narrowing(
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
        } else if let Some((name, target)) = self.array_is_array_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.in_operator_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.property_equality_guard(expression, body) {
            out.insert(name, target);
        } else if let Some((name, target)) = self.instanceof_local_guard(expression, body) {
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
    pub(in crate::lowering) fn inverse_guard_narrowing(
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
    pub(in crate::lowering) fn optional_some_guard(
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
        let local_ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
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
    pub(in crate::lowering) fn typeof_inverse_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let (name, kind, matches_kind) = Self::typeof_comparison(expression)?;
        if matches_kind {
            self.typeof_excluded_type(&name, &kind, body)
        } else {
            self.typeof_matched_type(&name, &kind, body)
        }
    }

    /// Return a type with members matching a `typeof` kind removed.
    pub(in crate::lowering) fn remove_typeof_member(
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
    pub(in crate::lowering) fn type_matches_typeof(
        &self,
        ty: smelt_hir::TypeId,
        kind: &str,
    ) -> bool {
        let resolved_ty = self.type_param_constraint_or_self(ty);
        match (self.ctx.krate.types.get(resolved_ty), kind) {
            (Some(Type::Bool), "boolean")
            | (Some(Type::Float | Type::Int), "number")
            | (Some(Type::String), "string")
            | (Some(Type::Function(_)), "function")
            | (Some(Type::Optional(_)), "undefined")
            | (
                Some(
                    Type::List(_)
                    | Type::Set(_)
                    | Type::Dict(_, _)
                    | Type::Tuple(_)
                    | Type::Class { .. }
                    | Type::Optional(_),
                ),
                "object",
            )
            | (Some(Type::None), "undefined" | "object") => true,
            (Some(Type::Union(items)), _) => items
                .iter()
                .copied()
                .any(|item| self.type_matches_typeof(item, kind)),
            _ => false,
        }
    }

    /// Return whether two branch types can be represented by one callable shape.
    pub(in crate::lowering) fn compatible_function_branch_types(
        &self,
        left: smelt_hir::TypeId,
        right: smelt_hir::TypeId,
    ) -> bool {
        let left = self.type_param_constraint_or_self(left);
        let right = self.type_param_constraint_or_self(right);
        let (Some(Type::Function(left_fn)), Some(Type::Function(right_fn))) = (
            self.ctx.krate.types.get(left),
            self.ctx.krate.types.get(right),
        ) else {
            return false;
        };
        left_fn.params.len() == right_fn.params.len() && left_fn.is_async == right_fn.is_async
    }

    /// Return the callable branch when the other branch is imprecise metadata.
    pub(in crate::lowering) fn single_function_branch_type(
        &self,
        left: smelt_hir::TypeId,
        right: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let left = self.type_param_constraint_or_self(left);
        let right = self.type_param_constraint_or_self(right);
        match (
            self.ctx.krate.types.get(left),
            self.ctx.krate.types.get(right),
        ) {
            (Some(Type::Function(_)), Some(Type::Class { .. })) => Some(left),
            (Some(Type::Class { .. }), Some(Type::Function(_))) => Some(right),
            _ => None,
        }
    }

    /// Recognize `value === undefined/null` guards whose true branch exits.
    pub(in crate::lowering) fn optional_none_inverse_guard(
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
        let local_ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
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
    pub(in crate::lowering) fn statement_must_exit(statement: &Statement<'_>) -> bool {
        match statement {
            Statement::ReturnStatement(_)
            | Statement::BreakStatement(_)
            | Statement::ContinueStatement(_)
            | Statement::ThrowStatement(_) => true,
            Statement::BlockStatement(block) => {
                block.body.last().is_some_and(Self::statement_must_exit)
            }
            Statement::IfStatement(if_stmt) => {
                if_stmt.alternate.as_ref().is_some_and(|alternate| {
                    Self::statement_must_exit(&if_stmt.consequent)
                        && Self::statement_must_exit(alternate)
                })
            }
            _ => false,
        }
    }

    /// Return whether a no-else guarded branch unconditionally reassigns `name`
    /// to a non-nullish value on its taken path.
    ///
    /// This recognizes the ubiquitous default-initialization idiom
    /// `if (x == null) { x = <value>; }`: on the *not-taken* path the nullish
    /// guard's inverse already proves `x` is non-null, and on the *taken* path
    /// this reassignment makes it non-null too, so `x` is non-null after the `if`.
    /// Only a top-level (unconditionally reached) `x = <expr>` statement counts —
    /// an assignment nested inside another branch is not guaranteed to run — and
    /// the assigned value must not be a `null`/`undefined` literal. The last such
    /// top-level assignment wins, matching JavaScript's flow order.
    pub(in crate::lowering) fn branch_reassigns_to_nonnull(
        consequent: &Statement<'_>,
        name: &str,
    ) -> bool {
        let statements = match consequent {
            Statement::BlockStatement(block) => block.body.as_slice(),
            other => std::slice::from_ref(other),
        };
        let mut reassigned_nonnull = false;
        for statement in statements {
            let Statement::ExpressionStatement(expr_stmt) = statement else {
                continue;
            };
            let Expression::AssignmentExpression(assign) = &expr_stmt.expression else {
                continue;
            };
            if assign.operator != oxc::syntax::operator::AssignmentOperator::Assign {
                continue;
            }
            let oxc::ast::ast::AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left
            else {
                continue;
            };
            if target.name != name {
                continue;
            }
            reassigned_nonnull = !Self::expression_is_nullish_literal(&assign.right);
        }
        reassigned_nonnull
    }

    /// Collect the identifier names assigned at the top level of a branch.
    ///
    /// Only unconditionally reached `name = <expr>` statements are gathered
    /// (assignments nested inside a further branch are excluded), so callers
    /// can reason about facts that hold on every path through the branch. Used
    /// with [`branch_reassigns_to_nonnull`] to compute branch-join narrowing.
    pub(in crate::lowering) fn branch_top_level_assigned_names(
        statement: &Statement<'_>,
    ) -> Vec<String> {
        let statements = match statement {
            Statement::BlockStatement(block) => block.body.as_slice(),
            other => std::slice::from_ref(other),
        };
        let mut names = Vec::new();
        for branch_statement in statements {
            let Statement::ExpressionStatement(expr_stmt) = branch_statement else {
                continue;
            };
            let Expression::AssignmentExpression(assign) = &expr_stmt.expression else {
                continue;
            };
            let oxc::ast::ast::AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left
            else {
                continue;
            };
            let name = target.name.to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// Return the non-null type of an optional local currently in scope.
    ///
    /// Reads the local's active (possibly already-narrowed) type and, when it
    /// is `Optional<T>` or a nullable union, returns the type with the
    /// `None`/`undefined` member removed. Returns `None` for locals that are
    /// unknown here or not nullable. Used to narrow a local after both arms of
    /// an if/else reassign it to a non-null value (branch-join narrowing).
    pub(in crate::lowering) fn optional_local_nonnull_type(
        &mut self,
        name: &str,
        body: &Body,
    ) -> Option<smelt_hir::TypeId> {
        let local = self.locals.get(name).copied()?;
        let local_ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        match self.ctx.krate.types.get(local_ty).cloned() {
            Some(Type::Optional(inner)) => Some(inner),
            Some(Type::Union(items)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let remaining = items
                    .into_iter()
                    .filter(|item| *item != none_ty)
                    .collect::<Vec<_>>();
                match remaining.as_slice() {
                    [single] => Some(*single),
                    [] => None,
                    _ => Some(self.ctx.krate.types.intern(Type::Union(remaining))),
                }
            }
            _ => None,
        }
    }

    /// Return whether an expression is a `null` or `undefined` literal.
    fn expression_is_nullish_literal(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::NullLiteral(_) => true,
            Expression::Identifier(identifier) => identifier.name == "undefined",
            Expression::ParenthesizedExpression(paren) => {
                Self::expression_is_nullish_literal(&paren.expression)
            }
            _ => false,
        }
    }

    /// Recognize `typeof value === "kind"` guard expressions.
    pub(in crate::lowering) fn typeof_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let (name, kind, matches_kind) = Self::typeof_comparison(expression)?;
        if matches_kind {
            self.typeof_matched_type(&name, &kind, body)
        } else {
            self.typeof_excluded_type(&name, &kind, body)
        }
    }

    /// Return a normalized local `typeof` comparison.
    ///
    /// JavaScript code commonly writes both `typeof value === "kind"` and
    /// `"kind" !== typeof value`; the boolean indicates whether the expression
    /// proves that the local matches the `typeof` kind.
    pub(in crate::lowering) fn typeof_comparison(
        expression: &Expression<'_>,
    ) -> Option<(String, String, bool)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        let matches_kind = match binary.operator {
            BinaryOperator::StrictEquality | BinaryOperator::Equality => true,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality => false,
            _ => return None,
        };
        let left = Self::typeof_identifier_name(&binary.left);
        let right = Self::typeof_identifier_name(&binary.right);
        match (left, &binary.right, right, &binary.left) {
            (Some(name), Expression::StringLiteral(kind), _, _)
            | (_, _, Some(name), Expression::StringLiteral(kind)) => {
                Some((name, kind.value.to_string(), matches_kind))
            }
            _ => None,
        }
    }

    /// Return a string literal's value, if the expression is one.
    ///
    /// Used to read `switch (typeof x)` case labels (`case 'string':`) as raw
    /// `typeof` kind strings for per-arm narrowing.
    pub(in crate::lowering) fn string_literal_value(expression: &Expression<'_>) -> Option<String> {
        match expression {
            Expression::StringLiteral(literal) => Some(literal.value.to_string()),
            _ => None,
        }
    }

    /// Return the identifier operand of a `typeof name` expression.
    pub(in crate::lowering) fn typeof_identifier_name(
        expression: &Expression<'_>,
    ) -> Option<String> {
        let Expression::UnaryExpression(unary) = expression else {
            return None;
        };
        if unary.operator != UnaryOperator::Typeof {
            return None;
        }
        let Expression::Identifier(identifier) = &unary.argument else {
            return None;
        };
        Some(identifier.name.to_string())
    }

    /// Return the local type proven by a positive `typeof` comparison.
    pub(in crate::lowering) fn typeof_matched_type(
        &mut self,
        name: &str,
        kind: &str,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let ty = match kind {
            "boolean" => self.ctx.krate.types.intern(Type::Bool),
            "number" => self.ctx.krate.types.intern(Type::Float),
            "string" => self.ctx.krate.types.intern(Type::String),
            "function" => {
                let local_ty = self.locals.get(name).map(|local| {
                    self.narrowed_type(name)
                        .unwrap_or_else(|| Self::local_ty(body, *local))
                });
                local_ty
                    .and_then(|ty| self.function_member_type(ty))
                    .unwrap_or_else(|| {
                        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                        self.ctx.krate.types.intern(Type::Function(FunctionType {
                            params: vec![unknown_ty],
                            rest: None,
                            required_params: None,
                            mutable_params: Vec::new(),
                            return_ty: unknown_ty,
                            is_async: false,
                            may_throw: false,
                        }))
                    })
            }
            "undefined" => self.ctx.krate.types.intern(Type::None),
            "object" => self.ctx.krate.types.intern(Type::Unknown),
            _ => return None,
        };
        Some((name.to_owned(), ty))
    }

    /// Return the local type proven by excluding one `typeof` kind.
    pub(in crate::lowering) fn typeof_excluded_type(
        &mut self,
        name: &str,
        kind: &str,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let local = self.locals.get(name).copied()?;
        let local_ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let remaining = self.remove_typeof_member(local_ty, kind)?;
        Some((name.to_owned(), remaining))
    }

    /// Extract a callable member from a union or function type.
    pub(in crate::lowering) fn function_member_type(
        &mut self,
        ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        self.function_member_type_for_arg_count(ty, None)
    }

    /// Extract a callable member matching a supplied argument count when available.
    pub(in crate::lowering) fn function_member_type_for_arg_count(
        &mut self,
        ty: smelt_hir::TypeId,
        arg_count: Option<usize>,
    ) -> Option<smelt_hir::TypeId> {
        let resolved_ty = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(resolved_ty).cloned() {
            Some(Type::Function(_)) => Some(resolved_ty),
            Some(Type::Optional(item)) => self.function_member_type_for_arg_count(item, arg_count),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .find_map(|item| self.function_member_type_for_arg_count(item, arg_count)),
            Some(Type::Class { name, args }) => {
                self.interface_call_signature_type(name, &args, arg_count)
            }
            _ => None,
        }
    }

    /// Instantiate an interface call signature as a HIR function type.
    pub(in crate::lowering) fn interface_call_signature_type(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
        arg_count: Option<usize>,
    ) -> Option<smelt_hir::TypeId> {
        let signatures = self.interface_call_signatures.get(&name).cloned()?;
        let interface = self.find_interface(name).cloned();
        let type_params = interface
            .map(|interface| interface.type_params)
            .unwrap_or_default();
        let substitutions = self
            .type_argument_substitution(&type_params, args, self.span(0, 0))
            .ok()?;
        // Prefer an overload whose declared arity can actually accept the call.
        // A rest parameter or optional trailing parameters relax the exact match,
        // so the requested `arg_count` may sit anywhere in
        // `required_params..=params.len()` (or above it when a rest slot
        // absorbs the surplus). Fall back to an exact match, then the first
        // signature, so callers with no argument count still resolve.
        let signature = signatures
            .iter()
            .find(|signature| {
                arg_count.is_some_and(|count| Self::signature_accepts_arg_count(signature, count))
            })
            .or_else(|| {
                signatures.iter().find(|signature| {
                    arg_count.is_some_and(|count| signature.params.len() == count)
                })
            })
            .or_else(|| signatures.first())?;
        let rest = signature.rest;
        let required_params = signature.required_params;
        let is_async = signature.is_async;
        let params = signature
            .params
            .iter()
            .map(|param| self.substitute_type_params(*param, &substitutions))
            .collect::<Vec<_>>();
        let return_ty = self.substitute_type_params(signature.return_ty, &substitutions);
        let mutable_params = self.mutable_params_from_returned_tuple_state(&params, return_ty);
        Some(self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest,
            required_params,
            mutable_params,
            return_ty,
            is_async,
            may_throw: false,
        })))
    }

    /// Return whether a call-signature overload can accept `arg_count` arguments.
    ///
    /// The requested arity is acceptable when it supplies at least the required
    /// parameters and does not overflow the declared parameters — unless the
    /// signature has a rest parameter, which absorbs any surplus. This mirrors
    /// [`Self::function_arity_assignable`] at the call site so overload
    /// selection honours optional/rest arity instead of demanding an exact
    /// `params.len()` match.
    fn signature_accepts_arg_count(signature: &FunctionType, arg_count: usize) -> bool {
        let required = signature.required_params.unwrap_or(signature.params.len());
        if arg_count < required {
            return false;
        }
        if signature.rest.is_some() {
            return true;
        }
        arg_count <= signature.params.len()
    }

    /// Recognize `Array.isArray(value)` guard expressions.
    pub(in crate::lowering) fn array_is_array_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
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
        let name = identifier.name.as_str();
        let local_ty = self
            .locals
            .get(name)
            .and_then(|local| Self::local_ty_checked(body, *local))
            .and_then(|ty| self.narrowed_type(name).or(Some(ty)));
        if let Some(ty) = local_ty
            && let Some(members) = self.filtered_union_members(ty, |union_member| {
                matches!(union_member, Type::List(_) | Type::Tuple(_))
            })
        {
            let narrowed = self.intern_filtered_union(members)?;
            return Some((name.to_owned(), narrowed));
        }
        let unknown = self.ctx.krate.types.intern(Type::Unknown);
        Some((
            name.to_owned(),
            self.ctx.krate.types.intern(Type::List(unknown)),
        ))
    }

    /// Recognize `'field' in value` and retain only union arms exposing that field.
    pub(in crate::lowering) fn in_operator_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if binary.operator != BinaryOperator::In {
            return None;
        }
        let Expression::StringLiteral(field) = &binary.left else {
            return None;
        };
        let Expression::Identifier(identifier) = &binary.right else {
            return None;
        };
        let name = identifier.name.as_str();
        let local = self.locals.get(name).copied()?;
        let ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let field_name = field.value.as_str();
        let retained = self
            .filtered_union_members(ty, |member| self.type_has_known_field(member, field_name))?;
        let narrowed = self.intern_filtered_union(retained)?;
        Some((name.to_owned(), narrowed))
    }

    /// Recognize `value.field === literal` discriminant guards.
    ///
    /// TypeScript discriminated unions are narrowed by comparing a shared
    /// discriminant property against a literal. Smelt erases string/number
    /// literal *types* to `String`/`Float`, so arms cannot be told apart by the
    /// discriminant field's value type. What Smelt can prove structurally is
    /// which arms even *carry* the accessed field: comparing `value.field` to a
    /// literal is only meaningful for arms that expose `field`, so the true
    /// branch narrows the union to those arms. This mirrors [`in_operator_guard`]
    /// but keys off a property comparison rather than an `in` test.
    ///
    /// The narrowing is intentionally conservative: it only fires when at least
    /// one union arm lacks the field (so the guard actually excludes a member).
    /// When every arm carries the field the comparison proves nothing about the
    /// union shape and no fact is recorded.
    pub(in crate::lowering) fn property_equality_guard(
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
        let (name, field_name) = Self::member_literal_comparison(binary)?;
        let local = self.locals.get(name.as_str()).copied()?;
        let ty = self
            .narrowed_type(&name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        // Only narrow when the comparison distinguishes union arms: some arm must
        // carry the field and some other arm must not. `filtered_union_members`
        // returns `None` for non-unions, so single concrete types are untouched.
        let Type::Union(items) = self.ctx.krate.types.get(ty)?.clone() else {
            return None;
        };
        let all_have_field = items.iter().all(|item| {
            self.ctx
                .krate
                .types
                .get(*item)
                .is_some_and(|member| self.type_has_known_field(member, &field_name))
        });
        if all_have_field {
            return None;
        }
        let retained = self
            .filtered_union_members(ty, |member| self.type_has_known_field(member, &field_name))?;
        let narrowed = self.intern_filtered_union(retained)?;
        Some((name, narrowed))
    }

    /// Return the local name and property of a `value.field <op> literal` test.
    ///
    /// Recognizes both operand orders (`value.field === "x"` and
    /// `"x" === value.field`). Only static member access on a plain identifier
    /// compared against a literal counts; anything else yields `None`.
    fn member_literal_comparison(
        binary: &oxc::ast::ast::BinaryExpression<'_>,
    ) -> Option<(String, String)> {
        let member_side = Self::static_member_of_local(&binary.left)
            .filter(|_| Self::is_comparison_literal(&binary.right))
            .or_else(|| {
                Self::static_member_of_local(&binary.right)
                    .filter(|_| Self::is_comparison_literal(&binary.left))
            })?;
        Some(member_side)
    }

    /// Return `(local, property)` when an expression is `local.property`.
    fn static_member_of_local(expression: &Expression<'_>) -> Option<(String, String)> {
        let Expression::StaticMemberExpression(member) = expression else {
            return None;
        };
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        Some((object.name.to_string(), member.property.name.to_string()))
    }

    /// Return whether an expression is a literal usable as a discriminant value.
    fn is_comparison_literal(expression: &Expression<'_>) -> bool {
        matches!(
            expression,
            Expression::StringLiteral(_)
                | Expression::NumericLiteral(_)
                | Expression::BooleanLiteral(_)
                | Expression::NullLiteral(_)
        )
    }

    /// Compute the fact proven by a `switch (value.field)` discriminant.
    ///
    /// Inside every labeled case a `switch (value.field)` proves that `value`
    /// carries `field`, so the union narrows to arms exposing it. As with
    /// [`property_equality_guard`], the fact is only recorded when it actually
    /// excludes an arm (some union member lacks the field); otherwise the switch
    /// proves nothing about the union shape. Returns `None` for non-union locals
    /// and for discriminants that are not a plain `local.field` access.
    pub(in crate::lowering) fn switch_discriminant_narrowing(
        &mut self,
        discriminant: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let (name, field_name) = Self::static_member_of_local(discriminant)?;
        let local = self.locals.get(name.as_str()).copied()?;
        let ty = self
            .narrowed_type(&name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let Type::Union(items) = self.ctx.krate.types.get(ty)?.clone() else {
            return None;
        };
        let all_have_field = items.iter().all(|item| {
            self.ctx
                .krate
                .types
                .get(*item)
                .is_some_and(|member| self.type_has_known_field(member, &field_name))
        });
        if all_have_field {
            return None;
        }
        let retained = self
            .filtered_union_members(ty, |member| self.type_has_known_field(member, &field_name))?;
        let narrowed = self.intern_filtered_union(retained)?;
        Some((name, narrowed))
    }

    /// Compute the fact proven inside a `switch (typeof x) { case 'k': … }` arm.
    ///
    /// A `typeof` switch discriminates the same union that a chain of
    /// `if (typeof x === 'k')` guards would: each arm proves `x` is the
    /// member(s) whose runtime `typeof` matches the arm's label(s). Grouped
    /// labels (`case 'a': case 'b':`) union their matching members. When `x`'s
    /// static type is a union (optionally wrapped in `Optional`), the members are
    /// filtered structurally via [`Self::type_matches_typeof`] so a
    /// `case 'object'` arm keeps the array/record/class members instead of
    /// widening to `unknown`; a non-union local falls back to the canonical
    /// single-kind type from [`Self::typeof_matched_type`] (a narrowing no-op
    /// when the local already has that type). `kinds` are the arm's string
    /// labels; an empty result (no member matches, or grouped labels we cannot
    /// read as kinds) records no fact.
    pub(in crate::lowering) fn switch_typeof_case_narrowing(
        &mut self,
        name: &str,
        kinds: &[String],
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let local = self.locals.get(name).copied()?;
        let current_ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        // Look through an `Optional` wrapper: a `typeof` case label never matches
        // the absent (`undefined`) member unless it is an explicit
        // `case 'undefined'`, handled by the single-kind fallback below.
        let inner_ty = match self.ctx.krate.types.get(current_ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => current_ty,
        };
        if let Some(Type::Union(items)) = self.ctx.krate.types.get(inner_ty).cloned() {
            let retained = items
                .into_iter()
                .filter(|item| {
                    // `typeof null === 'object'`, so a `None` member matches the
                    // `'object'` kind — but a value proven present enough to be
                    // indexed/field-accessed in this arm is never nullish (that is
                    // the null guards' domain). Drop `None` so a `case 'object'`
                    // narrows to the real object-shaped arm, not `string[] | null`.
                    !matches!(self.ctx.krate.types.get(*item), Some(Type::None))
                        && kinds
                            .iter()
                            .any(|kind| self.type_matches_typeof(*item, kind))
                })
                .collect::<Vec<_>>();
            if let Some(narrowed) = self.intern_filtered_union(retained) {
                return Some((name.to_owned(), narrowed));
            }
        }
        // Non-union local (or no arm matched the label): the canonical primitive
        // for a single kind still narrows a widened scrutinee usefully.
        if let [kind] = kinds {
            return self.typeof_matched_type(name, kind, body);
        }
        None
    }

    /// Recognize `value instanceof Class` and retain matching concrete class arms.
    pub(in crate::lowering) fn instanceof_local_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if binary.operator != BinaryOperator::Instanceof {
            return None;
        }
        let (Expression::Identifier(identifier), Expression::Identifier(class)) =
            (&binary.left, &binary.right)
        else {
            return None;
        };
        let local_name = identifier.name.as_str();
        let local = self.locals.get(local_name).copied()?;
        let ty = self
            .narrowed_type(local_name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let class_name = class.name.as_str();
        let retained = self.filtered_union_members(ty, |member| {
            matches!(
                member,
                Type::Class {
                    name: class_symbol,
                    ..
                } if self.ctx.krate.symbols.get(*class_symbol) == Some(class_name)
            )
        })?;
        let narrowed = self.intern_filtered_union(retained)?;
        Some((local_name.to_owned(), narrowed))
    }

    /// Filter a union through a structural flow fact.
    fn filtered_union_members(
        &self,
        ty: smelt_hir::TypeId,
        mut keep: impl FnMut(&Type) -> bool,
    ) -> Option<Vec<smelt_hir::TypeId>> {
        let Type::Union(items) = self.ctx.krate.types.get(ty)?.clone() else {
            return None;
        };
        let retained = items
            .into_iter()
            .filter(|item| self.ctx.krate.types.get(*item).is_some_and(&mut keep))
            .collect::<Vec<_>>();
        (!retained.is_empty()).then_some(retained)
    }

    /// Intern the canonical type produced by a non-empty union filter.
    fn intern_filtered_union(
        &mut self,
        retained: Vec<smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
        match retained.as_slice() {
            [] => None,
            [single] => Some(*single),
            _ => Some(self.ctx.krate.types.intern(Type::Union(retained))),
        }
    }

    /// Return whether a concrete type has a statically modeled property.
    fn type_has_known_field(&self, ty: &Type, field: &str) -> bool {
        match ty {
            Type::String | Type::List(_) | Type::Tuple(_) if field == "length" => true,
            Type::Function(_) if matches!(field, "length" | "name" | "prototype") => true,
            Type::Dict(_, _) => true,
            Type::Class { name, .. } => self.ctx.krate.items.iter().any(|item| match item {
                smelt_hir::Item::Class(class) if class.name == *name => class
                    .fields
                    .iter()
                    .any(|candidate| self.ctx.krate.symbols.get(candidate.name) == Some(field)),
                smelt_hir::Item::Interface(interface) if interface.name == *name => interface
                    .fields
                    .iter()
                    .any(|candidate| self.ctx.krate.symbols.get(candidate.name) == Some(field)),
                _ => false,
            }),
            _ => false,
        }
    }

    /// Recognize a call to a user-defined `value is T` predicate function.
    pub(in crate::lowering) fn predicate_call_guard(
        &self,
        expression: &Expression<'_>,
    ) -> Option<(String, smelt_hir::TypeId)> {
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
    pub(in crate::lowering) fn null_guard(
        &mut self,
        expression: &Expression<'_>,
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
    pub(in crate::lowering) fn truthy_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::Identifier(identifier) = expression else {
            return None;
        };
        let name = identifier.name.as_str();
        let local = self.locals.get(name).copied()?;
        let local_ty = match self.narrowed_type(name) {
            Some(ty) => ty,
            None => Self::local_ty_checked(body, local)?,
        };
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
    pub(in crate::lowering) fn catch_binding(
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
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
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

    /// Return whether an initializer needs its declaration binding before evaluation.
    ///
    /// A valid TypeScript initializer can refer to the binding receiving its
    /// result only through deferred execution, for example
    /// `const recursive = wrap(() => recursive())`. Temporarily making that
    /// binding visible lets the existing capture walk identify this case
    /// without assigning premature types to unrelated factory results.
    pub(in crate::lowering) fn initializer_needs_deferred_self_binding(
        &mut self,
        initializer: &Expression<'_>,
        name: &str,
    ) -> bool {
        if matches!(
            initializer,
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
        ) {
            return false;
        }
        let previous = self
            .locals
            .insert(name.to_owned(), smelt_hir::LocalId(u32::MAX));
        let mut captures = Vec::new();
        self.collect_expression_capture_names(initializer, &HashSet::new(), &mut captures);
        if let Some(previous) = previous {
            self.locals.insert(name.to_owned(), previous);
        } else {
            self.locals.remove(name);
        }
        captures.iter().any(|capture| capture == name)
    }

    /// Predeclare function-local callable bindings before statement lowering.
    ///
    /// JavaScript closures can reference `const` arrow callbacks declared later
    /// in the same function body, and a callback passed into a callable factory
    /// can reference the binding receiving that factory's result. Smelt still
    /// lowers statements in source order, so this pass reserves required locals
    /// first; declaration lowering later fills in the runtime value.
    pub(in crate::lowering) fn predeclare_local_arrow_callbacks(
        &mut self,
        statements: &[Statement<'_>],
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        for statement in statements {
            let Statement::VariableDeclaration(decl) = statement else {
                continue;
            };
            if decl.kind != oxc::ast::ast::VariableDeclarationKind::Const {
                continue;
            }
            for declarator in &decl.declarations {
                let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                    continue;
                };
                let Some(initializer) = &declarator.init else {
                    continue;
                };
                let is_deferred_self_binding = self
                    .initializer_needs_deferred_self_binding(initializer, binding.name.as_str());
                let direct_arrow = if let Expression::ArrowFunctionExpression(arrow) = initializer {
                    Some(arrow)
                } else {
                    None
                };
                if direct_arrow.is_none() && !is_deferred_self_binding {
                    continue;
                }
                if self.locals.contains_key(binding.name.as_str()) {
                    continue;
                }
                let annotated_ty = declarator
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?;
                let ty = if let Some(arrow) = direct_arrow {
                    self.local_arrow_function_type(arrow, annotated_ty)
                        .unwrap_or_else(|_| {
                            let unknown = self.ctx.krate.types.intern(Type::Unknown);
                            let return_ty = self
                                .contextual_function_type(annotated_ty)
                                .map_or(unknown, |function| function.return_ty);
                            self.ctx.krate.types.intern(Type::Function(FunctionType {
                                params: vec![unknown; arrow.params.items.len()],
                                rest: None,
                                required_params: None,
                                mutable_params: Vec::new(),
                                return_ty,
                                is_async: arrow.r#async,
                                may_throw: false,
                            }))
                        })
                } else {
                    annotated_ty.unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown))
                };
                let symbol = self.intern_source_name(binding.name.as_str());
                let local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty,
                    mutable: false,
                    span: self.span(binding.span.start, binding.span.end),
                });
                self.locals.insert(binding.name.as_str().to_owned(), local);
            }
        }
        Ok(())
    }

    /// Predeclare nested function declarations before source-order statement lowering.
    ///
    /// JavaScript hoists `function name(...) {}` declarations within a function
    /// body. Reserving callable locals here lets earlier sibling functions call
    /// declarations that appear later in the same block.
    pub(in crate::lowering) fn predeclare_local_function_declarations(
        &mut self,
        statements: &[Statement<'_>],
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        // `const Foo = function () { … }` constructor bindings used with
        // `new`/`instanceof`/`Foo.prototype.x = …` in this block are synthesized
        // into classes here, before the binding is treated as a function value.
        self.synthesize_const_constructor_functions(statements)?;
        for statement in statements {
            let Statement::FunctionDeclaration(function) = statement else {
                continue;
            };
            let Some(id) = &function.id else {
                continue;
            };
            if self.locals.contains_key(id.name.as_str()) {
                continue;
            }
            // A `function Foo(){}` used as `new Foo()` / `instanceof Foo` /
            // `Foo.prototype.x = …` in this block is a JavaScript constructor
            // function: synthesize a class for it instead of a plain local
            // function so the construction and prototype-chain sites resolve.
            if !self.classes.contains_key(id.name.as_str())
                && Self::statements_use_function_as_constructor(id.name.as_str(), statements)
            {
                self.synthesize_constructor_function_class(function, statements)?;
                continue;
            }
            self.push_type_parameter_scope(function.type_parameters.as_deref())?;
            let result = (|| {
                let mut params = Vec::new();
                for param in &function.params.items {
                    params.push(self.function_parameter_type(param)?);
                }
                let required_params = function
                    .params
                    .items
                    .iter()
                    .position(|param| param.optional || Self::formal_parameter_has_default(param))
                    .unwrap_or(function.params.items.len());
                let return_ty = function
                    .return_type
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .unwrap_or_else(|| {
                        let unknown = self.ctx.krate.types.intern(Type::Unknown);
                        if function.r#async {
                            self.ctx.krate.types.intern(Type::Future(unknown))
                        } else {
                            unknown
                        }
                    });
                let fn_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params,
                    rest: None,
                    required_params: Some(required_params),
                    mutable_params: Vec::new(),
                    return_ty,
                    is_async: function.r#async,
                    may_throw: false,
                }));
                let symbol = self.intern_source_name(id.name.as_str());
                let local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty: fn_ty,
                    mutable: false,
                    span: self.span(id.span.start, id.span.end),
                });
                self.locals.insert(id.name.as_str().to_owned(), local);
                Ok(())
            })();
            self.pop_type_parameter_scope();
            result?;
        }
        Ok(())
    }

    /// Lower a variable declaration statement.
    pub(in crate::lowering) fn variable_declaration(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        for declarator in &decl.declarations {
            // A module-level `let`/`var` binding lifted to a mutable global is
            // fully represented by its thread-local item (its literal
            // initializer is the cell's initial value), so lowering the SAME
            // source declaration — in the module body or replayed as top-level
            // test setup — must not re-declare it as a shadowing local. The
            // declaration is recognized by its binding span; a same-named
            // binding declared elsewhere (a function or block scope) has a
            // different span and still creates its ordinary shadowing local.
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && self.is_lifted_global_declarator(binding.name.as_str(), binding.span)
            {
                continue;
            }
            // A `const Foo = function () { … }` binding recognized as a
            // constructor function was already synthesized into a class during
            // the block prepass; its declarator contributes no runtime binding.
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && Self::const_constructor_function(declarator).is_some()
                && self.classes.contains_key(binding.name.as_str())
            {
                continue;
            }
            // `const { A: B } = await import('./mod')` re-imports statically
            // known module members (a vitest module-reset idiom). Compiled Rust
            // has no module registry to reset, so the fresh namespace is the
            // same module: each destructured member binds as a compile-time
            // alias of the statically resolved import, exactly like
            // `import { A as B } from './mod'`.
            if let BindingPattern::ObjectPattern(object) = &declarator.id
                && let Some(Expression::AwaitExpression(await_expr)) = &declarator.init
                && let Expression::ImportExpression(import_expr) = &await_expr.argument
                && let Expression::StringLiteral(source) = &import_expr.source
            {
                self.dynamic_import_destructure_aliases(object, source.value.as_str())?;
                continue;
            }
            // `const g = globalThis;` records a local global-object alias so that
            // later `g.Object.keys(x)` / `"Map" in g` normalize and erase exactly
            // like the bare `globalThis` spelling. The alias is purely a
            // compile-time name-tracking aid; no HIR local is emitted because the
            // global object is never materialized in Phase 1. A `let` is allowed:
            // a later reassignment off the alias is handled where assignments are
            // lowered (the name is cleared), and a write *through* the alias stays
            // on the erasure denylist and produces an honest blocker.
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && let Some(initializer) = &declarator.init
                && self.expr_is_global_alias(initializer)
            {
                self.global_object_aliases
                    .insert(binding.name.as_str().to_owned());
                continue;
            }
            // A `const f = (…) => …` arrow lifts to the compact callback/function
            // form, which references `f` as an immutable item — sound because a
            // `const` can never be reassigned. A `let`/`var` arrow may be
            // reassigned later (`let cmp = defaultCmp; … cmp = other;`), and the
            // callback form has no assignable place for that write. Only lift
            // `const` arrows here; mutable-binding arrows fall through to the
            // general initializer path, which binds a mutable local holding the
            // arrow as a closure value so later reassignments lower to a plain
            // local write.
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init
                && declarator.kind == oxc::ast::ast::VariableDeclarationKind::Const
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
                    block,
                )?;
                continue;
            }
            let annotated_ty = declarator
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            let deferred_self_local = if let BindingPattern::BindingIdentifier(binding) =
                &declarator.id
                && let Some(initializer) = &declarator.init
                && self.initializer_needs_deferred_self_binding(initializer, binding.name.as_str())
            {
                self.local_arrow_existing_body_local(binding.name.as_str(), body)
            } else {
                None
            };
            let predeclared_self = if let BindingPattern::BindingIdentifier(binding) =
                &declarator.id
                && matches!(declarator.init, Some(Expression::FunctionExpression(_)))
            {
                let ty = annotated_ty.unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                let symbol = self.intern_source_name(binding.name.as_str());
                let local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty,
                    mutable: false,
                    span: self.span(binding.span.start, binding.span.end),
                });
                self.locals.insert(binding.name.as_str().to_owned(), local)
            } else {
                None
            };
            let prior_deferred_updates = self.deferred_postfix_updates.replace(Vec::new());
            let value_result = declarator
                .init
                .as_ref()
                .map(|init| self.expression_with_hint(init, body, annotated_ty))
                .transpose();
            let deferred_updates = self.deferred_postfix_updates.take().unwrap_or_default();
            self.deferred_postfix_updates = prior_deferred_updates;
            let value = value_result?;
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && value.is_none()
                && let Some(previous) = predeclared_self
            {
                self.locals
                    .insert(binding.name.as_str().to_owned(), previous);
            }
            match (deferred_self_local, value) {
                (Some(local), Some(value)) => {
                    let local_index = usize::try_from(local.0).map_err(|err| {
                        SmeltError::unsupported(
                            self.span(declarator.span.start, declarator.span.end),
                            format!("deferred callable local id does not fit in usize: {err}"),
                        )
                    })?;
                    let ty = body
                        .locals
                        .get(local_index)
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(declarator.span.start, declarator.span.end),
                                "deferred callable local is missing from its function body",
                            )
                        })?
                        .ty;
                    let pat = body.push_pattern(Pattern::Binding(local));
                    body.push_stmt_to_block(
                        block,
                        Stmt::Let {
                            pat,
                            ty,
                            value: Some(value),
                        },
                    );
                }
                (_, value) => {
                    self.binding_declaration(
                        &declarator.id,
                        value,
                        annotated_ty,
                        matches!(declarator.kind, oxc::ast::ast::VariableDeclarationKind::Let),
                        body,
                        block,
                    )?;
                }
            }
            for update in deferred_updates {
                body.push_stmt_to_block(block, update);
            }
        }
        Ok(())
    }

    /// Register `const { A: B } = await import('./mod')` bindings as import aliases.
    ///
    /// Each destructured member re-binds a statically resolvable export of the
    /// imported module under the local pattern name, reusing the same alias
    /// machinery as `import { A as B } from './mod'`. Computed keys, nested
    /// patterns, and rest elements have no static import equivalent and stay
    /// unsupported.
    fn dynamic_import_destructure_aliases(
        &mut self,
        object: &oxc::ast::ast::ObjectPattern<'_>,
        source: &str,
    ) -> Result<(), SmeltError> {
        if object.rest.is_some() {
            return Err(SmeltError::unsupported(
                self.span(object.span.start, object.span.end),
                "dynamic import destructuring does not support rest elements",
            ));
        }
        for property in &object.properties {
            let (Some(imported), BindingPattern::BindingIdentifier(local)) =
                (property.key.static_name(), &property.value)
            else {
                return Err(SmeltError::unsupported(
                    self.span(property.span.start, property.span.end),
                    "dynamic import destructuring requires static keys and identifier bindings",
                ));
            };
            self.alias_imported_item(source, imported.as_ref(), local.name.as_str());
            self.value_imports.insert(local.name.as_str().to_owned());
        }
        Ok(())
    }

    /// Lower a local arrow function variable as a non-escaping closure value.
    pub(in crate::lowering) fn local_arrow_callback_declaration(
        &mut self,
        name: &str,
        start: u32,
        end: u32,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        self.push_type_parameter_scope(arrow.type_parameters.as_deref())?;
        let saved_outer_locals = self.locals.clone();
        let result = (|| {
            let contextual_function = self.contextual_function_type(type_hint);
            let mut params =
                self.arrow_callback_param_types_with_hint(arrow, contextual_function.as_ref())?;
            for (param, ty) in arrow.params.items.iter().zip(params.iter_mut()) {
                if param.optional
                    && !matches!(self.ctx.krate.types.get(*ty), Some(Type::Optional(_)))
                {
                    *ty = self.ctx.krate.types.intern(Type::Optional(*ty));
                }
            }
            let mut default_params = HashMap::new();
            for (index, param) in arrow.params.items.iter().enumerate() {
                let ty = params
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                Self::bind_local_callback_default_param(
                    &param.pattern,
                    index,
                    ty,
                    &mut default_params,
                )?;
            }
            let defaults = arrow
                .params
                .items
                .iter()
                .map(|param| {
                    param
                        .initializer
                        .as_ref()
                        .map(|default| {
                            self.callback_expression(default, &default_params, body)
                                .map(LocalCallbackDefault::Callback)
                        })
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
            let required_params = arrow
                .params
                .items
                .iter()
                .position(|param| param.optional || Self::formal_parameter_has_default(param))
                .unwrap_or(arrow.params.items.len());
            let mut closure_defaults = defaults;
            if rest.is_some() {
                closure_defaults.push(None);
            }
            let return_ty = arrow
                .return_type
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            let callback_result = if arrow.r#async {
                Err(SmeltError::unsupported(
                    self.span(arrow.span.start, arrow.span.end),
                    "async callbacks need closure-body lowering",
                ))
            } else {
                match self.arrow_return_expression(arrow) {
                    Ok(Expression::CallExpression(_)) => Err(SmeltError::unsupported(
                        self.span(arrow.span.start, arrow.span.end),
                        "call-bodied local arrows lower through closure bodies",
                    )),
                    _ => self.arrow_callback_from_params(arrow, &params, body),
                }
            };
            let mut return_ty = return_ty
                .or_else(|| {
                    contextual_function
                        .as_ref()
                        .map(|function| function.return_ty)
                })
                .unwrap_or_else(|| {
                    callback_result.as_ref().map_or_else(
                        |_| self.ctx.krate.types.intern(Type::Unknown),
                        |callback| callback.ty,
                    )
                });
            if arrow.r#async
                && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
            {
                return_ty = self.ctx.krate.types.intern(Type::Future(return_ty));
            }
            let symbol = self.intern_source_name(name);
            let fn_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: params.clone(),
                rest: rest.map(|rest| rest.index),
                required_params: Some(required_params),
                mutable_params: Vec::new(),
                return_ty,
                is_async: arrow.r#async,
                may_throw: false,
            }));
            let predeclared_local = self.local_arrow_existing_body_local(name, body);
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
                let local = self.local_arrow_binding_local(
                    name,
                    symbol,
                    fn_ty,
                    self.span(start, end),
                    body,
                );
                self.locals.insert(name.to_owned(), local);
                if predeclared_local.is_some() {
                    let value = self.callback_expr_to_closure_with_return_ty(
                        return_ty,
                        &callback,
                        &params,
                        rest.map(|rest| rest.index),
                        Some(required_params),
                        self.span(arrow.span.start, arrow.span.end),
                        body,
                    )?;
                    let pat = body.push_pattern(Pattern::Binding(local));
                    body.push_stmt_to_block(
                        block,
                        Stmt::Let {
                            pat,
                            ty: fn_ty,
                            value: Some(value),
                        },
                    );
                }
                self.local_callbacks.insert(
                    name.to_owned(),
                    LocalCallback {
                        defining_body_span: body.blocks.first().map(|root_block| root_block.span),
                        callback,
                        params,
                        defaults: closure_defaults,
                        rest,
                        required_params: Some(required_params),
                        return_ty,
                    },
                );
                return Ok(());
            }
            let value = self.arrow_closure_body_expr(arrow, &params, return_ty, body)?;
            let local =
                self.local_arrow_binding_local(name, symbol, fn_ty, self.span(start, end), body);
            self.locals.insert(name.to_owned(), local);
            let pat = body.push_pattern(Pattern::Binding(local));
            body.push_stmt_to_block(
                block,
                Stmt::Let {
                    pat,
                    ty: fn_ty,
                    value: Some(value),
                },
            );
            Ok(())
        })();
        self.pop_type_parameter_scope();
        let declared_local = result
            .as_ref()
            .ok()
            .and_then(|()| self.locals.get(name).copied());
        self.locals = saved_outer_locals;
        if let Some(local) = declared_local {
            self.locals.insert(name.to_owned(), local);
        }
        result
    }

    /// Bind parameter names that may be referenced by local callback default values.
    pub(in crate::lowering) fn bind_local_callback_default_param<'a>(
        pattern: &'a BindingPattern<'a>,
        index: usize,
        ty: smelt_hir::TypeId,
        params: &mut HashMap<&'a str, CallbackExpr>,
    ) -> Result<(), SmeltError> {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                params.insert(
                    binding.name.as_str(),
                    CallbackExpr {
                        kind: CallbackExprKind::Param(index),
                        ty,
                    },
                );
                Ok(())
            }
            BindingPattern::AssignmentPattern(assign) => {
                Self::bind_local_callback_default_param(&assign.left, index, ty, params)
            }
            _ => Ok(()),
        }
    }

    /// Lower a nested `function name(...) { ... }` declaration as a local closure.
    pub(in crate::lowering) fn local_function_declaration(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        outer_body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let id = function.id.as_ref().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "anonymous local function declarations are not lowered yet",
            )
        })?;
        // A constructor function recognized during the block prepass was already
        // synthesized into a class; its declaration statement contributes no
        // local closure.
        if self.classes.contains_key(id.name.as_str()) {
            return Ok(());
        }
        let Some(function_body) = &function.body else {
            return Ok(());
        };
        self.push_type_parameter_scope(function.type_parameters.as_deref())?;
        let result = (|| {
            let mut param_tys = Vec::new();
            let mut closure_body = Body::new(
                None,
                self.span(function_body.span.start, function_body.span.end),
            );
            let mut closure_params = Vec::new();
            let mut param_names = HashSet::new();
            let mut saved_locals = Vec::new();

            for (index, param) in function.params.items.iter().enumerate() {
                let ty = self.function_parameter_type(param)?;
                let (symbol, param_span, source_name) = match &param.pattern {
                    BindingPattern::BindingIdentifier(binding) => (
                        self.intern_source_name(binding.name.as_str()),
                        self.span(binding.span.start, binding.span.end),
                        Some(binding.name.as_str().to_owned()),
                    ),
                    _ => (
                        self.synthetic_param_symbol(index),
                        self.span(param.span.start, param.span.end),
                        None,
                    ),
                };
                let local = closure_body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty,
                    mutable: false,
                    span: param_span,
                });
                closure_body.params.push(local);
                closure_params.push(Param {
                    name: symbol,
                    local,
                    ty,
                    span: param_span,
                });
                param_tys.push(ty);
                if let Some(source_name) = source_name {
                    param_names.insert(source_name.clone());
                    saved_locals
                        .push((source_name.clone(), self.locals.insert(source_name, local)));
                }
            }

            // Lower an optional `...rest` parameter exactly as top-level functions,
            // arrow expressions, and function-expression values do: resolve its
            // array element type, push a packed list local/param into the closure
            // body, bind its source name, and record the rest index so the closure
            // collects the trailing source arguments into one list. A nested
            // `function name(...args) { ... }` is a real local closure (e.g. the
            // curry/curryRight `makeCurry` family), so it must carry rest the same
            // way every other closure form does instead of aborting.
            let rest_index = if let Some(rest_param) = &function.params.rest {
                let BindingPattern::BindingIdentifier(binding) = &rest_param.rest.argument else {
                    return Err(SmeltError::unsupported(
                        self.span(rest_param.span.start, rest_param.span.end),
                        "nested function destructured rest parameters need rest binding lowering",
                    ));
                };
                let source_ty = rest_param
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                let Ok((ty, _item_ty)) = self.rest_param_array_type(source_ty) else {
                    return Err(SmeltError::unsupported(
                        self.span(rest_param.span.start, rest_param.span.end),
                        "nested function rest parameter type must be an array type",
                    ));
                };
                let rest_index = closure_params.len();
                let symbol = self.intern_source_name(binding.name.as_str());
                let local = closure_body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty,
                    mutable: false,
                    span: self.span(binding.span.start, binding.span.end),
                });
                closure_body.params.push(local);
                closure_params.push(Param {
                    name: symbol,
                    local,
                    ty,
                    span: self.span(binding.span.start, binding.span.end),
                });
                param_tys.push(ty);
                let source_name = binding.name.as_str().to_owned();
                param_names.insert(source_name.clone());
                saved_locals.push((source_name.clone(), self.locals.insert(source_name, local)));
                Some(rest_index)
            } else {
                None
            };

            let declared_return_ty = function
                .return_type
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            let provisional_return_ty =
                declared_return_ty.unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
            let provisional_fn_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: param_tys.clone(),
                rest: rest_index,
                required_params: None,
                mutable_params: Vec::new(),
                return_ty: provisional_return_ty,
                is_async: function.r#async,
                may_throw: false,
            }));
            let function_symbol = self.intern_source_name(id.name.as_str());
            let self_local = closure_body.push_local(LocalDecl {
                name: Some(function_symbol),
                ty: provisional_fn_ty,
                mutable: false,
                span: self.span(id.span.start, id.span.end),
            });
            param_names.insert(id.name.as_str().to_owned());
            saved_locals.push((
                id.name.as_str().to_owned(),
                self.locals.insert(id.name.as_str().to_owned(), self_local),
            ));

            let mut capture_names = Vec::new();
            for statement in &function_body.statements {
                self.collect_statement_capture_names(statement, &param_names, &mut capture_names);
            }
            capture_names.sort();
            capture_names.dedup();

            let mut captures = Vec::new();
            for name in capture_names {
                let Some(source_local) = saved_locals
                    .iter()
                    .find_map(|(saved_name, prior)| {
                        (saved_name == &name).then_some(*prior).flatten()
                    })
                    .or_else(|| self.locals.get(name.as_str()).copied())
                else {
                    continue;
                };
                let Some(source_decl) = usize::try_from(source_local.0)
                    .ok()
                    .and_then(|index| outer_body.locals.get(index))
                else {
                    continue;
                };
                let symbol = source_decl
                    .name
                    .unwrap_or_else(|| self.ctx.krate.symbols.intern(name.as_str()));
                let body_local = closure_body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty: source_decl.ty,
                    mutable: source_decl.mutable,
                    span: source_decl.span,
                });
                saved_locals.push((name.clone(), self.locals.insert(name, body_local)));
                captures.push(ClosureCapture {
                    source_local,
                    body_local: Some(body_local),
                    symbol,
                    ty: source_decl.ty,
                    mode: CaptureMode::ByRef,
                });
            }

            let saved_return_ty = self.current_return_ty;
            let saved_async = self.current_async;
            self.current_return_ty = declared_return_ty;
            self.current_async = function.r#async;
            self.current_arguments_arities
                .push(function.params.items.len());
            let mut lowering_result = Ok(());
            for statement in &function_body.statements {
                if let Err(error) = self.statement(statement, &mut closure_body) {
                    lowering_result = Err(error);
                    break;
                }
            }
            if function.r#async {
                closure_body.build_async_state_machine();
            }
            self.current_return_ty = saved_return_ty;
            self.current_async = saved_async;
            self.current_arguments_arities.pop();
            for (name, prior) in saved_locals.into_iter().rev() {
                if let Some(local) = prior {
                    self.locals.insert(name, local);
                } else {
                    self.locals.remove(name.as_str());
                }
            }
            lowering_result?;

            let return_ty = declared_return_ty
                .or_else(|| self.last_return_type(&closure_body))
                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::None));
            let body_id = self.ctx.krate.push_body(closure_body);
            let fn_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: param_tys,
                rest: rest_index,
                required_params: None,
                mutable_params: Vec::new(),
                return_ty,
                is_async: function.r#async,
                may_throw: false,
            }));
            let local = if let Some(existing) = self.locals.get(id.name.as_str()).copied() {
                if let Ok(index) = usize::try_from(existing.0)
                    && let Some(decl) = outer_body.locals.get_mut(index)
                {
                    decl.ty = fn_ty;
                    existing
                } else {
                    outer_body.push_local(LocalDecl {
                        name: Some(function_symbol),
                        ty: fn_ty,
                        mutable: false,
                        span: self.span(id.span.start, id.span.end),
                    })
                }
            } else {
                outer_body.push_local(LocalDecl {
                    name: Some(function_symbol),
                    ty: fn_ty,
                    mutable: false,
                    span: self.span(id.span.start, id.span.end),
                })
            };
            self.locals.insert(id.name.as_str().to_owned(), local);
            let value = outer_body.push_expr(Expr {
                kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                    params: closure_params,
                    rest: rest_index,
                    required_params: None,
                    return_ty,
                    captures,
                    body: body_id,
                    function_item: None,
                    span: self.span(function.span.start, function.span.end),
                }),
                ty: fn_ty,
                span: self.span(function.span.start, function.span.end),
            });
            let pat = outer_body.push_pattern(Pattern::Binding(local));
            outer_body.push_stmt_to_block(
                block,
                Stmt::Let {
                    pat,
                    ty: fn_ty,
                    value: Some(value),
                },
            );
            Ok(())
        })();
        self.pop_type_parameter_scope();
        result
    }

    /// Return a contextual function type from an optional type hint.
    pub(in crate::lowering) fn contextual_function_type(
        &mut self,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Option<FunctionType> {
        type_hint.and_then(|hint| {
            let function_hint = self.function_member_type(hint).unwrap_or(hint);
            if let Some(Type::Function(function)) = self.ctx.krate.types.get(function_hint) {
                Some(function.clone())
            } else {
                None
            }
        })
    }

    /// Build the public function type for a local arrow declaration.
    pub(in crate::lowering) fn local_arrow_function_type(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let contextual_function = self.contextual_function_type(type_hint);
        let mut params =
            self.arrow_callback_param_types_with_hint(arrow, contextual_function.as_ref())?;
        for (param, ty) in arrow.params.items.iter().zip(params.iter_mut()) {
            if param.optional && !matches!(self.ctx.krate.types.get(*ty), Some(Type::Optional(_))) {
                *ty = self.ctx.krate.types.intern(Type::Optional(*ty));
            }
        }
        let return_ty = arrow
            .return_type
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .or_else(|| {
                contextual_function
                    .as_ref()
                    .map(|function| function.return_ty)
            })
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
        let mutable_params = self.mutable_params_from_returned_tuple_state(&params, return_ty);
        Ok(self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest: None,
            required_params: Some(
                arrow
                    .params
                    .items
                    .iter()
                    .position(|param| param.optional || Self::formal_parameter_has_default(param))
                    .unwrap_or(arrow.params.items.len()),
            ),
            mutable_params,
            return_ty,
            is_async: arrow.r#async,
            may_throw: false,
        })))
    }

    /// Return a predeclared local arrow binding that belongs to this body.
    pub(in crate::lowering) fn local_arrow_existing_body_local(
        &self,
        name: &str,
        body: &Body,
    ) -> Option<smelt_hir::LocalId> {
        let local = self.locals.get(name).copied()?;
        usize::try_from(local.0)
            .ok()
            .and_then(|index| body.locals.get(index))
            .map(|_| local)
    }

    /// Return the local slot for a local arrow declaration, updating predeclared slots.
    pub(in crate::lowering) fn local_arrow_binding_local(
        &self,
        name: &str,
        symbol: smelt_hir::Symbol,
        ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::LocalId {
        if let Some(local) = self.local_arrow_existing_body_local(name, body)
            && let Some(local_decl) = usize::try_from(local.0)
                .ok()
                .and_then(|index| body.locals.get_mut(index))
        {
            local_decl.name = Some(symbol);
            local_decl.ty = ty;
            local_decl.mutable = false;
            local_decl.span = span;
            return local;
        }
        body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span,
        })
    }

    /// Return whether a lowered local callback body satisfies its declared return type.
    pub(in crate::lowering) fn local_callback_return_type_compatible(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        self.type_assignable_to(actual, expected)
            || matches!(self.ctx.krate.types.get(expected), Some(Type::Class { .. }))
            || matches!(
                self.ctx.krate.types.get(expected),
                Some(Type::TypeParam { .. })
            )
            || matches!(
                self.ctx.krate.types.get(expected),
                Some(Type::Optional(inner)) if *inner == actual
            )
            || matches!(self.ctx.krate.types.get(actual), Some(Type::Unknown))
            || matches!(
                (
                    self.ctx.krate.types.get(actual),
                    self.ctx.krate.types.get(expected)
                ),
                (Some(Type::Function(_)), Some(Type::Function(_)))
            )
    }

    /// Lower a binding pattern in a variable declaration.
    pub(in crate::lowering) fn binding_declaration(
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
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
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
                if value.is_some_and(|value| self.expression_is_known_date_value(value, body))
                    || self.type_is_known_date_value(ty)
                {
                    self.date_value_locals.insert(local);
                }
                // An explicit `any` annotation pins the local to the erased
                // `Unknown` boundary: record it so later concrete assignments do
                // not flow-narrow its storage type away from `Unknown`.
                if annotated_ty.is_some_and(|annotated| {
                    matches!(self.ctx.krate.types.get(annotated), Some(Type::Unknown))
                }) {
                    self.explicit_any_locals.insert(local);
                }
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
                        let receiver_ty = Self::expr_ty(body, receiver);
                        let is_stdlib_len = self.ctx.krate.symbols.get(field) == Some("length")
                            && self.supports_stdlib_length(receiver_ty);
                        let is_stdlib_size = self.ctx.krate.symbols.get(field) == Some("size")
                            && self.supports_stdlib_size(receiver_ty);
                        let ty = self.class_field_type(receiver_ty, field)?;
                        let extracted = if is_stdlib_len || is_stdlib_size {
                            body.push_expr(Expr {
                                kind: ExprKind::Len { operand: receiver },
                                ty,
                                span: self.span(property.span.start, property.span.end),
                            })
                        } else {
                            body.push_expr(Expr {
                                kind: ExprKind::Field { receiver, field },
                                ty,
                                span: self.span(property.span.start, property.span.end),
                            })
                        };
                        (ty, extracted, omitted_key)
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
                let Some(initial_receiver) = value else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "array destructuring requires an initializer",
                    ));
                };
                let mut receiver = initial_receiver;
                let mut receiver_ty = Self::expr_ty(body, receiver);
                if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(receiver_ty).cloned()
                {
                    receiver = body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: receiver },
                        ty: inner,
                        span: self.span(array.span.start, array.span.end),
                    });
                    receiver_ty = inner;
                }
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
                    let source_item_ty = tuple_items
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
                    let (extracted_kind, item_ty) = if tuple_items.is_some() {
                        (
                            ExprKind::TupleIndex {
                                tuple: receiver,
                                index: usize::try_from(idx).map_err(|err| {
                                    SmeltError::unsupported(
                                        self.span(array.span.start, array.span.end),
                                        format!(
                                            "array destructuring tuple index is too large: {err}"
                                        ),
                                    )
                                })?,
                            },
                            source_item_ty,
                        )
                    } else {
                        let optional_item_ty =
                            self.ctx.krate.types.intern(Type::Optional(source_item_ty));
                        (ExprKind::Index { receiver, index }, optional_item_ty)
                    };
                    let extracted = body.push_expr(Expr {
                        kind: extracted_kind,
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
                        let item_ty = match selected.as_slice() {
                            [] => self.ctx.krate.types.intern(Type::Unknown),
                            [single] => *single,
                            _ => {
                                let mut unique = Vec::new();
                                for item in selected {
                                    if !unique.contains(&item) {
                                        unique.push(item);
                                    }
                                }
                                match unique.as_slice() {
                                    [single] => *single,
                                    _ => self.ctx.krate.types.intern(Type::Union(unique)),
                                }
                            }
                        };
                        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                        let mut rest_items = Vec::new();
                        for index in start..end {
                            let item_ty = items.get(index).copied().unwrap_or(item_ty);
                            rest_items.push(body.push_expr(Expr {
                                kind: ExprKind::TupleIndex {
                                    tuple: receiver,
                                    index,
                                },
                                ty: item_ty,
                                span: self.span(rest.span.start, rest.span.end),
                            }));
                        }
                        let extracted = body.push_expr(Expr {
                            kind: ExprKind::ListLit(rest_items),
                            ty,
                            span: self.span(rest.span.start, rest.span.end),
                        });
                        (ty, extracted)
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
            BindingPattern::AssignmentPattern(assign) => {
                let fallback_hint = annotated_ty
                    .and_then(|ty| self.non_nullish_type(ty))
                    .or(annotated_ty);
                let fallback = self.expression_with_hint(&assign.right, body, fallback_hint)?;
                let fallback_ty = Self::expr_ty(body, fallback);
                let value = if let Some(value) = value {
                    let value_ty = Self::expr_ty(body, value);
                    if self.ctx.krate.types.get(value_ty) == Some(&Type::None) {
                        fallback
                    } else if self.ctx.krate.types.get(value_ty) == Some(&Type::Unknown) {
                        body.push_expr(Expr {
                            kind: ExprKind::OptionalCoalesce {
                                optional: value,
                                fallback,
                            },
                            ty: fallback_ty,
                            span: self.span(assign.span.start, assign.span.end),
                        })
                    } else if let Some(non_null_ty) = self.non_nullish_type(value_ty) {
                        if self.ctx.krate.types.get(non_null_ty) == Some(&Type::Unknown) {
                            let optional = body.push_expr(Expr {
                                kind: ExprKind::TypeAssert { value },
                                ty: smelt_hir::type_normalize::optional_of(
                                    &mut self.ctx.krate.types,
                                    fallback_ty,
                                ),
                                span: self.span(assign.left.span().start, assign.left.span().end),
                            });
                            let coalesced_value = body.push_expr(Expr {
                                kind: ExprKind::OptionalCoalesce { optional, fallback },
                                ty: fallback_ty,
                                span: self.span(assign.span.start, assign.span.end),
                            });
                            let ty = Self::expr_ty(body, coalesced_value);
                            return self.binding_declaration(
                                &assign.left,
                                Some(coalesced_value),
                                Some(ty),
                                mutable,
                                body,
                                block,
                            );
                        }
                        // The default's type either already fits the element type
                        // (equal, numerically compatible, or a member of an
                        // element union that can inject it) or it does not. In the
                        // latter case the JavaScript binding is the *union* of the
                        // element and default types — e.g. `const [s, n = 0] =
                        // str.split('e')` types `n` as `string | number`, the
                        // string element defaulting to the numeric `0`. Force-
                        // asserting a numeric default to a `String` element would
                        // leave a runtime `f64` statically typed as `String`; unify
                        // into the union instead and let the coercion seam inject
                        // each arm.
                        let element_can_hold_fallback = fallback_ty == non_null_ty
                            || self.numeric_type_compatible(non_null_ty, fallback_ty)
                            || matches!(
                                self.ctx.krate.types.get(non_null_ty),
                                Some(Type::Union(_) | Type::Unknown)
                            );
                        if element_can_hold_fallback {
                            let fallback = if fallback_ty == non_null_ty
                                || self.numeric_type_compatible(non_null_ty, fallback_ty)
                            {
                                fallback
                            } else {
                                body.push_expr(Expr {
                                    kind: ExprKind::TypeAssert { value: fallback },
                                    ty: non_null_ty,
                                    span: self
                                        .span(assign.right.span().start, assign.right.span().end),
                                })
                            };
                            body.push_expr(Expr {
                                kind: ExprKind::OptionalCoalesce {
                                    optional: value,
                                    fallback,
                                },
                                ty: non_null_ty,
                                span: self.span(assign.span.start, assign.span.end),
                            })
                        } else {
                            let unified = self
                                .ctx
                                .krate
                                .types
                                .intern(Type::Union(vec![non_null_ty, fallback_ty]));
                            body.push_expr(Expr {
                                kind: ExprKind::OptionalCoalesce {
                                    optional: value,
                                    fallback,
                                },
                                ty: unified,
                                span: self.span(assign.span.start, assign.span.end),
                            })
                        }
                    } else {
                        value
                    }
                } else {
                    fallback
                };
                let ty = Self::expr_ty(body, value);
                self.binding_declaration(&assign.left, Some(value), Some(ty), mutable, body, block)
            }
        }
    }

    /// Create a string key expression for a static object destructuring property.
    pub(in crate::lowering) fn object_destructuring_static_key_expr(
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
    pub(in crate::lowering) fn object_rest_binding_declaration(
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
    pub(in crate::lowering) fn body_expr_span(body: &Body, expr: smelt_hir::ExprId) -> Span {
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
    pub(in crate::lowering) fn binding_pattern_names(
        pattern: &BindingPattern<'_>,
        names: &mut Vec<String>,
    ) {
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
