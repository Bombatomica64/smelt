//! Callback-surface classification: resolving callback function items,
//! arrow/function callback recognizers, factory predicates, and opaque
//! (local/member) callback values, plus building callbacks from param lists.

use crate::lowering::{
    Argument, BinOp, BindingPattern, Body, CallbackCallArg, CallbackExpr, CallbackExprKind,
    CaptureMode, ClosureCapture, Expr, ExprKind, Expression, FunctionType, HashMap, Item,
    Literal, LocalDecl, ModuleBuilder, Param, SmeltError, Span, Statement, Type, UnaryOp,
};
use oxc::span::GetSpan;

impl ModuleBuilder<'_> {
    /// Resolve a callback function symbol back to its normal HIR item.
    pub(in crate::lowering) fn callback_function_item(
        &self,
        function: smelt_hir::Symbol,
        span: Span,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        self.ctx
            .krate
            .items
            .iter()
            .enumerate()
            .find_map(|(index, item)| {
                if matches!(item, Item::Function(item_function) if item_function.name == function) {
                    Some(smelt_hir::ItemId(u32::try_from(index).ok()?))
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                SmeltError::unsupported(
                    span,
                    "callback function reference does not resolve to an item",
                )
            })
    }

    /// Wrap a function item in a first-class closure value.
    pub(in crate::lowering) fn callback_function_item_closure(
        &mut self,
        item: smelt_hir::ItemId,
        function_ty: smelt_hir::TypeId,
        outer_body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned() else {
            return Err(SmeltError::unsupported(
                span,
                "callback function reference must have a function type",
            ));
        };
        let mut closure_body = Body::new(None, span);
        let mut closure_params = Vec::new();
        let mut call_args = Vec::new();
        for (index, ty) in function.params.iter().copied().enumerate() {
            let name = self.ctx.krate.symbols.intern(&format!("arg{index}"));
            let local = closure_body.push_local(LocalDecl {
                name: Some(name),
                ty,
                mutable: false,
                span,
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name,
                local,
                ty,
                span,
            });
            call_args.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span,
            }));
        }
        let callee = closure_body.push_expr(Expr {
            kind: ExprKind::Item(item),
            ty: function_ty,
            span,
        });
        let call = closure_body.push_expr(Expr {
            kind: ExprKind::Call {
                callee,
                args: call_args,
            },
            ty: function.return_ty,
            span,
        });
        if let Some(block) = closure_body.blocks.first_mut() {
            block.tail = Some(call);
        }
        let body = self.ctx.krate.push_body(closure_body);
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest: function.rest,
                required_params: function.required_params,
                return_ty: function.return_ty,
                captures: Vec::new(),
                body,
                // Same bare function-item-as-value wrapper as
                // `item_function_closure_expression`, reached through the
                // callback-reference path. Tag it with the source item so all
                // references to this named function share one cached runtime
                // wrapper and compare equal under JavaScript `===`.
                function_item: Some(item),
                span,
            }),
            ty: function_ty,
            span,
        }))
    }

    /// Convert stored callback call arguments into normal HIR argument expressions.
    pub(in crate::lowering) fn callback_call_args_to_body_exprs(
        &mut self,
        call_args: &[CallbackCallArg],
        args: &[smelt_hir::ExprId],
        body: &mut Body,
        span: Span,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        call_args
            .iter()
            .map(|arg| {
                self.callback_expr_to_body_expr(&arg.expr, args, body, span)
            })
            .collect()
    }

    /// Expand spread callback arguments into fixed function parameters and an optional rest list.
    pub(in crate::lowering) fn callback_spread_call_args_to_body_exprs(
        &mut self,
        function: &FunctionType,
        call_args: &[CallbackCallArg],
        args: &[smelt_hir::ExprId],
        body: &mut Body,
        span: Span,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let mut lowered = Vec::new();
        let mut fixed_index = 0usize;
        let rest_index = function.rest.unwrap_or(function.params.len());
        let rest_ty = function.rest.and_then(|index| {
            function
                .params
                .get(index)
                .copied()
                .filter(|ty| matches!(self.ctx.krate.types.get(*ty), Some(Type::List(_))))
        });
        let mut rest_list = None;
        for (arg_index, arg) in call_args.iter().enumerate() {
            if !arg.spread {
                let value = self.callback_expr_to_body_expr(&arg.expr, args, body, span)?;
                if function.rest.is_some() && fixed_index >= rest_index {
                    let rest_ty = rest_ty.ok_or_else(|| {
                        SmeltError::unsupported(span, "callback rest spread type is not a list")
                    })?;
                    let list = body.push_expr(Expr {
                        kind: ExprKind::ListLit(vec![value]),
                        ty: rest_ty,
                        span,
                    });
                    rest_list = Some(rest_list.map_or(list, |left| {
                        body.push_expr(Expr {
                            kind: ExprKind::ListConcat { left, right: list },
                            ty: rest_ty,
                            span,
                        })
                    }));
                } else {
                    lowered.push(value);
                    fixed_index += 1;
                }
                continue;
            }

            let spread_list = self.callback_expr_to_body_expr(&arg.expr, args, body, span)?;
            let remaining_fixed_values = call_args
                .get(arg_index + 1..)
                .unwrap_or(&[])
                .iter()
                .filter(|remaining_arg| !remaining_arg.spread)
                .count();
            let fixed_target = rest_index.saturating_sub(remaining_fixed_values);
            let mut consumed_from_spread = 0usize;
            while fixed_index < fixed_target {
                let index = self.usize_float_literal(consumed_from_spread, span, body)?;
                let ty = function
                    .params
                    .get(fixed_index)
                    .copied()
                    .unwrap_or_else(|| {
                        self.index_type(Self::expr_ty(body, spread_list))
                            .unwrap_or_else(|_| Self::expr_ty(body, spread_list))
                    });
                let kind = if matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
                    ExprKind::OptionalIndex {
                        receiver: spread_list,
                        index,
                    }
                } else {
                    ExprKind::Index {
                        receiver: spread_list,
                        index,
                    }
                };
                lowered.push(body.push_expr(Expr { kind, ty, span }));
                fixed_index += 1;
                consumed_from_spread += 1;
            }

            if let Some(rest_ty) = rest_ty {
                let rest_piece = if consumed_from_spread == 0 {
                    spread_list
                } else {
                    let start = self.usize_float_literal(consumed_from_spread, span, body)?;
                    body.push_expr(Expr {
                        kind: ExprKind::ListSlice {
                            list: spread_list,
                            start: Some(start),
                            end: None,
                        },
                        ty: rest_ty,
                        span,
                    })
                };
                rest_list = Some(rest_list.map_or(rest_piece, |left| {
                    body.push_expr(Expr {
                        kind: ExprKind::ListConcat {
                            left,
                            right: rest_piece,
                        },
                        ty: rest_ty,
                        span,
                    })
                }));
            }
        }
        if let Some(rest_ty) = rest_ty {
            lowered.push(rest_list.unwrap_or_else(|| {
                body.push_expr(Expr {
                    kind: ExprKind::ListLit(Vec::new()),
                    ty: rest_ty,
                    span,
                })
            }));
        }
        Ok(lowered)
    }

    /// Pack callback call arguments that contain spreads into a single list expression.
    pub(in crate::lowering) fn callback_packed_spread_call_args(
        &mut self,
        item_ty: smelt_hir::TypeId,
        call_args: &[CallbackCallArg],
        args: &[smelt_hir::ExprId],
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let mut current_items = Vec::new();
        let mut packed = None;
        for arg in call_args {
            if arg.spread {
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
                let spread_expr = self.callback_expr_to_body_expr(&arg.expr, args, body, span)?;
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
                continue;
            }
            current_items.push(self.callback_expr_to_body_expr(&arg.expr, args, body, span)?);
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
        Ok(packed.unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::ListLit(Vec::new()),
                ty: list_ty,
                span,
            })
        }))
    }

    /// Recursively collect captures and upgrade assigned captures to mutable mode.
    pub(in crate::lowering) fn collect_callback_captures(
        &mut self,
        callback: &CallbackExpr,
        body: &Body,
        captures: &mut HashMap<smelt_hir::LocalId, ClosureCapture>,
    ) {
        match &callback.kind {
            CallbackExprKind::Capture(local) => {
                if let Some(local_decl) = usize::try_from(local.0)
                    .ok()
                    .and_then(|index| body.locals.get(index))
                {
                    captures.entry(*local).or_insert_with(|| ClosureCapture {
                        source_local: *local,
                        body_local: None,
                        symbol: local_decl
                            .name
                            .unwrap_or_else(|| self.ctx.krate.symbols.intern("__capture")),
                        ty: local_decl.ty,
                        mode: CaptureMode::ByRef,
                    });
                }
            }
            CallbackExprKind::AssignCapture { target, value } => {
                if let Some(local_decl) = usize::try_from(target.0)
                    .ok()
                    .and_then(|index| body.locals.get(index))
                {
                    captures.insert(
                        *target,
                        ClosureCapture {
                            source_local: *target,
                            body_local: None,
                            symbol: local_decl
                                .name
                                .unwrap_or_else(|| self.ctx.krate.symbols.intern("__capture")),
                            ty: local_decl.ty,
                            mode: CaptureMode::ByMut,
                        },
                    );
                }
                self.collect_callback_captures(value, body, captures);
            }
            CallbackExprKind::ListLit(items) => {
                for item in items {
                    self.collect_callback_captures(item, body, captures);
                }
            }
            CallbackExprKind::Sequence { effects, result } => {
                for effect in effects {
                    self.collect_callback_captures(effect, body, captures);
                }
                self.collect_callback_captures(result, body, captures);
            }
            CallbackExprKind::DictLit(entries) => {
                for (_, value) in entries {
                    self.collect_callback_captures(value, body, captures);
                }
            }
            CallbackExprKind::Throw { message } => {
                if let Some(message) = message {
                    self.collect_callback_captures(message, body, captures);
                }
            }
            CallbackExprKind::Index { receiver, .. }
            | CallbackExprKind::Field { receiver, .. }
            | CallbackExprKind::HasField { receiver, .. }
            | CallbackExprKind::FieldTruthy { receiver, .. } => {
                self.collect_callback_captures(receiver, body, captures);
            }
            CallbackExprKind::DynamicIndex { receiver, index } => {
                self.collect_callback_captures(receiver, body, captures);
                self.collect_callback_captures(index, body, captures);
            }
            CallbackExprKind::HasDynamicField { receiver, field } => {
                self.collect_callback_captures(receiver, body, captures);
                self.collect_callback_captures(field, body, captures);
            }
            CallbackExprKind::Unary { operand, .. } => {
                self.collect_callback_captures(operand, body, captures);
            }
            CallbackExprKind::Binary { lhs, rhs, .. } => {
                self.collect_callback_captures(lhs, body, captures);
                self.collect_callback_captures(rhs, body, captures);
            }
            CallbackExprKind::UnknownIs { value, .. } => {
                self.collect_callback_captures(value, body, captures);
            }
            CallbackExprKind::TypeofValue { value } => {
                self.collect_callback_captures(value, body, captures);
            }
            CallbackExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                self.collect_callback_captures(cond, body, captures);
                self.collect_callback_captures(then_expr, body, captures);
                self.collect_callback_captures(else_expr, body, captures);
            }
            CallbackExprKind::Call { callee, args } => {
                self.collect_callback_captures(callee, body, captures);
                for arg in args {
                    self.collect_callback_captures(&arg.expr, body, captures);
                }
            }
            CallbackExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                if self.ctx.krate.symbols.get(*method).is_some_and(|name| {
                    matches!(name, "push" | "pop" | "shift" | "unshift" | "splice")
                }) && let CallbackExprKind::Capture(local) = receiver.kind
                    && let Some(local_decl) = usize::try_from(local.0)
                        .ok()
                        .and_then(|index| body.locals.get(index))
                {
                    captures.insert(
                        local,
                        ClosureCapture {
                            source_local: local,
                            body_local: None,
                            symbol: local_decl
                                .name
                                .unwrap_or_else(|| self.ctx.krate.symbols.intern("__capture")),
                            ty: local_decl.ty,
                            mode: CaptureMode::ByMut,
                        },
                    );
                }
                self.collect_callback_captures(receiver, body, captures);
                for arg in args {
                    self.collect_callback_captures(&arg.expr, body, captures);
                }
            }
            CallbackExprKind::FunctionTableLookup { key, .. } => {
                self.collect_callback_captures(key, body, captures);
            }
            CallbackExprKind::Param(_)
            | CallbackExprKind::Function(_)
            | CallbackExprKind::Literal(_) => {}
        }
    }

    /// Lower a supported arrow callback to a typed expression tree.
    pub(in crate::lowering) fn arrow_callback(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        if let Some(callback) = self.asserted_arrow_callback(argument, expected_param_tys, body)? {
            return Ok(callback);
        }
        let Argument::ArrowFunctionExpression(arrow) = argument else {
            if let Some(callback) =
                self.known_callback_factory_predicate(argument, expected_param_tys)?
            {
                return Ok(callback);
            }
            if let Argument::CallExpression(_call) = argument {
                return Ok(self.opaque_member_callback(expected_param_tys));
            }
            if let Argument::FunctionExpression(function) = argument {
                return self.function_callback_from_params(function, expected_param_tys, body);
            }
            if let Argument::Identifier(identifier) = argument
                && let Some(item) = self.items.get(identifier.name.as_str()).copied()
            {
                let span = self.span(identifier.span.start, identifier.span.end);
                let Item::Function(function) = self.item_ref(item) else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array callback function references must resolve to functions",
                    ));
                };
                let function_name = function.name;
                let function_params_len = function.params.len();
                let function_return_ty = function.return_ty;
                let function_ty = self.item_expr_type(item, span)?;
                if function_params_len < expected_param_tys.len().min(1) {
                    return Err(SmeltError::unsupported(
                        span,
                        "array callback function reference has too few parameters",
                    ));
                }
                let args = expected_param_tys
                    .iter()
                    .copied()
                    .take(function_params_len)
                    .enumerate()
                    .map(|(index, ty)| CallbackCallArg {
                        expr: CallbackExpr {
                            kind: CallbackExprKind::Param(index),
                            ty,
                        },
                        spread: false,
                    })
                    .collect();
                return Ok(CallbackExpr {
                    kind: CallbackExprKind::Call {
                        callee: Box::new(CallbackExpr {
                            kind: CallbackExprKind::Function(function_name),
                            ty: function_ty,
                        }),
                        args,
                    },
                    ty: function_return_ty,
                });
            }
            if let Argument::Identifier(identifier) = argument
                && let Some(local) = self.locals.get(identifier.name.as_str()).copied()
            {
                let local_ty = Self::local_ty(body, local);
                if let Some(Type::Function(function)) = self.ctx.krate.types.get(local_ty).cloned()
                {
                    if function.params.len() < expected_param_tys.len().min(1) {
                        return Err(SmeltError::unsupported(
                            self.span(identifier.span.start, identifier.span.end),
                            "array callback function reference has too few parameters",
                        ));
                    }
                    let args = expected_param_tys
                        .iter()
                        .copied()
                        .take(function.params.len())
                        .enumerate()
                        .map(|(index, ty)| CallbackCallArg {
                            expr: CallbackExpr {
                                kind: CallbackExprKind::Param(index),
                                ty,
                            },
                            spread: false,
                        })
                        .collect();
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Call {
                            callee: Box::new(CallbackExpr {
                                kind: CallbackExprKind::Capture(local),
                                ty: local_ty,
                            }),
                            args,
                        },
                        ty: function.return_ty,
                    });
                }
            }
            if let Argument::StaticMemberExpression(_member) = argument {
                return Ok(self.opaque_member_callback(expected_param_tys));
            }
            // A callback chosen at runtime between callable values
            // (`xs.map(cond ? Object : identity)`) or coalesced from one
            // (`xs.map(maybeFn ?? identity)`). The selected callee is an opaque
            // callable surface here, so model the whole argument as an opaque
            // callback that forwards the receiver's element arguments, the same
            // way a member or imported-value callback is handled.
            if matches!(
                argument,
                Argument::ConditionalExpression(_) | Argument::LogicalExpression(_)
            ) {
                return Ok(self.opaque_member_callback(expected_param_tys));
            }
            if let Argument::Identifier(identifier) = argument
                && self.is_opaque_callback_value(identifier.name.as_str())
            {
                return Ok(self.opaque_member_callback(expected_param_tys));
            }
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "array callback methods currently require arrow function callbacks",
            ));
        };
        if arrow.r#async {
            return Err(SmeltError::unsupported(
                self.span(arrow.span.start, arrow.span.end),
                "async callbacks need closure-body lowering",
            ));
        }
        self.arrow_callback_from_params(arrow, expected_param_tys, body)
    }

    /// Lower common lodash/fp predicate factories when they are passed as array callbacks.
    pub(in crate::lowering) fn known_callback_factory_predicate(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
    ) -> Result<Option<CallbackExpr>, SmeltError> {
        let Argument::CallExpression(call) = argument else {
            return Ok(None);
        };
        let Some(item_ty) = expected_param_tys.first().copied() else {
            return Ok(None);
        };
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        if Self::is_identifier_callee(&call.callee, "has")
            && let [Argument::StringLiteral(field)] = call.arguments.as_slice()
        {
            let field = self.intern_source_name(field.value.as_str());
            return Ok(Some(CallbackExpr {
                kind: CallbackExprKind::HasField {
                    receiver: Box::new(CallbackExpr {
                        kind: CallbackExprKind::Param(0),
                        ty: item_ty,
                    }),
                    field,
                },
                ty: bool_ty,
            }));
        }
        if Self::is_identifier_callee(&call.callee, "omit") && call.arguments.len() == 1 {
            return Ok(Some(CallbackExpr {
                kind: CallbackExprKind::Param(0),
                ty: item_ty,
            }));
        }
        if Self::is_identifier_callee(&call.callee, "isNot")
            && let [Argument::Identifier(predicate)] = call.arguments.as_slice()
            && let Some(item) = self.items.get(predicate.name.as_str()).copied()
        {
            let function = match self.item_ref(item) {
                Item::Function(function) => function.clone(),
                _ => return Ok(None),
            };
            if function.params.is_empty() {
                return Ok(None);
            }
            let function_name = function.name;
            let return_ty = function.return_ty;
            let function_ty =
                self.item_expr_type(item, self.span(predicate.span.start, predicate.span.end))?;
            return Ok(Some(CallbackExpr {
                kind: CallbackExprKind::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(CallbackExpr {
                        kind: CallbackExprKind::Call {
                            callee: Box::new(CallbackExpr {
                                kind: CallbackExprKind::Function(function_name),
                                ty: function_ty,
                            }),
                            args: vec![CallbackCallArg {
                                expr: CallbackExpr {
                                    kind: CallbackExprKind::Param(0),
                                    ty: item_ty,
                                },
                                spread: false,
                            }],
                        },
                        ty: return_ty,
                    }),
                },
                ty: bool_ty,
            }));
        }
        if let Expression::StaticMemberExpression(member) = &call.callee
            && member.property.name == "prop"
            && self.imported_utility_object(&member.object)
            && let [Argument::StringLiteral(field)] = call.arguments.as_slice()
        {
            return Ok(Some(CallbackExpr {
                kind: CallbackExprKind::Field {
                    receiver: Box::new(CallbackExpr {
                        kind: CallbackExprKind::Param(0),
                        ty: item_ty,
                    }),
                    field: self.intern_source_name(field.value.as_str()),
                },
                ty: self.ctx.krate.types.intern(Type::Unknown),
            }));
        }
        if Self::is_identifier_callee(&call.callee, "emitEvent") && call.arguments.len() == 1 {
            return Ok(Some(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::None),
                ty: self.ctx.krate.types.intern(Type::None),
            }));
        }
        if Self::is_static_member_callee(&call.callee, "async", "pipe") {
            return Ok(Some(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::None),
                ty: self.ctx.krate.types.intern(Type::Unknown),
            }));
        }
        if Self::is_static_member_callee(&call.callee, "_", "negate")
            && let [Argument::StaticMemberExpression(inner)] = call.arguments.as_slice()
            && matches!(&inner.object, Expression::Identifier(object) if object.name == "_")
            && inner.property.name == "isNil"
        {
            return Ok(Some(CallbackExpr {
                kind: CallbackExprKind::Binary {
                    op: BinOp::NotEq,
                    lhs: Box::new(CallbackExpr {
                        kind: CallbackExprKind::Param(0),
                        ty: item_ty,
                    }),
                    rhs: Box::new(CallbackExpr {
                        kind: CallbackExprKind::Literal(Literal::None),
                        ty: self.ctx.krate.types.intern(Type::None),
                    }),
                },
                ty: bool_ty,
            }));
        }
        Ok(None)
    }

    /// Return whether a call callee is a bare identifier with the given name.
    pub(in crate::lowering) fn is_identifier_callee(callee: &Expression<'_>, expected: &str) -> bool {
        matches!(callee, Expression::Identifier(identifier) if identifier.name == expected)
    }

    /// Return whether a call callee is `object.property`.
    pub(in crate::lowering) fn is_static_member_callee(
        callee: &Expression<'_>,
        object_name: &str,
        property_name: &str,
    ) -> bool {
        matches!(
            callee,
            Expression::StaticMemberExpression(member)
                if matches!(&member.object, Expression::Identifier(object) if object.name == object_name)
                    && member.property.name == property_name
        )
    }

    /// Lower callbacks wrapped in erased TypeScript assertion syntax.
    pub(in crate::lowering) fn asserted_arrow_callback(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &Body,
    ) -> Result<Option<CallbackExpr>, SmeltError> {
        match argument {
            Argument::TSAsExpression(as_expr) => Ok(Some(self.arrow_callback_expression(
                &as_expr.expression,
                expected_param_tys,
                body,
            )?)),
            Argument::TSTypeAssertion(assertion) => Ok(Some(self.arrow_callback_expression(
                &assertion.expression,
                expected_param_tys,
                body,
            )?)),
            Argument::TSSatisfiesExpression(satisfies) => Ok(Some(
                self.arrow_callback_expression(&satisfies.expression, expected_param_tys, body)?,
            )),
            Argument::TSNonNullExpression(non_null) => Ok(Some(self.arrow_callback_expression(
                &non_null.expression,
                expected_param_tys,
                body,
            )?)),
            _ => Ok(None),
        }
    }

    /// Lower the expression inside a TypeScript assertion when it is a callback.
    pub(in crate::lowering) fn arrow_callback_expression(
        &mut self,
        expression: &Expression<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        match expression {
            Expression::ArrowFunctionExpression(arrow) => {
                if arrow.r#async {
                    return Err(SmeltError::unsupported(
                        self.span(arrow.span.start, arrow.span.end),
                        "async callbacks need closure-body lowering",
                    ));
                }
                self.arrow_callback_from_params(arrow, expected_param_tys, body)
            }
            Expression::FunctionExpression(function) => {
                self.function_callback_from_params(function, expected_param_tys, body)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.arrow_callback_expression(&parenthesized.expression, expected_param_tys, body)
            }
            Expression::TSAsExpression(as_expr) => {
                self.arrow_callback_expression(&as_expr.expression, expected_param_tys, body)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.arrow_callback_expression(&satisfies.expression, expected_param_tys, body)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.arrow_callback_expression(&non_null.expression, expected_param_tys, body)
            }
            Expression::StaticMemberExpression(_member) => {
                Ok(self.opaque_member_callback(expected_param_tys))
            }
            Expression::ConditionalExpression(_) | Expression::LogicalExpression(_) => {
                Ok(self.opaque_member_callback(expected_param_tys))
            }
            _ => Err(SmeltError::unsupported(
                self.span(expression.span().start, expression.span().end),
                "array callback methods currently require arrow function callbacks",
            )),
        }
    }

    /// Return whether a local's type can hold a callable value handed to an
    /// array method as a named callback.
    ///
    /// `Type::Function` locals are lowered directly elsewhere; this covers the
    /// erased/generic surfaces — `unknown`/`any`, type parameters, and unions
    /// that include a function or erased branch — where the value is genuinely
    /// callable at runtime but lacks a clean static function type.
    pub(in crate::lowering) fn callback_local_value_is_callable_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. }) => true,
            Some(Type::Union(items)) => items.iter().copied().any(|item| {
                matches!(
                    self.ctx.krate.types.get(item),
                    Some(Type::Function(_) | Type::Unknown)
                )
            }),
            _ => false,
        }
    }

    /// Build an opaque callback that calls a captured outer local value.
    ///
    /// Mirrors [`Self::opaque_member_callback`], but the callee is the named
    /// local itself (captured by id) instead of a `None` placeholder resolved by
    /// name. This lowers `xs.map(fn)` where `fn` is a local holding a callable
    /// value whose static type is erased (`any`/`unknown`), a type parameter, or
    /// a union that includes a function — cases where the local is genuinely
    /// callable but does not have a clean `Type::Function`, so the direct
    /// local-value branch above does not fire. The wrapper closure forwards the
    /// receiver's element arguments, matching how a direct `fn(...)` call lowers.
    pub(in crate::lowering) fn opaque_local_callback(
        &mut self,
        local: smelt_hir::LocalId,
        local_ty: smelt_hir::TypeId,
        expected_param_tys: &[smelt_hir::TypeId],
    ) -> CallbackExpr {
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let args = expected_param_tys
            .iter()
            .copied()
            .enumerate()
            .map(|(index, ty)| CallbackCallArg {
                expr: CallbackExpr {
                    kind: CallbackExprKind::Param(index),
                    ty,
                },
                spread: false,
            })
            .collect();
        CallbackExpr {
            kind: CallbackExprKind::Call {
                callee: Box::new(CallbackExpr {
                    kind: CallbackExprKind::Capture(local),
                    ty: local_ty,
                }),
                args,
            },
            ty: unknown_ty,
        }
    }

    /// Build an opaque callback expression for imported predicate/function members.
    pub(in crate::lowering) fn opaque_member_callback(&mut self, expected_param_tys: &[smelt_hir::TypeId]) -> CallbackExpr {
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let function_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: expected_param_tys.to_vec(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: unknown_ty,
            is_async: false,
            may_throw: false,
        }));
        let args = expected_param_tys
            .iter()
            .copied()
            .enumerate()
            .map(|(index, ty)| CallbackCallArg {
                expr: CallbackExpr {
                    kind: CallbackExprKind::Param(index),
                    ty,
                },
                spread: false,
            })
            .collect();
        CallbackExpr {
            kind: CallbackExprKind::Call {
                callee: Box::new(CallbackExpr {
                    kind: CallbackExprKind::Literal(Literal::None),
                    ty: function_ty,
                }),
                args,
            },
            ty: unknown_ty,
        }
    }

    /// Return whether a bare identifier names a callable value whose body is
    /// opaque to this module — an imported function value or a module-level
    /// function/const callable declared elsewhere in the source.
    ///
    /// Such names resolve like a direct call would (see `call.rs`), so when one
    /// is handed to an array method as a named-local callback we can lower it to
    /// an opaque closure that calls the value, rather than rejecting it for not
    /// being an inline arrow. The name must not shadow a local binding, because a
    /// local with the same name is lexically nearer and handled separately.
    pub(in crate::lowering) fn is_opaque_callback_value(&self, name: &str) -> bool {
        !self.locals.contains_key(name)
            && !self.items.contains_key(name)
            && (self.value_imports.contains(name) || self.source_contains_forward_callable(name))
    }

    /// Return whether an expression is an imported utility namespace/object.
    pub(in crate::lowering) fn imported_utility_object(&self, expression: &Expression<'_>) -> bool {
        matches!(
            expression,
            Expression::Identifier(object)
                if self.namespace_imports.contains(object.name.as_str())
                    || self.object_namespaces.contains_key(object.name.as_str())
        )
    }

    /// Lower a function-expression callback after expected parameter types are known.
    pub(in crate::lowering) fn function_callback_from_params(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        if function.r#async {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "async callbacks need closure-body lowering",
            ));
        }
        if function.params.rest.is_some() || function.params.items.len() > expected_param_tys.len()
        {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "array callback parameter count is not supported for this method",
            ));
        }
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "function expression callbacks must have a body",
            ));
        };
        let mut params = HashMap::new();
        for (index, param) in function.params.items.iter().enumerate() {
            let Some(expected_ty) = expected_param_tys.get(index).copied() else {
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "array callback parameter count is not supported for this method",
                ));
            };
            self.bind_callback_param_pattern(&param.pattern, index, expected_ty, &mut params)?;
        }
        self.callback_block_expression(&function_body.statements, &mut params, body)
    }

    /// Lower an arrow callback after the expected parameter types are known.
    pub(in crate::lowering) fn arrow_callback_from_params(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        if arrow.params.items.len() > expected_param_tys.len() {
            return Err(SmeltError::unsupported(
                self.span(arrow.span.start, arrow.span.end),
                "array callback parameter count is not supported for this method",
            ));
        }
        let mut params = HashMap::new();
        for (index, param) in arrow.params.items.iter().enumerate() {
            let Some(expected_ty) = expected_param_tys.get(index).copied() else {
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "array callback parameter count is not supported for this method",
                ));
            };
            self.bind_callback_param_pattern(&param.pattern, index, expected_ty, &mut params)?;
        }
        if let Some(rest) = &arrow.params.rest {
            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest callback parameters need closure-body lowering",
                ));
            };
            let rest_index = arrow.params.items.len();
            let rest_ty = expected_param_tys.get(rest_index).copied().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest callback parameter has no expected input type",
                )
            })?;
            if matches!(self.ctx.krate.types.get(rest_ty), Some(Type::List(_)))
                && expected_param_tys.len() == rest_index + 1
            {
                params.insert(
                    binding.name.as_str(),
                    CallbackExpr {
                        kind: CallbackExprKind::Param(rest_index),
                        ty: rest_ty,
                    },
                );
            } else {
                let rest_items = expected_param_tys
                    .iter()
                    .enumerate()
                    .skip(rest_index)
                    .map(|(index, ty)| CallbackExpr {
                        kind: CallbackExprKind::Param(index),
                        ty: *ty,
                    })
                    .collect::<Vec<_>>();
                let item_ty = rest
                    .type_annotation
                    .as_ref()
                    .and_then(|annotation| {
                        let list_ty = self.ts_type_to_hir(&annotation.type_annotation).ok()?;
                        match self.ctx.krate.types.get(list_ty) {
                            Some(Type::List(item_ty)) => Some(*item_ty),
                            _ => None,
                        }
                    })
                    .or_else(|| rest_items.first().map(|item| item.ty))
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                params.insert(
                    binding.name.as_str(),
                    CallbackExpr {
                        kind: CallbackExprKind::ListLit(rest_items),
                        ty: self.ctx.krate.types.intern(Type::List(item_ty)),
                    },
                );
            }
        }
        if arrow.expression {
            let [Statement::ExpressionStatement(statement)] = arrow.body.statements.as_slice()
            else {
                return Err(SmeltError::unsupported(
                    self.span(arrow.body.span.start, arrow.body.span.end),
                    "expression-bodied callbacks must contain one expression",
                ));
            };
            self.callback_expression(&statement.expression, &params, body)
        } else {
            self.callback_block_expression(&arrow.body.statements, &mut params, body)
        }
    }

}
