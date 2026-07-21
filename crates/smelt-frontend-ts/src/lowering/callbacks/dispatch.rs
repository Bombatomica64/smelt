//! Callback-argument dispatch: the entry points that turn a source argument
//! into a [`CallbackExpr`] (with body/truthy fallbacks) and the large
//! `callback_expression` match that lowers every supported callback body form.

use crate::lowering::{
    Argument, ArrayExpressionElement, AssignmentOperator, AssignmentTarget, BinOp, BinaryOperator,
    Body, CallbackCallArg, CallbackExpr, CallbackExprKind, ClosureCallback, ConstCollectionValue,
    Expr, ExprKind, Expression, FunctionType, HashMap, Item, Literal, LogicalOperator, ModuleBuilder,
    ObjectPropertyKind, PropertyKey, SmeltError, Span, Type, UnaryOp, UnaryOperator, UnknownKind,
};
use oxc::span::GetSpan;

impl ModuleBuilder<'_> {
    /// Lower either an inline arrow callback or a local closure callback value.
    pub(in crate::lowering) fn callback_argument(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        context: &'static str,
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        if let Argument::Identifier(identifier) = argument {
            if let Some(local) = self.locals.get(identifier.name.as_str()).copied() {
                let local_ty = Self::local_ty(body, local);
                if let Some(Type::Function(function)) = self.ctx.krate.types.get(local_ty).cloned()
                {
                    let expr = self.identifier_expression(
                        identifier.name.as_str(),
                        identifier.span.start,
                        identifier.span.end,
                        body,
                    )?;
                    return Ok(ClosureCallback {
                        expr,
                        return_ty: function.return_ty,
                    });
                }
            }
            if let Some(item) = self.items.get(identifier.name.as_str()).copied() {
                let span = self.span(identifier.span.start, identifier.span.end);
                let Item::Function(function) = self.item_ref(item) else {
                    return Err(SmeltError::unsupported(
                        span,
                        format!(
                            "{context} callback item `{}` is not a function",
                            identifier.name
                        ),
                    ));
                };
                // JavaScript adapts callback arity at the call site: an item
                // declaring fewer parameters than the receiver supplies (down
                // to zero, e.g. `values.map(stubTrue)`) ignores the extra
                // arguments, and one declaring more (e.g. `xs.map(orderBy)`
                // with a four-parameter `orderBy`) receives `undefined` for the
                // unsupplied optional tail. Wrap the item capped at the
                // receiver's supplied arity so the generated closure matches
                // what the callback caller actually passes.
                let return_ty = function.return_ty;
                let expr = self.item_function_closure_expression_with_max_params(
                    item,
                    expected_param_tys.len(),
                    identifier.span.start,
                    identifier.span.end,
                    body,
                )?;
                return Ok(ClosureCallback { expr, return_ty });
            }
            // The global `Object` function passed as a callback
            // (`xs.map(Object)`) boxes each element into its wrapper object.
            // Smelt does not model wrapper objects separately from their
            // primitive values — a boxed string coerces back to the same
            // string everywhere it is used — so the conversion is the identity
            // on the receiver's element type. Lowering it as a typed identity
            // closure keeps the mapped list's concrete element type instead of
            // erasing it.
            if identifier.name == "Object"
                && !self.builtin_call_identifier_is_shadowed("Object")
            {
                let span = self.span(identifier.span.start, identifier.span.end);
                let param_ty = expected_param_tys
                    .first()
                    .copied()
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                let expr = self.builtin_unary_closure_expression(
                    param_ty,
                    param_ty,
                    span,
                    body,
                    |value_expr| ExprKind::TypeAssert { value: value_expr },
                );
                return Ok(ClosureCallback {
                    expr,
                    return_ty: param_ty,
                });
            }
            // Recognized global builtin *functions* passed as callbacks
            // (`xs.map(Number)`, `xs.filter(Boolean)`, `xs.map(parseInt)`).
            // Lower them to the same concrete single-argument closures used in
            // ordinary value position so the array method runs the builtin's
            // real behavior instead of a placeholder.
            if let Some(expr) = self.builtin_function_value_expression(
                identifier.name.as_str(),
                identifier.span.start,
                identifier.span.end,
                body,
            ) {
                let return_ty = self.closure_value_return_ty(expr, body);
                return Ok(ClosureCallback { expr, return_ty });
            }
            // Imported es-toolkit/lodash predicates whose bodies are opaque here
            // but whose `(value) => bool` shape is known. These are not builtins,
            // so they are gated on being a value import.
            if matches!(
                identifier.name.as_str(),
                "isEmpty" | "isArray" | "isString" | "isObject" | "trim"
            ) && self.value_imports.contains(identifier.name.as_str())
            {
                let param_ty = expected_param_tys
                    .first()
                    .copied()
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                let return_ty = self.ctx.krate.types.intern(Type::Bool);
                let function_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params: vec![param_ty],
                    rest: None,
                    required_params: None,
                    mutable_params: Vec::new(),
                    return_ty,
                    is_async: false,
                    may_throw: false,
                }));
                let expr = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: function_ty,
                    span: self.span(identifier.span.start, identifier.span.end),
                });
                return Ok(ClosureCallback { expr, return_ty });
            }
            if self.is_opaque_callback_value(identifier.name.as_str()) {
                // The callback names an imported or forward-declared callable
                // whose body is opaque here. Lower it like an opaque member
                // callback: a closure that calls the value with the receiver's
                // element arguments. This matches how a direct call to the same
                // value lowers, and lets array methods accept named-local
                // callbacks instead of requiring an inline arrow.
                let callback = self.opaque_member_callback(expected_param_tys);
                let return_ty = callback.ty;
                let expr = self.callback_expr_to_closure(
                    &callback,
                    expected_param_tys,
                    self.span(identifier.span.start, identifier.span.end),
                    body,
                )?;
                return Ok(ClosureCallback { expr, return_ty });
            }
            if !self.locals.contains_key(identifier.name.as_str()) {
                return Err(SmeltError::unsupported(
                    self.span(identifier.span.start, identifier.span.end),
                    format!(
                        "{context} local callback `{}` is not in scope",
                        identifier.name
                    ),
                ));
            }
            let Some(callback) = self.local_callbacks.get(identifier.name.as_str()).cloned() else {
                // The name is a local holding a value but is not an inlined
                // callback literal. If its (possibly erased) type is a callable
                // surface — `any`/`unknown`, a type parameter, or a union that
                // includes a function — call it through a wrapper closure that
                // captures the local and forwards the receiver's element
                // arguments, the same way a direct `fn(...)` call would lower.
                let local = self
                    .locals
                    .get(identifier.name.as_str())
                    .copied()
                    .expect("local checked present above");
                let local_ty = Self::local_ty(body, local);
                if self.callback_local_value_is_callable_surface(local_ty) {
                    let callback = self.opaque_local_callback(local, local_ty, expected_param_tys);
                    let return_ty = callback.ty;
                    let expr = self.callback_expr_to_closure(
                        &callback,
                        expected_param_tys,
                        self.span(identifier.span.start, identifier.span.end),
                        body,
                    )?;
                    return Ok(ClosureCallback { expr, return_ty });
                }
                return Err(SmeltError::unsupported(
                    self.span(identifier.span.start, identifier.span.end),
                    format!(
                        "{context} local callback `{}` is not defined",
                        identifier.name
                    ),
                ));
            };
            // A local callback declaring fewer parameters than the receiver
            // supplies (including zero) is valid JavaScript — the extra
            // arguments are simply ignored — so only reject the shape the
            // compact callback IR cannot express: a body that references more
            // parameters than the receiver will ever pass.
            if callback.params.len() > expected_param_tys.len() {
                return Err(SmeltError::unsupported(
                    self.span(identifier.span.start, identifier.span.end),
                    format!("{context} local callback parameter count is not supported"),
                ));
            }
            for (actual, expected) in callback.params.iter().zip(expected_param_tys) {
                if actual != expected {
                    return Err(SmeltError::unsupported(
                        self.span(identifier.span.start, identifier.span.end),
                        format!("{context} local callback parameter type does not match receiver"),
                    ));
                }
            }
            if callback.callback.ty != callback.return_ty {
                return Err(SmeltError::unsupported(
                    self.span(identifier.span.start, identifier.span.end),
                    format!("{context} local callback return type is inconsistent"),
                ));
            }
            let expr = self.callback_expr_to_closure_with_return_ty(
                callback.return_ty,
                &callback.callback,
                &callback.params,
                callback.rest.map(|rest| rest.index),
                callback.required_params,
                self.span(identifier.span.start, identifier.span.end),
                body,
            )?;
            return Ok(ClosureCallback {
                expr,
                return_ty: callback.return_ty,
            });
        }
        if !matches!(
            argument,
            Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_)
        ) {
            let direct_expr = self.argument(argument, body)?;
            if let Some(Type::Function(function)) = self
                .ctx
                .krate
                .types
                .get(Self::expr_ty(body, direct_expr))
                .cloned()
            {
                return Ok(ClosureCallback {
                    expr: direct_expr,
                    return_ty: function.return_ty,
                });
            }
        }
        let callback = self.arrow_callback(argument, expected_param_tys, body)?;
        let return_ty = callback.ty;
        let expr = self.callback_expr_to_closure(
            &callback,
            expected_param_tys,
            self.span(argument.span().start, argument.span().end),
            body,
        )?;
        Ok(ClosureCallback { expr, return_ty })
    }

    /// Lower an array predicate callback, coercing JavaScript truthy returns into booleans.
    pub(in crate::lowering) fn truthy_callback_argument_with_body_fallback(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        context: &'static str,
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        match self.arrow_callback(argument, expected_param_tys, body) {
            Ok(callback) => {
                let span = self.span(argument.span().start, argument.span().end);
                let callback = self.coerce_callback_expr_to_truthy(callback, span)?;
                let expr = self.callback_expr_to_closure_with_return_ty(
                    bool_ty,
                    &callback,
                    expected_param_tys,
                    None,
                    None,
                    span,
                    body,
                )?;
                Ok(ClosureCallback {
                    expr,
                    return_ty: bool_ty,
                })
            }
            Err(error)
                if Self::should_fallback_to_closure_body_for_callback(&error)
                    && Self::is_closure_body_fallback_argument(argument) =>
            {
                let expr =
                    self.callback_closure_body_expr(argument, expected_param_tys, bool_ty, body)?;
                Ok(ClosureCallback {
                    expr,
                    return_ty: bool_ty,
                })
            }
            Err(error) => {
                drop(error);
                let callback =
                    self.callback_argument(argument, expected_param_tys, context, body)?;
                if callback.return_ty == bool_ty {
                    Ok(callback)
                } else if matches!(
                    self.ctx.krate.types.get(callback.return_ty),
                    Some(Type::Unknown | Type::TypeParam { .. })
                ) || self.erased_or_union_surface(callback.return_ty)
                {
                    // An opaque/named predicate (`xs.some(matchFunc)`) lowers to a
                    // wrapper closure whose result is an erased `unknown` value.
                    // JavaScript predicates use the truthiness of that result, and
                    // the downstream array predicate op coerces an erased callback
                    // result to bool, so accept the erased return type instead of
                    // rejecting the named-callback form.
                    Ok(ClosureCallback {
                        expr: callback.expr,
                        return_ty: bool_ty,
                    })
                } else {
                    Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        format!(
                            "{context} callback returns an unsupported type ({:?})",
                            self.ctx.krate.types.get(callback.return_ty)
                        ),
                    ))
                }
            }
        }
    }

    /// Convert a callback expression result into the boolean value used by JS predicates.
    pub(in crate::lowering) fn coerce_callback_expr_to_truthy(
        &mut self,
        callback: CallbackExpr,
        span: Span,
    ) -> Result<CallbackExpr, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        if callback.ty == bool_ty {
            return Ok(callback);
        }
        match callback.kind {
            CallbackExprKind::DynamicIndex { receiver, index } => Ok(CallbackExpr {
                kind: CallbackExprKind::HasDynamicField {
                    receiver,
                    field: index,
                },
                ty: bool_ty,
            }),
            CallbackExprKind::Field { receiver, field } => Ok(CallbackExpr {
                kind: CallbackExprKind::FieldTruthy { receiver, field },
                ty: bool_ty,
            }),
            kind if self.ctx.krate.types.get(callback.ty) == Some(&Type::Unknown) => {
                Ok(CallbackExpr {
                    kind: CallbackExprKind::UnknownIs {
                        value: Box::new(CallbackExpr {
                            kind,
                            ty: callback.ty,
                        }),
                        kind: UnknownKind::Bool,
                    },
                    ty: bool_ty,
                })
            }
            kind if self.ctx.krate.types.get(callback.ty) == Some(&Type::String) => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op: BinOp::NotEq,
                        lhs: Box::new(CallbackExpr {
                            kind,
                            ty: callback.ty,
                        }),
                        rhs: Box::new(CallbackExpr {
                            kind: CallbackExprKind::Literal(Literal::String(String::new())),
                            ty: string_ty,
                        }),
                    },
                    ty: bool_ty,
                })
            }
            kind if self.is_nullishable_type(callback.ty)
                || self.type_is_truthy_condition_surface(callback.ty) =>
            {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op: BinOp::NotEq,
                        lhs: Box::new(CallbackExpr {
                            kind,
                            ty: callback.ty,
                        }),
                        rhs: Box::new(CallbackExpr {
                            kind: CallbackExprKind::Literal(Literal::None),
                            ty: none_ty,
                        }),
                    },
                    ty: bool_ty,
                })
            }
            _ => Err(SmeltError::unsupported(
                span,
                "array predicate callback return cannot be coerced to boolean",
            )),
        }
    }

    /// Lower an array callback, falling back to a normal closure body when needed.
    pub(in crate::lowering) fn callback_argument_with_body_fallback(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        fallback_return_ty: smelt_hir::TypeId,
        context: &'static str,
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        match self.callback_argument(argument, expected_param_tys, context, body) {
            Ok(callback) => Ok(callback),
            Err(error)
                if Self::should_fallback_to_closure_body_for_callback(&error)
                    && Self::is_closure_body_fallback_argument(argument) =>
            {
                let expr = self.callback_closure_body_expr(
                    argument,
                    expected_param_tys,
                    fallback_return_ty,
                    body,
                )?;
                let return_ty = match self.ctx.krate.types.get(Self::expr_ty(body, expr)) {
                    Some(Type::Function(function)) => function.return_ty,
                    _ => fallback_return_ty,
                };
                Ok(ClosureCallback { expr, return_ty })
            }
            Err(error) => Err(error),
        }
    }

    /// Return whether an argument is a callback literal (arrow or `function`
    /// expression) whose body can be retried through full closure-body lowering.
    ///
    /// Both inline callback forms carry a body the compact callback IR may fail
    /// to model; when that happens the caller retries via
    /// [`Self::callback_closure_body_expr`]. Named/opaque callback identifiers
    /// have no local body to retry, so they are excluded here.
    pub(in crate::lowering) fn is_closure_body_fallback_argument(argument: &Argument<'_>) -> bool {
        matches!(
            argument,
            Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_)
        )
    }

    /// Lower an inline callback literal through a real HIR closure body.
    ///
    /// Dispatches to the arrow or `function` expression closure-body lowering
    /// depending on the callback form, so both are retried identically when the
    /// compact callback IR rejects a body it cannot model.
    pub(in crate::lowering) fn callback_closure_body_expr(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        fallback_return_ty: smelt_hir::TypeId,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match argument {
            Argument::ArrowFunctionExpression(arrow) => {
                self.arrow_closure_body_expr(arrow, expected_param_tys, fallback_return_ty, body)
            }
            Argument::FunctionExpression(function) => self.function_closure_body_expr(
                function,
                expected_param_tys,
                fallback_return_ty,
                body,
            ),
            _ => Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "callback closure-body lowering requires an arrow or function expression",
            )),
        }
    }

    /// Return whether compact callback lowering should retry as a normal closure.
    pub(in crate::lowering) fn should_fallback_to_closure_body_for_callback(error: &SmeltError) -> bool {
        error.message == "callback expression kind is not supported yet"
            || error.message == "callback member assignment needs closure-body lowering"
            // Reassigning a callback parameter (`(value) => { value = ...; }`)
            // cannot be modeled by the side-effect-free expression IR, but the
            // full closure-body path makes parameters mutable locals, so retry
            // there.
            || error.message == "callback parameter assignment is not supported yet"
            || error.message
                == "callback expression statements must be followed by a return or throw"
            || error.message == "callback side-effect blocks only support expression statements"
            || error.message
                == "callback side-effect blocks only support expression and throw statements"
            || error.message == "callback block declarations require simple bindings"
            // A callback body statement form the side-effect-free expression IR
            // cannot represent (e.g. `try`/`catch`, loops, `let` reassignment).
            // Full closure-body lowering supports these statements, so retry there.
            || error.message
                == "callback block statements must be const declarations, if guards, return, or throw"
            // A callback `if/else` (or `if/else if` chain) whose arms mutate a
            // captured local / the callback parameter before falling through to
            // shared trailing statements cannot be modeled by the compact
            // side-effect-free callback IR. Full closure-body lowering makes
            // parameters mutable locals and lowers the branch natively, so retry
            // there.
            || error.message
                == "callback if/else blocks need direct conditional expression lowering"
            // A callback `if` guard whose consequent assigns a captured local
            // (`if (!called) { ret = fn(); called = true; }`) cannot be modeled
            // by the compact side-effect-free ternary IR (the assignment would
            // hoist out of the guard). Full closure-body lowering emits a native
            // branch, so retry there.
            || error.message
                == "callback if guard mutates a captured local; needs closure-body lowering"
            || error.message == "async callbacks need closure-body lowering"
            // A method/receiver call the compact callback dispatcher does not
            // model but the full method-call lowering does (e.g. `String.repeat`,
            // `Array.at` on a richer receiver). Retrying through the closure body
            // routes the receiver through the general `expression` path, which
            // knows the full method table and the closure parameter element
            // types, so it can lower calls the restricted dispatcher rejects.
            || error
                .message
                .ends_with("is not lowered into closure bodies yet")
            || error
                .message
                .starts_with("unresolved callback identifier `")
            // A callback body that reads a non-callable module/import item as an
            // ordinary value (`value !== whitespace`, where `whitespace` is a
            // `string` const) cannot be modeled by the compact callback IR, which
            // only resolves callable item references. Full closure-body lowering
            // routes the identifier through the general expression path, which
            // reads a value item of any type, so retry there.
            || error.message == "callback item references must resolve to callable values"
            || error
                .message
                .contains("resolves outside the current callback body")
    }

    /// Collapse tuple item types into the element type used by array callbacks.
    pub(in crate::lowering) fn tuple_items_element_type(&mut self, items: &[smelt_hir::TypeId]) -> smelt_hir::TypeId {
        match items {
            [] => self.ctx.krate.types.intern(Type::Unknown),
            [single] => *single,
            [first, rest @ ..] if rest.iter().all(|item| item == first) => *first,
            _ => self.ctx.krate.types.intern(Type::Union(items.to_vec())),
        }
    }

    /// Lower a supported callback expression.
    pub(in crate::lowering) fn callback_expression(
        &mut self,
        expression: &Expression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        match expression {
            Expression::Identifier(identifier) => {
                if let Some(param) = params.get(identifier.name.as_str()).cloned() {
                    return Ok(param);
                }
                if identifier.name == "undefined" {
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Literal(Literal::Undefined),
                        ty: self.ctx.krate.types.intern(Type::None),
                    });
                }
                if let Some(value) = self.const_literals.get(identifier.name.as_str()) {
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Literal(value.literal.clone()),
                        ty: value.ty,
                    });
                }
                if let Some(collection) = self.const_collections.get(identifier.name.as_str()) {
                    let items = collection
                        .items
                        .iter()
                        .map(|item| match &item.value {
                            ConstCollectionValue::Expr(ExprKind::Literal(literal)) => {
                                CallbackExpr {
                                    kind: CallbackExprKind::Literal(literal.clone()),
                                    ty: item.ty,
                                }
                            }
                            ConstCollectionValue::UnknownNull => CallbackExpr {
                                kind: CallbackExprKind::Literal(Literal::None),
                                ty: self.ctx.krate.types.intern(Type::None),
                            },
                            _ => CallbackExpr {
                                kind: CallbackExprKind::Literal(Literal::None),
                                ty: item.ty,
                            },
                        })
                        .collect();
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::ListLit(items),
                        ty: collection.ty,
                    });
                }
                // An enclosing local is lexically nearer than an imported or
                // module-scoped item, including when both are callable.
                if !self.locals.contains_key(identifier.name.as_str())
                    && let Some(item) = self.items.get(identifier.name.as_str()).copied()
                {
                    let span = self.span(identifier.span.start, identifier.span.end);
                    let ty = self.item_expr_type(item, span)?;
                    let function_name = if let Item::Function(function) = self.item_ref(item) {
                        Some(function.name)
                    } else if matches!(
                        self.ctx.krate.types.get(ty),
                        Some(
                            Type::Function(_)
                                | Type::Unknown
                                | Type::TypeParam { .. }
                                | Type::Class { .. }
                        )
                    ) {
                        None
                    } else {
                        return Err(SmeltError::unsupported(
                            span,
                            "callback item references must resolve to callable values",
                        ));
                    };
                    return Ok(CallbackExpr {
                        kind: function_name.map_or(
                            CallbackExprKind::Literal(Literal::None),
                            CallbackExprKind::Function,
                        ),
                        ty,
                    });
                }
                if !self.locals.contains_key(identifier.name.as_str())
                    && let Some((name, ty)) = self
                        .forward_function_types
                        .get(identifier.name.as_str())
                        .copied()
                {
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Function(name),
                        ty,
                    });
                }
                let Some(local) = self.locals.get(identifier.name.as_str()).copied() else {
                    if self.source_contains_forward_callable(identifier.name.as_str()) {
                        return Ok(CallbackExpr {
                            kind: CallbackExprKind::Literal(Literal::None),
                            ty: self.ctx.krate.types.intern(Type::Unknown),
                        });
                    }
                    return Err(SmeltError::for_unresolved_name(
                        self.span(identifier.span.start, identifier.span.end),
                        identifier.name.as_str(),
                        format!("unresolved callback identifier `{}`", identifier.name),
                    ));
                };
                let local_index = usize::try_from(local.0).map_err(|err| {
                    SmeltError::unsupported(
                        self.span(identifier.span.start, identifier.span.end),
                        format!("callback local id does not fit in usize: {err}"),
                    )
                })?;
                let Some(local_decl) = body.locals.get(local_index) else {
                    return Err(SmeltError::unsupported(
                        self.span(identifier.span.start, identifier.span.end),
                        format!(
                            "callback identifier `{}` resolves outside the current callback body",
                            identifier.name
                        ),
                    ));
                };
                let ty = local_decl.ty;
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Capture(local),
                    ty,
                })
            }
            Expression::NumericLiteral(literal) => Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::Float(literal.value)),
                ty: self.ctx.krate.types.intern(Type::Float),
            }),
            Expression::BigIntLiteral(literal) => {
                let value = literal.value.as_str().parse::<f64>().map_err(|err| {
                    SmeltError::unsupported(
                        self.span(literal.span.start, literal.span.end),
                        format!("bigint literal cannot be represented numerically: {err}"),
                    )
                })?;
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Literal(Literal::Float(value)),
                    ty: self.ctx.krate.types.intern(Type::Float),
                })
            }
            Expression::StringLiteral(literal) => Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::String(literal.value.to_string())),
                ty: self.ctx.krate.types.intern(Type::String),
            }),
            Expression::BooleanLiteral(literal) => Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::Bool(literal.value)),
                ty: self.ctx.krate.types.intern(Type::Bool),
            }),
            Expression::NullLiteral(_) => Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::None),
                ty: self.ctx.krate.types.intern(Type::None),
            }),
            Expression::ArrayExpression(array) => {
                let mut items = Vec::new();
                for element in &array.elements {
                    let expr = match element {
                        ArrayExpressionElement::SpreadElement(_) => {
                            return Err(SmeltError::unsupported(
                                self.span(element.span().start, element.span().end),
                                "callback array spread elements are not supported yet",
                            ));
                        }
                        ArrayExpressionElement::Elision(_) => {
                            return Err(SmeltError::unsupported(
                                self.span(element.span().start, element.span().end),
                                "callback array elisions are not supported",
                            ));
                        }
                        ArrayExpressionElement::NumericLiteral(literal) => CallbackExpr {
                            kind: CallbackExprKind::Literal(Literal::Float(literal.value)),
                            ty: self.ctx.krate.types.intern(Type::Float),
                        },
                        ArrayExpressionElement::BigIntLiteral(literal) => {
                            let value = literal.value.as_str().parse::<f64>().map_err(|err| {
                                SmeltError::unsupported(
                                    self.span(literal.span.start, literal.span.end),
                                    format!(
                                        "bigint literal cannot be represented numerically: {err}"
                                    ),
                                )
                            })?;
                            CallbackExpr {
                                kind: CallbackExprKind::Literal(Literal::Float(value)),
                                ty: self.ctx.krate.types.intern(Type::Float),
                            }
                        }
                        ArrayExpressionElement::StringLiteral(literal) => CallbackExpr {
                            kind: CallbackExprKind::Literal(Literal::String(
                                literal.value.to_string(),
                            )),
                            ty: self.ctx.krate.types.intern(Type::String),
                        },
                        ArrayExpressionElement::BooleanLiteral(literal) => CallbackExpr {
                            kind: CallbackExprKind::Literal(Literal::Bool(literal.value)),
                            ty: self.ctx.krate.types.intern(Type::Bool),
                        },
                        ArrayExpressionElement::Identifier(identifier) => {
                            if let Some(param) = params.get(identifier.name.as_str()).cloned() {
                                param
                            } else if let Some(local) =
                                self.locals.get(identifier.name.as_str()).copied()
                            {
                                let ty = Self::local_ty(body, local);
                                CallbackExpr {
                                    kind: CallbackExprKind::Capture(local),
                                    ty,
                                }
                            } else if self
                                .source_contains_forward_callable(identifier.name.as_str())
                            {
                                CallbackExpr {
                                    kind: CallbackExprKind::Literal(Literal::None),
                                    ty: self.ctx.krate.types.intern(Type::Unknown),
                                }
                            } else {
                                return Err(SmeltError::for_unresolved_name(
                                    self.span(identifier.span.start, identifier.span.end),
                                    identifier.name.as_str(),
                                    format!("unresolved callback identifier `{}`", identifier.name),
                                ));
                            }
                        }
                        // Binary elements deliberately have no dedicated arm:
                        // they fall through to the `as_expression` case below so
                        // the full callback expression dispatcher handles them.
                        // That keeps operator forms with dedicated lowering —
                        // `in`, `typeof` comparisons, `instanceof`, nullish
                        // checks — working inside array literals exactly as
                        // they do in any other callback position.
                        ArrayExpressionElement::ComputedMemberExpression(member) => {
                            let receiver =
                                self.callback_expression(&member.object, params, body)?;
                            let index =
                                self.callback_expression(&member.expression, params, body)?;
                            if self
                                .ctx
                                .krate
                                .types
                                .get(self.type_param_constraint_or_self(index.ty))
                                != Some(&Type::Float)
                                && self
                                    .ctx
                                    .krate
                                    .types
                                    .get(self.type_param_constraint_or_self(index.ty))
                                    != Some(&Type::Int)
                                && self
                                    .ctx
                                    .krate
                                    .types
                                    .get(self.type_param_constraint_or_self(index.ty))
                                    != Some(&Type::Unknown)
                            {
                                return Err(SmeltError::unsupported(
                                    self.span(
                                        member.expression.span().start,
                                        member.expression.span().end,
                                    ),
                                    "callback dynamic computed access index must be a number",
                                ));
                            }
                            let item_ty = match self
                                .ctx
                                .krate
                                .types
                                .get(self.type_param_constraint_or_self(receiver.ty))
                            {
                                Some(Type::List(item_ty)) => *item_ty,
                                Some(Type::String) => self.ctx.krate.types.intern(Type::String),
                                Some(Type::Unknown | Type::TypeParam { .. }) => {
                                    self.ctx.krate.types.intern(Type::Unknown)
                                }
                                Some(Type::Union(union_items))
                                    if union_items.iter().any(|item| {
                                        matches!(
                                            self.ctx.krate.types.get(*item),
                                            Some(
                                                Type::List(_)
                                                    | Type::Unknown
                                                    | Type::TypeParam { .. }
                                            )
                                        )
                                    }) =>
                                {
                                    self.ctx.krate.types.intern(Type::Unknown)
                                }
                                _ => self.ctx.krate.types.intern(Type::Unknown),
                            };
                            CallbackExpr {
                                kind: CallbackExprKind::DynamicIndex {
                                    receiver: Box::new(receiver),
                                    index: Box::new(index),
                                },
                                ty: item_ty,
                            }
                        }
                        other => {
                            if let Some(expr) = other.as_expression() {
                                self.callback_expression(expr, params, body)?
                            } else {
                                return Err(SmeltError::unsupported(
                                    self.span(element.span().start, element.span().end),
                                    "callback array element kind is not supported yet",
                                ));
                            }
                        }
                    };
                    items.push(expr);
                }
                let item_ty = if let Some(first) = items.first() {
                    if items.iter().all(|item| item.ty == first.ty) {
                        first.ty
                    } else {
                        let mut item_tys = Vec::new();
                        for item in &items {
                            if !item_tys.contains(&item.ty) {
                                item_tys.push(item.ty);
                            }
                        }
                        self.ctx.krate.types.intern(Type::Union(item_tys))
                    }
                } else {
                    self.ctx.krate.types.intern(Type::Unknown)
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::ListLit(items),
                    ty: self.ctx.krate.types.intern(Type::List(item_ty)),
                })
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.callback_expression(&parenthesized.expression, params, body)
            }
            Expression::TSAsExpression(as_expr) => {
                let mut expr = self.callback_expression(&as_expr.expression, params, body)?;
                expr.ty = self.ts_type_to_hir(&as_expr.type_annotation)?;
                Ok(expr)
            }
            Expression::TSTypeAssertion(assertion) => {
                let mut expr = self.callback_expression(&assertion.expression, params, body)?;
                expr.ty = self.ts_type_to_hir(&assertion.type_annotation)?;
                Ok(expr)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.callback_expression(&satisfies.expression, params, body)
            }
            Expression::TSNonNullExpression(non_null) => {
                let mut expr = self.callback_expression(&non_null.expression, params, body)?;
                if let Some(non_null_ty) = self.non_nullish_type(expr.ty) {
                    expr.ty = non_null_ty;
                }
                Ok(expr)
            }
            Expression::ObjectExpression(object) => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let mut entries = Vec::new();
                for property in &object.properties {
                    let ObjectPropertyKind::ObjectProperty(property) = property else {
                        if let ObjectPropertyKind::SpreadProperty(spread) = property {
                            drop(self.callback_expression(&spread.argument, params, body)?);
                            continue;
                        }
                        return Err(SmeltError::unsupported(
                            self.span(property.span().start, property.span().end),
                            "callback object literals only support plain properties",
                        ));
                    };
                    let key_text = match &property.key {
                        PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str(),
                        PropertyKey::StringLiteral(literal) => literal.value.as_str(),
                        _ => {
                            let value = self.callback_expression(&property.value, params, body)?;
                            entries.push((self.intern_exact_source_name("__computed"), value));
                            continue;
                        }
                    };
                    let value = self.callback_expression(&property.value, params, body)?;
                    entries.push((self.intern_exact_source_name(key_text), value));
                }
                Ok(CallbackExpr {
                    kind: CallbackExprKind::DictLit(entries),
                    ty: self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty)),
                })
            }
            Expression::NewExpression(new_expr) if matches!(
                &new_expr.callee,
                Expression::Identifier(callee)
                    if Self::is_ts_stdlib_class_name(
                        callee.name.as_str(),
                        smelt_stdlib::StdlibClass::RegExp
                    )
            ) =>
            {
                let Some(first) = new_expr.arguments.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "RegExp callback constructors require a pattern argument",
                    ));
                };
                let Some(pattern) = first.as_expression() else {
                    return Err(SmeltError::unsupported(
                        self.span(first.span().start, first.span().end),
                        "RegExp callback constructor pattern kind is not supported yet",
                    ));
                };
                let mut expr = self.callback_expression(pattern, params, body)?;
                let name = self.intern_type_name("RegExp");
                expr.ty = self.ctx.krate.types.intern(Type::Class {
                    name,
                    args: Vec::new(),
                });
                Ok(expr)
            }
            Expression::CallExpression(call) => {
                if let Expression::StaticMemberExpression(member) = &call.callee
                    && let Expression::Identifier(object) = &member.object
                    && object.name == "Object"
                    && matches!(member.property.name.as_str(), "keys" | "values" | "entries")
                {
                    let [argument] = call.arguments.as_slice() else {
                        return Err(SmeltError::unsupported(
                            self.span(call.span.start, call.span.end),
                            "Object callback projection calls require one argument",
                        ));
                    };
                    let Some(argument) = argument.as_expression() else {
                        return Err(SmeltError::unsupported(
                            self.span(argument.span().start, argument.span().end),
                            "Object callback projection argument kind is not supported yet",
                        ));
                    };
                    let value = self.callback_expression(argument, params, body)?;
                    let string_ty = self.ctx.krate.types.intern(Type::String);
                    let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                    let item_ty = match member.property.name.as_str() {
                        "keys" => string_ty,
                        "entries" => self
                            .ctx
                            .krate
                            .types
                            .intern(Type::Tuple(vec![string_ty, unknown_ty])),
                        _ => unknown_ty,
                    };
                    let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Call {
                            callee: Box::new(CallbackExpr {
                                kind: CallbackExprKind::Field {
                                    receiver: Box::new(value),
                                    field: self.intern_source_name(member.property.name.as_str()),
                                },
                                ty: self.ctx.krate.types.intern(Type::Unknown),
                            }),
                            args: Vec::new(),
                        },
                        ty,
                    });
                }
                if let Expression::StaticMemberExpression(member) = &call.callee
                    && let Expression::Identifier(object) = &member.object
                    && object.name == "Array"
                    && member.property.name == "isArray"
                {
                    let [argument] = call.arguments.as_slice() else {
                        return Err(SmeltError::unsupported(
                            self.span(call.span.start, call.span.end),
                            "Array.isArray callback calls require one argument",
                        ));
                    };
                    let Some(argument) = argument.as_expression() else {
                        return Err(SmeltError::unsupported(
                            self.span(argument.span().start, argument.span().end),
                            "Array.isArray callback argument kind is not supported yet",
                        ));
                    };
                    let value = self.callback_expression(argument, params, body)?;
                    let ty = self.ctx.krate.types.intern(Type::Bool);
                    if matches!(
                        self.ctx.krate.types.get(value.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) {
                        return Ok(CallbackExpr {
                            kind: CallbackExprKind::UnknownIs {
                                value: Box::new(value),
                                kind: UnknownKind::Array,
                            },
                            ty,
                        });
                    }
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Literal(Literal::Bool(matches!(
                            self.ctx.krate.types.get(value.ty),
                            Some(Type::List(_))
                        ))),
                        ty,
                    });
                }
                if let Expression::StaticMemberExpression(member) = &call.callee
                    && let Expression::Identifier(object) = &member.object
                    && object.name == "console"
                    && matches!(member.property.name.as_str(), "log" | "warn" | "error")
                {
                    let span = self.span(member.span.start, member.span.end);
                    let item = self.ensure_console_log_item(span);
                    let function_ty = self.item_expr_type(item, span)?;
                    let Item::Function(function) = self.item_ref(item) else {
                        return Err(SmeltError::unsupported(
                            span,
                            "console member calls must resolve to a function",
                        ));
                    };
                    let function_name = function.name;
                    let function_return_ty = function.return_ty;
                    let mut args = Vec::new();
                    for arg in &call.arguments {
                        let (expr, spread) = match arg {
                            Argument::SpreadElement(spread) => (
                                self.callback_expression(&spread.argument, params, body)?,
                                true,
                            ),
                            other => {
                                let Some(arg_expression) = other.as_expression() else {
                                    return Err(SmeltError::unsupported(
                                        self.span(other.span().start, other.span().end),
                                        "callback console argument kind is not supported yet",
                                    ));
                                };
                                (
                                    self.callback_expression(arg_expression, params, body)?,
                                    false,
                                )
                            }
                        };
                        args.push(CallbackCallArg { expr, spread });
                    }
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
                if let Expression::Identifier(callee) = &call.callee
                    && callee.name == "String"
                {
                    let Some(first_arg) = call.arguments.first() else {
                        return Err(SmeltError::unsupported(
                            self.span(call.span.start, call.span.end),
                            "String() callback conversion requires one argument",
                        ));
                    };
                    if call.arguments.len() != 1 {
                        return Err(SmeltError::unsupported(
                            self.span(call.span.start, call.span.end),
                            "String() callback conversion only supports one argument",
                        ));
                    }
                    let Some(argument) = first_arg.as_expression() else {
                        return Err(SmeltError::unsupported(
                            self.span(first_arg.span().start, first_arg.span().end),
                            "String() callback argument kind is not supported yet",
                        ));
                    };
                    let receiver = self.callback_expression(argument, params, body)?;
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::MethodCall {
                            receiver: Box::new(receiver),
                            method: self.intern_source_name("toString"),
                            args: Vec::new(),
                        },
                        ty: self.ctx.krate.types.intern(Type::String),
                    });
                }
                if let Some(expr) =
                    self.callback_regex_replace_uppercase_call(call, params, body)?
                {
                    return Ok(expr);
                }
                if let Expression::StaticMemberExpression(member) = &call.callee
                    && matches!(
                        member.property.name.as_str(),
                        "trim" | "trimStart" | "trimEnd"
                    )
                    && let Some(first_arg) = call.arguments.first()
                    && let Some(argument) = first_arg.as_expression()
                {
                    let receiver = self.callback_expression(argument, params, body)?;
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::MethodCall {
                            receiver: Box::new(receiver),
                            method: self.intern_source_name(member.property.name.as_str()),
                            args: Vec::new(),
                        },
                        ty: self.ctx.krate.types.intern(Type::String),
                    });
                }
                if let Expression::StaticMemberExpression(member) = &call.callee {
                    let receiver = self.callback_expression(&member.object, params, body)?;
                    if matches!(member.property.name.as_str(), "filter" | "sort")
                        && matches!(
                            self.ctx
                                .krate
                                .types
                                .get(self.type_param_constraint_or_self(receiver.ty)),
                            Some(Type::List(_) | Type::Tuple(_))
                        )
                    {
                        let method = self.intern_source_name(member.property.name.as_str());
                        let mut args = Vec::new();
                        for arg in &call.arguments {
                            let (expr, spread) = match arg {
                                Argument::SpreadElement(spread) => (
                                    self.callback_expression(&spread.argument, params, body)?,
                                    true,
                                ),
                                other => {
                                    let Some(arg_expression) = other.as_expression() else {
                                        return Err(SmeltError::unsupported(
                                            self.span(other.span().start, other.span().end),
                                            "callback array method argument kind is not supported yet",
                                        ));
                                    };
                                    (
                                        self.callback_expression(arg_expression, params, body)?,
                                        false,
                                    )
                                }
                            };
                            args.push(CallbackCallArg { expr, spread });
                        }
                        let return_ty = receiver.ty;
                        return Ok(CallbackExpr {
                            kind: CallbackExprKind::MethodCall {
                                receiver: Box::new(receiver),
                                method,
                                args,
                            },
                            ty: return_ty,
                        });
                    }
                    if matches!(member.property.name.as_str(), "map" | "flatMap")
                        && call
                            .arguments
                            .first()
                            .is_some_and(Self::argument_is_callback_like)
                    {
                        let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                        return Ok(CallbackExpr {
                            kind: CallbackExprKind::MethodCall {
                                receiver: Box::new(receiver),
                                method: self.intern_source_name(member.property.name.as_str()),
                                args: Vec::new(),
                            },
                            ty: self.ctx.krate.types.intern(Type::List(item_ty)),
                        });
                    }
                    let method = self.intern_source_name(member.property.name.as_str());
                    let declared_method_return = self
                        .class_field_type(receiver.ty, method)
                        .ok()
                        .and_then(|method_ty| match self.ctx.krate.types.get(method_ty) {
                            Some(Type::Function(function)) => Some(function.return_ty),
                            _ => None,
                        });
                    let return_ty = match member.property.name.as_str() {
                        "toString" => self.ctx.krate.types.intern(Type::String),
                        "match" => self.ctx.krate.types.intern(Type::Bool),
                        "has"
                            if matches!(
                                self.ctx.krate.types.get(receiver.ty),
                                Some(Type::Set(_))
                            ) =>
                        {
                            self.ctx.krate.types.intern(Type::Bool)
                        }
                        "getFullYear" | "getMonth" | "getDate" | "getHours" | "getMinutes"
                        | "getSeconds" | "getMilliseconds" | "getTime" => {
                            self.ctx.krate.types.intern(Type::Float)
                        }
                        _ => declared_method_return
                            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown)),
                    };
                    let mut args = Vec::new();
                    for arg in &call.arguments {
                        let (expr, spread) = match arg {
                            Argument::SpreadElement(spread) => (
                                self.callback_expression(&spread.argument, params, body)?,
                                true,
                            ),
                            other => {
                                let Some(arg_expression) = other.as_expression() else {
                                    return Err(SmeltError::unsupported(
                                        self.span(other.span().start, other.span().end),
                                        "callback method argument kind is not supported yet",
                                    ));
                                };
                                (
                                    self.callback_expression(arg_expression, params, body)?,
                                    false,
                                )
                            }
                        };
                        args.push(CallbackCallArg { expr, spread });
                    }
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::MethodCall {
                            receiver: Box::new(receiver),
                            method,
                            args,
                        },
                        ty: return_ty,
                    });
                }
                let callee = self.callback_expression(&call.callee, params, body)?;
                let return_ty = match self.ctx.krate.types.get(callee.ty) {
                    Some(Type::Function(function)) => function.return_ty,
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => {
                        self.ctx.krate.types.intern(Type::Unknown)
                    }
                    _ => self.ctx.krate.types.intern(Type::Unknown),
                };
                let mut args = Vec::new();
                for arg in &call.arguments {
                    let (expr, spread) = match arg {
                        Argument::SpreadElement(spread) => (
                            self.callback_expression(&spread.argument, params, body)?,
                            true,
                        ),
                        other => {
                            let Some(arg_expression) = other.as_expression() else {
                                return Err(SmeltError::unsupported(
                                    self.span(other.span().start, other.span().end),
                                    "callback call argument kind is not supported yet",
                                ));
                            };
                            (
                                self.callback_expression(arg_expression, params, body)?,
                                false,
                            )
                        }
                    };
                    args.push(CallbackCallArg { expr, spread });
                }
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Call {
                        callee: Box::new(callee),
                        args,
                    },
                    ty: return_ty,
                })
            }
            Expression::StaticMemberExpression(member) => {
                let receiver = self.callback_expression(&member.object, params, body)?;
                let field = self.intern_source_name(member.property.name.as_str());
                let ty = match self.ctx.krate.types.get(receiver.ty) {
                    Some(Type::Dict(_, value) | Type::JsMap(_, value)) => *value,
                    Some(Type::Optional(_)) => self.class_field_type(receiver.ty, field)?,
                    Some(Type::Class { .. }) => self.class_field_type(receiver.ty, field)?,
                    Some(Type::Unknown | Type::TypeParam { .. }) => {
                        self.ctx.krate.types.intern(Type::Unknown)
                    }
                    _ => self.ctx.krate.types.intern(Type::Unknown),
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Field {
                        receiver: Box::new(receiver),
                        field,
                    },
                    ty,
                })
            }
            Expression::ComputedMemberExpression(member) => {
                if let Expression::Identifier(receiver_ident) = &member.object
                    && let Some(namespace) = self
                        .object_namespaces
                        .get(receiver_ident.name.as_str())
                        .cloned()
                {
                    let key = self.callback_expression(&member.expression, params, body)?;
                    if self.ctx.krate.types.get(key.ty) != Some(&Type::String) {
                        return Err(SmeltError::unsupported(
                            self.span(member.expression.span().start, member.expression.span().end),
                            "callback function-table lookup key must be a string",
                        ));
                    }
                    let mut cases = Vec::new();
                    let mut function_ty = None;
                    for (key_text, item) in namespace {
                        let span = self.span(member.span.start, member.span.end);
                        let ty = self.item_expr_type(item, span)?;
                        let Item::Function(function) = self.item_ref(item) else {
                            return Err(SmeltError::unsupported(
                                span,
                                "callback function-table values must resolve to functions",
                            ));
                        };
                        if let Some(existing) = function_ty {
                            if existing != ty {
                                return Err(SmeltError::unsupported(
                                    span,
                                    "callback function-table entries must share one callable type",
                                ));
                            }
                        } else {
                            function_ty = Some(ty);
                        }
                        cases.push((key_text, function.name));
                    }
                    let ty = function_ty.ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(member.span.start, member.span.end),
                            "callback function-table lookup requires at least one entry",
                        )
                    })?;
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::FunctionTableLookup {
                            key: Box::new(key),
                            cases,
                        },
                        ty,
                    });
                }
                let receiver = self.callback_expression(&member.object, params, body)?;
                if let Expression::NumericLiteral(index) = &member.expression {
                    if index.value.fract() != 0.0 || index.value < 0.0 {
                        return Err(SmeltError::unsupported(
                            self.span(index.span.start, index.span.end),
                            "callback computed access index must be a non-negative integer",
                        ));
                    }
                    let index_usize = index.value.to_string().parse::<usize>().map_err(|err| {
                        SmeltError::unsupported(
                            self.span(index.span.start, index.span.end),
                            format!("callback computed access index is invalid: {err}"),
                        )
                    })?;
                    let item_ty = match self
                        .ctx
                        .krate
                        .types
                        .get(self.type_param_constraint_or_self(receiver.ty))
                    {
                        Some(Type::Tuple(items)) => {
                            items.get(index_usize).copied().ok_or_else(|| {
                                SmeltError::unsupported(
                                    self.span(member.span.start, member.span.end),
                                    "callback tuple index is out of bounds",
                                )
                            })?
                        }
                        Some(Type::List(item_ty)) => *item_ty,
                        Some(Type::String) => self.ctx.krate.types.intern(Type::String),
                        Some(Type::Unknown | Type::TypeParam { .. }) => {
                            self.ctx.krate.types.intern(Type::Unknown)
                        }
                        _ => {
                            return Err(SmeltError::unsupported(
                                self.span(member.span.start, member.span.end),
                                "callback computed access receiver must be a tuple, array, or string",
                            ));
                        }
                    };
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Index {
                            receiver: Box::new(receiver),
                            index: index_usize,
                        },
                        ty: item_ty,
                    });
                }
                let mut index = self.callback_expression(&member.expression, params, body)?;
                // A nullish-guarded receiver (`obj: T | null | undefined`)
                // keeps its Optional wrapper through erased narrowing; the
                // computed access itself operates on the payload type.
                let receiver_ty = self.optional_receiver_inner_type(
                    self.type_param_constraint_or_self(receiver.ty),
                );
                let index_ty = self.type_param_constraint_or_self(index.ty);
                let numeric_index = matches!(
                    self.ctx.krate.types.get(index_ty),
                    Some(Type::Float | Type::Int | Type::Unknown)
                );
                let string_key_index = (self.ctx.krate.types.get(index_ty) == Some(&Type::String)
                    || self.erased_or_union_surface(index_ty))
                    && matches!(
                        self.ctx.krate.types.get(receiver_ty),
                        Some(
                            Type::Dict(_, _)
                                | Type::Class { .. }
                                | Type::Unknown
                                | Type::TypeParam { .. }
                        )
                    );
                // An erased/union index over an array-like receiver (e.g.
                // `args[i]` where `i` flows from a flattened `Many<number>`)
                // is a genuine dynamic boundary: retype it `unknown` so the
                // dynamic-index coercion handles it at runtime.
                let erased_numeric_index = !numeric_index
                    && !string_key_index
                    && (self.erased_or_union_surface(index_ty)
                        || matches!(self.ctx.krate.types.get(index_ty), Some(Type::Union(_))))
                    && matches!(
                        self.ctx.krate.types.get(receiver_ty),
                        Some(Type::List(_) | Type::Tuple(_) | Type::String)
                    );
                if erased_numeric_index {
                    index.ty = self.ctx.krate.types.intern(Type::Unknown);
                }
                if !numeric_index && !string_key_index && !erased_numeric_index {
                    return Err(SmeltError::unsupported(
                        self.span(member.expression.span().start, member.expression.span().end),
                        "callback dynamic computed access index must be numeric or a string record key",
                    ));
                }
                let item_ty = match self.ctx.krate.types.get(receiver_ty) {
                    Some(Type::List(item_ty)) => *item_ty,
                    Some(Type::String) => self.ctx.krate.types.intern(Type::String),
                    Some(Type::Dict(_, value_ty) | Type::JsMap(_, value_ty)) => *value_ty,
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => {
                        self.ctx.krate.types.intern(Type::Unknown)
                    }
                    Some(Type::Union(items))
                        if items.iter().any(|item| {
                            matches!(
                                self.ctx.krate.types.get(*item),
                                Some(
                                    Type::List(_)
                                        | Type::Dict(_, _)
                                        | Type::Unknown
                                        | Type::TypeParam { .. }
                                )
                            )
                        }) =>
                    {
                        self.ctx.krate.types.intern(Type::Unknown)
                    }
                    _ => self.ctx.krate.types.intern(Type::Unknown),
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::DynamicIndex {
                        receiver: Box::new(receiver),
                        index: Box::new(index),
                    },
                    ty: item_ty,
                })
            }
            Expression::ConditionalExpression(conditional) => {
                let cond = self.callback_truthy_expression(&conditional.test, params, body)?;
                let then_params =
                    self.callback_params_with_guard_narrowing(params, &conditional.test);
                let then_expr =
                    self.callback_expression(&conditional.consequent, &then_params, body)?;
                let else_expr = self.callback_expression(&conditional.alternate, params, body)?;
                let (then_expr, else_expr, ty) = self.callback_unify_conditional_exprs(
                    then_expr,
                    else_expr,
                    conditional.span.start,
                    conditional.span.end,
                )?;
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Conditional {
                        cond: Box::new(cond),
                        then_expr: Box::new(then_expr),
                        else_expr: Box::new(else_expr),
                    },
                    ty,
                })
            }
            Expression::AssignmentExpression(assign) => {
                let AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left else {
                    if matches!(
                        &assign.left,
                        AssignmentTarget::StaticMemberExpression(_)
                            | AssignmentTarget::ComputedMemberExpression(_)
                    ) {
                        // A member-target store (`obj[k] = v` / `obj.k = v`, including
                        // compound forms like `args[1] += ''`) mutates the receiver, but
                        // the side-effect-free callback expression IR cannot represent
                        // the store — only its right-hand value. Bail so the caller
                        // re-lowers this arrow through full closure-body lowering, which
                        // keeps the mutation. (Previously the store was silently dropped,
                        // leaving mutating reducers like `(acc, x) => { acc[x] = x;
                        // return acc; }` as identity functions.)
                        return Err(SmeltError::unsupported(
                            self.span(assign.span.start, assign.span.end),
                            "callback member assignment needs closure-body lowering",
                        ));
                    }
                    return Err(SmeltError::unsupported(
                        self.span(assign.span.start, assign.span.end),
                        "callback assignment targets must be captured locals",
                    ));
                };
                if params.contains_key(target.name.as_str()) {
                    return Err(SmeltError::unsupported(
                        self.span(target.span.start, target.span.end),
                        "callback parameter assignment is not supported yet",
                    ));
                }
                let Some(local) = self.locals.get(target.name.as_str()).copied() else {
                    return Err(SmeltError::for_unresolved_name(
                        self.span(target.span.start, target.span.end),
                        target.name.as_str(),
                        format!("unresolved callback assignment target `{}`", target.name),
                    ));
                };
                let local_index = usize::try_from(local.0).map_err(|err| {
                    SmeltError::unsupported(
                        self.span(target.span.start, target.span.end),
                        format!("callback assignment target index is invalid: {err}"),
                    )
                })?;
                let local_decl = body.locals.get(local_index).ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(target.span.start, target.span.end),
                        "callback assignment target does not resolve to a local",
                    )
                })?;
                if !local_decl.mutable {
                    return Err(SmeltError::unsupported(
                        self.span(target.span.start, target.span.end),
                        "callback assignment to captured const local is not supported",
                    ));
                }
                let right = self.callback_expression(&assign.right, params, body)?;
                let value = match assign.operator {
                    AssignmentOperator::Assign => right,
                    AssignmentOperator::Addition
                    | AssignmentOperator::Subtraction
                    | AssignmentOperator::Multiplication
                    | AssignmentOperator::Division => {
                        let op = match assign.operator {
                            AssignmentOperator::Addition => BinOp::Add,
                            AssignmentOperator::Subtraction => BinOp::Sub,
                            AssignmentOperator::Multiplication => BinOp::Mul,
                            AssignmentOperator::Division => BinOp::Div,
                            other => {
                                return Err(SmeltError::unsupported(
                                    self.span(assign.span.start, assign.span.end),
                                    format!(
                                        "callback assignment operator is not supported yet: {other:?}"
                                    ),
                                ));
                            }
                        };
                        CallbackExpr {
                            kind: CallbackExprKind::Binary {
                                op,
                                lhs: Box::new(CallbackExpr {
                                    kind: CallbackExprKind::Capture(local),
                                    ty: local_decl.ty,
                                }),
                                rhs: Box::new(right),
                            },
                            ty: local_decl.ty,
                        }
                    }
                    other => {
                        return Err(SmeltError::unsupported(
                            self.span(assign.span.start, assign.span.end),
                            format!("callback assignment operator is not supported yet: {other:?}"),
                        ));
                    }
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::AssignCapture {
                        target: local,
                        value: Box::new(value),
                    },
                    ty: local_decl.ty,
                })
            }
            Expression::UnaryExpression(unary) => {
                if unary.operator == UnaryOperator::Typeof {
                    return self.callback_typeof_unary(unary, params, body);
                }
                let op = match unary.operator {
                    UnaryOperator::LogicalNot => UnaryOp::Not,
                    UnaryOperator::UnaryNegation => UnaryOp::Neg,
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(unary.span.start, unary.span.end),
                            format!(
                                "callback unary operator is not supported yet: {:?}",
                                unary.operator
                            ),
                        ));
                    }
                };
                let operand = if matches!(op, UnaryOp::Not) {
                    self.callback_truthy_expression(&unary.argument, params, body)?
                } else {
                    self.callback_expression(&unary.argument, params, body)?
                };
                let ty = if matches!(op, UnaryOp::Not) {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    operand.ty
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Unary {
                        op,
                        operand: Box::new(operand),
                    },
                    ty,
                })
            }
            Expression::BinaryExpression(binary) => {
                if let Some(expr) = self.callback_nullish_binary(binary, params, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.callback_typeof_binary(binary, params, body)? {
                    return Ok(expr);
                }
                if binary.operator == BinaryOperator::Instanceof {
                    let value = self.callback_expression(&binary.left, params, body)?;
                    let ty = self.ctx.krate.types.intern(Type::Bool);
                    let Expression::Identifier(target) = &binary.right else {
                        let _target = self.callback_expression(&binary.right, params, body)?;
                        if self.ctx.krate.types.get(value.ty) == Some(&Type::Unknown) {
                            return Ok(CallbackExpr {
                                kind: CallbackExprKind::UnknownIs {
                                    value: Box::new(value),
                                    kind: UnknownKind::Object,
                                },
                                ty,
                            });
                        }
                        return Ok(CallbackExpr {
                            kind: CallbackExprKind::Literal(Literal::Bool(
                                self.instanceof_concrete_class(value.ty),
                            )),
                            ty,
                        });
                    };
                    if self.ctx.krate.types.get(value.ty) == Some(&Type::Unknown) {
                        let kind = match target.name.as_str() {
                            "Array" => UnknownKind::Array,
                            "Function" => UnknownKind::Function,
                            "String" => UnknownKind::String,
                            "Number" => UnknownKind::Number,
                            "Promise" => UnknownKind::Promise,
                            _ => UnknownKind::Object,
                        };
                        return Ok(CallbackExpr {
                            kind: CallbackExprKind::UnknownIs {
                                value: Box::new(value),
                                kind,
                            },
                            ty,
                        });
                    }
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Literal(Literal::Bool(
                            Self::instanceof_builtin_target(target.name.as_str())
                                || self.instanceof_concrete_class(value.ty),
                        )),
                        ty,
                    });
                }
                if binary.operator == BinaryOperator::In {
                    if let Expression::Identifier(receiver_ident) = &binary.right
                        && let Some(namespace) =
                            self.object_namespaces.get(receiver_ident.name.as_str())
                    {
                        let case_keys = namespace.keys().cloned().collect::<Vec<_>>();
                        let key = self.callback_expression(&binary.left, params, body)?;
                        return self.callback_function_table_has_key(
                            &key,
                            &case_keys,
                            self.span(binary.span.start, binary.span.end),
                        );
                    }
                    if let Expression::Identifier(receiver_ident) = &binary.right
                        && let Some(object_const) = self
                            .const_objects
                            .get(receiver_ident.name.as_str())
                            .cloned()
                    {
                        let case_keys = object_const
                            .entries
                            .iter()
                            .map(|entry| entry.key.clone())
                            .collect::<Vec<_>>();
                        let key = self.callback_expression(&binary.left, params, body)?;
                        return self.callback_function_table_has_key(
                            &key,
                            &case_keys,
                            self.span(binary.span.start, binary.span.end),
                        );
                    }
                    let receiver = self.callback_expression(&binary.right, params, body)?;
                    if let Expression::StringLiteral(field) = &binary.left {
                        return Ok(CallbackExpr {
                            kind: CallbackExprKind::HasField {
                                receiver: Box::new(receiver),
                                field: self.ctx.krate.symbols.intern(field.value.as_str()),
                            },
                            ty: self.ctx.krate.types.intern(Type::Bool),
                        });
                    }
                    let mut field = self.callback_expression(&binary.left, params, body)?;
                    if self.ctx.krate.types.get(field.ty) != Some(&Type::String) {
                        field = CallbackExpr {
                            kind: CallbackExprKind::Call {
                                callee: Box::new(CallbackExpr {
                                    kind: CallbackExprKind::Literal(Literal::String(
                                        "String".to_owned(),
                                    )),
                                    ty: self.ctx.krate.types.intern(Type::Unknown),
                                }),
                                args: vec![CallbackCallArg {
                                    expr: field,
                                    spread: false,
                                }],
                            },
                            ty: self.ctx.krate.types.intern(Type::String),
                        };
                    }
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::HasDynamicField {
                            receiver: Box::new(receiver),
                            field: Box::new(field),
                        },
                        ty: self.ctx.krate.types.intern(Type::Bool),
                    });
                }
                let op =
                    self.callback_binary_op(binary.operator, binary.span.start, binary.span.end)?;
                let lhs = self.callback_expression(&binary.left, params, body)?;
                let rhs = self.callback_expression(&binary.right, params, body)?;
                let ty = self.binary_result_type(op, lhs.ty, rhs.ty);
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty,
                })
            }
            Expression::LogicalExpression(logical) => {
                if logical.operator == LogicalOperator::Coalesce {
                    let lhs = self.callback_expression(&logical.left, params, body)?;
                    let rhs = self.callback_expression(&logical.right, params, body)?;
                    let none_ty = self.ctx.krate.types.intern(Type::None);
                    let cond_ty = self.ctx.krate.types.intern(Type::Bool);
                    let cond = CallbackExpr {
                        kind: CallbackExprKind::Binary {
                            op: BinOp::NotEq,
                            lhs: Box::new(lhs.clone()),
                            rhs: Box::new(CallbackExpr {
                                kind: CallbackExprKind::Literal(Literal::None),
                                ty: none_ty,
                            }),
                        },
                        ty: cond_ty,
                    };
                    let (lhs, rhs, ty) = self.callback_unify_conditional_exprs(
                        lhs,
                        rhs,
                        logical.span.start,
                        logical.span.end,
                    )?;
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Conditional {
                            cond: Box::new(cond),
                            then_expr: Box::new(lhs),
                            else_expr: Box::new(rhs),
                        },
                        ty,
                    });
                }
                if logical.operator == LogicalOperator::And {
                    let rhs = self.callback_expression(&logical.right, params, body)?;
                    if self.is_numeric_like_type(rhs.ty) {
                        let cond = self.callback_truthy_expression(&logical.left, params, body)?;
                        let zero = CallbackExpr {
                            kind: match self.ctx.krate.types.get(rhs.ty) {
                                Some(Type::Int) => CallbackExprKind::Literal(Literal::Int(0)),
                                _ => CallbackExprKind::Literal(Literal::Float(0.0)),
                            },
                            ty: rhs.ty,
                        };
                        return Ok(CallbackExpr {
                            kind: CallbackExprKind::Conditional {
                                cond: Box::new(cond),
                                then_expr: Box::new(rhs.clone()),
                                else_expr: Box::new(zero),
                            },
                            ty: rhs.ty,
                        });
                    }
                }
                if logical.operator == LogicalOperator::Or {
                    let lhs = self.callback_expression(&logical.left, params, body)?;
                    let lhs_ty = lhs.ty;
                    if self.is_numeric_like_type(lhs_ty) {
                        let rhs = self.callback_expression(&logical.right, params, body)?;
                        if self.numeric_type_compatible(lhs_ty, rhs.ty) {
                            let zero = CallbackExpr {
                                kind: match self.ctx.krate.types.get(lhs_ty) {
                                    Some(Type::Int) => CallbackExprKind::Literal(Literal::Int(0)),
                                    _ => CallbackExprKind::Literal(Literal::Float(0.0)),
                                },
                                ty: lhs_ty,
                            };
                            let cond = CallbackExpr {
                                kind: CallbackExprKind::Binary {
                                    op: BinOp::NotEq,
                                    lhs: Box::new(lhs.clone()),
                                    rhs: Box::new(zero),
                                },
                                ty: self.ctx.krate.types.intern(Type::Bool),
                            };
                            return Ok(CallbackExpr {
                                kind: CallbackExprKind::Conditional {
                                    cond: Box::new(cond),
                                    then_expr: Box::new(lhs),
                                    else_expr: Box::new(rhs),
                                },
                                ty: lhs_ty,
                            });
                        }
                    }
                }
                let lhs = self.callback_expression(&logical.left, params, body)?;
                let rhs = self.callback_expression(&logical.right, params, body)?;
                let bool_ty = self.ctx.krate.types.intern(Type::Bool);
                // A value-producing `a || b` (`record[k] || "'"`) selects one
                // of its operands, not a boolean: lower it as a truthiness
                // conditional like the Coalesce branch above. Boolean-typed
                // operands keep the plain binary form used by predicates.
                if logical.operator == LogicalOperator::Or
                    && (lhs.ty != bool_ty || rhs.ty != bool_ty)
                {
                    let span = self.span(logical.span.start, logical.span.end);
                    let cond = self.coerce_callback_expr_to_truthy(lhs.clone(), span)?;
                    let (lhs, rhs, ty) = self.callback_unify_conditional_exprs(
                        lhs,
                        rhs,
                        logical.span.start,
                        logical.span.end,
                    )?;
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Conditional {
                            cond: Box::new(cond),
                            then_expr: Box::new(lhs),
                            else_expr: Box::new(rhs),
                        },
                        ty,
                    });
                }
                let op = match logical.operator {
                    LogicalOperator::And => BinOp::And,
                    LogicalOperator::Or => BinOp::Or,
                    LogicalOperator::Coalesce => BinOp::Or,
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty: bool_ty,
                })
            }
            Expression::TemplateLiteral(template) => {
                self.callback_template_literal(template, params, body)
            }
            _ => Err(SmeltError::unsupported(
                self.expression_span(expression),
                "callback expression kind is not supported yet",
            )),
        }
    }
}
