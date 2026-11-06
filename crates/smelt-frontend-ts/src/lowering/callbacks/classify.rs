//! Callback-surface classification: resolving callback function items,
//! arrow/function callback recognizers, factory predicates, and opaque
//! (local/member) callback values, plus building callbacks from param lists.

use crate::lowering::js_text::js_str_text;
use crate::lowering::{
    Argument, BinOp, Body, CallbackCallArg, CallbackExpr, CallbackExprKind,
    CaptureMode, ClosureCapture, Expr, ExprKind, Expression, FunctionType, HashMap, Item,
    Literal, LocalDecl, ModuleBuilder, Param, SmeltError, Span, Type,
};
use smelt_hir::ErasureReason;
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
                    // A capture is normally by reference: the value is already
                    // in the local when the closure is built, so a clone of it
                    // is the value the closure observes forever. A binding this
                    // closure READS BEFORE ITS DECLARATION is the one shape
                    // where that is wrong -- the slot is still empty at capture
                    // time and the declaration fills it later -- so it is
                    // captured through the shared cell instead, exactly as an
                    // assigned capture is.
                    let mode = if local_decl
                        .name
                        .and_then(|symbol| self.ctx.krate.symbols.get(symbol))
                        .is_some_and(|name| self.is_forward_referenced_local(name))
                    {
                        CaptureMode::ByMut
                    } else {
                        CaptureMode::ByRef
                    };
                    captures.entry(*local).or_insert_with(|| ClosureCapture {
                        source_local: *local,
                        body_local: None,
                        symbol: local_decl
                            .name
                            .unwrap_or_else(|| self.ctx.krate.symbols.intern("__capture")),
                        ty: local_decl.ty,
                        mode,
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
            | CallbackExprKind::FieldTruthy { receiver, .. }
            | CallbackExprKind::ValueTruthy { value: receiver } => {
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
        body: &mut Body,
    ) -> Result<CallbackExpr, SmeltError> {
        if let Some(callback) = self.asserted_arrow_callback(argument, expected_param_tys, body)? {
            return Ok(callback);
        }
        {
            // A callback produced by a call (`xs.filter(negate(isEven))`) is
            // deliberately NOT modeled here. The compact callback IR can only
            // stand in a fabricated callee for it, which silently replaces the
            // factory's real result with a null value. It has to be evaluated
            // once, in the enclosing body, and then called — see the callable
            // surface branch of `Self::callback_argument`, which owns the
            // enclosing `Body` and can bind the result to a local.
            if let Argument::Identifier(identifier) = argument
                && let Some(callback) = self.named_callback_reference(
                    identifier.name.as_str(),
                    identifier.span.start,
                    identifier.span.end,
                    expected_param_tys,
                    body,
                )?
            {
                return Ok(callback);
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
            if let Argument::ArrayExpression(array) = argument
                && let Some(callback) =
                    self.matches_property_iteratee_callback(array, expected_param_tys, body)?
            {
                return Ok(callback);
            }
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "array callback methods currently require arrow function callbacks",
            ))
        }
    }

    /// Resolve a bare identifier callback (`xs.map(fn)`) to a typed callback
    /// expression tree, if the name statically names a callable value.
    ///
    /// Handles, in resolution order:
    /// - a module-level function item, called by name through
    ///   [`CallbackExprKind::Function`];
    /// - a local/parameter binding whose static type is a `Type::Function`,
    ///   called through a captured [`CallbackExprKind::Capture`];
    /// - an opaque callable value — an import or forward-declared function whose
    ///   body is not visible here — modeled as an [`Self::opaque_member_callback`].
    ///
    /// Returns `Ok(None)` when the name is not a resolvable callable so the
    /// caller can fall through to its remaining forms (or raise its own error).
    /// Typed function signatures and receiver element arguments are preserved so
    /// the generated call keeps its concrete parameter/return types.
    pub(in crate::lowering) fn named_callback_reference(
        &mut self,
        name: &str,
        start: u32,
        end: u32,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &Body,
    ) -> Result<Option<CallbackExpr>, SmeltError> {
        if let Some(item) = self.items.get(name).copied() {
            let span = self.span(start, end);
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
            // JavaScript ignores callback arguments beyond the declared
            // parameters, so a reference declaring fewer parameters than the
            // receiver supplies — down to zero (`values.filter(stubTrue)`) —
            // is called with just its declared prefix.
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
            return Ok(Some(CallbackExpr {
                kind: CallbackExprKind::Call {
                    callee: Box::new(CallbackExpr {
                        kind: CallbackExprKind::Function(function_name),
                        ty: function_ty,
                    }),
                    args,
                },
                ty: function_return_ty,
            }));
        }
        if let Some(local) = self.scope.lookup(name) {
            let local_ty = Self::local_ty(body, local);
            if let Some(Type::Function(function)) = self.ctx.krate.types.get(local_ty).cloned() {
                // Same arity rule as item references above: fewer declared
                // parameters (including zero) ignore the extra supplied
                // arguments.
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
                return Ok(Some(CallbackExpr {
                    kind: CallbackExprKind::Call {
                        callee: Box::new(CallbackExpr {
                            kind: CallbackExprKind::Capture(local),
                            ty: local_ty,
                        }),
                        args,
                    },
                    ty: function.return_ty,
                }));
            }
        }
        if self.is_opaque_callback_value(name) {
            self.reject_unresolved_callback_import(name, start, end)?;
            return Ok(Some(self.opaque_member_callback(expected_param_tys)));
        }
        Ok(None)
    }

    /// Report a callback that names an import no source file or host module
    /// provides, instead of standing a null callee in for it.
    pub(in crate::lowering) fn reject_unresolved_callback_import(
        &self,
        name: &str,
        span_start: u32,
        span_end: u32,
    ) -> Result<(), SmeltError> {
        self.imports.unresolved_value_import(name).map_or(Ok(()), |blocker| {
            Err(SmeltError::missing_stdlib(self.span(span_start, span_end), blocker.to_owned()))
        })
    }

    /// Lower the lodash array iteratee shorthand `[path, srcValue]` passed as a
    /// callback (`_.find(list, ['key', key])`).
    ///
    /// Lodash-style collection functions accept a two-element array in callback
    /// position as a `matchesProperty(path, srcValue)` predicate: it returns
    /// whether `element[path] === srcValue`. A callable is required in the same
    /// position for native array methods, so an array literal reaching callback
    /// classification can only be this iteratee form. Only a string-literal
    /// path is modeled; other shapes return `Ok(None)` so the caller keeps its
    /// unsupported-callback diagnostic.
    fn matches_property_iteratee_callback(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &mut Body,
    ) -> Result<Option<CallbackExpr>, SmeltError> {
        let [first, second] = array.elements.as_slice() else {
            return Ok(None);
        };
        let oxc::ast::ast::ArrayExpressionElement::StringLiteral(path) = first else {
            return Ok(None);
        };
        let Some(value_expression) = second.as_expression() else {
            return Ok(None);
        };
        let item_ty = expected_param_tys
            .first()
            .copied()
            .unwrap_or_else(|| self.erase_with(ErasureReason::MissingAnnotation, "iteratee element"));
        // JavaScript builds the iteratee array once, at the call.
        let source_value = self.expression(value_expression, body)?;
        let value_ty = Self::expr_ty(body, source_value);
        let span = self.span(array.span.start, array.span.end);
        let symbol = self.ctx.krate.symbols.intern("__iteratee_value");
        let local = self.capture_bind_value(source_value, value_ty, symbol, span, body);
        let value = CallbackExpr { kind: CallbackExprKind::Capture(local), ty: value_ty };
        let field = self.intern_source_name(&js_str_text(path.value));
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let field_ty = self.erase_with(ErasureReason::ByName, &js_str_text(path.value));
        Ok(Some(CallbackExpr {
            kind: CallbackExprKind::Binary {
                op: BinOp::JsStrictEq,
                lhs: Box::new(CallbackExpr {
                    kind: CallbackExprKind::Field {
                        receiver: Box::new(CallbackExpr {
                            kind: CallbackExprKind::Param(0),
                            ty: item_ty,
                        }),
                        field,
                    },
                    ty: field_ty,
                }),
                rhs: Box::new(value),
            },
            ty: bool_ty,
        }))
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
            // A bare identifier inside an assertion (`(fn as Fn)`, `fn!`) that
            // statically names a function item, a function-typed local, or an
            // opaque imported callable. Resolve it the same way the unwrapped
            // `xs.map(fn)` form does so the assertion is transparent.
            Expression::Identifier(identifier) => self
                .named_callback_reference(
                    identifier.name.as_str(),
                    identifier.span.start,
                    identifier.span.end,
                    expected_param_tys,
                    body,
                )?
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(identifier.span.start, identifier.span.end),
                        "array callback methods currently require arrow function callbacks",
                    )
                }),
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
    pub(in crate::lowering) fn callback_local_value_is_callable_surface(&mut self, ty: smelt_hir::TypeId) -> bool {
        self.callback_local_value_is_callable_surface_depth(ty, 0)
    }

    /// Depth-guarded worker for [`Self::callback_local_value_is_callable_surface`].
    ///
    /// The depth bound stops runaway recursion when a type alias resolves to a
    /// type that references itself.
    fn callback_local_value_is_callable_surface_depth(
        &mut self,
        ty: smelt_hir::TypeId,
        depth: u32,
    ) -> bool {
        if depth > 8 {
            return false;
        }
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
            .cloned()
        {
            Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. }) => true,
            // An optional callable (`compareKeys?: (a, b) => number` used as
            // `keys.sort(compareKeys)`) is a callable surface too; the wrapper
            // closure observes the nullish payload at runtime.
            Some(Type::Optional(inner)) => {
                self.callback_local_value_is_callable_surface_depth(inner, depth + 1)
            }
            Some(Type::Union(items)) => items.iter().copied().any(|item| {
                matches!(
                    self.ctx.krate.types.get(item),
                    Some(Type::Function(_) | Type::Unknown)
                )
            }),
            // A named type reference (`ListIterateeCustom<T, boolean>`).
            //
            // When the alias definition is visible in this lowering unit, resolve
            // it and inspect the underlying type: a union alias like
            // `((v, i, c) => R) | PropertyKey | ...` still exposes a callable
            // (function) arm, so a local of that type is a callable surface.
            //
            // When the reference is *unresolved* — the alias is imported from
            // another module and its definition is not present in this lowering
            // unit (single-file lowering, e.g. `smelt probe`) — it is an erased
            // type boundary: its concrete shape is unknown here. A concrete
            // nominal type (a real `class`/`interface` declared in scope) is not
            // a callable surface, so keep rejecting those. But an opaque imported
            // reference is treated like `unknown` — a callable surface whose
            // wrapper closure observes the real value at runtime — matching how
            // the whole-crate build (where the alias resolves to its union) lowers
            // the same call.
            Some(Type::Class { name, args }) => {
                if self.types.is_callable_object_alias(name) {
                    return true;
                }
                if let Some(underlying) = self.resolve_type_alias_underlying(name, &args)
                    && underlying != ty
                {
                    return self
                        .callback_local_value_is_callable_surface_depth(underlying, depth + 1);
                }
                !self.symbol_names_concrete_nominal_type(name)
            }
            _ => false,
        }
    }

    /// Resolve a `Type::Class` alias reference to its underlying definition with
    /// the reference's type arguments substituted for the alias parameters.
    ///
    /// Returns `None` when the symbol does not name a type alias (e.g. a real
    /// class or interface), so callers can fall back to their opaque handling.
    fn resolve_type_alias_underlying(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
    ) -> Option<smelt_hir::TypeId> {
        let alias = self.find_type_alias(name).cloned()?;
        let mut substitutions = HashMap::new();
        for (index, param) in alias.type_params.iter().enumerate() {
            let actual = args
                .get(index)
                .copied()
                .or(param.default)
                .unwrap_or_else(|| {
                    self.ctx
                        .krate
                        .types
                        .intern(Type::TypeParam { name: param.name })
                });
            substitutions.insert(param.name, actual);
        }
        Some(self.substitute_type_params(alias.ty, &substitutions))
    }

    /// Return whether `name` refers to a concrete nominal type — a `class` or
    /// `interface` declared in this lowering unit — as opposed to an unresolved
    /// (imported, definition-not-present) type reference.
    fn symbol_names_concrete_nominal_type(&self, name: smelt_hir::Symbol) -> bool {
        if self.find_interface(name).is_some() {
            return true;
        }
        self.ctx
            .krate
            .items
            .iter()
            .any(|item| matches!(item, Item::Class(class) if class.name == name))
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
        let unknown_ty = if self.ctx.krate.types.get(local_ty) == Some(&Type::Unknown) {
            // not a new erasure: the callee local is already erased
            self.unknown_type()
        } else {
            // A type-parameter or function-union callee has no single return type.
            self.erase_with(ErasureReason::TypeLevel, "opaque local callback")
        };
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
        let unknown_ty = self.erase_with(ErasureReason::UntypedPackage, "opaque member callback");
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
    /// Such names resolve like a direct call would (see `call_expression` in
    /// `stdlib/call_dispatch.rs`), so when one
    /// is handed to an array method as a named-local callback we can lower it to
    /// an opaque closure that calls the value, rather than rejecting it for not
    /// being an inline arrow. The name must not shadow a local binding, because a
    /// local with the same name is lexically nearer and handled separately.
    pub(in crate::lowering) fn is_opaque_callback_value(&self, name: &str) -> bool {
        !self.scope.is_bound(name)
            && !self.items.contains_key(name)
            && (self.imports.is_value(name) || self.source_contains_forward_callable(name))
    }

    /// Return whether an expression is an imported utility namespace/object.
    pub(in crate::lowering) fn imported_utility_object(&self, expression: &Expression<'_>) -> bool {
        matches!(
            expression,
            Expression::Identifier(object)
                if self.imports.is_namespace(object.name.as_str())
                    || self.object_namespaces.contains_key(object.name.as_str())
        )
    }

    /// Return whether a name-matched global builtin call is shadowed by a real
    /// binding in scope, so the global interceptor must not fire.
    ///
    /// The builtin call interceptors (`isNaN(...)`, `parseInt(...)`, ...) are
    /// dispatched by identifier name *before* the callee is resolved against the
    /// module's bindings. That is only correct when the name still refers to the
    /// JavaScript global. A value import, module item, or local binding of the
    /// same name lexically shadows the global — for example es-toolkit's
    /// `import { isNaN } from './isNaN'` makes `isNaN(true)` a call to the
    /// imported function, not the numeric global predicate. In that case the
    /// interceptor bails and the ordinary call path lowers the real binding
    /// (mirroring how [`Self::imported_utility_object`] defers namespace member
    /// calls). A file that genuinely calls the global has no such binding, so the
    /// interceptor keeps firing unchanged.
    pub(in crate::lowering) fn builtin_call_identifier_is_shadowed(&self, name: &str) -> bool {
        self.scope.is_bound(name)
            || self.source_facts.declares_module_value(name)
            || (self.imports.is_value(name) && self.host_module_identifier_export_path(name).is_none())
    }

}
