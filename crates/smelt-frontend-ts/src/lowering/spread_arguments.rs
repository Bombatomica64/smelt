//! The single place a spread argument is interpreted for a fixed-arity call.
//!
//! Each path that collects a call's arguments interprets a spread through this
//! module, so the shape has one definition: independent interpretations would
//! produce a call with every argument defaulted that still compiles cleanly.

use crate::SmeltError;
use crate::lowering::{ModuleBuilder, RestParam};
use oxc::ast::ast::{Argument, Expression};
use smelt_hir::{BinOp, Body, Expr, ExprKind, FunctionType, Literal, Span, Stmt, Type};

/// One argument of a call, after spreads have been interpreted.
///
/// A spread of a tuple contributes SEVERAL arguments, so an argument list
/// cannot be walked as one AST node per parameter. Every fixed-arity call
/// path therefore walks this instead: entries that are still source keep their
/// contextual hint and their deferred-callback handling, and entries a spread
/// produced are already lowered.
#[derive(Clone, Copy)]
pub(in crate::lowering) enum CallArg<'a> {
    /// An argument still in source form, to be lowered with its hint.
    Source(&'a Argument<'a>),
    /// One element of an expanded tuple spread, already lowered.
    Lowered(smelt_hir::ExprId),
    /// A spread whose width is not statically known, left for the caller's own
    /// variadic handling.
    ///
    /// A spread of a LIST cannot be distributed positionally -- its length is a
    /// runtime value -- but it is not an error either: a rest-parameter list
    /// spread into a callee that absorbs it was lowering, and passing at
    /// runtime, long before this module existed (es-toolkit's `isEqualWith`
    /// spreads a customizer's `...args` into a function-typed parameter, and
    /// its suite was green on that shape). Turning it into a blocker broke a
    /// working corpus. So the shape is handed back UNEXPANDED and the caller
    /// lowers it exactly as it did before, which is the behaviour that was
    /// already correct.
    UnexpandedSpread(&'a oxc::ast::ast::SpreadElement<'a>),
}

