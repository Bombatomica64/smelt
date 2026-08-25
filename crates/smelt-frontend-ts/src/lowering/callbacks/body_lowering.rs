//! Callback body lowering: block/terminating/side-effect statements, guard
//! narrowing, param-pattern binding and inference, arrow closure bodies,
//! throw analysis, and capture-name collection over expressions/statements.

use crate::lowering::{
    Argument, ArrayExpressionElement, AssignmentTarget, BinOp, BinaryOperator, BindingPattern,
    Body, CallbackExpr, CallbackExprKind, CaptureMode, ChainElement, ClosureCapture, Expr,
    ExprKind, Expression, ForStatementInit, ForStatementLeft, FunctionType, HashMap, HashSet,
    Literal, LocalDecl, LogicalOperator, ModuleBuilder, ObjectPropertyKind, Param, PropertyKey,
    SimpleAssignmentTarget, SmeltError, Span, Statement, Stmt, Type, UnknownKind,
};
use oxc::span::GetSpan;

/// The body form of a callback lowered through a real HIR closure body.
///
/// Selects how [`ModuleBuilder::closure_body_expr_from_parts`] lowers the
/// callback body: an arrow expression body (`x => x + 1`) whose single
/// expression becomes the closure's return, or a statement block (`{ ... }`)
/// shared by block-bodied arrows and `function` expression callbacks.
///
/// The variants hold only borrowed AST references, so the enum is `Copy` and is
/// passed by value.
#[derive(Clone, Copy)]
enum ClosureBodyKind<'a, 'src> {
    /// An expression-bodied arrow; its return expression is lowered with a
    /// contextual type hint and may drive return-type inference.
    ArrowExpression(&'a oxc::ast::ast::ArrowFunctionExpression<'src>),
    /// A statement block, used by block-bodied arrows and function expressions.
    Statements(&'a [Statement<'src>]),
}

