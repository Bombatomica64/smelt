//! `Promise`: its statics, its constructor and executor, and `then`/`catch`/`finally`.

use crate::lowering::erasure::NotNewErasure;
use crate::lowering::body_arena::BodyArena;
use crate::SmeltError;
use crate::lowering::{ArrayExpressionElement, ModuleBuilder};
use crate::lowering::support::arrow_block_statements;
use oxc::ast::ast::{Argument, BindingPattern, Expression, Statement};
use oxc::span::GetSpan;
use smelt_hir::{AsyncOp, Body, ErasureReason, Expr, ExprKind, FunctionType, Literal, Type};

impl ModuleBuilder<'_> {
    /// Lower supported `Promise.*` calls into shared async runtime operations.
    pub(in crate::lowering) fn promise_static_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Promise" {
            return Ok(None);
        }
        if member.property.name == "resolve" {
            if call.arguments.len() > 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Promise.resolve supports at most one value argument",
                ));
            }
            // The resolution value has to travel in the op's operands. Lowering
            // to a bare `Sleep` kept only its *type*, so the emitted future
            // resolved with `default_value(inner_ty)` -- `Promise.resolve(1)`
            // settled as `0`, `Promise.resolve('hello')` as `""`. `Resolve`
            // carries the operand itself; the duration operand preserves the
            // microtask deferral `Sleep` was standing in for.
            let (inner_ty, value) = if let Some(argument) = call.arguments.first() {
                let value = self.argument(argument, body)?;
                (Self::expr_ty(body, value), Some(value))
            } else {
                (self.ctx.krate.types.intern(Type::None), None)
            };
            let duration = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: self.ctx.krate.types.intern(Type::Float),
                span: self.span(call.span.start, call.span.start),
            });
            let mut args = vec![duration];
            args.extend(value);
            let ty = self.ctx.krate.types.intern(Type::Future(inner_ty));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::AsyncOp {
                    op: AsyncOp::Resolve,
                    args,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if member.property.name == "reject" {
            // `Promise.reject(reason)` was not lowered at all: it fell through to
            // the combinator arm below, which only knows `all`/`race`/
            // `allSettled`, and the rejection vanished — `await
            // Promise.reject(new Error("boom"))` ran straight on to the next
            // statement and no `catch` ever fired. The reason enters the ordinary
            // `throw` channel, so the future's item type is the erased carrier:
            // TypeScript types this `Promise<never>`, i.e. it yields no value at
            // all, and `Unknown` is the one item type every awaiting context can
            // accept.
            if call.arguments.len() > 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Promise.reject supports at most one reason argument",
                ));
            }
            let reason = call
                .arguments
                .first()
                .map(|argument| self.argument(argument, body))
                .transpose()?;
            let duration = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: self.ctx.krate.types.intern(Type::Float),
                span: self.span(call.span.start, call.span.start),
            });
            let mut args = vec![duration];
            args.extend(reason);
            let unknown = self.erase_with(ErasureReason::UntypedBuiltin, "Promise.reject");
            let ty = self.ctx.krate.types.intern(Type::Future(unknown));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::AsyncOp {
                    op: AsyncOp::Reject,
                    args,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let op = match member.property.name.as_str() {
            "all" => AsyncOp::All,
            "race" => AsyncOp::Race,
            "allSettled" => AsyncOp::AllSettled,
            other => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("Promise.{other} is not lowered yet"),
                ));
            }
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Promise combinators require exactly one array argument",
            ));
        }
        let Some(first_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Promise combinators require exactly one array argument",
            ));
        };
        let (args, output_ty) = if let Argument::ArrayExpression(array) = first_argument {
            let args = self.promise_array_args(array, body)?;
            let output_ty = self.promise_literal_combinator_output(op, &args, array.span, body)?;
            (args, output_ty)
        } else {
            self.promise_list_combinator_args(op, first_argument, body)?
        };
        let ty = self.ctx.krate.types.intern(Type::Future(output_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp { op, args },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return output type for Promise combinators over a source array literal.
    pub(in crate::lowering) fn promise_literal_combinator_output(
        &mut self,
        op: AsyncOp,
        args: &[smelt_hir::ExprId],
        span: oxc::span::Span,
        body: &Body,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match op {
            AsyncOp::All | AsyncOp::AllSettled => {
                let outputs = args
                    .iter()
                    .map(|arg| {
                        self.future_inner_type(Self::expr_ty(body, *arg))
                            .ok_or_else(|| {
                                SmeltError::unsupported(
                                    self.span(span.start, span.end),
                                    "Promise combinator entries must be Promise<T> values",
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.ctx.krate.types.intern(Type::Tuple(outputs)))
            }
            AsyncOp::Race => {
                let Some(first) = args.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(span.start, span.end),
                        "Promise.race requires at least one promise",
                    ));
                };
                self.future_inner_type(Self::expr_ty(body, *first))
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(span.start, span.end),
                            "Promise.race entries must be Promise<T> values",
                        )
                    })
            }
            AsyncOp::Sleep
            | AsyncOp::ExitDrain
            | AsyncOp::Resolve
            | AsyncOp::Reject
            | AsyncOp::CreateTask
            | AsyncOp::WaitFor
            | AsyncOp::HttpGetText
            | AsyncOp::HttpFetch
            | AsyncOp::SetTimeout
            | AsyncOp::ClearTimeout
            | AsyncOp::SetInterval
            | AsyncOp::ClearInterval
            | AsyncOp::Promise
            | AsyncOp::Then
            | AsyncOp::Catch
            | AsyncOp::Finally
            | AsyncOp::SpawnLocal => Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                format!("Promise.{op:?} is not lowered yet"),
            )),
        }
    }

    /// Lower Promise combinators over a non-literal list of homogeneous futures.
    pub(in crate::lowering) fn promise_list_combinator_args(
        &mut self,
        op: AsyncOp,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<(Vec<smelt_hir::ExprId>, smelt_hir::TypeId), SmeltError> {
        if op == AsyncOp::Race {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "Promise.race over a non-literal array is not lowered yet",
            ));
        }
        let list = self.argument(argument, body)?;
        let list_ty = self.type_param_constraint_or_self(Self::expr_ty(body, list));
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty).cloned() else {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "Promise combinators require an array of Promise<T> values",
            ));
        };
        let (list, output_item_ty) = if let Some(output_item_ty) = self.future_inner_type(item_ty) {
            (list, output_item_ty)
        } else {
            let future_item_ty = self.ctx.krate.types.intern(Type::Future(item_ty));
            let future_list_ty = self.ctx.krate.types.intern(Type::List(future_item_ty));
            let list = body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: list,
                    target: future_list_ty,
                },
                ty: future_list_ty,
                span: self.span(argument.span().start, argument.span().end),
            });
            (list, item_ty)
        };
        let output_ty = self.ctx.krate.types.intern(Type::List(output_item_ty));
        Ok((vec![list], output_ty))
    }

    /// Lower supported `new Promise<T>(executor)` expressions to future values.
    ///
    /// Timer executors keep their timeout duration. Other executor forms lower
    /// to `AsyncOp::Promise` with the explicit `Promise<T>` output type so async
    /// batching helpers can keep their type surface.
    pub(in crate::lowering) fn promise_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if callee.name != "Promise" {
            return Ok(None);
        }
        let [executor_arg] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Promise constructor lowering requires one executor",
            ));
        };
        let bare_delay_timer = match executor_arg {
            Argument::ArrowFunctionExpression(executor) => {
                Self::promise_executor_timer_call(executor).filter(|timer_call| {
                    Self::promise_executor_is_bare_delay(executor, timer_call)
                })
            }
            _ => None,
        };
        let mut lowered_executor = None;
        let contextual_output = type_hint
            .map(|hint| {
                // A Promise constructor can be contextualized either by a
                // `Promise<T>` value slot or by the resolved `T` hint supplied
                // for a return expression inside an async function. Both
                // contexts describe the constructor's concrete output `T`.
                self.future_inner_type(hint).unwrap_or(hint)
            })
            .or_else(|| {
                self.promise_constructor_output_type(new_expr)
                    .ok()
                    .flatten()
            });
        let inferred_output = if contextual_output.is_none()
            && bare_delay_timer.is_none()
            && matches!(executor_arg, Argument::Identifier(_))
        {
            let executor = self.argument(executor_arg, body)?;
            let executor_ty = Self::expr_ty(body, executor);
            let inferred = self.promise_executor_resolved_value_type(executor_ty);
            if inferred.is_some() {
                lowered_executor = Some(executor);
            }
            inferred
        } else {
            None
        };
        let declared_output = contextual_output.or(inferred_output);
        // Only collapse a Promise to a bare `Sleep` when its executor is the pure
        // delay shape `new Promise(resolve => setTimeout(resolve, ms))` — the
        // timer's callback IS the `resolve` parameter, so the promise resolves
        // `undefined` after the delay and carries no value. Any executor whose
        // timer callback does real work (e.g. `() => resolve(value)`) must flow
        // through `AsyncOp::Promise`, which threads `resolve`/`reject`; treating
        // it as `Sleep` would silently discard the resolved value.
        let Some(timer_call) = bare_delay_timer else {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            let mut output_ty = declared_output.unwrap_or(none_ty);
            let executor_expr = if let Some(executor) = lowered_executor {
                executor
            } else {
                let mark = self.speculation_mark(body);
                let executor_ty = self.promise_executor_type(output_ty);
                let executor =
                    self.argument_with_hint(executor_arg, body, Some(executor_ty))?;
                let resolved = if declared_output.is_none() {
                    self.promise_output_from_resolve_calls(body, executor)
                } else {
                    None
                };
                // not a new erasure: only compared against the inferred output below
                let unknown_ty = self.unknown_type(NotNewErasure::Comparison);
                match resolved {
                    // `unknown` is exactly the erased resolver the executor
                    // was just lowered against, so only the output changes.
                    Some(resolved) if resolved == unknown_ty => {
                        output_ty = resolved;
                        executor
                    }
                    Some(resolved) if resolved != output_ty => {
                        // The executor was lowered with an erased `resolve`
                        // to read what it resolves with; lower it again
                        // against the typed resolver so the values flow at
                        // their own type. The first lowering is rolled back
                        // through the speculation mark.
                        self.roll_back_to(mark, body);
                        let typed_executor_ty = self.promise_executor_type(resolved);
                        if let Ok(typed) =
                            self.argument_with_hint(executor_arg, body, Some(typed_executor_ty))
                        {
                            output_ty = resolved;
                            typed
                        } else {
                            // A value the typed resolver does not accept
                            // keeps the erased resolver the probe used.
                            self.roll_back_to(mark, body);
                            self.argument_with_hint(executor_arg, body, Some(executor_ty))?
                        }
                    }
                    _ => executor,
                }
            };
            let ty = self.ctx.krate.types.intern(Type::Future(output_ty));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::AsyncOp {
                    op: AsyncOp::Promise,
                    args: vec![executor_expr],
                },
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            })));
        };
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let ty = self
            .ctx
            .krate
            .types
            .intern(Type::Future(declared_output.unwrap_or(none_ty)));
        let Some(duration_argument) = timer_call.arguments.get(1) else {
            return Err(SmeltError::unsupported(
                self.span(timer_call.span.start, timer_call.span.end),
                "Promise timer executor must pass a duration argument",
            ));
        };
        let duration = self.argument(duration_argument, body)?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op: AsyncOp::Sleep,
                args: vec![duration],
            },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// The executor signature `new Promise<T>(executor)` hands its callback.
    ///
    /// `resolve`/`reject` accept their value argument optionally: TypeScript
    /// types them `(value?: T) => void`, and `resolve()` with no argument is
    /// valid (it settles with `undefined`). Recording `required_params: 0`
    /// lets the callbacks satisfy shorter expected function slots such as the
    /// `Array<() => void>` deferred-task queue used by promise concurrency
    /// primitives (semaphore/mutex). A valueless `Promise<void>` still takes an
    /// erased (ignored) resolve value.
    fn promise_executor_type(&mut self, output_ty: smelt_hir::TypeId) -> smelt_hir::TypeId {
        // runtime-helper signature: a valueless promise's resolve takes an ignored tagged value
        let unknown_ty = self.unknown_type(NotNewErasure::RuntimeHelperSignature);
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let resolve_value_ty = if output_ty == none_ty { unknown_ty } else { output_ty };
        let resolve_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![resolve_value_ty],
            rest: None,
            required_params: Some(0),
            mutable_params: Vec::new(),
            return_ty: none_ty,
            is_async: false,
            may_throw: false,
        }));
        let reject_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![unknown_ty],
            rest: None,
            required_params: Some(0),
            mutable_params: Vec::new(),
            return_ty: none_ty,
            is_async: false,
            may_throw: false,
        }));
        self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![resolve_ty, reject_ty],
            rest: None,
            required_params: Some(2),
            mutable_params: Vec::new(),
            return_ty: none_ty,
            is_async: false,
            may_throw: false,
        }))
    }

    /// The value type an unannotated Promise resolves with, read off its executor.
    ///
    /// Applies the rule in [`crate::lowering::promise_resolution`]: one argument type ->
    /// that type (optional when some call passes nothing), disagreeing types ->
    /// `unknown`, no valued call -> `None` (keep the valueless promise).
    fn promise_output_from_resolve_calls(
        &mut self,
        body: &Body,
        executor: smelt_hir::ExprId,
    ) -> Option<smelt_hir::TypeId> {
        let calls = crate::lowering::promise_resolution::executor_resolve_calls(
            &self.ctx.krate.bodies,
            body,
            executor,
        )?;
        let first = *calls.argument_types.first()?;
        // A `resolve(fail())` whose argument never returns settles nothing.
        if self.ctx.krate.types.get(first) == Some(&Type::Never) {
            return None;
        }
        let resolved = if calls.argument_types.iter().all(|ty| *ty == first) {
            first
        } else {
            self.erase_with(ErasureReason::MissingAnnotation, "Promise resolve types disagree")
        };
        if calls.has_empty_call && !self.is_nullishable_type(resolved) {
            return Some(self.ctx.krate.types.intern(Type::Optional(resolved)));
        }
        Some(resolved)
    }

    /// Infer `Promise<T>`'s resolved `T` from a named executor's first callback.
    ///
    /// A declaration such as `(resolve: (value: T) => void)` already carries
    /// the useful static shape, so recovering it avoids an erased resolver ABI.
    pub(in crate::lowering) fn promise_executor_resolved_value_type(
        &self,
        executor_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let Type::Function(executor) = self.ctx.krate.types.get(executor_ty)? else {
            return None;
        };
        let resolve_ty = *executor.params.first()?;
        let Type::Function(resolve) = self.ctx.krate.types.get(resolve_ty)? else {
            return None;
        };
        resolve.params.first().copied()
    }

    /// Return the explicit `Promise<T>` constructor output type when present.
    pub(in crate::lowering) fn promise_constructor_output_type(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let Some(type_arguments) = &new_expr.type_arguments else {
            return Ok(None);
        };
        let [item] = type_arguments.params.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Promise construction supports exactly one type argument",
            ));
        };
        self.ts_type_to_hir(item).map(Some)
    }

    /// Return the `setTimeout` call inside a supported Promise executor.
    pub(in crate::lowering) fn promise_executor_timer_call<'a>(
        executor: &'a oxc::ast::ast::ArrowFunctionExpression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        // The executor is written either concisely (`resolve => setTimeout(..)`)
        // or as a block with one expression statement. Since oxc 0.147 the
        // concise form carries the expression directly instead of wrapping it in
        // an `ExpressionStatement`, so both shapes are read here.
        let body_expression = if let Some(expression) = executor.get_expression() {
            expression
        } else {
            let [Statement::ExpressionStatement(expr_stmt)] = arrow_block_statements(executor)
            else {
                return None;
            };
            &expr_stmt.expression
        };
        let Expression::CallExpression(call) = body_expression else {
            return None;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        (callee.name == "setTimeout" && call.arguments.len() == 2).then_some(call)
    }

    /// Return whether a `setTimeout`-bodied Promise executor is a pure delay.
    ///
    /// The pure-delay shape is `(resolve) => setTimeout(resolve, ms)`: the timer
    /// callback is exactly the executor's first (`resolve`) parameter, so the
    /// promise resolves `undefined` after `ms` and carries no value — it is sound
    /// to lower the whole construct to `AsyncOp::Sleep`. When the callback is
    /// anything else (`() => resolve(value)`, a block that rejects, etc.) the
    /// resolved value would be lost by the `Sleep` collapse, so the caller must
    /// route it through `AsyncOp::Promise` instead.
    pub(in crate::lowering) fn promise_executor_is_bare_delay(
        executor: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        timer_call: &oxc::ast::ast::CallExpression<'_>,
    ) -> bool {
        let Some(resolve_param) = executor.params.items.first() else {
            return false;
        };
        let BindingPattern::BindingIdentifier(resolve_binding) = &resolve_param.pattern else {
            return false;
        };
        let Some(Argument::Identifier(callback)) = timer_call.arguments.first() else {
            return false;
        };
        callback.name == resolve_binding.name
    }

    /// Lower Promise/Future `.then(...)`, `.catch(...)` and `.finally(...)`
    /// continuation calls.
    pub(in crate::lowering) fn promise_continuation_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "then" | "catch" | "finally") {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let Some(Type::Future(inner_ty)) = self.ctx.krate.types.get(receiver_ty).cloned() else {
            return Ok(None);
        };
        let [callback_arg, ..] = call.arguments.as_slice() else {
            return Ok(Some(receiver));
        };
        // not a new erasure: only the contextual return hint for the callback,
        // whose real result is inferred from its body below.
        let unknown_ty = self.unknown_type(NotNewErasure::NoHint);
        // The continuation parameter type is contextual, but its return type is
        // inferred from the callback body. `Promise.then` is generic in that
        // return (`U | PromiseLike<U>`); forcing `void` here erased values such
        // as a returned `Generator<Y, R, N>` before the surrounding `await` and
        // made typed `yield* await promise.then(...)` impossible.
        // `onFinally`'s result is discarded (only a returned promise is
        // awaited, and an `async` callback is a future by its own `async`), so
        // its contextual return is `void`, not an erased value.
        let callback_return_ty = if method == "finally" {
            self.ctx.krate.types.intern(Type::None)
        } else {
            unknown_ty
        };
        // `onFinally` is called with no arguments (it cannot observe the
        // outcome); `onRejected` gets the erased reason, `onFulfilled` the value.
        let callback_params = match method {
            "finally" => Vec::new(),
            // The rejection reason is `any` in TypeScript's own lib: any value
            // may be thrown anywhere in the awaited call graph.
            "catch" => vec![self.erase_with(ErasureReason::ExternalType, "rejection reason")],
            _ => vec![inner_ty],
        };
        let callback_required = callback_params.len();
        let callback_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: callback_params,
            rest: None,
            required_params: Some(callback_required),
            mutable_params: Vec::new(),
            return_ty: callback_return_ty,
            is_async: false,
            may_throw: false,
        }));
        let callback = self.argument_with_hint(callback_arg, body, Some(callback_ty))?;
        let declared_callback_result = body.expr_at(callback)
            .and_then(|expr| match &expr.kind {
                ExprKind::Closure(closure) => Some(closure.return_ty),
                _ => None,
            })
            .or_else(|| match self.ctx.krate.types.get(Self::expr_ty(body, callback)) {
                Some(Type::Function(function)) => Some(function.return_ty),
                _ => None,
            });
        let callback_result_ty = match declared_callback_result {
            Some(return_ty) => match self.ctx.krate.types.get(return_ty) {
                // Promise callbacks flatten a returned promise rather than
                // producing `Promise<Promise<T>>`.
                Some(Type::Future(item)) => *item,
                _ => return_ty,
            },
            // No closure or function type to read the callback's result from.
            _ => self.erase_with(ErasureReason::MissingAnnotation, "promise callback result"),
        };
        let (op, output_ty) = if method == "catch" {
            (AsyncOp::Catch, receiver_ty)
        } else if method == "finally" {
            // The outcome passes through unchanged, so the type does too.
            (AsyncOp::Finally, receiver_ty)
        } else {
            (
                AsyncOp::Then,
                self.ctx.krate.types.intern(Type::Future(callback_result_ty)),
            )
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op,
                args: vec![receiver, callback],
            },
            ty: output_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower array entries passed to a `Promise.*` combinator.
    pub(in crate::lowering) fn promise_array_args(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        array
            .elements
            .iter()
            .map(|element| match element {
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => {
                    Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "Promise combinator arrays cannot use spread or elision",
                    ))
                }
                _ => {
                    let value = self.array_element(element, body)?;
                    if matches!(
                        self.ctx.krate.types.get(Self::expr_ty(body, value)),
                        Some(Type::Future(_))
                    ) {
                        return Ok(value);
                    }
                    // A combinator element that is not statically a `Future`
                    // is a plain value (or an erased one that may be a promise
                    // at run time), which `Promise.all` adopts as-is. It must
                    // travel in the op's operands: lowering to a bare `Sleep`
                    // kept only its *type*, so the element expression -- and any
                    // side effect in it -- was dropped and the combinator saw
                    // `default_value(ty)`. `Promise.all([f(), g()])` on erased
                    // callables never called `f` or `g` at all.
                    let duration = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::Float(0.0)),
                        ty: self.ctx.krate.types.intern(Type::Float),
                        span: self.span(element.span().start, element.span().start),
                    });
                    let ty = self
                        .ctx
                        .krate
                        .types
                        .intern(Type::Future(Self::expr_ty(body, value)));
                    Ok(body.push_expr(Expr {
                        kind: ExprKind::AsyncOp {
                            op: AsyncOp::Resolve,
                            args: vec![duration, value],
                        },
                        ty,
                        span: self.span(element.span().start, element.span().end),
                    }))
                }
            })
            .collect()
    }
}
