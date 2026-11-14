//! Test doubles: `vi.fn`, `vi.spyOn` and the methods of the mock handle they return,
//! under both the `vi` and the `jest` spelling.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Argument, CallExpression, Expression};
use oxc::span::GetSpan;
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, Literal, Type};

/// Whether an identifier names a test framework's mock/timer namespace.
///
/// Vitest exposes its mock API as `vi` and Jest exposes the SAME API
/// (`fn`, `spyOn`, `mock`, `restoreAllMocks`, `useFakeTimers`, ...) as the
/// `jest` global; Vitest's own docs describe `vi` as the Jest-compatible
/// replacement. A suite written against Jest globals (`jest.fn()` with
/// ambient `describe`/`test`) therefore lowers through the same rules.
pub(in crate::lowering) fn is_test_mock_namespace(name: &str) -> bool {
    matches!(name, "vi" | "jest")
}

impl ModuleBuilder<'_> {
    /// Lower Vitest `vi.fn([impl])` mock factories to stateful runtime mocks.
    ///
    /// The mock lowers to `ExprKind::VitestMockFn`, an erased callable object
    /// (`__smelt_call`) whose shared state records every call and serves the
    /// outcomes configured through the chainable `mock*` methods — which are
    /// real runtime fields on the object, so the configuration chain lowers
    /// through the ordinary dynamic method-call path with no special casing.
    /// A `vi.fn()` mock is a genuine dynamic boundary (no declared shape,
    /// behavior reconfigured imperatively at runtime), so the expression type
    /// is `Type::Unknown`; typed call sites recover a concrete callback
    /// through the standard callable-object dispatch.
    pub(in crate::lowering) fn vitest_mock_function_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !is_test_mock_namespace(&object.name) || member.property.name != "fn" {
            return Ok(None);
        }
        if let Some(type_args) = &call.type_arguments {
            // The annotation only constrains the mock's declared shape for
            // type-checking (tsc's job); the runtime mock is erased either way.
            // Keep the arity/shape validation so malformed spellings still fail
            // loudly instead of silently dropping the annotation.
            let [target] = type_args.params.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "vi.fn<T>() supports exactly one function type argument",
                ));
            };
            let ty = self.ts_type_to_hir(target)?;
            if !matches!(self.ctx.krate.types.get(ty), Some(Type::Function(_))) {
                return Err(SmeltError::unsupported(
                    self.span(target.span().start, target.span().end),
                    "vi.fn<T>() type argument must be a function type",
                ));
            }
        }
        let implementation = call
            .arguments
            .first()
            .map(|implementation| self.argument(implementation, body))
            .transpose()?;
        let ty = self.erase_with(ErasureReason::TestHarness, "vi.fn");
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::VitestMockFn { implementation },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower Vitest `vi.spyOn(target, "method")` calls as mock handles.
    pub(in crate::lowering) fn vitest_spy_on_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if !is_test_mock_namespace(&object.name) || member.property.name != "spyOn" {
            return Ok(None);
        }
        let [target, method] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "vi.spyOn requires target object and method name arguments",
            ));
        };
        let target_expr = self.argument(target, body)?;
        let method_expr = self.argument(method, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, method_expr)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(method.span().start, method.span().end),
                "vi.spyOn method name must be a string",
            ));
        }
        let span = self.span(call.span.start, call.span.end);
        // `Date.prototype.getTimezoneOffset` is not an ordinary member: the
        // generated runtime models the ambient timezone offset as its own piece
        // of state rather than as a field on a `Date` object, so there is no
        // member for a spy to replace. That one spelling keeps the dedicated
        // class-marked handle whose `mockReturnValue`/`mockRestore` drive that
        // state (see `DateSetTimezoneOffset`).
        if matches!(method, Argument::StringLiteral(method_name) if method_name.value == "getTimezoneOffset")
        {
            return Ok(Some(self.vitest_mock_handle_expr(span, body, true)));
        }
        // Every other spy is a real boundary adapter: it resolves the member's
        // CURRENT value, wraps it in a mock that forwards to it, and writes the
        // mock back onto the target. Library code that later reads the member
        // therefore calls the mock, which is what makes the recorded calls the
        // ones the program actually made.
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "vi.spyOn");
        let target_expr = Self::erase_to_unknown(target_expr, unknown_ty, span, body);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::VitestSpyOn {
                target: target_expr,
                name: method_expr,
            },
            ty: unknown_ty,
            span,
        })))
    }

    /// Lower Vitest mock-handle methods used to configure or restore mocks.
    pub(in crate::lowering) fn vitest_mock_handle_method_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(
            method,
            "mockImplementation" | "mockReturnValue" | "mockRestore" | "mockClear" | "mockReset"
        ) {
            return Ok(None);
        }
        // A `vi.fn()` mock is a real runtime object whose chainable `mock*`
        // configuration methods are fields on the object, so mock configuration
        // must lower through the generic dynamic method-call path instead of
        // this interception. Only two receiver shapes can still name the
        // date-timezone spy this function models: a plain identifier binding and
        // a direct `vi.spyOn(...)` chain. Every other receiver falls through
        // immediately (before lowering, so no receiver side effects duplicate).
        let receiver_is_spy_on_chain = matches!(
            &member.object,
            Expression::CallExpression(chain_call)
                if matches!(
                    &chain_call.callee,
                    Expression::StaticMemberExpression(chain_member)
                        if chain_member.property.name == "spyOn"
                            && matches!(
                                &chain_member.object,
                                Expression::Identifier(chain_object) if is_test_mock_namespace(&chain_object.name)
                            )
                )
        );
        if !matches!(&member.object, Expression::Identifier(_)) && !receiver_is_spy_on_chain {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let is_date_timezone_offset_spy = self.is_vitest_date_timezone_offset_spy(receiver_ty);
        // An erased (`Type::Unknown`) identifier receiver is a live `vi.fn()`
        // mock (or an inert spy handle); its `mock*` methods are runtime fields,
        // so fall through to the generic dynamic call. Re-lowering an identifier
        // is side-effect free, and the dangling receiver expression is never
        // referenced by a statement, so MIR never materializes it.
        if !is_date_timezone_offset_spy
            && !receiver_is_spy_on_chain
            && self.ctx.krate.types.get(receiver_ty) == Some(&Type::Unknown)
        {
            return Ok(None);
        }
        if is_date_timezone_offset_spy && method == "mockReturnValue" {
            let [offset] = call.arguments.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "mockReturnValue requires exactly one return value",
                ));
            };
            let offset = self.argument(offset, body)?;
            let ty = Self::expr_ty(body, receiver);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateSetTimezoneOffset { offset },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if is_date_timezone_offset_spy && matches!(method, "mockRestore" | "mockReset") {
            if !call.arguments.is_empty() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("{method} does not accept arguments"),
                ));
            }
            let ty = Self::expr_ty(body, receiver);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DateResetTimezoneOffset,
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if method == "mockImplementation" {
            let [implementation] = call.arguments.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "mockImplementation requires exactly one implementation callback",
                ));
            };
            let _implementation = self.argument(implementation, body)?;
            return Ok(Some(receiver));
        }
        // `mockReturnValue(value)` configures a non-date mock's return value.
        // The mock handle is a placeholder, so evaluate the configured value
        // for its side effects and yield the receiver, matching Vitest's
        // chainable mock API (`mock.mockReturnValue(x)` returns the mock).
        if method == "mockReturnValue" {
            let [value] = call.arguments.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "mockReturnValue requires exactly one return value",
                ));
            };
            let _value = self.argument(value, body)?;
            return Ok(Some(receiver));
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{method} does not accept arguments"),
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::None);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Build the opaque value Smelt uses for Vitest mock handles.
    fn vitest_mock_handle_expr(
        &mut self,
        span: smelt_hir::Span,
        body: &mut Body,
        date_timezone_offset_spy: bool,
    ) -> smelt_hir::ExprId {
        let ty = if date_timezone_offset_spy {
            let name = self.intern_synthesized_name("__SmeltVitestDateTimezoneOffsetSpy");
            self.ctx.krate.types.intern(Type::Class {
                name,
                args: Vec::new(),
            })
        } else {
            self.erase_with(ErasureReason::TestHarness, "mock handle")
        };
        body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span,
        })
    }

    /// Return whether a mock handle controls `Date.prototype.getTimezoneOffset`.
    fn is_vitest_date_timezone_offset_spy(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Class { name, .. })
                if self.ctx.krate.symbols.get(*name)
                    == Some("__smelt_vitest_date_timezone_offset_spy")
        )
    }
}
