//! Fake clocks in tests: `vi.useFakeTimers` and its controls, and sinon's
//! `useFakeTimers`.

use crate::lowering::support::source_spelling;
use crate::lowering::js_text::js_str_text;
use oxc::ast::ast::{Argument, CallExpression, Expression, ObjectPropertyKind, PropertyKey};
use oxc::span::GetSpan;
use smelt_hir::{Body, Expr, ExprKind, Literal, Type};
use std::borrow::Cow;
use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::testing::is_test_mock_namespace;

impl ModuleBuilder<'_> {
    /// Lower Vitest fake-clock APIs to the mutable timestamp observed by `Date.now()`.
    pub(in crate::lowering) fn vitest_fake_timer_call(
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
        if !is_test_mock_namespace(&object.name) {
            return Ok(None);
        }
        let ty = self.ctx.krate.types.intern(Type::None);
        match member.property.name.as_str() {
            "setSystemTime" => {
                let [timestamp] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "vi.setSystemTime requires exactly one Date-compatible value",
                    ));
                };
                let timestamp = self.argument(timestamp, body)?;
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::DateSetNow { timestamp },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            // `vi.restoreAllMocks()` replays the spy restore table. It is
            // grouped with the other nullary `vi.*` lifecycle calls because it
            // is the same shape: a statement whose whole effect is on shared
            // harness state.
            "restoreAllMocks" => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "vi.restoreAllMocks does not accept arguments",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::VitestRestoreAllMocks,
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "useRealTimers" => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "vi.useRealTimers does not accept arguments",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::DateResetNow,
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "useFakeTimers" => {
                let Some(argument) = call.arguments.first() else {
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                };
                if call.arguments.len() != 1 {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "vi.useFakeTimers accepts at most one options object",
                    ));
                }
                let Argument::ObjectExpression(options) = argument else {
                    return Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        "vi.useFakeTimers currently expects an options object",
                    ));
                };
                for property in &options.properties {
                    let ObjectPropertyKind::ObjectProperty(property) = property else {
                        continue;
                    };
                    let key = match &property.key {
                        PropertyKey::StaticIdentifier(identifier) => {
                            Cow::Borrowed(identifier.name.as_str())
                        }
                        PropertyKey::StringLiteral(literal) => js_str_text(literal.value),
                        _ => continue,
                    };
                    if key == "now" {
                        let timestamp = self.expression(&property.value, body)?;
                        return Ok(Some(body.push_expr(Expr {
                            kind: ExprKind::DateSetNow { timestamp },
                            ty,
                            span: self.span(call.span.start, call.span.end),
                        })));
                    }
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            _ => Ok(None),
        }
    }

    /// Lower Sinon fake timer construction and clock methods used by test helpers.
    pub(in crate::lowering) fn sinon_fake_timers_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if let Expression::Identifier(object) = &member.object
            && object.name == "sinon"
            && self.imports.is_value("sinon")
            && member.property.name == "useFakeTimers"
        {
            if call.arguments.len() > 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "sinon.useFakeTimers supports at most one timestamp argument",
                ));
            }
            if let Some(argument) = call.arguments.first() {
                let timestamp = self.argument(argument, body)?;
                if !self.is_date_constructor_arg_type(Self::expr_ty(body, timestamp)) {
                    return Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        "sinon.useFakeTimers timestamp must be DateArg-compatible",
                    ));
                }
            }
            let name = self.intern_type_name("SinonFakeTimers");
            let ty = self.ctx.krate.types.intern(Type::Class {
                name,
                args: Vec::new(),
            });
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if !matches!(
            member.property.name.as_str(),
            "restore"
                | "reset"
                | "tick"
                | "tickAsync"
                | "next"
                | "nextAsync"
                | "runAll"
                | "runAllAsync"
                | "runToLast"
                | "runToLastAsync"
                | "setSystemTime"
        ) {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = self.optional_receiver_inner_type(Self::expr_ty(body, receiver));
        // Only claim these method names on an actual Sinon fake-timers clock
        // (`sinon.useFakeTimers()` yields a `SinonFakeTimers` class value, read
        // directly or through an `Optional` `clock?`). Matching any class or —
        // more dangerously — any `Unknown` receiver would mis-claim ordinary
        // JavaScript methods that share a name, most notably the iterator
        // protocol's `iterator.next()` on an erased `Iterable`.
        if !self.is_sinon_fake_timers_class(receiver_ty) {
            return Ok(None);
        }
        for argument in &call.arguments {
            self.argument(argument, body)?;
        }
        let ty = self.ctx.krate.types.intern(Type::None);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether `receiver_ty` is the synthetic `SinonFakeTimers` clock
    /// class emitted by [`Self::sinon_fake_timers_call`] for
    /// `sinon.useFakeTimers()`.
    ///
    /// The comparison resolves the class symbol back to its source spelling so
    /// the check does not depend on interning identity, which can differ between
    /// the declaration site and later member reads.
    fn is_sinon_fake_timers_class(&self, receiver_ty: smelt_hir::TypeId) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(receiver_ty) else {
            return false;
        };
        source_spelling(&self.ctx.krate, *name)
            == Some("SinonFakeTimers")
    }
}
