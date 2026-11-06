//! Forwarding closures: the callback a name or a callable value stands for.
//!
//! `xs.map(parse)` has no body of its own to lower. It is a closure that takes
//! the receiver's arguments and hands them to `parse`, and this module builds
//! that closure directly as HIR. A callback with a source body lowers through
//! [`ModuleBuilder::callback_argument`] and never reaches here.

use crate::lowering::{
    BinOp, Body, CaptureMode, ClosureCapture, Expr, ExprKind, FunctionType, Literal, LocalDecl,
    ModuleBuilder, Param, PrimitiveCastOp, SmeltError, Span, Type,
};

/// What a forwarding closure does with its parameters.
#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) enum ForwardTarget {
    /// Calls the function item with this symbol, at its value type.
    Item {
        /// The item's source symbol.
        function: smelt_hir::Symbol,
        /// The item's function type.
        ty: smelt_hir::TypeId,
    },
    /// Calls the value held by a local of the enclosing body.
    Local {
        /// The captured local.
        local: smelt_hir::LocalId,
        /// Its type, a function or a callable erased surface.
        ty: smelt_hir::TypeId,
    },
    /// Calls an opaque member value whose callee has no lowered form here.
    Opaque {
        /// The function type the receiver's arguments imply.
        ty: smelt_hir::TypeId,
    },
    /// The lodash `[path, value]` iteratee: `element[field] === value`.
    MatchesProperty {
        /// The element field read.
        field: smelt_hir::Symbol,
        /// The read's erased type.
        field_ty: smelt_hir::TypeId,
        /// The captured local holding the expected value.
        value: smelt_hir::LocalId,
        /// That local's type.
        value_ty: smelt_hir::TypeId,
    },
}

/// A forwarding callback: its target, what it passes, and what it answers.
#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) struct ForwardingCallback {
    /// What the closure calls or compares.
    pub(in crate::lowering) target: ForwardTarget,
    /// How many leading closure parameters are forwarded.
    pub(in crate::lowering) arity: usize,
    /// The closure's result type.
    pub(in crate::lowering) ty: smelt_hir::TypeId,
}