impl ModuleBuilder<'_> {
    /// Lower a terminating block-bodied callback into a nested callback expression.
    pub(in crate::lowering) fn callback_block_expression<'a>(
        &mut self,
        statements: &'a [Statement<'a>],
        params: &mut HashMap<&'a str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        let Some((first, rest)) = statements.split_first() else {
            return Err(SmeltError::unsupported(
                self.span(0, 0),
                "block-bodied callbacks must terminate with return or throw",
            ));
        };
        match first {
            Statement::VariableDeclaration(declaration) => {
                if declaration.declarations.len() != 1 {
                    return Err(SmeltError::unsupported(
                        self.span(declaration.span.start, declaration.span.end),
                        "callback block declarations must declare one local",
                    ));
                }
                let Some(declarator) = declaration.declarations.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(declaration.span.start, declaration.span.end),
                        "callback block declarations must declare one local",
                    ));
                };
                let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                    return Err(SmeltError::unsupported(
                        self.span(declarator.span.start, declarator.span.end),
                        "callback block declarations require simple bindings",
                    ));
                };
                let Some(init) = &declarator.init else {
                    return Err(SmeltError::unsupported(
                        self.span(declarator.span.start, declarator.span.end),
                        "callback block declarations require initializers",
                    ));
                };
                let value = self.callback_expression(init, params, body)?;
                let prior = params.insert(binding.name.as_str(), value);
                let result = self.callback_block_expression(rest, params, body);
                if let Some(prior) = prior {
                    params.insert(binding.name.as_str(), prior);
                } else {
                    params.remove(binding.name.as_str());
                }
                result
            }
            Statement::IfStatement(if_stmt) => {
                // A callback `if/else` where both arms terminate with a value
                // (`if (c) { return a; } else { return b; }`) is a direct
                // conditional expression: lower each arm as a terminating
                // statement and reconcile the two branch types the same way a
                // ternary does. When an arm does not terminate — e.g. it mutates
                // a captured local or the callback parameter and then falls
                // through to shared trailing statements (`if (c) { value = x; }
                // else if (d) { value = y; } ... return value;`) — the compact
                // side-effect-free callback IR cannot model the assignment, so
                // surface a fallback-eligible error that retries the whole arrow
                // through full closure-body lowering (which makes parameters
                // mutable locals and lowers `if/else if` chains natively).
                if let Some(alternate) = &if_stmt.alternate {
                    // The direct-conditional form requires both arms to be the
                    // final statement (nothing after the `if/else`) and to
                    // terminate with a value. Any other shape — trailing
                    // statements after the `if/else`, or an arm that mutates a
                    // local/parameter instead of returning — cannot be modeled by
                    // the compact IR, so surface the fallback-eligible error
                    // (`should_fallback_to_closure_body_for_callback`) that
                    // retries the whole arrow through full closure-body lowering.
                    let fallback_error = SmeltError::unsupported(
                        self.span(if_stmt.span.start, if_stmt.span.end),
                        "callback if/else blocks need direct conditional expression lowering",
                    );
                    if !rest.is_empty() {
                        return Err(fallback_error);
                    }
                    let cond = self.callback_truthy_expression(&if_stmt.test, params, body)?;
                    let mut then_params =
                        self.callback_params_with_guard_narrowing(params, &if_stmt.test);
                    let Ok(then_expr) = self.callback_terminating_statement(
                        &if_stmt.consequent,
                        &mut then_params,
                        body,
                    ) else {
                        return Err(fallback_error);
                    };
                    let mut else_params = params.clone();
                    let Ok(else_expr) =
                        self.callback_terminating_statement(alternate, &mut else_params, body)
                    else {
                        return Err(fallback_error);
                    };
                    let (then_expr, else_expr, ty) = self.callback_unify_conditional_exprs(
                        then_expr,
                        else_expr,
                        if_stmt.span.start,
                        if_stmt.span.end,
                    )?;
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Conditional {
                            cond: Box::new(cond),
                            then_expr: Box::new(then_expr),
                            else_expr: Box::new(else_expr),
                        },
                        ty,
                    });
                }
                let cond = self.callback_truthy_expression(&if_stmt.test, params, body)?;
                let then_expr =
                    match self.callback_terminating_statement(&if_stmt.consequent, params, body) {
                        Ok(expr) => expr,
                        Err(error) if !rest.is_empty() => {
                            drop(error);
                            let side_effect = self.callback_side_effect_statement(
                                &if_stmt.consequent,
                                params,
                                body,
                            )?;
                            // The guarded side effect is modeled as a ternary
                            // (`cond ? side_effect : none`), and the compact IR
                            // lowers each ternary arm as a pure sub-expression.
                            // An arm that assigns a captured local
                            // (`if (!called) { ret = fn(); called = true; }`)
                            // cannot live inside a ternary: `AssignCapture` emits
                            // its assignment as a statement, which would hoist out
                            // of the guard and run unconditionally. Surface the
                            // fallback-eligible error so the whole arrow retries
                            // through full closure-body lowering, which lowers the
                            // `if` natively with a real branch.
                            if Self::callback_expr_contains_capture_assignment(&side_effect) {
                                return Err(SmeltError::unsupported(
                                    self.span(if_stmt.span.start, if_stmt.span.end),
                                    "callback if guard mutates a captured local; needs closure-body lowering",
                                ));
                            }
                            let none_ty = self.ctx.krate.types.intern(Type::None);
                            let none_expr = CallbackExpr {
                                kind: CallbackExprKind::Literal(Literal::None),
                                ty: none_ty,
                            };
                            let guarded_effect = CallbackExpr {
                                kind: CallbackExprKind::Conditional {
                                    cond: Box::new(cond),
                                    then_expr: Box::new(side_effect),
                                    else_expr: Box::new(none_expr),
                                },
                                ty: none_ty,
                            };
                            let result = self.callback_block_expression(rest, params, body)?;
                            let result_ty = result.ty;
                            return Ok(CallbackExpr {
                                kind: CallbackExprKind::Sequence {
                                    effects: vec![guarded_effect],
                                    result: Box::new(result),
                                },
                                ty: result_ty,
                            });
                        }
                        Err(error) => return Err(error),
                    };
                let else_expr = self.callback_block_expression(rest, params, body)?;
                let (then_expr, else_expr, ty) = self.callback_unify_conditional_exprs(
                    then_expr,
                    else_expr,
                    if_stmt.span.start,
                    if_stmt.span.end,
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
            Statement::ReturnStatement(return_stmt) => {
                if !rest.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(return_stmt.span.start, return_stmt.span.end),
                        "callback statements after return are not supported",
                    ));
                }
                let value = return_stmt.argument.as_ref().ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(return_stmt.span.start, return_stmt.span.end),
                        "callback return statements must return a value",
                    )
                })?;
                self.callback_expression(value, params, body)
            }
            Statement::ThrowStatement(throw_stmt) => {
                if !rest.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(throw_stmt.span.start, throw_stmt.span.end),
                        "callback statements after throw are not supported",
                    ));
                }
                self.callback_throw_expression(Some(&throw_stmt.argument), params, body)
            }
            Statement::ExpressionStatement(expr_stmt) => {
                if rest.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(expr_stmt.span.start, expr_stmt.span.end),
                        "callback expression statements must be followed by a return or throw",
                    ));
                }
                let effect = self.callback_expression(&expr_stmt.expression, params, body)?;
                let result = self.callback_block_expression(rest, params, body)?;
                let result_ty = result.ty;
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Sequence {
                        effects: vec![effect],
                        result: Box::new(result),
                    },
                    ty: result_ty,
                })
            }
            _ => Err(SmeltError::unsupported(
                self.span(first.span().start, first.span().end),
                "callback block statements must be const declarations, if guards, return, or throw",
            )),
        }
    }

    /// Lower a callback statement that is evaluated only for side effects.
    pub(in crate::lowering) fn callback_side_effect_statement<'a>(
        &mut self,
        side_effect_statement: &'a Statement<'a>,
        params: &HashMap<&'a str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        let none_ty = self.ctx.krate.types.intern(Type::None);
        match side_effect_statement {
            Statement::ExpressionStatement(expr_stmt) => {
                self.callback_expression(&expr_stmt.expression, params, body)
            }
            Statement::BlockStatement(block) => {
                let mut effects = Vec::new();
                for block_statement in &block.body {
                    let effect = match block_statement {
                        Statement::ExpressionStatement(expr_stmt) => {
                            self.callback_expression(&expr_stmt.expression, params, body)?
                        }
                        Statement::ThrowStatement(throw_stmt) => self.callback_throw_expression(
                            Some(&throw_stmt.argument),
                            params,
                            body,
                        )?,
                        _ => {
                            return Err(SmeltError::unsupported(
                                self.span(block_statement.span().start, block_statement.span().end),
                                "callback side-effect blocks only support expression and throw statements",
                            ));
                        }
                    };
                    effects.push(effect);
                }
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Sequence {
                        effects,
                        result: Box::new(CallbackExpr {
                            kind: CallbackExprKind::Literal(Literal::None),
                            ty: none_ty,
                        }),
                    },
                    ty: none_ty,
                })
            }
            _ => Err(SmeltError::unsupported(
                self.span(
                    side_effect_statement.span().start,
                    side_effect_statement.span().end,
                ),
                "callback side-effect statement kind is not supported yet",
            )),
        }
    }

    /// Report whether a compact callback expression tree assigns a captured
    /// local anywhere (`AssignCapture`). Such assignments cannot survive inside
    /// a ternary arm of the compact IR — they are emitted as statements and
    /// would hoist out of their guard — so a guarded side effect containing one
    /// must fall back to full closure-body lowering.
    pub(in crate::lowering) fn callback_expr_contains_capture_assignment(
        callback: &CallbackExpr,
    ) -> bool {
        match &callback.kind {
            CallbackExprKind::AssignCapture { .. } => true,
            CallbackExprKind::Sequence { effects, result } => {
                effects
                    .iter()
                    .any(Self::callback_expr_contains_capture_assignment)
                    || Self::callback_expr_contains_capture_assignment(result)
            }
            CallbackExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                Self::callback_expr_contains_capture_assignment(cond)
                    || Self::callback_expr_contains_capture_assignment(then_expr)
                    || Self::callback_expr_contains_capture_assignment(else_expr)
            }
            CallbackExprKind::ListLit(items) => items
                .iter()
                .any(Self::callback_expr_contains_capture_assignment),
            CallbackExprKind::DictLit(entries) => entries
                .iter()
                .any(|(_, value)| Self::callback_expr_contains_capture_assignment(value)),
            CallbackExprKind::Throw { message } => message
                .as_ref()
                .is_some_and(|message| Self::callback_expr_contains_capture_assignment(message)),
            CallbackExprKind::Index { receiver, .. }
            | CallbackExprKind::Field { receiver, .. }
            | CallbackExprKind::HasField { receiver, .. }
            | CallbackExprKind::FieldTruthy { receiver, .. }
            | CallbackExprKind::UnknownIs {
                value: receiver, ..
            }
            | CallbackExprKind::TypeofValue { value: receiver } => {
                Self::callback_expr_contains_capture_assignment(receiver)
            }
            CallbackExprKind::DynamicIndex { receiver, index } => {
                Self::callback_expr_contains_capture_assignment(receiver)
                    || Self::callback_expr_contains_capture_assignment(index)
            }
            CallbackExprKind::HasDynamicField { receiver, field } => {
                Self::callback_expr_contains_capture_assignment(receiver)
                    || Self::callback_expr_contains_capture_assignment(field)
            }
            CallbackExprKind::Unary { operand, .. } => {
                Self::callback_expr_contains_capture_assignment(operand)
            }
            CallbackExprKind::Binary { lhs, rhs, .. } => {
                Self::callback_expr_contains_capture_assignment(lhs)
                    || Self::callback_expr_contains_capture_assignment(rhs)
            }
            CallbackExprKind::Call { callee, args } => {
                Self::callback_expr_contains_capture_assignment(callee)
                    || args
                        .iter()
                        .any(|arg| Self::callback_expr_contains_capture_assignment(&arg.expr))
            }
            CallbackExprKind::MethodCall { receiver, args, .. } => {
                Self::callback_expr_contains_capture_assignment(receiver)
                    || args
                        .iter()
                        .any(|arg| Self::callback_expr_contains_capture_assignment(&arg.expr))
            }
            CallbackExprKind::FunctionTableLookup { key, .. } => {
                Self::callback_expr_contains_capture_assignment(key)
            }
            CallbackExprKind::Capture(_)
            | CallbackExprKind::Param(_)
            | CallbackExprKind::Function(_)
            | CallbackExprKind::Literal(_) => false,
        }
    }

    /// Lower a callback statement that must terminate the current branch.
    pub(in crate::lowering) fn callback_terminating_statement<'a>(
        &mut self,
        statement: &'a Statement<'a>,
        params: &mut HashMap<&'a str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        match statement {
            Statement::BlockStatement(block) => {
                self.callback_block_expression(&block.body, params, body)
            }
            Statement::ReturnStatement(return_stmt) => {
                let value = return_stmt.argument.as_ref().ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(return_stmt.span.start, return_stmt.span.end),
                        "callback return statements must return a value",
                    )
                })?;
                self.callback_expression(value, params, body)
            }
            Statement::ThrowStatement(throw_stmt) => {
                self.callback_throw_expression(Some(&throw_stmt.argument), params, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(statement.span().start, statement.span().end),
                "callback branch must terminate with return or throw",
            )),
        }
    }

    /// Return callback parameters narrowed by facts proven in a true branch guard.
    pub(in crate::lowering) fn callback_params_with_guard_narrowing<'a>(
        &mut self,
        params: &HashMap<&'a str, CallbackExpr>,
        expression: &Expression<'_>,
    ) -> HashMap<&'a str, CallbackExpr> {
        let mut narrowed = params.clone();
        self.apply_callback_guard_narrowing(&mut narrowed, expression);
        narrowed
    }

    /// Apply simple `value !== undefined` callback type facts to a parameter map.
    pub(in crate::lowering) fn apply_callback_guard_narrowing(
        &mut self,
        params: &mut HashMap<&str, CallbackExpr>,
        expression: &Expression<'_>,
    ) {
        if let Expression::LogicalExpression(logical) = expression
            && logical.operator == LogicalOperator::And
        {
            self.apply_callback_guard_narrowing(params, &logical.left);
            self.apply_callback_guard_narrowing(params, &logical.right);
            return;
        }

        let Expression::BinaryExpression(binary) = expression else {
            return;
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ) {
            return;
        }
        let name = match (&binary.left, &binary.right) {
            (Expression::Identifier(identifier), Expression::Identifier(undefined))
                if undefined.name == "undefined" =>
            {
                identifier.name.as_str()
            }
            (Expression::Identifier(identifier), Expression::NullLiteral(_)) => {
                identifier.name.as_str()
            }
            _ => return,
        };
        let Some(param) = params.get_mut(name) else {
            return;
        };
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(param.ty) {
            param.ty = *inner;
        }
    }

    /// Lower a callback condition using JavaScript truthiness where needed.
    pub(in crate::lowering) fn callback_truthy_expression(
        &mut self,
        expression: &Expression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        if let Expression::ChainExpression(chain) = expression
            && let ChainElement::StaticMemberExpression(member) = &chain.expression
        {
            let receiver = self.callback_expression(&member.object, params, body)?;
            return Ok(CallbackExpr {
                kind: CallbackExprKind::FieldTruthy {
                    receiver: Box::new(receiver),
                    field: self.intern_source_name(member.property.name.as_str()),
                },
                ty: self.ctx.krate.types.intern(Type::Bool),
            });
        }
        if let Expression::ComputedMemberExpression(member) = expression {
            if let Expression::Identifier(receiver_ident) = &member.object
                && let Some(namespace) = self.object_namespaces.get(receiver_ident.name.as_str())
            {
                let case_keys = namespace.keys().cloned().collect::<Vec<_>>();
                let key = self.callback_expression(&member.expression, params, body)?;
                if self.ctx.krate.types.get(key.ty) != Some(&Type::String) {
                    return Err(SmeltError::unsupported(
                        self.span(member.expression.span().start, member.expression.span().end),
                        "callback function-table truthy key must be a string",
                    ));
                }
                return self.callback_function_table_has_key(
                    &key,
                    &case_keys,
                    self.span(member.span.start, member.span.end),
                );
            }
            let receiver = self.callback_expression(&member.object, params, body)?;
            let field = self.callback_expression(&member.expression, params, body)?;
            return Ok(CallbackExpr {
                kind: CallbackExprKind::HasDynamicField {
                    receiver: Box::new(receiver),
                    field: Box::new(field),
                },
                ty: self.ctx.krate.types.intern(Type::Bool),
            });
        }
        let expr = self.callback_expression(expression, params, body)?;
        let expr_ty = expr.ty;
        if self.ctx.krate.types.get(expr_ty) == Some(&Type::Bool) {
            return Ok(expr);
        }
        if let CallbackExprKind::FunctionTableLookup { key, cases } = &expr.kind {
            let case_keys = cases
                .iter()
                .map(|(case_key, _)| case_key.clone())
                .collect::<Vec<_>>();
            return self.callback_function_table_has_key(
                key,
                &case_keys,
                self.expression_span(expression),
            );
        }
        if matches!(
            self.ctx.krate.types.get(expr_ty),
            Some(Type::Function(_) | Type::Class { .. } | Type::TypeParam { .. })
        ) {
            return Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::Bool(true)),
                ty: self.ctx.krate.types.intern(Type::Bool),
            });
        }
        if self.ctx.krate.types.get(expr_ty) == Some(&Type::String) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            return Ok(CallbackExpr {
                kind: CallbackExprKind::Binary {
                    op: BinOp::NotEq,
                    lhs: Box::new(expr),
                    rhs: Box::new(CallbackExpr {
                        kind: CallbackExprKind::Literal(Literal::String(String::new())),
                        ty: string_ty,
                    }),
                },
                ty: self.ctx.krate.types.intern(Type::Bool),
            });
        }
        if self.ctx.krate.types.get(expr_ty) == Some(&Type::Int) {
            // JavaScript number truthiness: a non-NaN integer is truthy iff it is
            // non-zero. Integers are never NaN, so `n != 0` is exact (this lowers
            // the common `(value, index) => index ? a : b` index-guard idiom).
            let int_ty = self.ctx.krate.types.intern(Type::Int);
            return Ok(CallbackExpr {
                kind: CallbackExprKind::Binary {
                    op: BinOp::NotEq,
                    lhs: Box::new(expr),
                    rhs: Box::new(CallbackExpr {
                        kind: CallbackExprKind::Literal(Literal::Int(0)),
                        ty: int_ty,
                    }),
                },
                ty: self.ctx.krate.types.intern(Type::Bool),
            });
        }
        if self.ctx.krate.types.get(expr_ty) == Some(&Type::Float) {
            // JavaScript number truthiness: a float is truthy iff it is non-zero
            // (covering both `+0` and `-0`) and not `NaN`. `n != 0.0` rejects the
            // zeroes; `n == n` rejects `NaN` (the only value not equal to itself).
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            let float_ty = self.ctx.krate.types.intern(Type::Float);
            let non_zero = CallbackExpr {
                kind: CallbackExprKind::Binary {
                    op: BinOp::NotEq,
                    lhs: Box::new(expr.clone()),
                    rhs: Box::new(CallbackExpr {
                        kind: CallbackExprKind::Literal(Literal::Float(0.0)),
                        ty: float_ty,
                    }),
                },
                ty: bool_ty,
            };
            let not_nan = CallbackExpr {
                kind: CallbackExprKind::Binary {
                    op: BinOp::Eq,
                    lhs: Box::new(expr.clone()),
                    rhs: Box::new(expr),
                },
                ty: bool_ty,
            };
            return Ok(CallbackExpr {
                kind: CallbackExprKind::Binary {
                    op: BinOp::And,
                    lhs: Box::new(non_zero),
                    rhs: Box::new(not_nan),
                },
                ty: bool_ty,
            });
        }
        if self.ctx.krate.types.get(expr_ty) == Some(&Type::Unknown) {
            return Ok(CallbackExpr {
                kind: CallbackExprKind::UnknownIs {
                    value: Box::new(expr),
                    kind: UnknownKind::Bool,
                },
                ty: self.ctx.krate.types.intern(Type::Bool),
            });
        }
        if self.is_nullishable_type(expr_ty) || self.type_is_truthy_condition_surface(expr_ty) {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            return Ok(CallbackExpr {
                kind: CallbackExprKind::Binary {
                    op: BinOp::NotEq,
                    lhs: Box::new(expr),
                    rhs: Box::new(CallbackExpr {
                        kind: CallbackExprKind::Literal(Literal::None),
                        ty: none_ty,
                    }),
                },
                ty: self.ctx.krate.types.intern(Type::Bool),
            });
        }
        Err(SmeltError::unsupported(
            self.expression_span(expression),
            "callback conditions must be boolean, optional, or supported truthy checks",
        ))
    }

    /// Lower a truthy check on a function table lookup into explicit key tests.
    pub(in crate::lowering) fn callback_function_table_has_key(
        &mut self,
        key: &CallbackExpr,
        cases: &[String],
        span: Span,
    ) -> Result<CallbackExpr, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let mut condition = None;
        for case_key in cases {
            let equals_case = CallbackExpr {
                kind: CallbackExprKind::Binary {
                    op: BinOp::Eq,
                    lhs: Box::new(key.clone()),
                    rhs: Box::new(CallbackExpr {
                        kind: CallbackExprKind::Literal(Literal::String(case_key.clone())),
                        ty: string_ty,
                    }),
                },
                ty: bool_ty,
            };
            condition = Some(match condition {
                Some(previous) => CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op: BinOp::Or,
                        lhs: Box::new(previous),
                        rhs: Box::new(equals_case),
                    },
                    ty: bool_ty,
                },
                None => equals_case,
            });
        }
        condition.ok_or_else(|| {
            SmeltError::unsupported(span, "callback function-table truthy check has no entries")
        })
    }

    /// Lower a callback throw branch to a panic expression.
    pub(in crate::lowering) fn callback_throw_expression(
        &mut self,
        argument: Option<&Expression<'_>>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        let message = argument
            .map(|argument| self.callback_throw_message(argument, params, body))
            .transpose()?;
        Ok(CallbackExpr {
            kind: CallbackExprKind::Throw {
                message: message.map(Box::new),
            },
            ty: self.ctx.krate.types.intern(Type::Never),
        })
    }

    /// Lower a callback `throw` operand, preserving the value that was thrown.
    ///
    /// `throw` is value-preserving in JavaScript for every operand shape, so the
    /// operand lowers through the ordinary callback expression path. A built-in
    /// `Error` construction reaches
    /// [`Self::callback_error_object_expression`] there and becomes the erased
    /// error record, which is what makes `error instanceof Error`,
    /// `error.message` and `error.name` observable in the `catch`.
    ///
    /// This used to strip `new Error(m)` / `new TypeError(m)` / `new RangeError(m)`
    /// down to `m` and to replace every other construction with the empty string,
    /// so a `throw` inside an arrow lost the thrown object entirely. Only the
    /// non-`Error` construction keeps that empty-string fallback: the reduced
    /// callback expression language cannot build an arbitrary class instance, and
    /// widening that here would turn callbacks that lower today into blockers.
    pub(in crate::lowering) fn callback_throw_message(
        &mut self,
        argument: &Expression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        if matches!(argument, Expression::NewExpression(new_expr)
            if !matches!(&new_expr.callee, Expression::Identifier(callee)
                if Self::is_builtin_error_constructor(callee.name.as_str())))
        {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::String(String::new())),
                ty,
            });
        }
        self.callback_expression(argument, params, body)
    }

    /// Build the erased `Error` record for a built-in Error construction in a callback.
    ///
    /// This is the reduced-callback-IR twin of
    /// `ModuleBuilder::error_object_constructor_expression`, and produces the same
    /// `{ __smelt_error: <class>, message, cause? }` shape so a callback-thrown
    /// error is indistinguishable from a statement-thrown one at the `catch`. The
    /// `__smelt_error` value carries the spelled class name, which is what makes
    /// `error.name` read truthfully for `TypeError`, `RangeError` and the rest.
    ///
    /// `AggregateError`'s leading `errors` iterable is retained under the
    /// `errors` key, matching the statement path; its message is the second
    /// argument. A non-literal options argument is lowered for its effects only,
    /// because whether a `cause` is attached depends on `"cause" in options`,
    /// which a static rule can only answer for a literal spelling.
    pub(in crate::lowering) fn callback_error_object_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let class_name = match &new_expr.callee {
            Expression::Identifier(callee) => callee.name.to_string(),
            _ => "Error".to_owned(),
        };
        let is_aggregate = class_name == "AggregateError";
        let mut arguments = new_expr.arguments.iter();
        let errors = if is_aggregate {
            match arguments.next().and_then(Argument::as_expression) {
                Some(errors_arg) => Some(self.callback_expression(errors_arg, params, body)?),
                None => None,
            }
        } else {
            None
        };
        let message = match arguments.next().and_then(Argument::as_expression) {
            Some(message_arg) => self.callback_expression(message_arg, params, body)?,
            None => CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::String("Error".to_owned())),
                ty: string_ty,
            },
        };
        let mut cause = None;
        if let Some(Argument::ObjectExpression(options)) = arguments.next() {
            for property in &options.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    continue;
                };
                let value = self.callback_expression(&property.value, params, body)?;
                if matches!(&property.key, PropertyKey::StaticIdentifier(key) if key.name == "cause")
                {
                    cause = Some(value);
                }
            }
        }
        let mut entries = vec![
            (
                self.intern_exact_source_name("__smelt_error"),
                CallbackExpr {
                    kind: CallbackExprKind::Literal(Literal::String(class_name)),
                    ty: string_ty,
                },
            ),
            (self.intern_exact_source_name("message"), message),
        ];
        if let Some(cause) = cause {
            entries.push((self.intern_exact_source_name("cause"), cause));
        }
        if let Some(errors) = errors {
            entries.push((self.intern_exact_source_name("errors"), errors));
        }
        Ok(CallbackExpr {
            kind: CallbackExprKind::DictLit(entries),
            ty: self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty)),
        })
    }

    /// Bind names from a callback parameter pattern to callback expressions.
    pub(in crate::lowering) fn bind_callback_param_pattern<'a>(
        &mut self,
        pattern: &'a BindingPattern<'a>,
        param_index: usize,
        param_ty: smelt_hir::TypeId,
        params: &mut HashMap<&'a str, CallbackExpr>,
    ) -> Result<(), SmeltError> {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                params.insert(
                    binding.name.as_str(),
                    CallbackExpr {
                        kind: CallbackExprKind::Param(param_index),
                        ty: param_ty,
                    },
                );
                Ok(())
            }
            BindingPattern::ArrayPattern(array) => {
                let item_tys = match self.ctx.krate.types.get(param_ty) {
                    Some(Type::Tuple(items)) => items.clone(),
                    Some(Type::List(item)) => vec![*item; array.elements.len()],
                    _ => Vec::new(),
                };
                for (item_index, element) in array.elements.iter().enumerate() {
                    let Some(element_pattern) = element else {
                        continue;
                    };
                    let item_ty = item_tys.get(item_index).copied().unwrap_or(param_ty);
                    let BindingPattern::BindingIdentifier(binding) = element_pattern else {
                        return Err(SmeltError::unsupported(
                            self.span(element_pattern.span().start, element_pattern.span().end),
                            "nested callback parameter destructuring needs closure-body lowering",
                        ));
                    };
                    params.insert(
                        binding.name.as_str(),
                        CallbackExpr {
                            kind: CallbackExprKind::Index {
                                receiver: Box::new(CallbackExpr {
                                    kind: CallbackExprKind::Param(param_index),
                                    ty: param_ty,
                                }),
                                index: item_index,
                            },
                            ty: item_ty,
                        },
                    );
                }
                Ok(())
            }
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    if property.computed {
                        return Err(SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "computed callback parameter destructuring needs closure-body lowering",
                        ));
                    }
                    let field_text = match &property.key {
                        PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str(),
                        PropertyKey::StringLiteral(literal) => literal.value.as_str(),
                        _ => {
                            return Err(SmeltError::unsupported(
                                self.span(property.key.span().start, property.key.span().end),
                                "dynamic callback parameter destructuring needs closure-body lowering",
                            ));
                        }
                    };
                    let BindingPattern::BindingIdentifier(binding) = &property.value else {
                        return Err(SmeltError::unsupported(
                            self.span(property.value.span().start, property.value.span().end),
                            "nested callback parameter destructuring needs closure-body lowering",
                        ));
                    };
                    let field = self.intern_source_name(field_text);
                    let field_ty = match self.ctx.krate.types.get(param_ty) {
                        Some(Type::Dict(_, value) | Type::JsMap(_, value)) => *value,
                        Some(Type::Class { .. }) => self.class_field_type(param_ty, field)?,
                        _ => param_ty,
                    };
                    params.insert(
                        binding.name.as_str(),
                        CallbackExpr {
                            kind: CallbackExprKind::Field {
                                receiver: Box::new(CallbackExpr {
                                    kind: CallbackExprKind::Param(param_index),
                                    ty: param_ty,
                                }),
                                field,
                            },
                            ty: field_ty,
                        },
                    );
                }
                Ok(())
            }
            BindingPattern::AssignmentPattern(_) => Err(SmeltError::unsupported(
                self.span(pattern.span().start, pattern.span().end),
                "default callback parameter destructuring needs closure-body lowering",
            )),
        }
    }

    /// Read arrow parameter types, using contextual function types for omitted annotations.
    pub(in crate::lowering) fn arrow_callback_param_types_with_hint(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        contextual_function: Option<&FunctionType>,
    ) -> Result<Vec<smelt_hir::TypeId>, SmeltError> {
        let mut params = Vec::new();
        for (index, param) in arrow.params.items.iter().enumerate() {
            let ty = param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .or_else(|| {
                    contextual_function
                        .and_then(|function| self.contextual_param_type_at(function, index))
                });
            let ty = match (&param.pattern, ty) {
                (_, Some(ty)) => ty,
                (BindingPattern::BindingIdentifier(_), None) => {
                    self.infer_unannotated_arrow_param_type(arrow, index)
                }
                (_, None) => self.ctx.krate.types.intern(Type::Unknown),
            };
            // An optional parameter (`x?: T`) has type `T | undefined` inside the
            // body, matching the function-type lowering in `types.rs`. Without
            // this the param looks non-nullable, so an `x === undefined` guard
            // constant-folds to `false`.
            let ty = if param.optional
                && !matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_)))
            {
                self.ctx.krate.types.intern(Type::Optional(ty))
            } else {
                ty
            };
            params.push(ty);
        }
        if let Some(rest) = &arrow.params.rest {
            let BindingPattern::BindingIdentifier(_) = &rest.rest.argument else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest closure parameters need closure-body lowering",
                ));
            };
            let ty = rest
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .or_else(|| {
                    contextual_function.map(|function| {
                        let rest_index = arrow.params.items.len();
                        if function.rest == Some(rest_index)
                            && let Some(rest_ty) = function.params.get(rest_index)
                        {
                            return *rest_ty;
                        }
                        let mut item_tys = Vec::new();
                        for param_ty in function.params.iter().skip(rest_index).copied() {
                            if !item_tys.contains(&param_ty) {
                                item_tys.push(param_ty);
                            }
                        }
                        let item_ty = match item_tys.as_slice() {
                            [single] => *single,
                            [] => self.ctx.krate.types.intern(Type::Unknown),
                            _ => self.ctx.krate.types.intern(Type::Union(item_tys)),
                        };
                        self.ctx.krate.types.intern(Type::List(item_ty))
                    })
                })
                .unwrap_or_else(|| {
                    let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                    self.ctx.krate.types.intern(Type::List(item_ty))
                });
            let Ok((ty, _)) = self.rest_param_array_type(ty) else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest closure parameter type must be an array type",
                ));
            };
            params.push(ty);
        }
        Ok(params)
    }

    /// Resolve the contextual type for a *fixed* arrow parameter at `index`
    /// against a callback signature that may end in a rest parameter.
    ///
    /// A user callback can spell fewer, fixed parameters than a variadic
    /// contextual signature (e.g. `(item, item2, item3)` against
    /// `(...args: T[]) => R`, as in `unzipWith`). A fixed parameter that lands
    /// at or past the signature's rest position takes the rest's *element*
    /// type, not the rest list/tuple type itself — otherwise `item` would be
    /// typed `T[]` and arithmetic on it fails (E0369/E0308). Parameters before
    /// the rest position resolve positionally as before.
    fn contextual_param_type_at(
        &self,
        function: &FunctionType,
        index: usize,
    ) -> Option<smelt_hir::TypeId> {
        if let Some(rest_index) = function.rest
            && index >= rest_index
            && let Some(rest_ty) = function.params.get(rest_index).copied()
        {
            return match self.ctx.krate.types.get(rest_ty) {
                Some(Type::List(item)) => Some(*item),
                Some(Type::Tuple(items)) => items
                    .get(index - rest_index)
                    .copied()
                    .or_else(|| items.last().copied()),
                _ => Some(rest_ty),
            };
        }
        function.params.get(index).copied()
    }

    /// Infer a conservative type for an unannotated arrow parameter.
    ///
    /// TypeScript normally gets these from contextual typing. When Smelt loses
    /// that context through imported generic helpers, arithmetic use inside the
    /// callback is still enough to recover a numeric parameter; other cases use
    /// `unknown` so typed library code can keep lowering.
    pub(in crate::lowering) fn infer_unannotated_arrow_param_type(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        index: usize,
    ) -> smelt_hir::TypeId {
        let Some(param_name) = arrow
            .params
            .items
            .get(index)
            .and_then(|param| Self::simple_binding_pattern_name(&param.pattern))
        else {
            return self.ctx.krate.types.intern(Type::Unknown);
        };
        if self.arrow_param_used_as_number(arrow, param_name) {
            self.ctx.krate.types.intern(Type::Float)
        } else {
            self.ctx.krate.types.intern(Type::Unknown)
        }
    }

    /// Return a simple identifier name from a binding or defaulted binding pattern.
    pub(in crate::lowering) fn simple_binding_pattern_name<'a>(pattern: &'a BindingPattern<'a>) -> Option<&'a str> {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => Some(binding.name.as_str()),
            BindingPattern::AssignmentPattern(assign) => {
                Self::simple_binding_pattern_name(&assign.left)
            }
            _ => None,
        }
    }

    /// Return true when an arrow parameter participates in arithmetic.
    pub(in crate::lowering) fn arrow_param_used_as_number(
        &self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        param_name: &str,
    ) -> bool {
        self.arrow_return_expression(arrow)
            .ok()
            .is_some_and(|expr| Self::expression_uses_identifier_in_arithmetic(expr, param_name))
    }

    /// Scan an expression for arithmetic involving a named identifier.
    pub(in crate::lowering) fn expression_uses_identifier_in_arithmetic(expression: &Expression<'_>, name: &str) -> bool {
        match expression {
            Expression::BinaryExpression(binary)
                if matches!(
                    binary.operator,
                    BinaryOperator::Subtraction
                        | BinaryOperator::Multiplication
                        | BinaryOperator::Division
                        | BinaryOperator::Remainder
                        | BinaryOperator::Exponential
                ) =>
            {
                Self::expression_contains_identifier(&binary.left, name)
                    || Self::expression_contains_identifier(&binary.right, name)
            }
            Expression::BinaryExpression(binary) => {
                Self::expression_uses_identifier_in_arithmetic(&binary.left, name)
                    || Self::expression_uses_identifier_in_arithmetic(&binary.right, name)
            }
            Expression::NewExpression(new_expr) => {
                matches!(
                    &new_expr.callee,
                    Expression::Identifier(identifier)
                        if Self::is_ts_stdlib_class_name(
                            identifier.name.as_str(),
                            smelt_stdlib::StdlibClass::Date
                        )
                )
                    && new_expr.arguments.iter().any(|argument| {
                        argument
                            .as_expression()
                            .is_some_and(|expr| Self::expression_contains_identifier(expr, name))
                    })
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::expression_uses_identifier_in_arithmetic(&parenthesized.expression, name)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::expression_uses_identifier_in_arithmetic(&as_expr.expression, name)
            }
            _ => false,
        }
    }

    /// Return true when an expression contains a named identifier.
    pub(in crate::lowering) fn expression_contains_identifier(expression: &Expression<'_>, name: &str) -> bool {
        match expression {
            Expression::Identifier(identifier) => identifier.name == name,
            Expression::BinaryExpression(binary) => {
                Self::expression_contains_identifier(&binary.left, name)
                    || Self::expression_contains_identifier(&binary.right, name)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::expression_contains_identifier(&parenthesized.expression, name)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::expression_contains_identifier(&as_expr.expression, name)
            }
            _ => false,
        }
    }

    /// Lower a local arrow function through a real HIR closure body.
    ///
    /// Async arrows use the same body lowering as async function items: the
    /// closure body is lowered with `current_async` enabled, receives the
    /// contextual return type for return-expression hints, and records async
    /// state metadata after await expressions have been collected.
    ///
    /// This is a thin wrapper over [`Self::closure_body_expr_from_parts`], which
    /// carries the shared parameter-binding, capture-collection and closure-
    /// emission logic used by both arrow and `function` expression callbacks.
    pub(in crate::lowering) fn arrow_closure_body_expr(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        param_tys: &[smelt_hir::TypeId],
        return_ty: smelt_hir::TypeId,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(arrow.span.start, arrow.span.end);
        let body_kind = if arrow.expression {
            ClosureBodyKind::ArrowExpression(arrow)
        } else {
            ClosureBodyKind::Statements(&arrow.body.statements)
        };
        self.closure_body_expr_from_parts(
            &arrow.params,
            body_kind,
            arrow.r#async,
            span,
            param_tys,
            return_ty,
            outer_body,
        )
    }

    /// Lower a `function (...) { ... }` expression callback through a real HIR
    /// closure body.
    ///
    /// Function expressions are the non-arrow sibling of the callback surface
    /// accepted by array methods. They always have a block body (never an
    /// expression body) and reuse the same shared closure-body machinery as
    /// arrow callbacks, so a `function`-form callback whose body needs full
    /// closure lowering (e.g. a method call the compact callback IR cannot
    /// model) is lowered identically to the equivalent arrow. The callback's
    /// own `arguments` binding is not modeled here; array-method callbacks that
    /// reference `arguments` are rejected by the general body lowering as usual.
    pub(in crate::lowering) fn function_closure_body_expr(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        param_tys: &[smelt_hir::TypeId],
        return_ty: smelt_hir::TypeId,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(function.span.start, function.span.end);
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                span,
                "function expression callbacks must have a body",
            ));
        };
        self.closure_body_expr_from_parts(
            &function.params,
            ClosureBodyKind::Statements(&function_body.statements),
            function.r#async,
            span,
            param_tys,
            return_ty,
            outer_body,
        )
    }

    /// Lower a callback closure body from its constituent parts.
    ///
    /// Shared implementation behind [`Self::arrow_closure_body_expr`] and
    /// [`Self::function_closure_body_expr`]. It binds the formal parameters into
    /// closure locals, collects the captured outer locals referenced by the
    /// body, lowers the body (an arrow expression body or a statement block) and
    /// emits the closure expression with the inferred/annotated return type.
    ///
    /// `body_kind` selects the body form: [`ClosureBodyKind::ArrowExpression`]
    /// lowers the single expression through the return-expression hint path (and
    /// may infer the return type when it is left `Unknown`), while
    /// [`ClosureBodyKind::Statements`] lowers a statement block — the form used
    /// by block-bodied arrows and by `function` expression callbacks.
    #[expect(
        clippy::too_many_arguments,
        reason = "shared closure-body lowering threads the params, body form, async flag, span and both contextual types through one call"
    )]
    fn closure_body_expr_from_parts(
        &mut self,
        params: &oxc::ast::ast::FormalParameters<'_>,
        body_kind: ClosureBodyKind<'_, '_>,
        is_async: bool,
        span: Span,
        param_tys: &[smelt_hir::TypeId],
        return_ty: smelt_hir::TypeId,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let is_expression_body = matches!(body_kind, ClosureBodyKind::ArrowExpression(_));
        let mut closure_body = Body::new(None, span);
        let mut closure_params = Vec::new();
        let mut param_names = HashSet::new();
        let mut saved_locals = Vec::new();

        for (index, param) in params.items.iter().enumerate() {
            let ty = param_tys.get(index).copied().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "closure parameter count does not match its type",
                )
            })?;
            let (symbol, param_span) = match &param.pattern {
                BindingPattern::BindingIdentifier(binding) => (
                    self.intern_source_name(binding.name.as_str()),
                    self.span(binding.span.start, binding.span.end),
                ),
                _ => (
                    self.synthetic_param_symbol(index),
                    self.span(param.span.start, param.span.end),
                ),
            };
            let local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span: param_span,
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name: symbol,
                local,
                ty,
                span: param_span,
            });
            match &param.pattern {
                BindingPattern::BindingIdentifier(binding) => {
                    param_names.insert(binding.name.as_str().to_owned());
                    saved_locals.push((
                        binding.name.as_str().to_owned(),
                        self.scope.bind(binding.name.as_str().to_owned(), local),
                    ));
                }
                pattern => {
                    let mut names = Vec::new();
                    Self::binding_pattern_names(pattern, &mut names);
                    for name in &names {
                        param_names.insert(name.clone());
                        saved_locals.push((name.clone(), self.scope.lookup(name.as_str())));
                    }
                    let value = closure_body.push_expr(Expr {
                        kind: ExprKind::Local(local),
                        ty,
                        span: param_span,
                    });
                    let root = closure_body.root;
                    self.binding_declaration(
                        pattern,
                        Some(value),
                        Some(ty),
                        false,
                        &mut closure_body,
                        root,
                    )?;
                }
            }
        }

        if let Some(rest) = &params.rest {
            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest closure parameters need closure-body lowering",
                ));
            };
            let ty = param_tys.last().copied().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest closure parameter count does not match its type",
                )
            })?;
            let symbol = self.intern_source_name(binding.name.as_str());
            let local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span: self.span(binding.span.start, binding.span.end),
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name: symbol,
                local,
                ty,
                span: self.span(binding.span.start, binding.span.end),
            });
            param_names.insert(binding.name.as_str().to_owned());
            saved_locals.push((
                binding.name.as_str().to_owned(),
                self.scope.bind(binding.name.as_str().to_owned(), local),
            ));
        }

        let mut capture_names = Vec::new();
        match body_kind {
            ClosureBodyKind::ArrowExpression(arrow) => {
                let return_expression = self.arrow_return_expression(arrow)?;
                self.collect_expression_capture_names(
                    return_expression,
                    &param_names,
                    &mut capture_names,
                );
            }
            ClosureBodyKind::Statements(statements) => {
                for statement in statements {
                    self.collect_statement_capture_names(
                        statement,
                        &param_names,
                        &mut capture_names,
                    );
                }
            }
        }
        capture_names.sort();
        capture_names.dedup();

        let mut captures = Vec::new();
        for name in capture_names {
            let Some(source_local) = saved_locals
                .iter()
                .find_map(|(saved_name, prior)| (saved_name == &name).then_some(*prior).flatten())
                .or_else(|| self.scope.lookup(name.as_str()))
            else {
                continue;
            };
            let Some(source_decl) = usize::try_from(source_local.0)
                .ok()
                .and_then(|index| outer_body.locals.get(index))
            else {
                continue;
            };
            let symbol = source_decl
                .name
                .unwrap_or_else(|| self.ctx.krate.symbols.intern(name.as_str()));
            let body_local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty: source_decl.ty,
                mutable: source_decl.mutable,
                span: source_decl.span,
            });
            saved_locals.push((name.clone(), self.scope.bind(name, body_local)));
            captures.push(ClosureCapture {
                source_local,
                body_local: Some(body_local),
                symbol,
                ty: source_decl.ty,
                mode: CaptureMode::ByRef,
            });
        }

        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        let saved_narrowed_locals = self.scope.take_narrowings();
        // Postfix-update deferral must not cross this closure boundary: if this
        // closure is a variable-declaration initializer, an `x++` inside its body
        // belongs here, not the outer declaration's pending deferral list (see
        // the matching reset in `function_expression_value`).
        let saved_deferred_updates = self.deferred_postfix_updates.take();
        let infer_expression_return = is_expression_body
            && matches!(self.ctx.krate.types.get(return_ty), Some(Type::Unknown));
        self.current_async = is_async;
        self.current_return_ty = Some(return_ty);
        let mut actual_return_ty = return_ty;
        let predeclare_result = match body_kind {
            ClosureBodyKind::ArrowExpression(_) => Ok(()),
            ClosureBodyKind::Statements(statements) => self
                .predeclare_local_function_declarations(statements, &mut closure_body)
                .and_then(|()| {
                    self.predeclare_local_arrow_callbacks(statements, &mut closure_body)
                }),
        };
        let lowering_result = if let Err(error) = predeclare_result {
            Err(error)
        } else {
            match body_kind {
                ClosureBodyKind::ArrowExpression(arrow) => {
                    match self.arrow_return_expression(arrow) {
                        Ok(return_expression) => {
                            // For an async expression-bodied arrow the closure
                            // return type is `Promise<Inner>` (`Type::Future`),
                            // but the body expression itself produces `Inner` —
                            // the async wrapper adds the promise. Hint the body
                            // at the awaited inner type (via
                            // `return_statement_value_hint`, which unwraps one
                            // `Future` layer for async bodies) so an array or
                            // tuple literal body keeps its own value type instead
                            // of being coerced to the future type.
                            let hint = if infer_expression_return {
                                None
                            } else {
                                self.return_statement_value_hint()
                            };
                            self.expression_with_hint(return_expression, &mut closure_body, hint)
                                .map(|value| {
                                    if infer_expression_return {
                                        actual_return_ty = Self::expr_ty(&closure_body, value);
                                    }
                                    closure_body.push_stmt(Stmt::Return(Some(value)));
                                })
                        }
                        Err(error) => Err(error),
                    }
                }
                ClosureBodyKind::Statements(statements) => {
                    let mut result = Ok(());
                    for statement in statements {
                        if let Err(error) = self.statement(statement, &mut closure_body) {
                            result = Err(error);
                            break;
                        }
                    }
                    result
                }
            }
        };
        if is_async {
            closure_body.build_async_state_machine();
        }
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        self.scope.restore_narrowings(saved_narrowed_locals);
        self.deferred_postfix_updates = saved_deferred_updates;
        for (name, prior) in saved_locals.into_iter().rev() {
            if let Some(local) = prior {
                self.scope.bind(name, local);
            } else {
                self.scope.unbind(name.as_str());
            }
        }
        lowering_result?;
        let may_throw = Self::body_contains_uncaught_throw(&closure_body);
        let body_id = self.ctx.krate.push_body(closure_body);
        let rest_index = params.rest.as_ref().map(|_| params.items.len());
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: param_tys.to_vec(),
            rest: rest_index,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: actual_return_ty,
            is_async,
            may_throw,
        }));
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest: rest_index,
                required_params: None,
                return_ty: actual_return_ty,
                captures,
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Returns whether a legacy callback expression contains a source throw.
    pub(in crate::lowering) fn callback_expr_contains_throw(callback: &CallbackExpr) -> bool {
        match &callback.kind {
            CallbackExprKind::Throw { .. } => true,
            CallbackExprKind::Unary { operand, .. } => Self::callback_expr_contains_throw(operand),
            CallbackExprKind::Binary { lhs, rhs, .. } => {
                Self::callback_expr_contains_throw(lhs) || Self::callback_expr_contains_throw(rhs)
            }
            CallbackExprKind::UnknownIs { value, .. } => Self::callback_expr_contains_throw(value),
            CallbackExprKind::TypeofValue { value } => Self::callback_expr_contains_throw(value),
            CallbackExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                Self::callback_expr_contains_throw(cond)
                    || Self::callback_expr_contains_throw(then_expr)
                    || Self::callback_expr_contains_throw(else_expr)
            }
            CallbackExprKind::Call { callee, args } => {
                Self::callback_expr_contains_throw(callee)
                    || args
                        .iter()
                        .any(|arg| Self::callback_expr_contains_throw(&arg.expr))
            }
            CallbackExprKind::MethodCall { receiver, args, .. } => {
                Self::callback_expr_contains_throw(receiver)
                    || args
                        .iter()
                        .any(|arg| Self::callback_expr_contains_throw(&arg.expr))
            }
            CallbackExprKind::ListLit(items) => {
                items.iter().any(Self::callback_expr_contains_throw)
            }
            CallbackExprKind::DictLit(entries) => entries
                .iter()
                .any(|(_, value)| Self::callback_expr_contains_throw(value)),
            CallbackExprKind::Sequence { effects, result } => {
                effects.iter().any(Self::callback_expr_contains_throw)
                    || Self::callback_expr_contains_throw(result)
            }
            CallbackExprKind::FunctionTableLookup { key, .. } => {
                Self::callback_expr_contains_throw(key)
            }
            CallbackExprKind::AssignCapture { value, .. } => {
                Self::callback_expr_contains_throw(value)
            }
            CallbackExprKind::Index { receiver, .. }
            | CallbackExprKind::Field { receiver, .. }
            | CallbackExprKind::HasField { receiver, .. }
            | CallbackExprKind::FieldTruthy { receiver, .. } => {
                Self::callback_expr_contains_throw(receiver)
            }
            CallbackExprKind::DynamicIndex { receiver, index } => {
                Self::callback_expr_contains_throw(receiver)
                    || Self::callback_expr_contains_throw(index)
            }
            CallbackExprKind::HasDynamicField { receiver, field } => {
                Self::callback_expr_contains_throw(receiver)
                    || Self::callback_expr_contains_throw(field)
            }
            CallbackExprKind::Param(_)
            | CallbackExprKind::Capture(_)
            | CallbackExprKind::Function(_)
            | CallbackExprKind::Literal(_) => false,
        }
    }

    /// Returns whether a lowered body directly contains an `await` expression.
    ///
    /// Only this body's own expression arena is scanned; awaits inside nested
    /// closure bodies live in their own [`Body`] arenas and belong to those
    /// closures, not to this one. A body that directly awaits must itself be
    /// async — HIR validation rejects `await` outside an async function. Some
    /// lowerings (notably the Vitest `expect(...).rejects.toThrow(...)` async
    /// matcher) desugar into an inline `await` even when the source callback was
    /// not spelled `async`, because in JavaScript the test returns the pending
    /// promise for the framework to await; inlining that await makes the body
    /// genuinely async, so callers use this to mark the function accordingly.
    pub(in crate::lowering) fn body_contains_await(body: &Body) -> bool {
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Await(_)))
    }

    /// Returns whether a lowered closure body can throw past its own boundary.
    ///
    /// Try bodies with a catch handler are considered locally handled here: the
    /// MIR lowering attaches exception edges for nested calls and explicit
    /// throws, so only statements that can escape the closure need to widen the
    /// closure ABI to `Result`.
    pub(in crate::lowering) fn body_contains_uncaught_throw(body: &Body) -> bool {
        Self::block_contains_uncaught_throw(body, body.root)
    }

    /// Returns whether a HIR block contains a throw not protected by catch.
    pub(in crate::lowering) fn block_contains_uncaught_throw(body: &Body, block: smelt_hir::BlockId) -> bool {
        let Some(block_data) = usize::try_from(block.0)
            .ok()
            .and_then(|index| body.blocks.get(index))
        else {
            return false;
        };
        block_data.stmts.iter().any(|stmt_id| {
            let Some(stmt) = usize::try_from(stmt_id.0)
                .ok()
                .and_then(|index| body.stmts.get(index))
            else {
                return false;
            };
            Self::stmt_contains_uncaught_throw(body, stmt)
        })
    }

    /// Returns whether a HIR statement can throw out of the surrounding body.
    pub(in crate::lowering) fn stmt_contains_uncaught_throw(body: &Body, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Throw(_) => true,
            Stmt::If {
                then_block,
                else_block,
                ..
            } => {
                Self::block_contains_uncaught_throw(body, *then_block)
                    || else_block
                        .is_some_and(|block| Self::block_contains_uncaught_throw(body, block))
            }
            Stmt::While {
                body: loop_body, ..
            }
            | Stmt::WhileUpdate {
                body: loop_body, ..
            }
            | Stmt::WhileUpdateBlock {
                body: loop_body, ..
            }
            | Stmt::For {
                body: loop_body, ..
            } => Self::block_contains_uncaught_throw(body, *loop_body),
            Stmt::Match { arms, default, .. } => {
                arms.iter()
                    .any(|arm| Self::block_contains_uncaught_throw(body, arm.body))
                    || default.is_some_and(|block| Self::block_contains_uncaught_throw(body, block))
            }
            Stmt::TryCatch {
                body: try_body,
                catch_body,
                finally_body,
                ..
            } => {
                let try_escapes = if catch_body.is_none() {
                    Self::block_contains_uncaught_throw(body, *try_body)
                } else {
                    false
                };
                let catch_escapes = catch_body
                    .is_some_and(|block| Self::block_contains_uncaught_throw(body, block));
                let finally_escapes = finally_body
                    .is_some_and(|block| Self::block_contains_uncaught_throw(body, block));
                try_escapes || catch_escapes || finally_escapes
            }
            Stmt::Let { .. }
            | Stmt::Assign { .. }
            | Stmt::Expr(_)
            | Stmt::Return(_)
            | Stmt::Break
            | Stmt::Continue => false,
        }
    }

    /// Lower an arrow function expression as a first-class closure value.
    pub(in crate::lowering) fn arrow_function_expression(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.arrow_function_expression_with_hint(arrow, body, None)
    }

    /// Lower an arrow function expression using an optional contextual function type.
    pub(in crate::lowering) fn arrow_function_expression_with_hint(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.push_type_parameter_scope(arrow.type_parameters.as_deref())?;
        let result = (|| {
            let contextual_function = type_hint.and_then(|hint| {
                let function_hint = self.function_member_type(hint).unwrap_or(hint);
                if let Some(Type::Function(function)) = self.ctx.krate.types.get(function_hint) {
                    Some(function.clone())
                } else {
                    None
                }
            });
            let params =
                self.arrow_callback_param_types_with_hint(arrow, contextual_function.as_ref())?;
            let explicit_return_ty = arrow
                .return_type
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            if !arrow.r#async
                && let Ok(callback) = self.arrow_callback_from_params(arrow, &params, body)
            {
                let return_ty = explicit_return_ty.unwrap_or(callback.ty);
                if !self.type_assignable_to(callback.ty, return_ty) {
                    return Err(SmeltError::unsupported(
                        self.span(arrow.span.start, arrow.span.end),
                        "arrow expression return type does not match its annotation",
                    ));
                }
                let span = self.span(arrow.span.start, arrow.span.end);
                let rest = arrow
                    .params
                    .rest
                    .as_ref()
                    .map(|_| arrow.params.items.len())
                    .or_else(|| {
                        contextual_function
                            .as_ref()
                            .and_then(|function| function.rest)
                    });
                // The callback tree classified into the compact side-effect-free
                // expression IR, but the compact IR only lowers a bounded method
                // table (`.map`/`.at`/`Set.has`/...). A statically-resolvable
                // method call it does not model (`controller.abort()`,
                // `date.getTime()`, `text.localeCompare(other)`, a captured class
                // instance method, ...) is rejected here with a "closure body"
                // error even though the general expression path knows how to
                // dispatch it. When that happens, fall through to the full
                // closure-body lowering below, which routes the arrow body through
                // `expression`/`statement` (the same path a non-callback method
                // call uses) instead of surfacing the blocker.
                match self.callback_expr_to_closure_with_return_ty(
                    return_ty,
                    &callback,
                    &params,
                    rest,
                    contextual_function
                        .as_ref()
                        .and_then(|function| function.required_params),
                    span,
                    body,
                ) {
                    Ok(expr) => return Ok(expr),
                    Err(error) if Self::should_fallback_to_closure_body_for_callback(&error) => {
                        // Recoverable compact-IR gap: retry through the general
                        // closure-body path, preserving the compact path's
                        // resolved return type so the closure keeps its typed
                        // signature.
                        return self.arrow_closure_body_expr(arrow, &params, return_ty, body);
                    }
                    Err(error) => return Err(error),
                }
            }
            let mut return_ty = explicit_return_ty
                .or_else(|| {
                    contextual_function
                        .as_ref()
                        .map(|function| function.return_ty)
                })
                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
            if arrow.r#async
                && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
            {
                return_ty = self.ctx.krate.types.intern(Type::Future(return_ty));
            }
            self.arrow_closure_body_expr(arrow, &params, return_ty, body)
        })();
        self.pop_type_parameter_scope();
        result
    }

    /// Return the expression produced by an arrow function body.
    pub(in crate::lowering) fn arrow_return_expression<'a>(
        &self,
        arrow: &'a oxc::ast::ast::ArrowFunctionExpression<'a>,
    ) -> Result<&'a Expression<'a>, SmeltError> {
        if arrow.expression {
            let [Statement::ExpressionStatement(statement)] = arrow.body.statements.as_slice()
            else {
                return Err(SmeltError::unsupported(
                    self.span(arrow.body.span.start, arrow.body.span.end),
                    "expression-bodied closures must contain one expression",
                ));
            };
            Ok(&statement.expression)
        } else {
            let [Statement::ReturnStatement(statement)] = arrow.body.statements.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(arrow.body.span.start, arrow.body.span.end),
                    "block-bodied closures currently require a single return statement",
                ));
            };
            statement.argument.as_ref().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(statement.span.start, statement.span.end),
                    "closure return statements must return a value",
                )
            })
        }
    }

    /// Collect outer identifier names referenced by an arrow body expression.
    pub(in crate::lowering) fn collect_expression_capture_names(
        &self,
        expression: &Expression<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match expression {
            Expression::ThisExpression(_) => {
                if !param_names.contains("this") && self.scope.is_bound("this") {
                    captures.push("this".to_owned());
                }
            }
            Expression::Identifier(identifier) => {
                let name = identifier.name.as_str();
                if !param_names.contains(name) && self.scope.is_bound(name) {
                    captures.push(name.to_owned());
                }
            }
            Expression::CallExpression(call) => {
                self.collect_expression_capture_names(&call.callee, param_names, captures);
                for arg in &call.arguments {
                    self.collect_argument_capture_names(arg, param_names, captures);
                }
            }
            Expression::NewExpression(new_expr) => {
                self.collect_expression_capture_names(&new_expr.callee, param_names, captures);
                for arg in &new_expr.arguments {
                    self.collect_argument_capture_names(arg, param_names, captures);
                }
            }
            Expression::ParenthesizedExpression(parenthesized) => self
                .collect_expression_capture_names(&parenthesized.expression, param_names, captures),
            Expression::BinaryExpression(binary) => {
                self.collect_expression_capture_names(&binary.left, param_names, captures);
                self.collect_expression_capture_names(&binary.right, param_names, captures);
            }
            Expression::LogicalExpression(logical) => {
                self.collect_expression_capture_names(&logical.left, param_names, captures);
                self.collect_expression_capture_names(&logical.right, param_names, captures);
            }
            Expression::UnaryExpression(unary) => {
                self.collect_expression_capture_names(&unary.argument, param_names, captures);
            }
            Expression::AwaitExpression(await_expr) => {
                self.collect_expression_capture_names(&await_expr.argument, param_names, captures);
            }
            Expression::YieldExpression(yield_expr) => {
                if let Some(argument) = &yield_expr.argument {
                    self.collect_expression_capture_names(argument, param_names, captures);
                }
            }
            Expression::AssignmentExpression(assignment) => {
                self.collect_assignment_target_capture_names(
                    &assignment.left,
                    param_names,
                    captures,
                );
                self.collect_expression_capture_names(&assignment.right, param_names, captures);
            }
            Expression::ConditionalExpression(conditional) => {
                self.collect_expression_capture_names(&conditional.test, param_names, captures);
                self.collect_expression_capture_names(
                    &conditional.consequent,
                    param_names,
                    captures,
                );
                self.collect_expression_capture_names(
                    &conditional.alternate,
                    param_names,
                    captures,
                );
            }
            Expression::TemplateLiteral(template) => {
                for template_expression in &template.expressions {
                    self.collect_expression_capture_names(
                        template_expression,
                        param_names,
                        captures,
                    );
                }
            }
            Expression::StaticMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
            }
            Expression::ComputedMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
                self.collect_expression_capture_names(&member.expression, param_names, captures);
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(property) => {
                            self.collect_expression_capture_names(
                                &property.value,
                                param_names,
                                captures,
                            );
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => self
                            .collect_expression_capture_names(
                                &spread.argument,
                                param_names,
                                captures,
                            ),
                    }
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    match element {
                        ArrayExpressionElement::SpreadElement(spread) => self
                            .collect_expression_capture_names(
                                &spread.argument,
                                param_names,
                                captures,
                            ),
                        other => {
                            if let Some(expr) = other.as_expression() {
                                self.collect_expression_capture_names(expr, param_names, captures);
                            }
                        }
                    }
                }
            }
            Expression::ArrowFunctionExpression(arrow) => {
                let mut nested_params = param_names.clone();
                for param in &arrow.params.items {
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        nested_params.insert(binding.name.as_str().to_owned());
                    }
                }
                for statement in &arrow.body.statements {
                    self.collect_statement_capture_names(statement, &nested_params, captures);
                }
            }
            Expression::FunctionExpression(function) => {
                let mut nested_params = param_names.clone();
                for param in &function.params.items {
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        nested_params.insert(binding.name.as_str().to_owned());
                    }
                }
                if let Some(rest) = &function.params.rest
                    && let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument
                {
                    nested_params.insert(binding.name.as_str().to_owned());
                }
                if let Some(body) = &function.body {
                    for statement in &body.statements {
                        self.collect_statement_capture_names(statement, &nested_params, captures);
                    }
                }
            }
            Expression::TSAsExpression(as_expr) => {
                self.collect_expression_capture_names(&as_expr.expression, param_names, captures);
            }
            Expression::TSTypeAssertion(assertion) => {
                self.collect_expression_capture_names(&assertion.expression, param_names, captures);
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.collect_expression_capture_names(&satisfies.expression, param_names, captures);
            }
            Expression::TSNonNullExpression(non_null) => {
                self.collect_expression_capture_names(&non_null.expression, param_names, captures);
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::CallExpression(call) => {
                    self.collect_expression_capture_names(&call.callee, param_names, captures);
                    for arg in &call.arguments {
                        self.collect_argument_capture_names(arg, param_names, captures);
                    }
                }
                ChainElement::StaticMemberExpression(member) => {
                    self.collect_expression_capture_names(&member.object, param_names, captures);
                }
                ChainElement::ComputedMemberExpression(member) => {
                    self.collect_expression_capture_names(&member.object, param_names, captures);
                    self.collect_expression_capture_names(
                        &member.expression,
                        param_names,
                        captures,
                    );
                }
                ChainElement::TSNonNullExpression(non_null) => {
                    self.collect_expression_capture_names(
                        &non_null.expression,
                        param_names,
                        captures,
                    );
                }
                ChainElement::PrivateFieldExpression(_) => {}
            },
            Expression::UpdateExpression(update) => {
                self.collect_simple_assignment_target_capture_names(
                    &update.argument,
                    param_names,
                    captures,
                );
            }
            Expression::SequenceExpression(sequence) => {
                for sequence_expr in &sequence.expressions {
                    self.collect_expression_capture_names(sequence_expr, param_names, captures);
                }
            }
            _ => {}
        }
    }

    /// Collect captures referenced by a call/new argument, including function literals.
    fn collect_argument_capture_names(
        &self,
        argument: &Argument<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match argument {
            Argument::SpreadElement(spread) => self.collect_expression_capture_names(
                &spread.argument,
                param_names,
                captures,
            ),
            Argument::ArrowFunctionExpression(arrow) => {
                let mut nested_params = param_names.clone();
                for param in &arrow.params.items {
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        nested_params.insert(binding.name.as_str().to_owned());
                    }
                }
                for statement in &arrow.body.statements {
                    self.collect_statement_capture_names(statement, &nested_params, captures);
                }
            }
            Argument::FunctionExpression(function) => {
                let mut nested_params = param_names.clone();
                for param in &function.params.items {
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        nested_params.insert(binding.name.as_str().to_owned());
                    }
                }
                if let Some(rest) = &function.params.rest
                    && let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument
                {
                    nested_params.insert(binding.name.as_str().to_owned());
                }
                if let Some(body) = &function.body {
                    for statement in &body.statements {
                        self.collect_statement_capture_names(
                            statement,
                            &nested_params,
                            captures,
                        );
                    }
                }
            }
            other => {
                if let Some(expression) = other.as_expression() {
                    self.collect_expression_capture_names(expression, param_names, captures);
                }
            }
        }
    }

    /// Collect captured locals referenced by a simple assignment target
    /// (the target of `x++`, `--y`, or `obj[i]++`).
    ///
    /// `++counter` and `startIndex++` in the curry/bind/after family mutate a
    /// captured enclosing local; without traversing the update target the
    /// mutated local is never recorded as a capture and the closure body fails
    /// with `unresolved identifier`.
    pub(in crate::lowering) fn collect_simple_assignment_target_capture_names(
        &self,
        target: &SimpleAssignmentTarget<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let name = identifier.name.as_str();
                if !param_names.contains(name) && self.scope.is_bound(name) {
                    captures.push(name.to_owned());
                }
            }
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
                self.collect_expression_capture_names(&member.expression, param_names, captures);
            }
            _ => {}
        }
    }

    /// Collect captured locals referenced by an assignment target.
    pub(in crate::lowering) fn collect_assignment_target_capture_names(
        &self,
        target: &AssignmentTarget<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let name = identifier.name.as_str();
                if !param_names.contains(name) && self.scope.is_bound(name) {
                    captures.push(name.to_owned());
                }
            }
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
                self.collect_expression_capture_names(&member.expression, param_names, captures);
            }
            AssignmentTarget::StaticMemberExpression(member) => {
                self.collect_expression_capture_names(&member.object, param_names, captures);
            }
            _ => {}
        }
    }

    /// Collect outer identifier names referenced by a block-bodied arrow statement.
    pub(in crate::lowering) fn collect_statement_capture_names(
        &self,
        statement: &Statement<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match statement {
            Statement::VariableDeclaration(decl) => {
                let mut local_names = param_names.clone();
                for declarator in &decl.declarations {
                    if let BindingPattern::BindingIdentifier(binding) = &declarator.id {
                        local_names.insert(binding.name.as_str().to_owned());
                    }
                }
                for declarator in &decl.declarations {
                    if let Some(init) = &declarator.init {
                        self.collect_expression_capture_names(init, &local_names, captures);
                    }
                }
            }
            Statement::ExpressionStatement(statement) => {
                self.collect_expression_capture_names(&statement.expression, param_names, captures);
            }
            Statement::ReturnStatement(statement) => {
                if let Some(argument) = &statement.argument {
                    self.collect_expression_capture_names(argument, param_names, captures);
                }
            }
            Statement::IfStatement(statement) => {
                self.collect_expression_capture_names(&statement.test, param_names, captures);
                self.collect_statement_capture_names(&statement.consequent, param_names, captures);
                if let Some(alternate) = &statement.alternate {
                    self.collect_statement_capture_names(alternate, param_names, captures);
                }
            }
            Statement::BlockStatement(block) => {
                for child in &block.body {
                    self.collect_statement_capture_names(child, param_names, captures);
                }
            }
            Statement::ForOfStatement(statement) => {
                self.collect_expression_capture_names(&statement.right, param_names, captures);
                let mut local_names = param_names.clone();
                Self::collect_for_left_binding_names(&statement.left, &mut local_names);
                self.collect_statement_capture_names(&statement.body, &local_names, captures);
            }
            Statement::ForInStatement(statement) => {
                self.collect_expression_capture_names(&statement.right, param_names, captures);
                let mut local_names = param_names.clone();
                Self::collect_for_left_binding_names(&statement.left, &mut local_names);
                self.collect_statement_capture_names(&statement.body, &local_names, captures);
            }
            Statement::WhileStatement(statement) => {
                self.collect_expression_capture_names(&statement.test, param_names, captures);
                self.collect_statement_capture_names(&statement.body, param_names, captures);
            }
            Statement::TryStatement(statement) => {
                for child in &statement.block.body {
                    self.collect_statement_capture_names(child, param_names, captures);
                }
                if let Some(handler) = &statement.handler {
                    let mut local_names = param_names.clone();
                    if let Some(param) = &handler.param {
                        let mut binding_names = Vec::new();
                        Self::binding_pattern_names(&param.pattern, &mut binding_names);
                        local_names.extend(binding_names);
                    }
                    for child in &handler.body.body {
                        self.collect_statement_capture_names(child, &local_names, captures);
                    }
                }
                if let Some(finalizer) = &statement.finalizer {
                    for child in &finalizer.body {
                        self.collect_statement_capture_names(child, param_names, captures);
                    }
                }
            }
            Statement::ThrowStatement(statement) => {
                self.collect_expression_capture_names(&statement.argument, param_names, captures);
            }
            Statement::ForStatement(statement) => {
                // C-style `for (let i = 0; i < n; ++i)` loops appear throughout
                // the curry/bind/partial family's returned closures. The init
                // declaration introduces loop-scoped locals (`i`); the test,
                // update, and body all reference enclosing captures
                // (`partialArgs.length`, `predicates[i]`). Thread the init's
                // declared names so the loop variable is not treated as a
                // capture while the genuinely-enclosing locals still are.
                let mut local_names = param_names.clone();
                if let Some(ForStatementInit::VariableDeclaration(decl)) = &statement.init {
                    for declarator in &decl.declarations {
                        let mut binding_names = Vec::new();
                        Self::binding_pattern_names(&declarator.id, &mut binding_names);
                        local_names.extend(binding_names);
                    }
                }
                match &statement.init {
                    Some(ForStatementInit::VariableDeclaration(decl)) => {
                        for declarator in &decl.declarations {
                            if let Some(init) = &declarator.init {
                                self.collect_expression_capture_names(
                                    init,
                                    &local_names,
                                    captures,
                                );
                            }
                        }
                    }
                    Some(init) => {
                        if let Some(expression) = init.as_expression() {
                            self.collect_expression_capture_names(
                                expression,
                                &local_names,
                                captures,
                            );
                        }
                    }
                    None => {}
                }
                if let Some(test) = &statement.test {
                    self.collect_expression_capture_names(test, &local_names, captures);
                }
                if let Some(update) = &statement.update {
                    self.collect_expression_capture_names(update, &local_names, captures);
                }
                self.collect_statement_capture_names(&statement.body, &local_names, captures);
            }
            Statement::DoWhileStatement(statement) => {
                self.collect_statement_capture_names(&statement.body, param_names, captures);
                self.collect_expression_capture_names(&statement.test, param_names, captures);
            }
            Statement::SwitchStatement(statement) => {
                self.collect_expression_capture_names(
                    &statement.discriminant,
                    param_names,
                    captures,
                );
                for case in &statement.cases {
                    if let Some(test) = &case.test {
                        self.collect_expression_capture_names(test, param_names, captures);
                    }
                    for child in &case.consequent {
                        self.collect_statement_capture_names(child, param_names, captures);
                    }
                }
            }
            _ => {}
        }
    }

    /// Add names declared by a `for...of` or `for...in` left binding to a local name set.
    pub(in crate::lowering) fn collect_for_left_binding_names(left: &ForStatementLeft<'_>, names: &mut HashSet<String>) {
        let ForStatementLeft::VariableDeclaration(decl) = left else {
            return;
        };
        for declarator in &decl.declarations {
            let mut binding_names = Vec::new();
            Self::binding_pattern_names(&declarator.id, &mut binding_names);
            names.extend(binding_names);
        }
    }

}
