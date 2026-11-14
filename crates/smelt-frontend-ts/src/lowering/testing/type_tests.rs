//! Type-level test helpers: assertions that exist for the checker, such as
//! `expectTypeOf`, which lower their value under test and erase the assertion.

use crate::lowering::ModuleBuilder;
use crate::{SmeltError, test_support};
use oxc::ast::ast::Expression;
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, Literal, Type};

impl ModuleBuilder<'_> {
    /// Adapter so an infallible `Option`-returning handler fits the registry shape.
    ///
    /// Preserves the original guard `if let Some(expr) = self.typed_test_value_call(...)`
    /// which never produced an error.
    pub(in crate::lowering) fn typed_test_value_call_entry(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        Ok(self.typed_test_value_call(call, body))
    }

    /// Adapter so the type-test handler fits the builtin registry shape.
    pub(in crate::lowering) fn type_test_call_entry(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        self.type_test_call(call, body)
    }

    /// Lower type-test-only assertion calls to a no-op expression.
    ///
    /// APIs such as Vitest's `expectTypeOf` and `expect-type` assertions exist
    /// to make TypeScript's checker verify source types. They do not represent
    /// runtime behavior, so Smelt lowers the value under test to keep ordinary
    /// expression/type errors visible and erases the assertion call itself.
    pub(in crate::lowering) fn type_test_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(root_call) = self.type_test_root_call(call) else {
            return Ok(None);
        };
        if let Some(value) = root_call.arguments.first() {
            self.with_type_test_argument_mode(|this| this.argument(value, body))?;
        }
        let ty = self.ctx.krate.types.intern(Type::None);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower Remeda's `$typed<T>()` declaration-test helper to an opaque value.
    pub(in crate::lowering) fn typed_test_value_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        if callee.name != "$typed" || !self.allows_unknown_index_access() {
            return None;
        }
        let ty = self.erase_with(ErasureReason::TestHarness, "$typed");
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }

    /// Return the root `expectTypeOf(...)`-style call for a type-test chain.
    pub(in crate::lowering) fn type_test_root_call<'a>(
        &self,
        call: &'a oxc::ast::ast::CallExpression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        if self.is_type_test_root_callee(&call.callee) {
            return Some(call);
        }
        match &call.callee {
            Expression::StaticMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            Expression::ComputedMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            _ => None,
        }
    }

    /// Return the root type-test call for an expression in a member chain.
    pub(in crate::lowering) fn type_test_root_expression<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        match expression {
            Expression::CallExpression(call) => self.type_test_root_call(call),
            Expression::StaticMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            Expression::ComputedMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.type_test_root_expression(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                self.type_test_root_expression(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.type_test_root_expression(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.type_test_root_expression(&non_null.expression)
            }
            _ => None,
        }
    }

    /// Return whether `callee` starts a supported type-test assertion chain.
    pub(in crate::lowering) fn is_type_test_root_callee(&self, callee: &Expression<'_>) -> bool {
        matches!(
            callee,
            Expression::Identifier(ident)
                if self.imports.is_test_builtin(ident.name.as_str())
                    && test_support::is_type_test_builtin_name(ident.name.as_str())
        )
    }
}
