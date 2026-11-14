//! Immediately invoked function expressions: `(() => ..)()` and its async form,
//! lowered as a call of the literal.

use crate::SmeltError;
use crate::lowering::{ModuleBuilder, RestParam};
use oxc::ast::ast::{Argument, Expression};
use smelt_hir::{Body, Expr, ExprKind, FunctionType, Type};

impl ModuleBuilder<'_> {
    /// Return whether an immediately-invoked callee expression is `async`.
    ///
    /// Mirrors the callee shapes [`Self::immediately_invoked_function_call_with_hint`]
    /// accepts, including the parenthesized spellings, so the asyncness question
    /// is answered for exactly the callees that take a contextual hint.
    fn iife_callee_is_async(callee: &Expression<'_>) -> bool {
        match callee {
            Expression::FunctionExpression(function) => function.r#async,
            Expression::ArrowFunctionExpression(arrow) => arrow.r#async,
            Expression::ParenthesizedExpression(paren) => {
                Self::iife_callee_is_async(&paren.expression)
            }
            _ => false,
        }
    }

    /// Reduce a contextual type to the promise it offers an async return channel.
    ///
    /// An `async` function always returns a promise, so only a `Promise<T>` part
    /// of the contextual type can describe its return. A `Promise<T>` answers
    /// itself; a union answers its single promise arm; anything else answers
    /// `None`, which drops the hint and leaves the callee's own inference to
    /// decide. A union with more than one promise arm is ambiguous and is left
    /// to inference for the same reason.
    fn future_contextual_arm(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Future(_)) => Some(ty),
            Some(Type::Union(members)) => {
                let mut futures = members.iter().copied().filter(|member| {
                    matches!(self.ctx.krate.types.get(*member), Some(Type::Future(_)))
                });
                let only = futures.next()?;
                futures.next().is_none().then_some(only)
            }
            _ => None,
        }
    }

    /// Lower an immediately-invoked function expression (IIFE).
    ///
    /// `(function (a, b) { ... })(1, 2)` and `((a) => ...)(5)` invoke a function
    /// or arrow literal directly. The callee is lowered to a closure value, its
    /// function type provides the parameter/return shape, and the call becomes a
    /// `ClosureCall`. Spread arguments expand through the same rest-aware path
    /// used by other closure calls. Returns `Ok(None)` when the callee is not a
    /// function/arrow literal so the surrounding dispatch can continue.
    pub(in crate::lowering) fn immediately_invoked_function_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        self.immediately_invoked_function_call_with_hint(call, body, None)
    }

    /// Lower an immediately-invoked function expression, carrying the call's
    /// contextual type into the callee's RETURN position.
    ///
    /// `const f: (value: string) => number = (() => { .. return (value) => .. })()`
    /// gives the inner arrow its parameter types in TypeScript, because the
    /// contextual type of the call flows to the IIFE's return expression. Smelt
    /// lowered the callee with no hint at all, so the inner arrow's parameters
    /// erased to `SmeltUnknown` and the value it built was reported as erased --
    /// for a shape whose type is fully known. The rule is general: it holds for
    /// `const f: T = (..)()`, for `f = (..)()`, and for `f ||= (..)()` /
    /// `f ??= (..)()` alike, because all four reach here through the same
    /// contextual type.
    ///
    /// Only the RETURN channel is derivable from a call's context. A call gives
    /// its callee no contextual PARAMETER types -- TypeScript does not either --
    /// so the synthesized hint carries an empty parameter list and the callee's
    /// own annotations and inference supply them (`contextual_param_type_at` is
    /// index-based, so an empty list is a clean no-op at any arity).
    pub(in crate::lowering) fn immediately_invoked_function_call_with_hint(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        // An explicit return-type annotation on the callee wins over the
        // contextual type, exactly as it does in TypeScript. `(function* ():
        // Generator<number, T, unknown> { .. })()` already says what it returns;
        // handing it a second, contextual answer only creates a way for the two
        // to disagree.
        // `None` means "not an immediately-invoked function expression at all",
        // which is also the cheap early exit: every other callee shape leaves
        // before anything is interned.
        // `None` means "not an immediately-invoked function expression at all",
        // which is also the cheap early exit: every other callee shape leaves
        // before anything is interned. `true` means the callee already answers
        // the question the hint would answer, so the hint is dropped.
        //
        // Two ways it already answers it. An explicit RETURN-TYPE ANNOTATION
        // wins over the contextual type, exactly as in TypeScript; handing an
        // annotated callee a second answer only creates a way for the two to
        // disagree (`(function* (): Generator<number, T, unknown> { .. })()`).
        // And a callee WITH PARAMETERS has a parameter list of its own: the hint
        // synthesized here can only describe a nullary function, because a call
        // gives its callee no contextual parameter types, so applying it to a
        // parameterized callee replaces a real signature with an empty one --
        // es-toolkit's `(function (..._: unknown[]) { return arguments })(...array)`
        // kept its rest parameter in the emitted closure while the call site lost
        // the argument (E0057). A nullary callee is the shape the hint fits
        // exactly, and it is the shape contextual typing is for.
        // One level of parentheses is looked through, as `(function () {})()`
        // is written.
        let unparenthesized_callee = match &call.callee {
            Expression::ParenthesizedExpression(paren) => &paren.expression,
            other => other,
        };
        let declares_signature =
            |has_return_type: bool, params: &oxc::ast::ast::FormalParameters<'_>| {
                has_return_type || !params.items.is_empty() || params.rest.is_some()
            };
        let skip_hint = match unparenthesized_callee {
            Expression::FunctionExpression(function) => Some(declares_signature(
                function.return_type.is_some(),
                &function.params,
            )),
            Expression::ArrowFunctionExpression(arrow) => Some(declares_signature(
                arrow.return_type.is_some(),
                &arrow.params,
            )),
            _ => None,
        };
        let Some(skip_hint) = skip_hint else {
            return Ok(None);
        };
        let type_hint = if skip_hint { None } else { type_hint };
        // The target's OPTIONALITY does not belong to the callee's return
        // channel: `let x: T | undefined; x = (() => .. return v ..)()` produces
        // a `T` and the assignment wraps it. Hinting `T | undefined` instead
        // makes the callee's emitted tail `Some(<concrete closure>)`, whose
        // inferred Rust type is `Option<Rc<{closure}>>` rather than the declared
        // `Option<Rc<dyn Fn..>>` (E0308) -- and it is a less precise contextual
        // type besides. The arrow's own `return` statements still decide its
        // real return type, so a callee that really can answer nothing still
        // infers the optional.
        let type_hint = type_hint.map(|hint| match self.ctx.krate.types.get(hint) {
            Some(Type::Optional(inner)) => *inner,
            _ => hint,
        });
        // An ASYNC callee's return channel is a promise, so only the part of the
        // contextual type an async function can actually produce reaches it. A
        // union contributes its promise ARM (`Response | Promise<Response>`
        // says `Promise<Response>` to an async callee), and a contextual type
        // with no promise arm at all says nothing.
        //
        // Without this, `return (async () => new Response(..))()` inside a
        // function declared `Response | Promise<Response>` gave the arrow the
        // whole union as its return type, so it lowered as
        // `async fn() -> Future<Response | Future<Response>>` and the call
        // produced a promise of the union where the union itself was expected
        // (Hono's `hono-base.ts` HEAD branch, 6 × E0308).
        let type_hint = if Self::iife_callee_is_async(&call.callee) {
            type_hint.and_then(|hint| self.future_contextual_arm(hint))
        } else {
            type_hint
        };
        let callee_hint = type_hint.map(|return_ty| {
            self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: Vec::new(),
                rest: None,
                required_params: None,
                mutable_params: Vec::new(),
                return_ty,
                is_async: false,
                may_throw: false,
            }))
        });
        let callee = match &call.callee {
            Expression::FunctionExpression(function) => {
                self.function_expression_value(function, callee_hint, function.span, body)?
            }
            Expression::ArrowFunctionExpression(arrow) => {
                self.arrow_function_expression_with_hint(arrow, body, callee_hint)?
            }
            Expression::ParenthesizedExpression(paren) => match &paren.expression {
                Expression::FunctionExpression(function) => {
                    self.function_expression_value(function, callee_hint, function.span, body)?
                }
                Expression::ArrowFunctionExpression(arrow) => {
                    self.arrow_function_expression_with_hint(arrow, body, callee_hint)?
                }
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        let callee_ty = Self::expr_ty(body, callee);
        let Some(Type::Function(function)) = self.ctx.krate.types.get(callee_ty).cloned() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "immediately-invoked function expression callee did not lower to a function",
            ));
        };
        let rest = function.rest.and_then(|index| {
            if let Some(Type::List(item_ty)) = function
                .params
                .get(index)
                .and_then(|param| self.ctx.krate.types.get(*param))
            {
                Some(RestParam {
                    index,
                    item_ty: *item_ty,
                })
            } else {
                None
            }
        });
        let args = if rest.is_some() && call.arguments.iter().any(Argument::is_spread) {
            self.spread_closure_call_arguments(&function, rest, call, body)?
        } else {
            let fixed_param_count = rest.map_or(function.params.len(), |rest| rest.index);
            let mut args = call
                .arguments
                .iter()
                .take(fixed_param_count)
                .enumerate()
                .map(|(index, arg)| {
                    let hint = function.params.get(index).copied();
                    self.argument_with_hint(arg, body, hint)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if let Some(rest) = rest {
                let rest_args = call
                    .arguments
                    .iter()
                    .skip(rest.index)
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                let rest_ty = self.ctx.krate.types.intern(Type::List(rest.item_ty));
                args.push(body.push_expr(Expr {
                    kind: ExprKind::ListLit(rest_args),
                    ty: rest_ty,
                    span: self.span(call.span.start, call.span.end),
                }));
            }
            args
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: function.return_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }
}
