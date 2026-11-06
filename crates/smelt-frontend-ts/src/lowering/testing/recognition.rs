//! Recognising the test framework in source: which statements and callees are
//! `test`, `describe`, lifecycle hooks or mock setup.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::support::arrow_block_statements;
use oxc::ast::ast::{ChainElement, Expression};
use oxc::syntax::operator::{BinaryOperator, UnaryOperator};
use smelt_hir::Body;

impl ModuleBuilder<'_> {
    /// Return whether an expression is a top-level Vitest organization call.
    pub(in crate::lowering) fn is_test_framework_statement(
        &self,
        expression: &Expression<'_>,
    ) -> bool {
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
    pub(in crate::lowering) fn is_vitest_mock_statement(expression: &Expression<'_>) -> bool {
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return false;
        };
        member.property.name == "mock"
            && matches!(&member.object, Expression::Identifier(object) if crate::lowering::testing::is_test_mock_namespace(&object.name))
    }

    /// Return a supported top-level test case call, if this expression is one.
    pub(in crate::lowering) fn test_case_call<'a>(
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
        (self.imports.is_test_builtin(name) && matches!(name, "it" | "test")).then_some(call)
    }

    /// Return whether an expression is a skipped Vitest test case.
    pub(in crate::lowering) fn skipped_test_case_call(&self, expression: &Expression<'_>) -> bool {
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
                if self.imports.is_test_builtin(object.name.as_str())
                    && matches!(object.name.as_str(), "it" | "test")
        )
    }

    /// Return whether a suite-level condition is known false for native Rust tests.
    pub(in crate::lowering) fn describe_condition_is_native_false(
        expression: &Expression<'_>,
    ) -> bool {
        Self::typeof_window_undefined_comparison(expression, false)
    }

    /// Return whether a suite-level condition is known true for native Rust tests.
    pub(in crate::lowering) fn describe_condition_is_native_true(
        expression: &Expression<'_>,
    ) -> bool {
        Self::typeof_window_undefined_comparison(expression, true)
    }

    /// Evaluate `typeof window ===/!== "undefined"` for the Rust test target.
    pub(in crate::lowering) fn typeof_window_undefined_comparison(
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
    pub(in crate::lowering) fn is_typeof_window(expression: &Expression<'_>) -> bool {
        let Expression::UnaryExpression(unary) = expression else {
            return false;
        };
        if unary.operator != UnaryOperator::Typeof {
            return false;
        }
        matches!(&unary.argument, Expression::Identifier(identifier) if identifier.name == "window")
    }

    /// Return a supported `test.each(...)` or `describe.each(...)` outer call.
    pub(in crate::lowering) fn table_test_call<'a>(
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
    pub(in crate::lowering) fn table_each_callee(&self, callee: &Expression<'_>) -> bool {
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
                if self.imports.is_test_builtin(object.name.as_str())
                    && matches!(object.name.as_str(), "test" | "it" | "describe")
        )
    }

    /// Return a supported `test.prop(...)` or `it.prop(...)` property-test call.
    pub(in crate::lowering) fn property_test_call<'a>(
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
                if self.imports.is_test_builtin(object.name.as_str())
                    && matches!(object.name.as_str(), "test" | "it")
        )
        .then_some(call)
    }

    /// Return a supported top-level `describe` call, if this expression is one.
    pub(in crate::lowering) fn describe_call<'a>(
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
    pub(in crate::lowering) fn collect_lifecycle_hook<'a>(
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
        if !self.imports.is_test_builtin(name)
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
    pub(in crate::lowering) fn inline_runtime_lifecycle_setup(
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
        if !self.imports.is_test_builtin(callee.name.as_str())
            || !matches!(callee.name.as_str(), "beforeAll" | "beforeEach")
        {
            return Ok(false);
        }
        let Some(callback_arg) = call.arguments.first() else {
            return Ok(false);
        };
        let callback = self.test_arrow_callback(callback_arg, "lifecycle callbacks")?;
        for statement in arrow_block_statements(callback) {
            self.statement_in_block(statement, body, block)?;
        }
        Ok(true)
    }

    /// Return whether a callee belongs to an imported test-framework API.
    pub(in crate::lowering) fn is_test_framework_callee(&self, callee: &Expression<'_>) -> bool {
        match callee {
            Expression::Identifier(ident) => self.imports.is_test_builtin(ident.name.as_str()),
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
                        if self.imports.is_test_builtin(object.name.as_str())
                )
            }
            _ => false,
        }
    }

    /// Return whether a callee is `describe` or `describe.concurrent`.
    pub(in crate::lowering) fn is_describe_callee(&self, callee: &Expression<'_>) -> bool {
        match callee {
            Expression::Identifier(ident) => {
                ident.name == "describe" && self.imports.is_test_builtin("describe")
            }
            Expression::StaticMemberExpression(member) if member.property.name == "concurrent" => {
                matches!(
                    &member.object,
                    Expression::Identifier(object)
                        if object.name == "describe" && self.imports.is_test_builtin("describe")
                )
            }
            _ => false,
        }
    }
}
