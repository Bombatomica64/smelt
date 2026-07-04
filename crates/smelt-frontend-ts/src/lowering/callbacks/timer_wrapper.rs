//! Typed wrapper-closure synthesis for timer extra arguments.
//!
//! `setTimeout(callback, ms, ...args)` and `setInterval(callback, ms, ...args)`
//! forward the trailing `...args` to `callback` when the timer fires. The erased
//! lowering packs those extras into one `Vec<SmeltUnknown>` and dispatches the
//! callback through the dynamic `SmeltUnknown` callable ABI. That is only needed
//! at a genuine dynamic boundary (source spread, an untyped extra, or a callback
//! whose value type is not a statically known function).
//!
//! When the callback is a statically typed function and every extra argument has
//! a concrete (non-`unknown`) static type, this module instead synthesizes a
//! zero-argument wrapper closure `move || callback(arg0, arg1, …)` that captures
//! the callback and each typed extra by value and invokes the original callback
//! directly. The resulting timer operand is a plain zero-parameter closure, so
//! the emitter's direct-call fast path runs it without any `Vec<SmeltUnknown>`.
//!
//! Capturing by value is required for ownership correctness: the wrapper is
//! stored in the timer registry and called after the enclosing function returns,
//! so it must own the callback and the captured argument values rather than
//! borrow stack locals that would already be gone. MIR escape analysis only
//! promotes closures reachable from a `return`, and a timer wrapper escapes
//! through a runtime async-op argument instead, so the by-value modes are set
//! here at construction time.

use crate::lowering::{
    Body, CaptureMode, ClosureCapture, Expr, ExprKind, FunctionType, LocalDecl, ModuleBuilder,
    Pattern, Span, Stmt, Type,
};
use smelt_hir::ClosureExpr;

