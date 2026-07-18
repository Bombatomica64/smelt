//! Closure conversion for callbacks: turning a [`CallbackExpr`] into an HIR
//! closure, remapping and collecting captures, and lowering callback method
//! calls (`.map`/`.at`/`.join`/string-case/etc.) into body expressions.

use crate::lowering::{
    Body, CallbackCallArg, CallbackExpr, CallbackExprKind, ClosureCapture, Expr, ExprKind,
    FunctionType, HashMap, ListSearchOp, Literal, LocalCallbackDefault, LocalDecl, ModuleBuilder,
    Param, PrimitiveCastOp, SmeltError, Span, Stmt, StringAffixOp, StringCaseOp, Type,
};
use smelt_hir::StringTrimSide;

impl ModuleBuilder<'_> {
    /// Validate the inferred callback return type for an array method.
    pub(in crate::lowering) fn require_callback_ty(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
        call: &oxc::ast::ast::CallExpression<'_>,
        context: &'static str,
    ) -> Result<(), SmeltError> {
        if actual == expected {
            Ok(())
        } else {
            Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{context} callback returns an unsupported type"),
            ))
        }
    }

    /// Store a lowered callback expression as a first-class closure expression.
    pub(in crate::lowering) fn callback_expr_to_closure(
        &mut self,
        callback: &CallbackExpr,
        params: &[smelt_hir::TypeId],
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.callback_expr_to_closure_with_return_ty(
            callback.ty,
            callback,
            params,
            None,
            None,
            span,
            body,
        )
    }

    /// Store a lowered callback expression as a closure with an explicit return type.
    pub(in crate::lowering) fn callback_expr_to_closure_with_return_ty(
        &mut self,
        return_ty: smelt_hir::TypeId,
        callback: &CallbackExpr,
        params: &[smelt_hir::TypeId],
        rest: Option<usize>,
        required_params: Option<usize>,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut closure_body = Body::new(None, span);
        let closure_params = params
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                let name = self
                    .ctx
                    .krate
                    .symbols
                    .intern(&format!("__callback_param_{index}"));
                let local = closure_body.push_local(LocalDecl {
                    name: Some(name),
                    ty: *ty,
                    mutable: false,
                    span,
                });
                closure_body.params.push(local);
                Param {
                    name,
                    local,
                    ty: *ty,
                    span,
                }
            })
            .collect::<Vec<_>>();
        let mut captures = self.callback_captures(callback, body);
        let may_throw = Self::callback_expr_contains_throw(callback);
        let mut migrated_callback = callback.clone();
        let mut capture_locals = HashMap::new();
        for capture in &mut captures {
            let local = closure_body.push_local(LocalDecl {
                name: Some(capture.symbol),
                ty: capture.ty,
                mutable: false,
                span,
            });
            capture.body_local = Some(local);
            capture_locals.insert(capture.source_local, local);
        }
        Self::remap_callback_captures(&mut migrated_callback, &capture_locals);
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
        let tail =
            self.callback_expr_to_body_expr(&migrated_callback, &param_exprs, &mut closure_body, span)?;
        if let Some(root) = closure_body.blocks.first_mut() {
            root.tail = Some(tail);
        }
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: params.to_vec(),
            rest,
            required_params,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw,
        }));
        Ok(body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest,
                required_params,
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

    /// Remap callback capture references to locals declared in the closure body.
    pub(in crate::lowering) fn remap_callback_captures(
        callback: &mut CallbackExpr,
        capture_locals: &HashMap<smelt_hir::LocalId, smelt_hir::LocalId>,
    ) {
        match &mut callback.kind {
            CallbackExprKind::Capture(local) => {
                if let Some(mapped) = capture_locals.get(local) {
                    *local = *mapped;
                }
            }
            CallbackExprKind::AssignCapture { target, value } => {
                if let Some(mapped) = capture_locals.get(target) {
                    *target = *mapped;
                }
                Self::remap_callback_captures(value, capture_locals);
            }
            CallbackExprKind::ListLit(items) => {
                for item in items {
                    Self::remap_callback_captures(item, capture_locals);
                }
            }
            CallbackExprKind::Sequence { effects, result } => {
                for effect in effects {
                    Self::remap_callback_captures(effect, capture_locals);
                }
                Self::remap_callback_captures(result, capture_locals);
            }
            CallbackExprKind::DictLit(entries) => {
                for (_, value) in entries {
                    Self::remap_callback_captures(value, capture_locals);
                }
            }
            CallbackExprKind::Throw { message } => {
                if let Some(message) = message {
                    Self::remap_callback_captures(message, capture_locals);
                }
            }
            CallbackExprKind::Index { receiver, .. }
            | CallbackExprKind::Field { receiver, .. }
            | CallbackExprKind::HasField { receiver, .. }
            | CallbackExprKind::FieldTruthy { receiver, .. }
            | CallbackExprKind::UnknownIs {
                value: receiver, ..
            }
            | CallbackExprKind::TypeofValue { value: receiver } => {
                Self::remap_callback_captures(receiver, capture_locals);
            }
            CallbackExprKind::DynamicIndex { receiver, index } => {
                Self::remap_callback_captures(receiver, capture_locals);
                Self::remap_callback_captures(index, capture_locals);
            }
            CallbackExprKind::HasDynamicField { receiver, field } => {
                Self::remap_callback_captures(receiver, capture_locals);
                Self::remap_callback_captures(field, capture_locals);
            }
            CallbackExprKind::Unary { operand, .. } => {
                Self::remap_callback_captures(operand, capture_locals);
            }
            CallbackExprKind::Binary { lhs, rhs, .. } => {
                Self::remap_callback_captures(lhs, capture_locals);
                Self::remap_callback_captures(rhs, capture_locals);
            }
            CallbackExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                Self::remap_callback_captures(cond, capture_locals);
                Self::remap_callback_captures(then_expr, capture_locals);
                Self::remap_callback_captures(else_expr, capture_locals);
            }
            CallbackExprKind::Call { callee, args } => {
                Self::remap_callback_captures(callee, capture_locals);
                for arg in args {
                    Self::remap_callback_captures(&mut arg.expr, capture_locals);
                }
            }
            CallbackExprKind::MethodCall { receiver, args, .. } => {
                Self::remap_callback_captures(receiver, capture_locals);
                for arg in args {
                    Self::remap_callback_captures(&mut arg.expr, capture_locals);
                }
            }
            CallbackExprKind::FunctionTableLookup { key, .. } => {
                Self::remap_callback_captures(key, capture_locals);
            }
            CallbackExprKind::Param(_)
            | CallbackExprKind::Function(_)
            | CallbackExprKind::Literal(_) => {}
        }
    }

    /// Collect explicit captures from a callback expression tree.
    pub(in crate::lowering) fn callback_captures(&mut self, callback: &CallbackExpr, body: &Body) -> Vec<ClosureCapture> {
        let mut captures = HashMap::new();
        self.collect_callback_captures(callback, body, &mut captures);
        // `HashMap` iteration order is randomized per process, so sort by the
        // stable source local id before returning. Capture clone preludes are an
        // unordered set, so this only fixes emission determinism (stable
        // snapshots) without changing semantics.
        let mut captures = captures.into_values().collect::<Vec<_>>();
        captures.sort_by_key(|capture| capture.source_local.0);
        captures
    }

    /// Instantiate a stored local-callback default expression at one call site.
    pub(in crate::lowering) fn local_callback_default_expr(
        &mut self,
        default: &LocalCallbackDefault,
        args: &[smelt_hir::ExprId],
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match default {
            LocalCallbackDefault::Callback(callback) => {
                self.callback_expr_to_body_expr(callback, args, body, span)
            }
        }
    }

    /// Convert a callback expression tree into a normal HIR expression using call-site arguments.
    pub(in crate::lowering) fn callback_expr_to_body_expr(
        &mut self,
        callback: &CallbackExpr,
        args: &[smelt_hir::ExprId],
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match &callback.kind {
            CallbackExprKind::Param(index) => args.get(*index).copied().ok_or_else(|| {
                SmeltError::unsupported(
                    span,
                    "default argument references an unavailable parameter",
                )
            }),
            CallbackExprKind::Capture(local) => Ok(body.push_expr(Expr {
                kind: ExprKind::Local(*local),
                ty: callback.ty,
                span,
            })),
            CallbackExprKind::Literal(literal) => Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(literal.clone()),
                ty: callback.ty,
                span,
            })),
            CallbackExprKind::ListLit(items) => {
                let items = items
                    .iter()
                    .map(|item| self.callback_expr_to_body_expr(item, args, body, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::ListLit(items),
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::Throw { message } => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let message = if let Some(message) = message {
                    self.callback_expr_to_body_expr(message, args, body, span)?
                } else {
                    body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::String("callback threw".to_owned())),
                        ty: string_ty,
                        span,
                    })
                };
                body.push_stmt(Stmt::Throw(message));
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::Function(function) => {
                let item = self.callback_function_item(*function, span)?;
                self.callback_function_item_closure(item, callback.ty, body, span)
            }
            CallbackExprKind::FunctionTableLookup { key, cases } => {
                let key = self.callback_expr_to_body_expr(key, args, body, span)?;
                let cases = cases
                    .iter()
                    .map(|(case_key, function)| {
                        let item = self.callback_function_item(*function, span)?;
                        let value =
                            self.callback_function_item_closure(item, callback.ty, body, span)?;
                        Ok((case_key.clone(), value))
                    })
                    .collect::<Result<Vec<_>, SmeltError>>()?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::FunctionTableLookup { key, cases },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::HasField { receiver, field } => {
                let dict = self.callback_expr_to_body_expr(receiver, args, body, span)?;
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let key = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(
                        self.ctx
                            .krate
                            .symbols
                            .get(*field)
                            .unwrap_or_default()
                            .to_owned(),
                    )),
                    ty: string_ty,
                    span,
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::DictContainsKey { dict, key },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::HasDynamicField { receiver, field } => {
                let dict = self.callback_expr_to_body_expr(receiver, args, body, span)?;
                let key = self.callback_expr_to_body_expr(field, args, body, span)?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::DictContainsKey { dict, key },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::Sequence { effects, result } => {
                for effect in effects {
                    let effect = self.callback_expr_to_body_expr(effect, args, body, span)?;
                    body.push_stmt(Stmt::Expr(effect));
                }
                self.callback_expr_to_body_expr(result, args, body, span)
            }
            CallbackExprKind::AssignCapture { target, value } => {
                let target = body.push_expr(Expr {
                    kind: ExprKind::Local(*target),
                    ty: callback.ty,
                    span,
                });
                let value = self.callback_expr_to_body_expr(value, args, body, span)?;
                body.push_stmt(Stmt::Assign { target, value });
                Ok(value)
            }
            CallbackExprKind::DictLit(entries) => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let entries = entries
                    .iter()
                    .map(|(key, value)| {
                        let key = body.push_expr(Expr {
                            kind: ExprKind::Literal(Literal::String(
                                self.ctx
                                    .krate
                                    .symbols
                                    .get(*key)
                                    .unwrap_or_default()
                                    .to_owned(),
                            )),
                            ty: string_ty,
                            span,
                        });
                        let value = self.callback_expr_to_body_expr(value, args, body, span)?;
                        Ok((key, value))
                    })
                    .collect::<Result<Vec<_>, SmeltError>>()?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::DictLit(entries),
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::UnknownIs { value, kind } => {
                let value = self.callback_expr_to_body_expr(value, args, body, span)?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::UnknownIs { value, kind: *kind },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::TypeofValue { value } => {
                let value = self.callback_expr_to_body_expr(value, args, body, span)?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::TypeofValue { value },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::Index { receiver, index } => {
                let receiver = self.callback_expr_to_body_expr(receiver, args, body, span)?;
                let index_ty = self.ctx.krate.types.intern(Type::Int);
                let index = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Int(i64::try_from(*index).map_err(
                        |error| {
                            SmeltError::unsupported(
                                span,
                                format!("callback index is too large: {error}"),
                            )
                        },
                    )?)),
                    ty: index_ty,
                    span,
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Index { receiver, index },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::DynamicIndex { receiver, index } => {
                let receiver = self.callback_expr_to_body_expr(receiver, args, body, span)?;
                let index = self.callback_expr_to_body_expr(index, args, body, span)?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Index { receiver, index },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::Field { receiver, field } => {
                let receiver = self.callback_expr_to_body_expr(receiver, args, body, span)?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Field {
                        receiver,
                        field: *field,
                    },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::FieldTruthy { receiver, field } => {
                let receiver_ty = receiver.ty;
                let receiver = self.callback_expr_to_body_expr(receiver, args, body, span)?;
                let field_ty = self.class_field_type(receiver_ty, *field)?;
                let field = body.push_expr(Expr {
                    kind: ExprKind::Field {
                        receiver,
                        field: *field,
                    },
                    ty: field_ty,
                    span,
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::PrimitiveCast {
                        op: PrimitiveCastOp::ToBool,
                        operand: field,
                    },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::Unary { op, operand } => {
                let operand = self.callback_expr_to_body_expr(operand, args, body, span)?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::UnaryOp { op: *op, operand },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::Binary { op, lhs, rhs } => {
                let lhs = self.callback_expr_to_body_expr(lhs, args, body, span)?;
                let rhs = self.callback_expr_to_body_expr(rhs, args, body, span)?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::BinOp { op: *op, lhs, rhs },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                let cond = self.callback_expr_to_body_expr(cond, args, body, span)?;
                let then_expr = self.callback_expr_to_body_expr(then_expr, args, body, span)?;
                let else_expr = self.callback_expr_to_body_expr(else_expr, args, body, span)?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Conditional {
                        cond,
                        then_expr,
                        else_expr,
                    },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::Call {
                callee,
                args: call_args,
            } => {
                let callee_ty = callee.ty;
                let callee = self.callback_expr_to_body_expr(callee, args, body, span)?;
                let callee_index = usize::try_from(callee.0).map_err(|_error| {
                    SmeltError::unsupported(
                        span,
                        "callback callee expression index does not fit usize",
                    )
                })?;
                let callee_is_item = matches!(
                    body.exprs.get(callee_index).map(|expr| &expr.kind),
                    Some(ExprKind::Item(_))
                );
                let has_spread = call_args.iter().any(|arg| arg.spread);
                if has_spread
                    && !callee_is_item
                    && !matches!(self.ctx.krate.types.get(callee_ty), Some(Type::Function(_)))
                {
                    let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                    let packed =
                        self.callback_packed_spread_call_args(item_ty, call_args, args, body, span)?;
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::ClosureCallSpread {
                            callee,
                            args: packed,
                        },
                        ty: callback.ty,
                        span,
                    }));
                }
                let call_args = if has_spread
                    && let Some(Type::Function(function)) = self.ctx.krate.types.get(callee_ty).cloned()
                {
                    self.callback_spread_call_args_to_body_exprs(&function, call_args, args, body, span)?
                } else {
                    self.callback_call_args_to_body_exprs(call_args, args, body, span)?
                };
                let kind = if callee_is_item {
                    ExprKind::Call {
                        callee,
                        args: call_args,
                    }
                } else {
                    ExprKind::ClosureCall {
                        callee,
                        args: call_args,
                    }
                };
                Ok(body.push_expr(Expr {
                    kind,
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::MethodCall {
                receiver,
                method,
                args: raw_call_args,
            } => {
                let receiver_ty = receiver.ty;
                let receiver = self.callback_expr_to_body_expr(receiver, args, body, span)?;
                let call_args =
                    self.callback_call_args_to_body_exprs(raw_call_args, args, body, span)?;
                self.callback_method_call_to_body_expr(
                    receiver,
                    receiver_ty,
                    *method,
                    raw_call_args,
                    &call_args,
                    callback.ty,
                    args,
                    body,
                    span,
                )
            }
        }
    }

    /// Convert a callback method call into the corresponding normal HIR expression.
    pub(in crate::lowering) fn callback_method_call_to_body_expr(
        &mut self,
        receiver: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
        raw_args: &[CallbackCallArg],
        args: &[smelt_hir::ExprId],
        ty: smelt_hir::TypeId,
        callback_args: &[smelt_hir::ExprId],
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let method_text = self
            .ctx
            .krate
            .symbols
            .get(method)
            .unwrap_or_default()
            .to_owned();
        match method_text.as_str() {
            "call" => self.callback_call_method_to_body_expr(
                receiver,
                receiver_ty,
                raw_args,
                ty,
                callback_args,
                body,
                span,
            ),
            "apply" => self.callback_apply_method_to_body_expr(
                receiver,
                receiver_ty,
                args,
                ty,
                body,
                span,
            ),
            "toString" | "to_string" if args.is_empty() => Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToString,
                    operand: receiver,
                },
                ty,
                span,
            })),
            "toString" | "to_string" if args.len() == 1 => Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToString,
                    operand: receiver,
                },
                ty,
                span,
            })),
            "toISOString" | "to_iso_string" if args.is_empty() => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let value = body.push_expr(Expr {
                    kind: ExprKind::DateToIsoString {
                        timestamp_ms: receiver,
                    },
                    ty: string_ty,
                    span,
                });
                if ty == string_ty {
                    Ok(value)
                } else {
                    Ok(body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value },
                        ty,
                        span,
                    }))
                }
            }
            "__smelt_replace_first_match_uppercase" if args.len() == 1 => Ok(body.push_expr(Expr {
                kind: ExprKind::RegexReplaceFirstMatchUppercase {
                    pattern: args.first().copied().ok_or_else(|| {
                        SmeltError::unsupported(
                            span,
                            "callback regex replacement requires a pattern",
                        )
                    })?,
                    haystack: receiver,
                },
                ty,
                span,
            })),
            "toLowerCase" | "toLocaleLowerCase" | "to_lower_case" | "to_locale_lower_case"
                if args.is_empty() =>
            {
                Self::callback_string_case_to_body_expr(StringCaseOp::Lower, receiver, ty, body, span)
            }
            "toUpperCase" | "toLocaleUpperCase" | "to_upper_case" | "to_locale_upper_case"
                if args.is_empty() =>
            {
                Self::callback_string_case_to_body_expr(StringCaseOp::Upper, receiver, ty, body, span)
            }
            "split" if (1..=2).contains(&args.len()) => {
                let separator = *args.first().ok_or_else(|| {
                    SmeltError::unsupported(span, "callback split call requires a separator")
                })?;
                let limit = args.get(1).copied();
                Ok(body.push_expr(Expr {
                    kind: ExprKind::StringSplit {
                        haystack: receiver,
                        separator,
                        limit,
                    },
                    ty,
                    span,
                }))
            }
            "test" if args.len() == 1 => Ok(body.push_expr(Expr {
                kind: ExprKind::RegexFind {
                    pattern: receiver,
                    haystack: args.first().copied().ok_or_else(|| {
                        SmeltError::unsupported(span, "callback regex test requires a haystack")
                    })?,
                },
                ty,
                span,
            })),
            "match" if args.len() == 1 => Ok(body.push_expr(Expr {
                kind: ExprKind::RegexFind {
                    pattern: args.first().copied().ok_or_else(|| {
                        SmeltError::unsupported(span, "callback string match requires a pattern")
                    })?,
                    haystack: receiver,
                },
                ty,
                span,
            })),
            "has" if args.len() == 1
                && matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::Set(_))) =>
            {
                let item = *args.first().ok_or_else(|| {
                    SmeltError::unsupported(span, "callback Set.has call requires one argument")
                })?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::SetContains {
                        set: receiver,
                        item,
                    },
                    ty,
                    span,
                }))
            }
            "has" if args.len() == 1
                && matches!(
                    self.ctx.krate.types.get(receiver_ty),
                    Some(Type::Dict(_, _) | Type::JsMap(_, _))
                ) =>
            {
                // `Map.prototype.has` inside a callback body mirrors the direct
                // `map_has_call` lowering: a `DictContainsKey` over the Map receiver
                // (Maps are represented internally as `Type::Dict`).
                let key = *args.first().ok_or_else(|| {
                    SmeltError::unsupported(span, "callback Map.has call requires one argument")
                })?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::DictContainsKey {
                        dict: receiver,
                        key,
                    },
                    ty,
                    span,
                }))
            }
            "at" if args.len() == 1 => {
                self.callback_at_call_to_body_expr(receiver, receiver_ty, args, ty, body, span)
            }
            "join" if args.len() <= 1
                && (self.callback_method_receiver_is_list_like(receiver_ty)
                    || matches!(
                        self.ctx.krate.types.get(receiver_ty),
                        Some(Type::Unknown | Type::TypeParam { .. })
                    )) =>
            {
                self.callback_join_call_to_body_expr(receiver, receiver_ty, args, ty, body, span)
            }
            "startsWith" | "starts_with" | "endsWith" | "ends_with"
                if args.len() == 1
                    && matches!(
                        self.ctx.krate.types.get(receiver_ty),
                        Some(Type::String | Type::Unknown | Type::TypeParam { .. })
                    ) =>
            {
                // `String.prototype.startsWith`/`endsWith` is unambiguous, so an
                // erased or type-param receiver is coerced to a string (matching
                // the direct `string_affix_call` lowering) before testing.
                let op = if matches!(method_text.as_str(), "startsWith" | "starts_with") {
                    StringAffixOp::StartsWith
                } else {
                    StringAffixOp::EndsWith
                };
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let haystack = Self::callback_coerce_to_string(receiver, string_ty, body, span);
                let needle = *args.first().ok_or_else(|| {
                    SmeltError::unsupported(
                        span,
                        "callback string prefix/suffix test requires one argument",
                    )
                })?;
                let needle = Self::callback_coerce_to_string(needle, string_ty, body, span);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::StringAffix {
                        op,
                        haystack,
                        needle,
                    },
                    ty,
                    span,
                }))
            }
            "trim" if args.is_empty()
                && matches!(
                    self.ctx.krate.types.get(receiver_ty),
                    Some(Type::String | Type::Unknown | Type::TypeParam { .. })
                ) =>
            {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let operand = Self::callback_coerce_to_string(receiver, string_ty, body, span);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::StringTrim {
                        operand,
                        side: StringTrimSide::Both,
                    },
                    ty: string_ty,
                    span,
                }))
            }
            "includes" if args.len() == 1
                && self.list_surface_type(receiver_ty).is_some() =>
            {
                let (list_ty, _) = self.list_surface_type(receiver_ty).ok_or_else(|| {
                    SmeltError::unsupported(span, "callback Array.includes call requires a list receiver")
                })?;
                let list = if receiver_ty == list_ty {
                    receiver
                } else {
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert {
                            value: receiver,
                        },
                        ty: list_ty,
                        span,
                    })
                };
                let item = *args.first().ok_or_else(|| {
                    SmeltError::unsupported(span, "callback Array.includes call requires one argument")
                })?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::ListContains {
                        list,
                        item,
                    },
                    ty,
                    span,
                }))
            }
            "indexOf" | "index_of" | "lastIndexOf" | "last_index_of"
                if args.len() == 1
                    && self.callback_method_receiver_is_list_like(receiver_ty) =>
            {
                let op = if matches!(method_text.as_str(), "lastIndexOf" | "last_index_of") {
                    ListSearchOp::RFind
                } else {
                    ListSearchOp::Find
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::ListSearch {
                        op,
                        list: receiver,
                        item: args.first().copied().ok_or_else(|| {
                            SmeltError::unsupported(
                                span,
                                "callback array search requires an item",
                            )
                        })?,
                        // The callback-expression path only accepts the single
                        // item argument (guarded by `args.len() == 1` above); the
                        // optional `fromIndex` form lowers through the regular
                        // statement path in `list_search_call`.
                        from_index: None,
                    },
                    ty,
                    span,
                }))
            }
            "concat"
                if args.len() == 1
                    && (self.callback_method_receiver_is_list_like(receiver_ty)
                        || matches!(self.ctx.krate.types.get(ty), Some(Type::List(_)))
                        || matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::Unknown))) =>
            {
                let right = args.first().copied().ok_or_else(|| {
                    SmeltError::unsupported(span, "callback concat requires a right operand")
                })?;
                let right =
                    self.callback_concat_right_to_body_expr(right, receiver_ty, ty, body, span);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::ListConcat {
                        left: receiver,
                        right,
                    },
                    ty,
                    span,
                }))
            }
            "flat" => {
                if args.len() > 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "callback Array.flat accepts at most one depth argument",
                    ));
                }
                let ty = self.callback_flat_result_type(receiver_ty, ty);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::ListFlat {
                        list: receiver,
                        depth: args.first().copied(),
                    },
                    ty,
                    span,
                }))
            }
            "map" | "flatMap" | "flat_map"
                if args.is_empty()
                    && (self.callback_method_receiver_is_list_like(receiver_ty)
                        || matches!(self.ctx.krate.types.get(ty), Some(Type::List(_)))) =>
            {
                Ok(body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: receiver },
                    ty,
                    span,
                }))
            }
            "push" if args.len() == 1
                && matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::List(_))) =>
            {
                let item = *args.first().ok_or_else(|| {
                    SmeltError::unsupported(span, "callback Array.push call requires one argument")
                })?;
                let number_ty = self.ctx.krate.types.intern(Type::Float);
                let value = body.push_expr(Expr {
                    kind: ExprKind::ListPush {
                        list: receiver,
                        item,
                    },
                    ty: number_ty,
                    span,
                });
                if ty == number_ty {
                    Ok(value)
                } else {
                    Ok(body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value },
                        ty,
                        span,
                    }))
                }
            }
            "slice" if args.len() <= 2 => {
                let start = args.first().copied();
                let end = args.get(1).copied();
                match self.ctx.krate.types.get(receiver_ty) {
                    Some(Type::String) => Ok(body.push_expr(Expr {
                        kind: ExprKind::StringSlice {
                            operand: receiver,
                            start,
                            end,
                        },
                        ty,
                        span,
                    })),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                        let string_ty = self.ctx.krate.types.intern(Type::String);
                        let value = body.push_expr(Expr {
                            kind: ExprKind::StringSlice {
                                operand: receiver,
                                start,
                                end,
                            },
                            ty: string_ty,
                            span,
                        });
                        if ty == string_ty {
                            Ok(value)
                        } else {
                            Ok(body.push_expr(Expr {
                                kind: ExprKind::TypeAssert { value },
                                ty,
                                span,
                            }))
                        }
                    }
                    Some(Type::List(_)) => Ok(body.push_expr(Expr {
                        kind: ExprKind::ListSlice {
                            list: receiver,
                            start,
                            end,
                        },
                        ty,
                        span,
                    })),
                    _ => Err(SmeltError::unsupported(
                        span,
                        "callback slice receiver is not lowered into closure bodies yet",
                    )),
                }
            }
            _ => self
                .callback_callable_field_method_to_body_expr(
                    receiver,
                    receiver_ty,
                    method,
                    raw_args,
                    ty,
                    callback_args,
                    body,
                    span,
                )
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        span,
                        format!(
                            "callback method `{method_text}` is not lowered into closure bodies yet"
                        ),
                    )
                }),
        }
    }

    /// Lower a callback method call whose receiver has a callable field.
    pub(in crate::lowering) fn callback_callable_field_method_to_body_expr(
        &mut self,
        receiver: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
        raw_args: &[CallbackCallArg],
        ty: smelt_hir::TypeId,
        callback_args: &[smelt_hir::ExprId],
        body: &mut Body,
        span: Span,
    ) -> Option<smelt_hir::ExprId> {
        let field_ty = self.class_field_type(receiver_ty, method).ok()?;
        let function = self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(field_ty))
            .and_then(|field_type| match field_type {
                Type::Function(function) => Some(function.clone()),
                _ => None,
            })?;
        let callee = body.push_expr(Expr {
            kind: ExprKind::Field {
                receiver,
                field: method,
            },
            ty: field_ty,
            span,
        });
        let args = self
            .callback_spread_call_args_to_body_exprs(
                &function,
                raw_args,
                callback_args,
                body,
                span,
            )
            .ok()?;
        Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall { callee, args },
            ty,
            span,
        }))
    }

    /// Convert callback-body `.call(...)` forwarding into a normal closure call.
    pub(in crate::lowering) fn callback_call_method_to_body_expr(
        &mut self,
        receiver: smelt_hir::ExprId,
        _receiver_ty: smelt_hir::TypeId,
        raw_args: &[CallbackCallArg],
        ty: smelt_hir::TypeId,
        callback_args: &[smelt_hir::ExprId],
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut callable_receiver = receiver;
        loop {
            let receiver_index = usize::try_from(callable_receiver.0).unwrap_or(usize::MAX);
            let Some(Expr {
                kind: ExprKind::TypeAssert { value },
                ..
            }) = body.exprs.get(receiver_index)
            else {
                break;
            };
            callable_receiver = *value;
        }
        let receiver_index = usize::try_from(callable_receiver.0).unwrap_or(usize::MAX);
        if matches!(
            body.exprs.get(receiver_index),
            Some(Expr {
                kind: ExprKind::Closure(_),
                ..
            })
        ) {
            let args =
                self.callback_call_args_to_body_exprs(raw_args, callback_args, body, span)?;
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall {
                    callee: callable_receiver,
                    args,
                },
                ty,
                span,
            }));
        }
        let receiver_ty = Self::expr_ty(body, receiver);
        let receiver_function = self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(receiver_ty))
            .and_then(|receiver_type| match receiver_type {
                Type::Function(function) => Some(function.clone()),
                _ => None,
            });
        // A `funnel.call(...args)` style forward where the *receiver* erases to
        // `SmeltUnknown` at codegen (e.g. a generic structural type alias such
        // as `Funnel<Args>`) reads its `call` member as a runtime
        // `SmeltUnknown` value and dispatches through the runtime call ABI. That
        // ABI consumes the packed argument list, so a spread call must lower to
        // `ClosureCallSpread` with the `call` field re-typed as `SmeltUnknown` —
        // never to the concrete-function `[fixed.., rest_list]` expansion below,
        // which the dynamic ABI would re-wrap into a nested array. This mirrors
        // the same receiver-erasure rule applied in `callable_static_member_call`.
        if self.receiver_type_dispatches_dynamically(receiver_ty)
            && raw_args.len() == 1
            && raw_args.first().is_some_and(|arg| arg.spread)
            && let Some(spread_args) = callback_args.first().copied()
        {
            let unknown = self.ctx.krate.types.intern(Type::Unknown);
            let field = self.intern_source_name("call");
            let callee = body.push_expr(Expr {
                kind: ExprKind::Field { receiver, field },
                ty: unknown,
                span,
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread {
                    callee,
                    args: spread_args,
                },
                ty,
                span,
            }));
        }

        let (callee, function) = if let Some(function) = receiver_function {
            (receiver, Some(function))
        } else {
            let field = self.intern_source_name("call");
            let field_ty = self
                .class_field_type(receiver_ty, field)
                .unwrap_or_else(|_| self.ctx.krate.types.intern(Type::Unknown));
            let function = self
                .ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(field_ty))
                .and_then(|field_type| match field_type {
                    Type::Function(function) => Some(function.clone()),
                    _ => None,
                });
            (
                body.push_expr(Expr {
                    kind: ExprKind::Field { receiver, field },
                    ty: field_ty,
                    span,
                }),
                function,
            )
        };

        if let Some(function) = function {
            let args =
                self.callback_spread_call_args_to_body_exprs(&function, raw_args, callback_args, body, span)?;
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall {
                    callee,
                    args,
                },
                ty,
                span,
            }));
        }

        if raw_args.len() == 1
            && raw_args.first().is_some_and(|arg| arg.spread)
            && let Some(args) = callback_args.first().copied()
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread {
                    callee,
                    args,
                },
                ty,
                span,
            }));
        }

        if raw_args.iter().any(|arg| arg.spread) {
            return Err(SmeltError::unsupported(
                span,
                "callback .call spread arguments need a callable receiver type",
            ));
        }

        let args =
            self.callback_call_args_to_body_exprs(raw_args, callback_args, body, span)?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee,
                args,
            },
            ty,
            span,
        }))
    }

    /// Lower a `Function.prototype.apply` call inside a closure body.
    ///
    /// `fn.apply(thisArg, argsArray)` invokes `fn` with the elements of
    /// `argsArray` as positional arguments. Smelt erases the JavaScript `this`
    /// binding for these callables (the source callbacks are written with
    /// `function (this: any, ...)`), so the leading `thisArg` operand is dropped
    /// and the trailing array is spread through `ClosureCallSpread`, mirroring the
    /// spread branch of [`Self::callback_call_method_to_body_expr`]. This
    /// generalizes the closure-body method table to the `apply` form the same way
    /// `call` is already supported, without special-casing any particular library
    /// function.
    pub(in crate::lowering) fn callback_apply_method_to_body_expr(
        &mut self,
        receiver: smelt_hir::ExprId,
        _receiver_ty: smelt_hir::TypeId,
        args: &[smelt_hir::ExprId],
        ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // `fn.apply(thisArg)` (or `fn.apply()`) forwards no positional
        // arguments, so it is an ordinary zero-argument closure call.
        if args.len() <= 1 {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall {
                    callee: receiver,
                    args: Vec::new(),
                },
                ty,
                span,
            }));
        }
        // The trailing operand is the array whose elements become the call
        // arguments; spread it through the packed-argument closure-call ABI.
        let arguments = *args.last().ok_or_else(|| {
            SmeltError::unsupported(span, "callback apply call requires an arguments array")
        })?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::ClosureCallSpread {
                callee: receiver,
                args: arguments,
            },
            ty,
            span,
        }))
    }

    /// Convert a callback `concat` argument into the list operand expected by HIR.
    pub(in crate::lowering) fn callback_concat_right_to_body_expr(
        &mut self,
        right: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        result_ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) -> smelt_hir::ExprId {
        if matches!(
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(Self::expr_ty(body, right))),
            Some(Type::List(_) | Type::Tuple(_))
        ) {
            return right;
        }
        let item_ty = match self.ctx.krate.types.get(result_ty) {
            Some(Type::List(item_ty)) => *item_ty,
            _ => match self.ctx.krate.types.get(receiver_ty) {
                Some(Type::List(item_ty)) => *item_ty,
                _ => Self::expr_ty(body, right),
            },
        };
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        body.push_expr(Expr {
            kind: ExprKind::ListLit(vec![right]),
            ty: list_ty,
            span,
        })
    }

    /// Return whether a callback method receiver has a list-like static surface.
    pub(in crate::lowering) fn callback_method_receiver_is_list_like(&self, receiver_ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(receiver_ty)),
            Some(Type::List(_) | Type::Tuple(_))
        )
    }

    /// Infer the result type for a callback-body `Array.prototype.flat` call.
    pub(in crate::lowering) fn callback_flat_result_type(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        fallback_ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        if matches!(self.ctx.krate.types.get(fallback_ty), Some(Type::List(_))) {
            return fallback_ty;
        }
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(receiver_ty))
            .cloned()
        {
            Some(Type::List(item_ty)) => {
                let item_ty = match self
                    .ctx
                    .krate
                    .types
                    .get(self.type_param_constraint_or_self(item_ty))
                    .cloned()
                {
                    Some(Type::List(flat_item_ty)) => flat_item_ty,
                    Some(Type::Tuple(items)) => self.flattened_tuple_item_type(items),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                        self.ctx.krate.types.intern(Type::Unknown)
                    }
                    _ => item_ty,
                };
                self.ctx.krate.types.intern(Type::List(item_ty))
            }
            Some(Type::Tuple(items)) => {
                let item_ty = self.flattened_tuple_item_type(items);
                self.ctx.krate.types.intern(Type::List(item_ty))
            }
            _ => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                self.ctx.krate.types.intern(Type::List(item_ty))
            }
        }
    }

    /// Convert callback string case methods into normal HIR.
    pub(in crate::lowering) fn callback_string_case_to_body_expr(
        op: StringCaseOp,
        operand: smelt_hir::ExprId,
        ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        Ok(body.push_expr(Expr {
            kind: ExprKind::StringCase { op, operand },
            ty,
            span,
        }))
    }

    /// Coerce a callback-body expression to `String` when it is not already one.
    ///
    /// Erased (`unknown`) and type-param receivers reaching string-only methods
    /// (such as `startsWith`/`endsWith`) are routed through a `TypeAssert` to the
    /// string type, mirroring the direct `string_affix_call` coercion.
    pub(in crate::lowering) fn callback_coerce_to_string(
        value: smelt_hir::ExprId,
        string_ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) -> smelt_hir::ExprId {
        if Self::expr_ty(body, value) == string_ty {
            value
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: string_ty,
                span,
            })
        }
    }

    /// Lower `.at(index)` on arrays and strings inside a callback body.
    ///
    /// Mirrors the direct `collection_at_call` lowering: array/string `.at`
    /// returns `undefined` for out-of-range positions, modelled with
    /// `OptionalIndex` so generated Rust uses checked normalized indexes. The
    /// receiver/argument are already lowered HIR expressions in callback bodies,
    /// so we only re-derive the optional element type and route through the same
    /// `ExprKind` the statement-position path emits.
    pub(in crate::lowering) fn callback_at_call_to_body_expr(
        &mut self,
        receiver: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        args: &[smelt_hir::ExprId],
        ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let item_ty = match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(receiver_ty))
        {
            Some(Type::List(item_ty)) => *item_ty,
            Some(Type::Tuple(_)) => {
                let (_, item_ty) = self.list_surface_type(receiver_ty).ok_or_else(|| {
                    SmeltError::unsupported(span, "callback array at requires a list receiver")
                })?;
                item_ty
            }
            Some(Type::String) => self.ctx.krate.types.intern(Type::String),
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "callback array/string at receiver is not lowered into closure bodies yet",
                ));
            }
        };
        let index = *args.first().ok_or_else(|| {
            SmeltError::unsupported(span, "callback array/string at requires one index argument")
        })?;
        let optional_ty = self.ctx.krate.types.intern(Type::Optional(item_ty));
        let value = body.push_expr(Expr {
            kind: ExprKind::OptionalIndex { receiver, index },
            ty: optional_ty,
            span,
        });
        if ty == optional_ty {
            Ok(value)
        } else {
            Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty,
                span,
            }))
        }
    }

    /// Lower `Array.prototype.join` inside a callback body.
    ///
    /// Mirrors the direct `string_join_call`/`finish_string_join_call` lowering:
    /// a `StringJoin` over the receiver with an optional separator argument that
    /// defaults to `","`. The receiver/argument are already lowered HIR
    /// expressions, so unknown/type-param receivers are coerced to a list
    /// surface through a `TypeAssert` before joining.
    pub(in crate::lowering) fn callback_join_call_to_body_expr(
        &mut self,
        receiver: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        args: &[smelt_hir::ExprId],
        ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let items = if self.callback_method_receiver_is_list_like(receiver_ty) {
            receiver
        } else {
            // Coerce erased/type-param receivers to an unknown-element list so the
            // join lowering has a concrete list surface to operate on.
            let item_ty = self.ctx.krate.types.intern(Type::Unknown);
            let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: receiver },
                ty: list_ty,
                span,
            })
        };
        let separator = args.first().copied().unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(",".to_owned())),
                ty: string_ty,
                span,
            })
        });
        let value = body.push_expr(Expr {
            kind: ExprKind::StringJoin { items, separator },
            ty: string_ty,
            span,
        });
        if ty == string_ty {
            Ok(value)
        } else {
            Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty,
                span,
            }))
        }
    }

}
