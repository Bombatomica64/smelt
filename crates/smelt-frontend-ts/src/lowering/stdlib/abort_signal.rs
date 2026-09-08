//! `AbortSignal` static lowering for the TypeScript frontend.
//!
//! The two class-side members, `AbortSignal.abort(reason?)` and
//! `AbortSignal.timeout(ms)`. Both answer a signal, which is the erased
//! marker-bearing record the abort subsystem owns, so the result type is
//! `unknown` — the same type `new AbortController().signal` has, and by the
//! same design decision: the signal's own `reason` holds any JavaScript value,
//! so nothing about it is concrete yet (see
//! `blocker-logs/abort-signal-reason-and-statics.md`).
//!
//! A user class named `AbortSignal` still wins, as with every other modeled
//! host name: the registry models the host name, not the spelling.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use smelt_hir::{AbortSignalOp, Body, Expr, ExprKind, Type};
use smelt_stdlib::RuleId;

impl ModuleBuilder<'_> {
    /// Lower a recognized `AbortSignal` static call.
    pub(in crate::lowering) fn abort_signal_static_call(
        &mut self,
        rule: RuleId,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.scope.is_bound("AbortSignal") || self.classes.contains("AbortSignal") {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let op = match rule {
            RuleId::TsAbortSignalAbort => AbortSignalOp::Abort,
            RuleId::TsAbortSignalTimeout => AbortSignalOp::Timeout,
            _ => return Ok(None),
        };
        // `abort` takes an OPTIONAL reason and `timeout` a required delay, and
        // the arity is checked here rather than in codegen so a mis-call is a
        // source-span blocker instead of an emitter error with no location.
        if op == AbortSignalOp::Timeout && call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                span,
                "`AbortSignal.timeout` takes exactly one argument, the delay in milliseconds",
            ));
        }
        if op == AbortSignalOp::Abort && call.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                span,
                "`AbortSignal.abort` takes at most one argument, the abort reason",
            ));
        }
        let args = call
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AbortSignalOp { op, args },
            ty: unknown_ty,
            span,
        })))
    }
}