impl ModuleBuilder<'_> {
    /// Try to lower typed timer extra arguments through a wrapper closure.
    ///
    /// Returns `Some(closure_expr)` when the timer arguments are fully static: the
    /// `callback` is a statically typed function (no rest parameter), every
    /// operand in `extras` has a concrete static type, and the extras cover the
    /// callback's parameters exactly. In that case it synthesizes a zero-argument
    /// wrapper that captures the callback and each extra by value and calls the
    /// callback directly, so no erased `Vec<SmeltUnknown>` is produced.
    ///
    /// Returns `None` for any residual dynamic boundary — a non-function callback,
    /// a rest-parameter callback, an `unknown`-typed extra, or an argument count
    /// that does not match the callback signature exactly — so the caller keeps
    /// the erased dynamic-dispatch path that mirrors JavaScript's
    /// fill-`undefined`/ignore-surplus argument semantics.
    ///
    /// `callback` and each entry of `extras` must already be lowered expressions
    /// in `body`; source spreads are handled by the caller and never reach here.
    pub(in crate::lowering) fn timer_typed_wrapper_closure(
        &mut self,
        callback: smelt_hir::ExprId,
        extras: &[smelt_hir::ExprId],
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let callback_ty = Self::expr_ty(body, callback);
        let Some(Type::Function(callback_fn)) = self.ctx.krate.types.get(callback_ty).cloned()
        else {
            // The callback is not a statically known function value (e.g. it is
            // erased), so calling it directly is not possible; keep the erased
            // dynamic-dispatch path.
            return None;
        };

        // The provided extras must cover every declared callback parameter
        // exactly. Forwarding fewer arguments would make the wrapper synthesize
        // omitted-parameter values, and Rust's typed default
        // (`String::default()`, `0.0`, …) does not match JavaScript's `undefined`
        // for an omitted optional — a callback that branches on `arg ?? fallback`
        // would observe the wrong value. A rest-parameter callback would need
        // variadic packing (its own erased boundary), so both the mismatched-arity
        // and rest cases fall back to the erased path, which mirrors JavaScript's
        // argument-forwarding semantics through the dynamic ABI.
        if callback_fn.rest.is_some() || extras.len() != callback_fn.params.len() {
            return None;
        }

        // Every extra argument must carry a concrete static type. An `unknown`
        // extra is a real dynamic boundary and must stay in the erased list.
        let extra_tys = extras
            .iter()
            .map(|&extra| Self::expr_ty(body, extra))
            .collect::<Vec<_>>();
        if extra_tys
            .iter()
            .any(|&ty| self.type_contains_unknown(ty))
        {
            return None;
        }

        // Bind the callback and each extra to fresh outer-body locals so the
        // wrapper can capture them by value from stable capture sources.
        let callback_local = self.bind_timer_capture_local(callback, callback_ty, "smelt_timer_callback", span, body);
        let extra_locals = extras
            .iter()
            .zip(&extra_tys)
            .enumerate()
            .map(|(index, (&extra, &ty))| {
                self.bind_timer_capture_local(
                    extra,
                    ty,
                    &format!("smelt_timer_arg_{index}"),
                    span,
                    body,
                )
            })
            .collect::<Vec<_>>();

        // Build the wrapper body: a zero-parameter closure whose captures are the
        // callback and the extra values, and whose tail calls the callback with
        // the captured extras.
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let mut closure_body = Body::new(None, span);

        let callback_symbol = self.ctx.krate.symbols.intern("smelt_timer_callback");
        let callback_body_local = closure_body.push_local(LocalDecl {
            name: Some(callback_symbol),
            ty: callback_ty,
            mutable: false,
            span,
        });
        let mut captures = vec![ClosureCapture {
            source_local: callback_local,
            body_local: Some(callback_body_local),
            symbol: callback_symbol,
            ty: callback_ty,
            mode: CaptureMode::ByValue,
        }];

        let mut arg_exprs = Vec::with_capacity(extra_locals.len());
        for (index, (&source_local, &ty)) in extra_locals.iter().zip(&extra_tys).enumerate() {
            let symbol = self
                .ctx
                .krate
                .symbols
                .intern(&format!("smelt_timer_arg_{index}"));
            let body_local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span,
            });
            captures.push(ClosureCapture {
                source_local,
                body_local: Some(body_local),
                symbol,
                ty,
                mode: CaptureMode::ByValue,
            });
            arg_exprs.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(body_local),
                ty,
                span,
            }));
        }

        let callee_expr = closure_body.push_expr(Expr {
            kind: ExprKind::Local(callback_body_local),
            ty: callback_ty,
            span,
        });
        let call_expr = closure_body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: callee_expr,
                args: arg_exprs,
            },
            ty: callback_fn.return_ty,
            span,
        });
        if let Some(root) = closure_body.blocks.first_mut() {
            root.tail = Some(call_expr);
        }
        let body_id = self.ctx.krate.push_body(closure_body);

        let wrapper_fn = FunctionType {
            params: Vec::new(),
            rest: None,
            required_params: Some(0),
            mutable_params: Vec::new(),
            return_ty: none_ty,
            is_async: false,
            may_throw: callback_fn.may_throw,
        };
        let wrapper_ty = self.ctx.krate.types.intern(Type::Function(wrapper_fn));

        Some(body.push_expr(Expr {
            kind: ExprKind::Closure(ClosureExpr {
                params: Vec::new(),
                rest: None,
                required_params: Some(0),
                return_ty: none_ty,
                captures,
                body: body_id,
                function_item: None,
                span,
            }),
            ty: wrapper_ty,
            span,
        }))
    }

    /// Bind a lowered value expression to a fresh immutable outer-body local.
    ///
    /// The synthesized wrapper closure captures its callback and extra arguments
    /// from body locals, so each value is first stored in its own `let` binding
    /// that becomes the capture source.
    fn bind_timer_capture_local(
        &mut self,
        value: smelt_hir::ExprId,
        ty: smelt_hir::TypeId,
        name: &str,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::LocalId {
        let symbol = self.ctx.krate.symbols.intern(name);
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span,
        });
        let pat = body.push_pattern(Pattern::Binding(local));
        body.push_stmt(Stmt::Let {
            pat,
            ty,
            value: Some(value),
        });
        local
    }
}
