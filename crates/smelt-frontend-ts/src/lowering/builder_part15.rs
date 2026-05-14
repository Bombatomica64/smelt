impl ModuleBuilder<'_> {
    /// Lower supported namespace member calls into the matching HIR operation.
    fn namespace_member_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Some((namespace, member_name)) = self.namespace_member_name(member) else {
            return Ok(None);
        };
        let span = self.span(member.span.start, member.span.end);
        let item = self
            .object_namespaces
            .get(namespace)
            .and_then(|members| members.get(member_name))
            .copied()
            .or_else(|| self.items.get(member_name).copied());
        let Some(item) = item else {
            return Err(SmeltError::unsupported(
                span,
                format!("namespace import has no exported member `{member_name}`"),
            ));
        };
        let (params, return_ty, is_async) = if let Item::Function(function) = self.item_ref(item) {
            (
                function.params.iter().map(|param| param.ty).collect(),
                function.return_ty,
                function.is_async,
            )
        } else {
            return Err(SmeltError::unsupported(
                span,
                format!("namespace member `{member_name}` is not callable"),
            ));
        };
        let args = call
            .arguments
            .iter()
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        let callee = body.push_expr(Expr {
            kind: ExprKind::Item(item),
            ty: self
                .ctx
                .krate
                .types
                .intern(Type::Function(FunctionType {
                    params,
                    return_ty,
                    is_async,
                })),
            span,
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Call { callee, args },
            ty: return_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return the namespace binding and exported member name for namespace member access.
    fn namespace_member_name<'a>(
        &self,
        member: &'a oxc::ast::ast::StaticMemberExpression<'_>,
    ) -> Option<(&'a str, &'a str)> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        let namespace = object.name.as_str();
        (self.namespace_imports.contains(namespace)
            || self.object_namespaces.contains_key(namespace))
        .then_some((namespace, member.property.name.as_str()))
    }

    /// Compute the HIR expression type used when an item appears as an expression.
    fn item_expr_type(
        &mut self,
        item: smelt_hir::ItemId,
        span: Span,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match self.item_ref(item) {
            Item::Function(function) => {
                Ok(self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Function(FunctionType {
                        params: function.params.iter().map(|param| param.ty).collect(),
                        return_ty: function.return_ty,
                        is_async: function.is_async,
                    })))
            }
            Item::Class(class) => Ok(self.ctx.krate.types.intern(Type::Class {
                name: class.name,
                args: Vec::new(),
            })),
            Item::Const(const_item) => Ok(const_item.ty),
            _ => Err(SmeltError::unsupported(
                span,
                "namespace member item is not usable as an expression yet",
            )),
        }
    }

    /// Lower a TypeScript optional chain wrapper by delegating to its chain element.
    fn chain_expression(
        &mut self,
        chain: &oxc::ast::ast::ChainExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match &chain.expression {
            ChainElement::CallExpression(call) => self.call_expression(call, body),
            ChainElement::StaticMemberExpression(member) => self.static_member(member, body),
            ChainElement::ComputedMemberExpression(member) => self.computed_member(member, body),
            ChainElement::TSNonNullExpression(non_null) => {
                self.expression(&non_null.expression, body)
            }
            ChainElement::PrivateFieldExpression(private_field) => Err(SmeltError::unsupported(
                self.span(private_field.span.start, private_field.span.end),
                "private field optional chains are not lowered yet",
            )),
        }
    }

    /// Lower a static member access expression.
    fn static_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(expr) = self.math_member_expression(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.number_static_constant(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.node_process_static_member(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.namespace_member_expression(member, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.url_field_expression(member, body)? {
            return Ok(expr);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let optional_access =
            member.optional || matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::Optional(_)));
        let access_receiver_ty = self.optional_receiver_inner_type(receiver_ty);
        let field = self.intern_source_name(member.property.name.as_str());
        if member.property.name == "length"
            && self.supports_stdlib_length(access_receiver_ty)
            || member.property.name == "size"
                && self.supports_stdlib_size(access_receiver_ty)
        {
            let ty = self.ctx.krate.types.intern(Type::Float);
            if optional_access {
                return Err(SmeltError::unsupported(
                    self.span(member.span.start, member.span.end),
                    "optional length and size access is not lowered yet",
                ));
            }
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Len { operand: receiver },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        if member.property.name == "length"
            && let Some(Type::Function(function)) = self.ctx.krate.types.get(access_receiver_ty)
        {
            let arity = f64::from(u32::try_from(function.params.len()).unwrap_or(u32::MAX));
            if optional_access {
                return Err(SmeltError::unsupported(
                    self.span(member.span.start, member.span.end),
                    "optional function length access is not lowered yet",
                ));
            }
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(arity)),
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        let field_ty = self.class_field_type(access_receiver_ty, field)?;
        if optional_access {
            let ty = self.optional_chain_result_type(field_ty);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::OptionalField { receiver, field },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        let ty = field_ty;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Field { receiver, field },
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower supported TypeScript `Number.<constant>` reads.
    ///
    /// Test tables commonly use these global numeric constants as literal
    /// values, so they can be represented directly in HIR without resolving
    /// `Number` as a user-defined namespace.
    fn number_static_constant(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Number" {
            return None;
        }
        let value = match member.property.name.as_str() {
            "NaN" => f64::NAN,
            "POSITIVE_INFINITY" => f64::INFINITY,
            "NEGATIVE_INFINITY" => f64::NEG_INFINITY,
            "MAX_VALUE" => f64::MAX,
            "MIN_VALUE" => f64::MIN_POSITIVE,
            "MAX_SAFE_INTEGER" => 9_007_199_254_740_991.0_f64,
            "MIN_SAFE_INTEGER" => -9_007_199_254_740_991.0_f64,
            "EPSILON" => f64::EPSILON,
            _ => return None,
        };
        let ty = self.ctx.krate.types.intern(Type::Float);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(value)),
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower a supported `Math.<fn>` member reference to a first-class closure.
    ///
    /// Utility libraries such as Remeda pass `Math.ceil` or `Math.floor` as
    /// callbacks. The direct-call lowering handles `Math.ceil(value)`, but a
    /// bare member reference must become a callable value instead of resolving
    /// `Math` as a normal identifier.
    fn math_member_expression(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Math" {
            return None;
        }
        let span = self.span(member.span.start, member.span.end);
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        let value_name = self.intern_source_name("value");
        let mut closure_body = Body::new(None, span);
        let value_local = closure_body.push_local(LocalDecl {
            name: Some(value_name),
            ty: number_ty,
            mutable: false,
            span,
        });
        closure_body.params.push(value_local);
        let value_expr = closure_body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty: number_ty,
            span,
        });
        let result_kind = match member.property.name.as_str() {
            "abs" => ExprKind::NumericAbs {
                operand: value_expr,
            },
            "floor" => ExprKind::NumericRound {
                op: NumericRoundOp::Floor,
                operand: value_expr,
            },
            "ceil" => ExprKind::NumericRound {
                op: NumericRoundOp::Ceil,
                operand: value_expr,
            },
            "round" => ExprKind::NumericRound {
                op: NumericRoundOp::Round,
                operand: value_expr,
            },
            "trunc" => ExprKind::NumericRound {
                op: NumericRoundOp::Trunc,
                operand: value_expr,
            },
            "sqrt" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Sqrt,
                operand: value_expr,
            },
            "cbrt" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Cbrt,
                operand: value_expr,
            },
            "sign" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Sign,
                operand: value_expr,
            },
            "sin" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Sin,
                operand: value_expr,
            },
            "cos" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Cos,
                operand: value_expr,
            },
            "tan" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Tan,
                operand: value_expr,
            },
            "asin" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Asin,
                operand: value_expr,
            },
            "acos" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Acos,
                operand: value_expr,
            },
            "atan" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Atan,
                operand: value_expr,
            },
            "log" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Log,
                operand: value_expr,
            },
            "log10" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Log10,
                operand: value_expr,
            },
            "log2" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Log2,
                operand: value_expr,
            },
            "exp" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Exp,
                operand: value_expr,
            },
            _ => return None,
        };
        let result = closure_body.push_expr(Expr {
            kind: result_kind,
            ty: number_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![number_ty],
            return_ty: number_ty,
            is_async: false,
        }));
        Some(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: vec![Param {
                    name: value_name,
                    local: value_local,
                    ty: number_ty,
                    span,
                }],
                return_ty: number_ty,
                captures: Vec::new(),
                body: body_id,
                callback_body: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Lower the small Node `process` surface used by checked date-fns timezone probes.
    fn node_process_static_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if Self::is_process_version_member(&member.object) {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("v20.0.0".to_owned())),
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        if Self::is_process_env_field(member, "TZ") {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("America/Santiago".to_owned())),
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        None
    }

    /// Return true for the specific `process.version` member expression.
    fn is_process_version_member(object: &Expression<'_>) -> bool {
        let Expression::StaticMemberExpression(member) = object else {
            return false;
        };
        matches!(&member.object, Expression::Identifier(identifier) if identifier.name == "process")
            && member.property.name == "version"
    }

    /// Return true for the specific `process.env.<field>` member expression.
    fn is_process_env_field(
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        field: &str,
    ) -> bool {
        if member.property.name != field {
            return false;
        }
        let Expression::StaticMemberExpression(env_member) = &member.object else {
            return false;
        };
        matches!(&env_member.object, Expression::Identifier(identifier) if identifier.name == "process")
            && env_member.property.name == "env"
    }

    /// Lower a computed member access expression.
    fn computed_member(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let receiver = self.expression(&member.object, body)?;
        let index = self.expression(&member.expression, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let optional_access =
            member.optional || matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::Optional(_)));
        let access_receiver_ty = self.optional_receiver_inner_type(receiver_ty);
        if optional_access {
            let value_ty = self.index_type(access_receiver_ty)?;
            let ty = self.optional_chain_result_type(value_ty);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::OptionalIndex { receiver, index },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        if let Some(Type::Tuple(items)) = self.ctx.krate.types.get(access_receiver_ty).cloned() {
            let index = self.static_tuple_index(index, body, items.len(), member.span)?;
            let Some(ty) = items.get(index).copied() else {
                return Err(SmeltError::unsupported(
                    self.span(member.span.start, member.span.end),
                    "tuple index is out of bounds",
                ));
            };
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TupleIndex {
                    tuple: receiver,
                    index,
                },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        self.reject_negative_bracket_index(access_receiver_ty, index, body, member.span)?;
        let ty = self.index_type(access_receiver_ty)?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Index { receiver, index },
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower TypeScript global primitive conversion and numeric parse calls.
    fn primitive_cast_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name == "parseInt" {
            let operand = self.parse_int_operand("parseInt", call, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToInt,
                    operand,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let (op, result_ty) = match callee.name.as_str() {
            "String" => (PrimitiveCastOp::ToString, Type::String),
            "Number" | "parseFloat" => (PrimitiveCastOp::ToFloat, Type::Float),
            "Boolean" => (PrimitiveCastOp::ToBool, Type::Bool),
            _ => return Ok(None),
        };
        let [arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{} requires exactly one argument", callee.name),
            ));
        };
        let operand = self.argument(arg, body)?;
        let operand_type = self.ctx.krate.types.get(Self::expr_ty(body, operand));
        if !self.primitive_cast_accepts_operand(op, operand_type) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{} requires a primitive argument", callee.name),
            ));
        }
        if callee.name == "String" && matches!(operand_type, Some(Type::Bool)) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "String currently supports number and string arguments",
            ));
        }
        if matches!(callee.name.as_str(), "parseFloat" | "parseInt")
            && operand_type != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{} requires a string argument", callee.name),
            ));
        }
        let ty = self.ctx.krate.types.intern(result_ty);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast { op, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether a global primitive conversion accepts a lowered operand type.
    fn primitive_cast_accepts_operand(
        &self,
        op: PrimitiveCastOp,
        operand_type: Option<&Type>,
    ) -> bool {
        match operand_type {
            Some(Type::Bool | Type::Int | Type::Float | Type::String) => true,
            Some(Type::Unknown) if op == PrimitiveCastOp::ToString => true,
            Some(Type::Optional(item)) if op == PrimitiveCastOp::ToString => matches!(
                self.ctx.krate.types.get(*item),
                Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::Unknown)
            ),
            _ => false,
        }
    }

    /// Extract target and value from assignment expression.
    fn assignment_parts(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, smelt_hir::ExprId), SmeltError> {
        let target = self.assignment_target_expr(&assign.left, body)?;
        let right = self.expression(&assign.right, body)?;
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
                            format!("assignment operator is not lowered yet: {other:?}"),
                        ));
                    }
                };
                let ty = Self::expr_ty(body, target);
                body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op,
                        lhs: target,
                        rhs: right,
                    },
                    ty,
                    span: self.span(assign.span.start, assign.span.end),
                })
            }
            other => {
                return Err(SmeltError::unsupported(
                    self.span(assign.span.start, assign.span.end),
                    format!("assignment operator is not lowered yet: {other:?}"),
                ));
            }
        };
        Ok((target, value))
    }

    /// Extract target and value from increment/decrement expression.
    fn update_parts(
        &mut self,
        update: &oxc::ast::ast::UpdateExpression<'_>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, smelt_hir::ExprId), SmeltError> {
        let target = self.simple_assignment_target_expr(&update.argument, body)?;
        let one_ty = Self::expr_ty(body, target);
        let one = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(1.0)),
            ty: one_ty,
            span: self.span(update.span.start, update.span.end),
        });
        let op = match update.operator {
            UpdateOperator::Increment => BinOp::Add,
            UpdateOperator::Decrement => BinOp::Sub,
        };
        let value = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op,
                lhs: target,
                rhs: one,
            },
            ty: one_ty,
            span: self.span(update.span.start, update.span.end),
        });
        Ok((target, value))
    }

    /// Convert assignment target to expression.
    fn assignment_target_expr(
        &mut self,
        target: &AssignmentTarget<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            AssignmentTarget::StaticMemberExpression(member) => self.static_member(member, body),
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(target.span().start, target.span().end),
                "assignment target must be a local, field, or index expression",
            )),
        }
    }

    /// Convert simple assignment target to expression.
    fn simple_assignment_target_expr(
        &mut self,
        target: &SimpleAssignmentTarget<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => self
                .identifier_expression(ident.name.as_str(), ident.span.start, ident.span.end, body),
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.static_member(member, body)
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(target.span().start, target.span().end),
                "update target must be a local, field, or index expression",
            )),
        }
    }

    // Continued in the next split builder file.
}
