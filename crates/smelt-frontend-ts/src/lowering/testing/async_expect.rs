//! Asynchronous expectations: `expect(..).resolves` and `expect(..).rejects`, whose
//! actual value only exists after an `await` or a caught rejection.

use oxc::ast::ast::{Argument, CallExpression, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, ErasureReason, Expr, ExprKind, Literal, LocalDecl, Pattern, Stmt, Type, UnaryOp,
};
use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::testing::LoweredActual;

impl ModuleBuilder<'_> {
    /// Lower async Vitest matcher chains as `Promise<void>` test-side effects.
    ///
    /// `expect(promise).resolves.M(...)` and `expect(promise).rejects.M(...)`
    /// are the ordinary matcher `M` applied to the promise's settled value.
    /// Rather than reimplementing every matcher per modifier, this awaits (and
    /// for `rejects`, catches) the actual and hands the resulting value to the
    /// ordinary `expect(value).M(...)` lowering as a
    /// [`LoweredActual`];
    /// `M` is therefore dispatched by exactly the same rules as the synchronous
    /// spelling. `rejects.toThrow` keeps its dedicated path because its
    /// assertion is about the rejection itself, not about a value.
    ///
    /// Two shapes still fall through to the inert `Promise<void>` placeholder:
    /// an erased actual (no static `Future<T>`, so awaiting it would be invalid
    /// HIR — mirroring the ordinary erased-`await` fallback), and a matcher the
    /// synchronous lowering does not implement either.
    pub(in crate::lowering) fn vitest_async_expect_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(matcher) = &call.callee else {
            return Ok(None);
        };
        let Expression::StaticMemberExpression(outer_modifier) = &matcher.object else {
            return Ok(None);
        };
        let mut modifier: &oxc::ast::ast::StaticMemberExpression<'_> = outer_modifier;
        // `expect(p).resolves.not.toBe(x)` puts `.not` between the modifier and
        // the matcher, exactly as the synchronous chain puts it between
        // `expect(...)` and the matcher. Peel it here so the delegation can pass
        // the same `inverted` flag the synchronous lowering derives.
        let mut inverted = false;
        if modifier.property.name == "not" {
            let Expression::StaticMemberExpression(inner) = &modifier.object else {
                return Ok(None);
            };
            inverted = true;
            modifier = &**inner;
        }
        if !matches!(modifier.property.name.as_str(), "resolves" | "rejects") {
            return Ok(None);
        }
        let rejects = modifier.property.name == "rejects";
        let Expression::CallExpression(expect_call) = &modifier.object else {
            return Ok(None);
        };
        let Expression::Identifier(expect_ident) = &expect_call.callee else {
            return Ok(None);
        };
        if expect_ident.name != "expect" || !self.imports.is_test_builtin("expect") {
            return Ok(None);
        }
        let Some(actual_arg) = expect_call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(expect_call.span.start, expect_call.span.end),
                "expect(...).resolves/rejects matcher requires an actual promise",
            ));
        };
        let matcher_name = matcher.property.name.as_str();
        if rejects
            && !inverted
            && matches!(
                matcher_name,
                "toThrow" | "toThrowErrorMatchingInlineSnapshot"
            )
        {
            return self.vitest_rejects_to_throw_call(call, actual_arg, body);
        }

        let actual = self.argument(actual_arg, body)?;
        let actual_ty = Self::expr_ty(body, actual);
        if self.awaitable_inner_type(actual_ty).is_none() {
            return Err(SmeltError::unsupported(
                self.span(actual_arg.span().start, actual_arg.span().end),
                "expect(...).resolves/rejects actual value must be a Promise<T>",
            ));
        }
        let span = self.span(call.span.start, call.span.end);
        if let Some(item_ty) = self.future_inner_type(actual_ty)
            && Self::matcher_accepts_lowered_actual(matcher_name)
        {
            if rejects {
                self.vitest_rejects_matcher(call, actual, item_ty, inverted, span, body)?;
            } else {
                let awaited = body.push_expr(Expr {
                    kind: ExprKind::Await(actual),
                    ty: item_ty,
                    span,
                });
                self.expect_matcher_call(
                    call,
                    Some(LoweredActual {
                        value: awaited,
                        inverted,
                    }),
                    body,
                )?;
            }
            return Ok(Some(self.vitest_async_void_expr(span, body)));
        }
        for argument in &call.arguments {
            self.argument(argument, body)?;
        }
        Ok(Some(self.vitest_async_void_expr(span, body)))
    }

    /// Build the inert `Promise<void>` an async matcher chain evaluates to.
    ///
    /// The assertion itself lowers to statements, so the chain's *expression*
    /// value only has to satisfy the enclosing `await`.
    fn vitest_async_void_expr(&mut self, span: smelt_hir::Span, body: &mut Body) -> smelt_hir::ExprId {
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let ty = self.ctx.krate.types.intern(Type::Future(none_ty));
        body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span,
        })
    }

    /// Lower `expect(promise).rejects.M(...)` for a value-comparing matcher `M`.
    ///
    /// The rejection payload is the actual value the matcher asserts on, so the
    /// await runs inside a `try` and the matcher is emitted into the `catch`
    /// body, where the caught value is in scope. A separate `did_throw` flag
    /// then reports a promise that resolved when the test required a rejection
    /// -- without it, a non-rejecting promise would skip the catch body and the
    /// assertion would pass vacuously, which is the failure mode this whole
    /// path exists to remove.
    ///
    /// The caught value is typed `Unknown`: a thrown JavaScript payload is
    /// genuinely dynamic (any value may be thrown, from anywhere in the awaited
    /// call graph), which is the same reason the ordinary `catch (e)` binding
    /// lowers to `Unknown`. `comparison_expr` inserts the checked cast on the
    /// expected side, so the matcher still compares against its concrete type.
    fn vitest_rejects_matcher(
        &mut self,
        call: &CallExpression<'_>,
        actual: smelt_hir::ExprId,
        item_ty: smelt_hir::TypeId,
        inverted: bool,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.erase_with(ErasureReason::TestHarness, "rejects payload");

        let did_throw = body.push_local(LocalDecl {
            name: Some(self.intern_source_name("did_throw")),
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
        self.push_assertion_stmt(
            body,
            Stmt::Let {
                pat: did_throw_pat,
                ty: bool_ty,
                value: Some(false_expr),
            },
        );

        let try_block = body.push_block(span);
        let awaited = body.push_expr(Expr {
            kind: ExprKind::Await(actual),
            ty: item_ty,
            span,
        });
        body.push_stmt_to_block(try_block, Stmt::Expr(awaited));

        let caught = body.push_local(LocalDecl {
            name: Some(self.intern_source_name("error")),
            ty: unknown_ty,
            mutable: false,
            span,
        });
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
        let caught_expr = body.push_expr(Expr {
            kind: ExprKind::Local(caught),
            ty: unknown_ty,
            span,
        });
        self.with_stmt_block(catch_block, |this| {
            this.expect_matcher_call(
                call,
                Some(LoweredActual {
                    value: caught_expr,
                    inverted,
                }),
                body,
            )
        })?;

        self.push_assertion_stmt(
            body,
            Stmt::TryCatch {
                body: try_block,
                catch_binding: Some(caught),
                catch_body: Some(catch_block),
                finally_body: None,
            },
        );

        let did_throw_check = body.push_expr(Expr {
            kind: ExprKind::Local(did_throw),
            ty: bool_ty,
            span,
        });
        let failed = self.unary_bool_expr(UnaryOp::Not, did_throw_check, call.span, body);
        self.push_test_failure_if(
            failed,
            "expect(...).rejects.<matcher>(...) did not reject",
            call.span,
            body,
        );
        Ok(())
    }

    /// Lower `await expect(promise).rejects.toThrow(...)` to native exception flow.
    ///
    /// The actual must be awaited, so it has to lower to a real `Future<T>`:
    /// imported `async` helpers resolve to `Type::Future` across the project
    /// graph and drive the full reject-and-message assertion below. When the
    /// actual is only an erased `Unknown` (single-file lowering before
    /// cross-module resolution determines the future), awaiting it would be
    /// invalid HIR, so — mirroring the ordinary erased-`await` fallback — the
    /// actual is evaluated for its effects and a benign `Promise<void>`
    /// placeholder is returned instead of rejecting the matcher outright.
    fn vitest_rejects_to_throw_call(
        &mut self,
        call: &CallExpression<'_>,
        actual_arg: &Argument<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let span = self.span(call.span.start, call.span.end);

        // Evaluate the actual once and classify it. Only a real `Future<T>`
        // supports the awaited assertion; an erased actual takes the benign
        // placeholder path, and anything non-awaitable is a genuine blocker.
        let actual = self.argument(actual_arg, body)?;
        let actual_ty = Self::expr_ty(body, actual);
        if self.awaitable_inner_type(actual_ty).is_none() {
            return Err(SmeltError::unsupported(
                self.span(actual_arg.span().start, actual_arg.span().end),
                "expect(...).rejects.toThrow(...) actual value must be a Promise<T>",
            ));
        }
        let Some(item_ty) = self.future_inner_type(actual_ty) else {
            for argument in &call.arguments {
                self.argument(argument, body)?;
            }
            return Ok(Some(self.vitest_async_void_expr(span, body)));
        };

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

        let try_block = body.push_block(span);
        let awaited = body.push_expr(Expr {
            kind: ExprKind::Await(actual),
            ty: item_ty,
            span,
        });
        body.push_stmt_to_block(try_block, Stmt::Expr(awaited));

        let catch_message = body.push_local(LocalDecl {
            name: Some(self.intern_source_name("error")),
            ty: string_ty,
            mutable: false,
            span,
        });
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
        if let Some(expected_arg) = call.arguments.first() {
            let expected = self.argument(expected_arg, body)?;
            if Self::expr_ty(body, expected) == string_ty {
                let error_expr = body.push_expr(Expr {
                    kind: ExprKind::Local(catch_message),
                    ty: string_ty,
                    span,
                });
                let contains = self.contains_expr(error_expr, expected, call.span, body)?;
                let failed = self.unary_bool_expr(UnaryOp::Not, contains, call.span, body);
                let failure_block = body.push_block(span);
                let message = self.string_literal_expr(
                    "expect(...).rejects.toThrow(...) message failed",
                    call.span,
                    body,
                );
                body.push_stmt_to_block(failure_block, Stmt::Throw(message));
                body.push_stmt_to_block(
                    catch_block,
                    Stmt::If {
                        cond: failed,
                        then_block: failure_block,
                        else_block: None,
                    },
                );
            }
        }
        body.push_stmt(Stmt::TryCatch {
            body: try_block,
            catch_binding: Some(catch_message),
            catch_body: Some(catch_block),
            finally_body: None,
        });

        let did_throw_check = body.push_expr(Expr {
            kind: ExprKind::Local(did_throw),
            ty: bool_ty,
            span,
        });
        let failed = self.unary_bool_expr(UnaryOp::Not, did_throw_check, call.span, body);
        self.push_test_failure_if(
            failed,
            "expect(...).rejects.toThrow(...) failed",
            call.span,
            body,
        );

        Ok(Some(self.vitest_async_void_expr(span, body)))
    }
}
