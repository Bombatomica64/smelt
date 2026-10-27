//! The single place a spread argument is interpreted for a fixed-arity call.
//!
//! Spread arguments used to be interpreted independently by each of the five
//! paths that collect a call's arguments, and they disagreed: one produced an
//! EMPTY argument list, two treated the spread as a single argument bound to
//! parameter 0, and the emitter then padded whatever was missing with
//! `default_value(..)`. The result was a call with every argument defaulted
//! that compiled cleanly. This module exists so that shape has one definition
//! instead of five.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::Argument;
use smelt_hir::{BinOp, Body, Expr, ExprKind, Literal, Span, Stmt, Type};

/// One argument of a call, after spreads have been interpreted.
///
/// A spread of a tuple contributes SEVERAL arguments, so an argument list can
/// no longer be walked as one AST node per parameter. Every fixed-arity call
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

    /// Distribute a trailing LIST spread over the callee's remaining fixed parameters.
    ///
    /// `f(...xs)` into a callee with fixed parameters and no rest parameter
    /// binds parameter `i` to `xs[i]` in JavaScript (`undefined` past the
    /// end), so the spread lowers to the element reads `xs[0]`, `xs[1]`, ...
    /// for the parameters it covers: the same `Index` read a source `xs[i]`
    /// produces, at the list's element type. The operand is bound to a local
    /// first so it is evaluated once. Hono's `get render()` returns
    /// `(...args) => this.#renderer(...args)` into a one-parameter renderer.
    ///
    /// Only a spread in the LAST position is distributed: arguments after a
    /// runtime-length spread have no static parameter position. And only into
    /// TYPED parameters: a callee whose covered parameters are erased (`any`,
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
        let Some(&CallArg::UnexpandedSpread(spread)) = arguments.last() else {
            return Ok(arguments);
        };
        let position = arguments.len().saturating_sub(1);
        let covered_params = params.get(position..).unwrap_or_default();
        let covered = covered_params.len();
        if covered == 0
            || covered_params.iter().any(|param| matches!(self.ctx.krate.types.get(*param), Some(Type::Unknown)))
        {
            return Ok(arguments);
        }
        arguments.pop();
        let span = self.span(spread.span.start, spread.span.end);
        let operand = self.expression(&spread.argument, body)?;
        let operand = Self::peel_assertion_of_list(operand, body, &self.ctx.krate.types);
        let operand_ty = Self::expr_ty(body, operand);
        let resolved = self.type_param_constraint_or_self(operand_ty);
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(resolved).cloned() else {
            arguments.push(CallArg::Lowered(operand));
            return Ok(arguments);
        };
        let spread_name = self.intern_source_name("__smelt_spread");
        let operand_local = self.capture_bind_value(operand, operand_ty, spread_name, span, body);
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        for offset in 0..covered {
            let receiver = body.push_expr(Expr { kind: ExprKind::Local(operand_local), ty: operand_ty, span });
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
        let (ExprKind::TypeAssert { value } | ExprKind::UnknownCast { value, .. }) = node.kind else {
            return expr;
        };
        if matches!(types.get(Self::expr_ty(body, value)), Some(Type::List(_) | Type::Tuple(_))) {
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
            // The pre-existing reading: lower the operand as ONE argument and
            // let the caller's variadic handling take it from there. This is
            // deliberately not the `Source` arm, so the assertion above still
            // catches a path that never came through `expanded_call_arguments`.
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

        let spread_name = self.intern_source_name("__smelt_spread");
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
        if let Some(block) = self.current_statement_block {
            body.push_stmt_to_block(block, stmt);
        } else {
            body.push_stmt(stmt);
        }
    }
}