impl ModuleBuilder<'_> {
    /// THE choke point for spread arguments in a fixed-arity call.
    ///
    /// Every fixed-arity argument collection goes through here, and nothing else
    /// interprets an `Argument::SpreadElement` for such a call. Before this
    /// existed the five paths that collect call arguments each had their own
    /// idea of what a spread was -- one returned an EMPTY argument list, two
    /// treated the spread as a single argument in parameter 0, and the emitter
    /// then padded whatever was missing with `default_value(..)`. So
    /// `sink(...triple)` became `sink(String::new(), String::new(), 0.0)`: every
    /// argument a default, the tuple never read, and it COMPILED. One entry
    /// point is what makes that unrepresentable rather than merely fixed in the
    /// paths someone remembered.
    ///
    /// `lower_call_arg` carries a debug assertion that it never sees a spread, so a
    /// future path that forgets to come through here trips in test builds
    /// instead of silently padding.
    pub(in crate::lowering) fn expanded_call_arguments<'a>(
        &mut self,
        arguments: &'a [Argument<'a>],
        body: &mut Body,
    ) -> Result<Vec<CallArg<'a>>, SmeltError> {
        let mut expanded = Vec::with_capacity(arguments.len());
        for argument in arguments {
            match argument {
                Argument::SpreadElement(spread) => {
                    match self.spread_argument_elements(spread, body)? {
                        Some(elements) => {
                            for element in elements {
                                expanded.push(CallArg::Lowered(element));
                            }
                        }
                        None => expanded.push(CallArg::UnexpandedSpread(spread)),
                    }
                }
                _ => expanded.push(CallArg::Source(argument)),
            }
        }
        Ok(expanded)
    }

    /// Distribute a LIST spread over the fixed parameters it covers.
    ///
    /// `f(...xs)` into a callee with fixed parameters and no rest parameter
    /// binds parameter `i` to `xs[i]` in JavaScript (`undefined` past the
    /// end), so the spread lowers to the element reads `xs[0]`, `xs[1]`, ...
    /// for the parameters it covers: the same `Index` read a source `xs[i]`
    /// produces, at the list's element type. The operand is bound to a local
    /// first so it is evaluated once. Hono's `get render()` returns
    /// `(...args) => this.#renderer(...args)` into a one-parameter renderer.
    ///
    /// One spread is distributed, wherever it is written: the fixed arguments
    /// after it keep the parameters they end on, so `func(...args, n)` covers the
    /// parameters before `n`. Only into TYPED parameters: a callee whose covered parameters are erased (`any`,
    /// es-toolkit's `IsEqualCustomizer`) is called through the erased
    /// packed-argument path, which spreads the list at run time, so per-element
    /// reads there would only add erased values. Any other shape is returned
    /// unchanged; an operand that turns out not to be a list is returned as the
    /// single already-lowered argument it was before.
    pub(in crate::lowering) fn distribute_trailing_list_spread<'a>(
        &mut self,
        mut arguments: Vec<CallArg<'a>>,
        params: &[smelt_hir::TypeId],
        body: &mut Body,
    ) -> Result<Vec<CallArg<'a>>, SmeltError> {
        let mut spreads = arguments
            .iter()
            .enumerate()
            .filter(|(_, argument)| matches!(argument, CallArg::UnexpandedSpread(_)));
        let Some((position, &CallArg::UnexpandedSpread(spread))) = spreads.next() else {
            return Ok(arguments);
        };
        if spreads.next().is_some() {
            return Ok(arguments);
        }
        let trailing = arguments.len().saturating_sub(position).saturating_sub(1);
        let covered_params = params
            .get(position..params.len().saturating_sub(trailing))
            .unwrap_or_default();
        let covered = covered_params.len();
        if covered == 0
            || covered_params
                .iter()
                .any(|param| matches!(self.ctx.krate.types.get(*param), Some(Type::Unknown)))
        {
            return Ok(arguments);
        }
        let after = arguments.split_off(position.saturating_add(1));
        arguments.pop();
        let span = self.span(spread.span.start, spread.span.end);
        let operand = self.expression(&spread.argument, body)?;
        let operand = Self::peel_assertion_of_list(operand, body, &self.ctx.krate.types);
        let operand_ty = Self::expr_ty(body, operand);
        let resolved = self.type_param_constraint_or_self(operand_ty);
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(resolved).cloned() else {
            arguments.push(CallArg::Lowered(operand));
            arguments.extend(after);
            return Ok(arguments);
        };
        let spread_name = self.intern_synthesized_name("__smelt_spread");
        let operand_local = self.capture_bind_value(operand, operand_ty, spread_name, span, body);
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        for offset in 0..covered {
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(operand_local),
                ty: operand_ty,
                span,
            });
            let index = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Int(i64::try_from(offset).unwrap_or(i64::MAX))),
                ty: index_ty,
                span,
            });
            arguments.push(CallArg::Lowered(body.push_expr(Expr {
                kind: ExprKind::Index { receiver, index },
                ty: element_ty,
                span,
            })));
        }
        arguments.extend(after);
        Ok(arguments)
    }

    /// The list or tuple under a type assertion, or `expr` itself.
    ///
    /// An assertion is erased at run time, so `...(args as Parameters<F>)`
    /// spreads `args`, every element of it: the asserted type decides neither
    /// the width nor the values. When the asserted type erased (an
    /// unevaluated `Parameters<..>`), the assertion node is all that hides
    /// the list from the distribution above; when it is a NARROWER tuple, it
    /// would cut a tuple rest parameter's elements off.
    fn peel_assertion_of_list(
        expr: smelt_hir::ExprId,
        body: &Body,
        types: &smelt_hir::TypeInterner,
    ) -> smelt_hir::ExprId {
        let Some(node) = body.exprs.get(expr.0 as usize) else {
            return expr;
        };
        let (ExprKind::TypeAssert { value } | ExprKind::UnknownCast { value, .. }) = node.kind
        else {
            return expr;
        };
        if matches!(
            types.get(Self::expr_ty(body, value)),
            Some(Type::List(_) | Type::Tuple(_))
        ) {
            value
        } else {
            expr
        }
    }

    /// Lower one [`CallArg`], applying a contextual hint only where there is
    /// still source to apply it to.
    pub(in crate::lowering) fn lower_call_arg(
        &mut self,
        argument: CallArg<'_>,
        hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match argument {
            CallArg::Source(argument) => {
                // The invariant this module exists to hold: a fixed-arity call
                // never lowers a spread as one argument. `expanded_call_arguments`
                // turns every spread into `Lowered` entries, so a `Source` spread
                // here means a path built its `CallArg`s some other way and is
                // about to bind a whole tuple to one parameter, with the emitter
                // defaulting the rest -- silently, and it would compile. Assert
                // in test builds rather than wait for a corpus to notice.
                debug_assert!(
                    !matches!(argument, Argument::SpreadElement(_)),
                    "a fixed-arity call reached `lower_call_arg` with an uninterpreted spread; \
                     collect its arguments through `expanded_call_arguments`"
                );
                // An erased parameter (`unknown`, `unknown[]`) is no shape to
                // lower the argument INTO; see `hint_preserves_argument_shape`.
                let hint = hint.filter(|hint| self.hint_preserves_argument_shape(*hint));
                self.argument_with_hint(argument, body, hint)
            }
            CallArg::Lowered(expr) => Ok(expr),
            // Lower the operand as ONE argument and let the caller's variadic handling
            // take it from there. This is deliberately not the `Source` arm, so the
            // assertion above still catches a path that never came through
            // `expanded_call_arguments`.
            CallArg::UnexpandedSpread(spread) => self.expression(&spread.argument, body),
        }
    }

    /// Expand one spread argument into the arguments it contributes.
    ///
    /// A tuple has a statically known width, so it distributes positionally:
    /// element `i` becomes one argument, at the element's own type. The operand
    /// is bound to a local FIRST and indexed from there, because an expression
    /// reused in HIR is re-materialized per use -- indexing it directly made
    /// `f(...g())` call `g()` once per tuple element.
    ///
    /// An OPTIONAL tuple unwraps through a throw. Spreading `undefined` or
    /// `null` is a `TypeError` in JavaScript ("is not iterable"), so the absent
    /// arm throws a branded `TypeError` through the ordinary throw route and is
    /// catchable exactly like a source `throw`. Per-element defaults would be
    /// the same silent-wrong-answer failure this whole family is about.
    ///
    /// Optional tuples are expanded, with an absent operand throwing as above;
    /// Hono reaches that case with `router.add(...routes[i])`, where the element
    /// read of an optional private field is an optional tuple (see
    /// `hono-h46-spread-arguments.md`). Anything else -- a list or an erased
    /// value -- has no statically known width here, so it is handed back
    /// unexpanded (`Ok(None)`, see `CallArg::UnexpandedSpread`) rather than
    /// rejected. A list's home is
    /// the packed-vector call path (`ClosureCallSpread`), which an erased callee
    /// already takes; a fixed-arity callee has nowhere to put a runtime-length
    /// argument list.
    fn spread_argument_elements(
        &mut self,
        spread: &oxc::ast::ast::SpreadElement<'_>,
        body: &mut Body,
    ) -> Result<Option<Vec<smelt_hir::ExprId>>, SmeltError> {
        let span = self.span(spread.span.start, spread.span.end);
        let operand = self.expression(&spread.argument, body)?;
        let operand = Self::peel_assertion_of_list(operand, body, &self.ctx.krate.types);
        let operand_ty = Self::expr_ty(body, operand);
        let resolved = self.type_param_constraint_or_self(operand_ty);
        let (tuple_ty, optional) = match self.ctx.krate.types.get(resolved).cloned() {
            Some(Type::Tuple(_)) => (resolved, false),
            // An OPTIONAL tuple still has a statically known width, so it
            // expands -- it just has to be unwrapped first. Hono reaches this
            // with `router.add(...routes[i])`, where the element read of an
            // optional private field is `Option<(String, String, T)>`.
            Some(Type::Optional(inner))
                if matches!(self.ctx.krate.types.get(inner), Some(Type::Tuple(_))) =>
            {
                (inner, true)
            }
            // Not statically widthed: hand it back unexpanded rather than
            // rejecting it. See `CallArg::UnexpandedSpread`.
            _ => return Ok(None),
        };
        let Some(Type::Tuple(items)) = self.ctx.krate.types.get(tuple_ty).cloned() else {
            return Ok(None);
        };

        let spread_name = self.intern_synthesized_name("__smelt_spread");
        let operand_local = self.capture_bind_value(operand, operand_ty, spread_name, span, body);
        let receiver_ty = if optional {
            self.throw_when_spread_operand_is_absent(operand_local, operand_ty, span, body);
            tuple_ty
        } else {
            operand_ty
        };

        let index_ty = self.ctx.krate.types.intern(Type::Int);
        let mut expanded = Vec::with_capacity(items.len());
        for (offset, item_ty) in items.iter().enumerate() {
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(operand_local),
                ty: receiver_ty,
                span,
            });
            // A tuple index must reach codegen as a constant INTEGER: the
            // emitter renders it as a Rust field access (`.0`, `.1`), which has
            // no runtime index to compute, and rejects anything it cannot read
            // as a constant.
            let index = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Int(i64::try_from(offset).unwrap_or(i64::MAX))),
                ty: index_ty,
                span,
            });
            expanded.push(body.push_expr(Expr {
                kind: ExprKind::Index { receiver, index },
                ty: *item_ty,
                span,
            }));
        }
        Ok(Some(expanded))
    }

    /// Emit `if (spread == null) throw new TypeError(..)` before an optional
    /// tuple spread is indexed.
    ///
    /// Spreading `undefined` or `null` is a `TypeError` in JavaScript, so the
    /// absent arm throws through the ordinary throw route and is catchable
    /// exactly like a source `throw`. Per-element defaults would be the silent
    /// wrong answer this whole family is about.
    ///
    /// `error_object_from_message` builds it with the `TypeError` class name, so
    /// `error.name` answers `TypeError` and `instanceof TypeError` holds --
    /// the same record a source `throw new TypeError(..)` produces.
    fn throw_when_spread_operand_is_absent(
        &mut self,
        operand_local: smelt_hir::LocalId,
        operand_ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let operand = body.push_expr(Expr {
            kind: ExprKind::Local(operand_local),
            ty: operand_ty,
            span,
        });
        let none = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: operand_ty,
            span,
        });
        // `== null` is the one comparison true for BOTH `undefined` and `null`,
        // which is exactly the pair that cannot be spread. `StrictEq` would miss
        // an arm.
        let absent = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::Eq,
                lhs: operand,
                rhs: none,
            },
            ty: bool_ty,
            span,
        });
        let message = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(
                "spread of an absent value is not iterable".to_owned(),
            )),
            ty: string_ty,
            span,
        });
        let error = self.error_object_from_message(Some(message), "TypeError", span, body);
        let throw_block = body.push_block(span);
        body.push_stmt_to_block(throw_block, Stmt::Throw(error));
        let stmt = Stmt::If {
            cond: absent,
            then_block: throw_block,
            else_block: None,
        };
        self.push_stmt_to_sink(body, stmt);
    }

    /// Expand a JavaScript spread call into fixed function parameters plus rest.
    pub(in crate::lowering) fn spread_closure_call_arguments(
        &mut self,
        function: &FunctionType,
        rest: Option<RestParam>,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let rest = rest.ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "spread closure calls require a rest-like list parameter",
            )
        })?;
        let fixed_param_count = rest.index;
        let mut args = Vec::new();
        let rest_ty = self.ctx.krate.types.intern(Type::List(rest.item_ty));
        let mut rest_items = Vec::new();
        let mut rest_list = None;
        let mut fixed_index = 0;
        for argument in &call.arguments {
            if fixed_index < fixed_param_count {
                let Argument::SpreadElement(spread) = argument else {
                    args.push(self.argument(argument, body)?);
                    fixed_index += 1;
                    continue;
                };
                // The fixed parameters read the list as written: only what lands in
                // the rest takes the rest's element type, and `predicate` of
                // `f(...args)` is not an element of `(n, predicate, ...rest: number[])`.
                let written = self.expression(&spread.argument, body)?;
                let spread_list = if matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, written)),
                    Some(Type::List(_))
                ) {
                    written
                } else {
                    self.rest_spread_list_value(written, rest_ty, spread.span, body)
                };
                let consumed = fixed_param_count - fixed_index;
                for offset in 0..consumed {
                    let index = self.usize_float_literal(
                        offset,
                        self.span(spread.span.start, spread.span.end),
                        body,
                    )?;
                    let ty = function
                        .params
                        .get(fixed_index + offset)
                        .copied()
                        .unwrap_or_else(|| {
                            self.index_type(Self::expr_ty(body, spread_list))
                                .unwrap_or(rest.item_ty)
                        });
                    let kind = if matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
                        ExprKind::OptionalIndex {
                            receiver: spread_list,
                            index,
                            from_end: false,
                        }
                    } else {
                        ExprKind::Index {
                            receiver: spread_list,
                            index,
                        }
                    };
                    args.push(body.push_expr(Expr {
                        kind,
                        ty,
                        span: self.span(spread.span.start, spread.span.end),
                    }));
                }
                fixed_index = fixed_param_count;
                let spread_rest = if consumed == 0 {
                    self.rest_spread_list_value(spread_list, rest_ty, spread.span, body)
                } else {
                    let start = self.usize_float_literal(
                        consumed,
                        self.span(spread.span.start, spread.span.end),
                        body,
                    )?;
                    let tail = body.push_expr(Expr {
                        kind: ExprKind::ListSlice {
                            list: spread_list,
                            start: Some(start),
                            end: None,
                        },
                        ty: Self::expr_ty(body, spread_list),
                        span: self.span(spread.span.start, spread.span.end),
                    });
                    self.rest_spread_list_value(tail, rest_ty, spread.span, body)
                };
                rest_list = Some(rest_list.map_or(spread_rest, |left| {
                    body.push_expr(Expr {
                        kind: ExprKind::ListConcat {
                            left,
                            right: spread_rest,
                        },
                        ty: rest_ty,
                        span: self.span(spread.span.start, spread.span.end),
                    })
                }));
                continue;
            }
            match argument {
                Argument::SpreadElement(spread) => {
                    Self::flush_rest_items(
                        &mut rest_items,
                        &mut rest_list,
                        rest_ty,
                        body,
                        self.span(spread.span.start, spread.span.end),
                    );
                    let spread_list = self.rest_spread_list_expression(
                        &spread.argument,
                        rest_ty,
                        spread.span,
                        body,
                    )?;
                    rest_list = Some(rest_list.map_or(spread_list, |left| {
                        body.push_expr(Expr {
                            kind: ExprKind::ListConcat {
                                left,
                                right: spread_list,
                            },
                            ty: rest_ty,
                            span: self.span(spread.span.start, spread.span.end),
                        })
                    }));
                }
                _ => rest_items.push(self.argument(argument, body)?),
            }
        }
        if fixed_index < fixed_param_count {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "spread closure call does not provide enough fixed parameters",
            ));
        }
        Self::flush_rest_items(
            &mut rest_items,
            &mut rest_list,
            rest_ty,
            body,
            self.span(call.span.start, call.span.end),
        );
        args.push(rest_list.unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::ListLit(Vec::new()),
                ty: rest_ty,
                span: self.span(call.span.start, call.span.end),
            })
        }));
        Ok(args)
    }

    /// Convert an array spread argument to the statically required rest-list type.
    ///
    /// Generic array parameters are represented as erased runtime values even
    /// when their source constraint is an array. A rest call still needs their
    /// array payload for concatenation, so extract that payload before building
    /// the packed argument vector.
    pub(in crate::lowering) fn rest_spread_list_expression(
        &mut self,
        expression: &Expression<'_>,
        rest_ty: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(expression, body)?;
        Ok(self.rest_spread_list_value(value, rest_ty, span, body))
    }

    /// [`Self::rest_spread_list_expression`] for an already lowered `value`.
    fn rest_spread_list_value(
        &self,
        value: smelt_hir::ExprId,
        rest_ty: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        self.rest_spread_value_at_span(value, rest_ty, self.span(span.start, span.end), body)
    }

    /// [`Self::rest_spread_list_value`] at an already resolved `span`.
    pub(in crate::lowering) fn rest_spread_value_at_span(
        &self,
        value: smelt_hir::ExprId,
        rest_ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let value_ty = Self::expr_ty(body, value);
        if value_ty == rest_ty {
            return value;
        }
        if self.type_contains_unknown(value_ty)
            || matches!(
                self.ctx.krate.types.get(value_ty),
                Some(Type::TypeParam { .. } | Type::Union(_))
            )
        {
            return body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value,
                    target: rest_ty,
                },
                ty: rest_ty,
                span,
            });
        }
        // A concrete list converts element by element: `g(...numbers)` into
        // `(...args: any[])` is a `List<Float>` for a `List<Unknown>` slot, and
        // the emitter reads a lone operand of another type as one element of the
        // rest, not all of it.
        body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: rest_ty,
            span,
        })
    }

    /// Lower a small positional index as the numeric literal used by JS indexing.
    pub(in crate::lowering) fn usize_float_literal(
        &mut self,
        value: usize,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = value.to_string().parse::<f64>().map_err(|err| {
            SmeltError::unsupported(span, format!("spread index cannot be represented: {err}"))
        })?;
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(value)),
            ty,
            span,
        }))
    }

    /// Append queued rest items to the accumulated rest list expression.
    pub(in crate::lowering) fn flush_rest_items(
        rest_items: &mut Vec<smelt_hir::ExprId>,
        rest_list: &mut Option<smelt_hir::ExprId>,
        rest_ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) {
        if rest_items.is_empty() {
            return;
        }
        let right = body.push_expr(Expr {
            kind: ExprKind::ListLit(std::mem::take(rest_items)),
            ty: rest_ty,
            span,
        });
        *rest_list = Some(rest_list.map_or(right, |left| {
            body.push_expr(Expr {
                kind: ExprKind::ListConcat { left, right },
                ty: rest_ty,
                span,
            })
        }));
    }

    /// Pack JavaScript spread call arguments into one variadic list argument.
    pub(in crate::lowering) fn packed_spread_call_arguments(
        &mut self,
        item_ty: smelt_hir::TypeId,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(call.span.start, call.span.end);
        self.packed_spread_arguments(item_ty, &call.arguments, span, body)
    }

    /// Pack an argument slice (call or `new` expression) into one variadic list.
    pub(in crate::lowering) fn packed_spread_arguments(
        &mut self,
        item_ty: smelt_hir::TypeId,
        arguments: &[Argument<'_>],
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let mut current_items = Vec::new();
        let mut packed = None;
        for arg in arguments {
            match arg {
                Argument::SpreadElement(spread) => {
                    if !current_items.is_empty() {
                        let left = body.push_expr(Expr {
                            kind: ExprKind::ListLit(std::mem::take(&mut current_items)),
                            ty: list_ty,
                            span,
                        });
                        packed = Some(packed.map_or(left, |existing| {
                            body.push_expr(Expr {
                                kind: ExprKind::ListConcat {
                                    left: existing,
                                    right: left,
                                },
                                ty: list_ty,
                                span,
                            })
                        }));
                    }
                    let spread_expr = self.expression(&spread.argument, body)?;
                    packed = Some(packed.map_or(spread_expr, |existing| {
                        body.push_expr(Expr {
                            kind: ExprKind::ListConcat {
                                left: existing,
                                right: spread_expr,
                            },
                            ty: list_ty,
                            span,
                        })
                    }));
                }
                other => current_items.push(self.argument(other, body)?),
            }
        }
        if !current_items.is_empty() {
            let right = body.push_expr(Expr {
                kind: ExprKind::ListLit(current_items),
                ty: list_ty,
                span,
            });
            packed = Some(packed.map_or(right, |existing| {
                body.push_expr(Expr {
                    kind: ExprKind::ListConcat {
                        left: existing,
                        right,
                    },
                    ty: list_ty,
                    span,
                })
            }));
        }
        packed.ok_or_else(|| {
            SmeltError::unsupported(span, "spread call requires at least one argument")
        })
    }

    /// Return whether a call uses JavaScript spread arguments.
    pub(in crate::lowering) fn call_has_spread_arguments(
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> bool {
        call.arguments
            .iter()
            .any(|argument| matches!(argument, Argument::SpreadElement(_)))
    }
}
