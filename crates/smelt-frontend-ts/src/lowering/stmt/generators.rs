//! Generators: `yield`, `yield*` and `generator.next(..)`.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Argument, Expression};
use smelt_hir::{Body, Expr, ExprKind, GeneratorResumeKind, Literal, Stmt, Type};

impl ModuleBuilder<'_> {
    /// Lower a generator suspension point while preserving its concrete yield type.
    pub(in crate::lowering) fn generator_yield_statement(
        &mut self,
        yield_expr: &oxc::ast::ast::YieldExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Some(generator) = self.fn_ctx.generator else {
            return Ok(false);
        };
        if yield_expr.delegate {
            let delegate = self.generator_delegate_expression(yield_expr, body)?;
            body.push_stmt_to_block(block, Stmt::Expr(delegate));
            return Ok(true);
        }
        let value = if let Some(argument) = &yield_expr.argument {
            self.expression(argument, body)?
        } else {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: none_ty,
                span: self.span(yield_expr.span.start, yield_expr.span.end),
            })
        };
        let yielded = if matches!(
            self.ctx.krate.types.get(generator.yield_ty),
            Some(Type::Unknown)
        ) {
            // Preserve the concrete expression while an unannotated generator
            // signature is being inferred after body lowering.
            value
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: generator.yield_ty,
                span: self.span(yield_expr.span.start, yield_expr.span.end),
            })
        };
        let suspend = body.push_expr(Expr {
            kind: ExprKind::GeneratorYield { value: yielded },
            // The MIR temporary retains `next_ty` even when the source statement
            // discards it, allowing every suspension in one producer to share
            // genawaiter's single typed resume channel.
            ty: generator.next_ty,
            span: self.span(yield_expr.span.start, yield_expr.span.end),
        });
        body.push_stmt_to_block(block, Stmt::Expr(suspend));
        Ok(true)
    }

    /// Lower a yield used as an expression, retaining the caller-provided resume type.
    pub(in crate::lowering) fn generator_yield_expression(
        &mut self,
        yield_expr: &oxc::ast::ast::YieldExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let generator = self.fn_ctx.generator.ok_or_else(|| {
            SmeltError::unsupported(
                self.span(yield_expr.span.start, yield_expr.span.end),
                "yield is only valid inside a generator",
            )
        })?;
        let value = if let Some(argument) = &yield_expr.argument {
            self.expression(argument, body)?
        } else {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: none_ty,
                span: self.span(yield_expr.span.start, yield_expr.span.end),
            })
        };
        let yielded = if matches!(
            self.ctx.krate.types.get(generator.yield_ty),
            Some(Type::Unknown)
        ) {
            value
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: generator.yield_ty,
                span: self.span(yield_expr.span.start, yield_expr.span.end),
            })
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::GeneratorYield { value: yielded },
            ty: generator.next_ty,
            span: self.span(yield_expr.span.start, yield_expr.span.end),
        }))
    }

    /// Lower expression-position `yield*` as a typed resume/forward/complete operation.
    pub(in crate::lowering) fn generator_delegate_expression(
        &mut self,
        yield_expr: &oxc::ast::ast::YieldExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(argument) = &yield_expr.argument else {
            return Err(SmeltError::unsupported(
                self.span(yield_expr.span.start, yield_expr.span.end),
                "yield* requires a delegated generator value",
            ));
        };
        let value = self.expression(argument, body)?;
        let value_ty = Self::expr_ty(body, value);
        let span = self.span(yield_expr.span.start, yield_expr.span.end);
        let outer_is_async = self
            .fn_ctx
            .generator
            .is_some_and(|generator| generator.is_async);
        let outer_yield_ty = self
            .fn_ctx
            .generator
            .map(|generator| generator.yield_ty)
            .ok_or_else(|| {
                SmeltError::unsupported(span, "yield* is only valid inside a generator")
            })?;
        let (generator, return_ty) = if let Some(Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            ..
        }) = self.ctx.krate.types.get(value_ty).cloned()
        {
            if is_async && !outer_is_async {
                return Err(SmeltError::unsupported(
                    span,
                    "a synchronous generator cannot delegate to an AsyncGenerator",
                ));
            }
            if !self.type_assignable_to(yield_ty, outer_yield_ty) {
                return Err(SmeltError::unsupported(
                    span,
                    "yield* item type is not assignable to the outer yield type",
                ));
            }
            (value, return_ty)
        } else if let Some(members) = match self.ctx.krate.types.get(value_ty).cloned() {
            Some(Type::Union(members)) => Some(members),
            _ => None,
        } {
            let mut return_types = Vec::with_capacity(members.len());
            for member in members {
                let (is_async, item_ty, return_ty) = match self.ctx.krate.types.get(member) {
                    Some(Type::Generator {
                        is_async,
                        yield_ty,
                        return_ty,
                        ..
                    }) => (*is_async, *yield_ty, *return_ty),
                    Some(Type::List(item_ty) | Type::Set(item_ty)) => {
                        (false, *item_ty, self.ctx.krate.types.intern(Type::None))
                    }
                    Some(Type::String) => {
                        let string_ty = self.ctx.krate.types.intern(Type::String);
                        (false, string_ty, self.ctx.krate.types.intern(Type::None))
                    }
                    _ => {
                        return Err(SmeltError::unsupported(
                            span,
                            "yield* union member is not a typed iterable carrier",
                        ));
                    }
                };
                if is_async && !outer_is_async {
                    return Err(SmeltError::unsupported(
                        span,
                        "a synchronous generator cannot delegate to an async union member",
                    ));
                }
                if !self.type_assignable_to(item_ty, outer_yield_ty) {
                    return Err(SmeltError::unsupported(
                        span,
                        "yield* union item type is not assignable to the outer yield type",
                    ));
                }
                if !return_types.contains(&return_ty) {
                    return_types.push(return_ty);
                }
            }
            (value, self.joined_inferred_type(return_types))
        } else if let Some(item_ty) = match self.ctx.krate.types.get(value_ty) {
            Some(Type::List(item_ty) | Type::Set(item_ty)) => Some(*item_ty),
            Some(Type::String) => Some(self.ctx.krate.types.intern(Type::String)),
            Some(Type::Tuple(items))
                if items
                    .iter()
                    .all(|item| self.type_assignable_to(*item, outer_yield_ty)) =>
            {
                Some(outer_yield_ty)
            }
            _ => None,
        } {
            if !self.type_assignable_to(item_ty, outer_yield_ty) {
                return Err(SmeltError::unsupported(
                    span,
                    "yield* iterable item type is not assignable to the outer yield type",
                ));
            }
            // Built-in iterable completion is JavaScript `undefined`, which
            // is represented by the frontend's void/none result type. The
            // operand remains a collection rather than masquerading as a
            // generator carrier; MIR/codegen adapt its concrete protocol.
            (value, self.ctx.krate.types.intern(Type::None))
        } else {
            // TypeScript's iterable protocol delegates through the
            // well-known `Symbol.iterator` member. Computed-key lowering
            // gives that member a stable synthetic name, so resolve it
            // through the same general class/interface member lookup used
            // by ordinary method calls rather than recognizing a source
            // library or concrete class.
            let mut candidates = Vec::new();
            if outer_is_async {
                candidates.push(self.intern_symbol_key("__smelt_symbol_async_iterator"));
            }
            candidates.push(self.intern_symbol_key("__smelt_symbol_iterator"));
            let mut resolved = None;
            for iterator in candidates {
                let Ok(iterator_ty) = self.class_field_type(value_ty, iterator) else {
                    continue;
                };
                let Some(Type::Function(iterator_fn)) =
                    self.ctx.krate.types.get(iterator_ty).cloned()
                else {
                    continue;
                };
                let generator_ty = iterator_fn.return_ty;
                let Some(Type::Generator {
                    is_async,
                    return_ty,
                    ..
                }) = self.ctx.krate.types.get(generator_ty).cloned()
                else {
                    continue;
                };
                if !is_async || outer_is_async {
                    resolved = Some((iterator, generator_ty, return_ty));
                    break;
                }
            }
            let Some((iterator, generator_ty, return_ty)) = resolved else {
                return Err(SmeltError::unsupported(
                    span,
                    format!(
                        "yield* requires a typed sync or async iterator method (received {:?})",
                        self.ctx.krate.types.get(value_ty),
                    ),
                ));
            };
            let generator = body.push_expr(Expr {
                kind: ExprKind::Method {
                    receiver: value,
                    method: iterator,
                    args: Vec::new(),
                },
                ty: generator_ty,
                span,
            });
            (generator, return_ty)
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::GeneratorDelegate { generator },
            ty: return_ty,
            span,
        }))
    }

    /// Lower a generator `next`, `return`, or `throw` call, resuming the generator.
    pub(in crate::lowering) fn generator_next_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "next" | "return" | "throw") || call.arguments.len() > 1 {
            return Ok(None);
        }
        let generator = self.expression(&member.object, body)?;
        let generator_ty = Self::expr_ty(body, generator);
        let Some(Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            next_ty,
        }) = self.ctx.krate.types.get(generator_ty).cloned()
        else {
            return Ok(None);
        };
        let result_ty = self.ctx.krate.types.intern(Type::GeneratorResult {
            yield_ty,
            return_ty,
        });
        let ty = if is_async {
            self.ctx.krate.types.intern(Type::Future(result_ty))
        } else {
            result_ty
        };
        let value = call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .map(|argument| self.expression(argument, body))
            .transpose()?;
        let kind = match method {
            "next" => GeneratorResumeKind::Next,
            "return" => GeneratorResumeKind::Return,
            "throw" => GeneratorResumeKind::Throw,
            _ => unreachable!(),
        };
        let expected_ty = match kind {
            GeneratorResumeKind::Next => Some(next_ty),
            GeneratorResumeKind::Return => Some(return_ty),
            GeneratorResumeKind::Throw => None,
        };
        if let (Some(value), Some(expected_ty)) = (value, expected_ty)
            && !self.type_assignable_to(Self::expr_ty(body, value), expected_ty)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "generator protocol value is not assignable to its declared type",
            ));
        }
        if matches!(kind, GeneratorResumeKind::Throw) && value.is_none() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "generator throw requires an error value",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::GeneratorNext {
                generator,
                value,
                kind,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }
}