impl ModuleBuilder<'_> {
    /// Build `callback` as a closure taking `params`.
    pub(in crate::lowering) fn forwarding_closure(
        &mut self,
        callback: ForwardingCallback,
        params: &[smelt_hir::TypeId],
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.forwarding_closure_answering(callback, params, callback.ty, span, body)
    }

    /// Build `callback` as a predicate: its answer goes through JavaScript truthiness.
    pub(in crate::lowering) fn forwarding_predicate_closure(
        &mut self,
        callback: ForwardingCallback,
        params: &[smelt_hir::TypeId],
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        self.forwarding_closure_answering(callback, params, bool_ty, span, body)
    }

    /// Build the closure and coerce its answer to `return_ty`.
    fn forwarding_closure_answering(
        &mut self,
        callback: ForwardingCallback,
        params: &[smelt_hir::TypeId],
        return_ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut closure_body = Body::new(None, span);
        let closure_params = params
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                let name = self.ctx.krate.symbols.intern(&format!("__callback_param_{index}"));
                let local = closure_body.push_local(LocalDecl {
                    name: Some(name),
                    ty: *ty,
                    mutable: false,
                    span,
                });
                closure_body.params.push(local);
                Param { name, local, ty: *ty, span }
            })
            .collect::<Vec<_>>();
        let (captures, body_local) = match callback.target {
            ForwardTarget::Local { local, .. } | ForwardTarget::MatchesProperty { value: local, .. } => {
                self.forwarded_capture(local, body, &mut closure_body, span)
            }
            ForwardTarget::Item { .. } | ForwardTarget::Opaque { .. } => (Vec::new(), None),
        };
        let param_exprs = closure_params
            .iter()
            .map(|param| {
                closure_body.push_expr(Expr {
                    kind: ExprKind::Local(param.local),
                    ty: param.ty,
                    span,
                })
            })
            .collect::<Vec<_>>();
        let answer = self.forwarded_answer(callback, body_local, &param_exprs, &mut closure_body, span)?;
        let tail = if return_ty == callback.ty {
            answer
        } else {
            self.forwarded_truthiness(answer, callback.ty, return_ty, &mut closure_body, span)?
        };
        if let Some(root) = closure_body.blocks.first_mut() {
            root.tail = Some(tail);
        }
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: params.to_vec(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        }));
        Ok(body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest: None,
                required_params: None,
                return_ty,
                captures,
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Capture `local` of the enclosing body into the closure.
    ///
    /// A capture is normally by reference: the value is already in the local when
    /// the closure is built. A binding the closure reads before its declaration
    /// is still empty then, so it goes through the shared cell instead.
    fn forwarded_capture(
        &mut self,
        local: smelt_hir::LocalId,
        body: &Body,
        closure_body: &mut Body,
        span: Span,
    ) -> (Vec<ClosureCapture>, Option<smelt_hir::LocalId>) {
        let Some(decl) = usize::try_from(local.0).ok().and_then(|index| body.locals.get(index)) else {
            return (Vec::new(), None);
        };
        let mode = if decl
            .name
            .and_then(|symbol| self.ctx.krate.symbols.get(symbol))
            .is_some_and(|name| self.is_forward_referenced_local(name))
        {
            CaptureMode::ByMut
        } else {
            CaptureMode::ByRef
        };
        let symbol = decl.name.unwrap_or_else(|| self.ctx.krate.symbols.intern("__capture"));
        let body_local = closure_body.push_local(LocalDecl {
            name: Some(symbol),
            ty: decl.ty,
            mutable: false,
            span,
        });
        let capture = ClosureCapture {
            source_local: local,
            body_local: Some(body_local),
            symbol,
            ty: decl.ty,
            mode,
        };
        (Vec::from([capture]), Some(body_local))
    }

    /// The expression a forwarding closure's body answers with.
    fn forwarded_answer(
        &mut self,
        callback: ForwardingCallback,
        body_local: Option<smelt_hir::LocalId>,
        params: &[smelt_hir::ExprId],
        closure_body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let args = params.get(..callback.arity).unwrap_or(params).to_vec();
        let kind = match callback.target {
            ForwardTarget::Item { function, ty } => {
                let item = self.callback_function_item(function, span)?;
                // A rest-parameter item keeps the value form: its wrapper is what
                // packs the positional arguments into the rest list.
                let has_rest = matches!(
                    self.item_ref(item),
                    smelt_hir::Item::Function(item_fn) if item_fn.rest.is_some()
                );
                if has_rest {
                    let callee = self.callback_function_item_closure(item, ty, closure_body, span)?;
                    ExprKind::ClosureCall { callee, args }
                } else {
                    let callee = closure_body.push_expr(Expr { kind: ExprKind::Item(item), ty, span });
                    ExprKind::Call { callee, args }
                }
            }
            ForwardTarget::Local { local, ty } => {
                let callee = closure_body.push_expr(Expr {
                    kind: ExprKind::Local(body_local.unwrap_or(local)),
                    ty,
                    span,
                });
                ExprKind::ClosureCall { callee, args }
            }
            ForwardTarget::Opaque { ty } => {
                let callee = closure_body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span,
                });
                ExprKind::ClosureCall { callee, args }
            }
            ForwardTarget::MatchesProperty { field, field_ty, value, value_ty } => {
                let receiver = params.first().copied().ok_or_else(|| {
                    SmeltError::unsupported(span, "matches-property iteratee takes an element")
                })?;
                let lhs = closure_body.push_expr(Expr {
                    kind: ExprKind::Field { receiver, field },
                    ty: field_ty,
                    span,
                });
                let rhs = closure_body.push_expr(Expr {
                    kind: ExprKind::Local(body_local.unwrap_or(value)),
                    ty: value_ty,
                    span,
                });
                ExprKind::BinOp { op: BinOp::JsStrictEq, lhs, rhs }
            }
        };
        Ok(closure_body.push_expr(Expr { kind, ty: callback.ty, span }))
    }

    /// JavaScript truthiness of a forwarded answer of type `answer_ty`.
    fn forwarded_truthiness(
        &mut self,
        answer: smelt_hir::ExprId,
        answer_ty: smelt_hir::TypeId,
        bool_ty: smelt_hir::TypeId,
        closure_body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let answer_type = self.ctx.krate.types.get(answer_ty).cloned();
        if matches!(answer_type, Some(Type::Unknown | Type::TypeParam { .. })) {
            return Ok(closure_body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast { op: PrimitiveCastOp::ToBool, operand: answer },
                ty: bool_ty,
                span,
            }));
        }
        let (literal, literal_ty) = if answer_type == Some(Type::String) {
            (Literal::String(String::new()), self.ctx.krate.types.intern(Type::String))
        } else if self.is_nullishable_type(answer_ty) || self.type_is_truthy_condition_surface(answer_ty) {
            (Literal::None, self.ctx.krate.types.intern(Type::None))
        } else {
            return Err(SmeltError::unsupported(
                span,
                "array predicate callback return cannot be coerced to boolean",
            ));
        };
        let rhs = closure_body.push_expr(Expr {
            kind: ExprKind::Literal(literal),
            ty: literal_ty,
            span,
        });
        Ok(closure_body.push_expr(Expr {
            kind: ExprKind::BinOp { op: BinOp::NotEq, lhs: answer, rhs },
            ty: bool_ty,
            span,
        }))
    }
}
