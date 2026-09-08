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
use smelt_hir::{Body, Expr, ExprKind, Literal, Type};

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
    /// `argument()` carries a debug assertion that it never sees a spread, so a
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
    /// Anything else -- a list, an erased value, an OPTIONAL tuple -- has no
    /// statically known width here and stays a named blocker. A list's home is
    /// the packed-vector call path (`ClosureCallSpread`), which an erased callee
    /// already takes; a fixed-arity callee has nowhere to put a runtime-length
    /// argument list.
    ///
    /// The optional case deserves a note, because the obvious lowering is a
    /// throw: spreading `undefined` is a `TypeError` in JavaScript. It is a
    /// blocker instead because no source shape reaches here with an optional
    /// operand -- every route resolves the optional FIRST. A cast (`...x as T`)
    /// lowers to `.expect(..)` through the narrowing path, and a list element
    /// read (`...xs[i]`, which is how Hono's `router.add(...routes[i])` is
    /// spelled) lowers to `.unwrap_or_else(|| default)` through the array-hole
    /// path. Both were tried in the runtime fixture. Writing the throw anyway
    /// would have added an untestable branch, so the shape is rejected here and
    /// the finding is recorded in `hono-h46-spread-arguments.md` instead.
    fn spread_argument_elements(
        &mut self,
        spread: &oxc::ast::ast::SpreadElement<'_>,
        body: &mut Body,
    ) -> Result<Option<Vec<smelt_hir::ExprId>>, SmeltError> {
        let span = self.span(spread.span.start, spread.span.end);
        let operand = self.expression(&spread.argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        let resolved = self.type_param_constraint_or_self(operand_ty);
        let tuple_ty = match self.ctx.krate.types.get(resolved).cloned() {
            Some(Type::Tuple(_)) => resolved,
            // Not statically widthed: hand it back unexpanded rather than
            // rejecting it. See `CallArg::UnexpandedSpread`.
            _ => return Ok(None),
        };
        let Some(Type::Tuple(items)) = self.ctx.krate.types.get(tuple_ty).cloned() else {
            return Ok(None);
        };

        let spread_name = self.intern_source_name("__smelt_spread");
        let operand_local = self.capture_bind_value(operand, operand_ty, spread_name, span, body);
        let receiver_ty = operand_ty;

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
}
