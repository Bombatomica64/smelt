impl ModuleBuilder<'_> {
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
                .intern(Type::Function(smelt_hir::FunctionType {
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
                    .intern(Type::Function(smelt_hir::FunctionType {
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

    /// Lower a static member access expression.
    fn static_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if member.optional {
            return Err(SmeltError::unsupported(
                self.span(member.span.start, member.span.end),
                "optional member access is not lowered yet",
            ));
        }
        if let Some(expr) = self.namespace_member_expression(member, body)? {
            return Ok(expr);
        }
        let receiver = self.expression(&member.object, body)?;
        let field = self.intern_source_name(member.property.name.as_str());
        if member.property.name == "length"
            && self.supports_stdlib_length(Self::expr_ty(body, receiver))
            || member.property.name == "size"
                && self.supports_stdlib_size(Self::expr_ty(body, receiver))
        {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Len { operand: receiver },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        let ty = self.class_field_type(Self::expr_ty(body, receiver), field)?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Field { receiver, field },
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower a computed member access expression.
    fn computed_member(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if member.optional {
            return Err(SmeltError::unsupported(
                self.span(member.span.start, member.span.end),
                "optional index access is not lowered yet",
            ));
        }
        let receiver = self.expression(&member.object, body)?;
        let index = self.expression(&member.expression, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        if let Some(Type::Tuple(items)) = self.ctx.krate.types.get(receiver_ty).cloned() {
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
        self.reject_negative_bracket_index(receiver_ty, index, body, member.span)?;
        let ty = self.index_type(receiver_ty)?;
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
        let (op, result_ty) = match callee.name.as_str() {
            "String" => (PrimitiveCastOp::ToString, Type::String),
            "Number" | "parseFloat" => (PrimitiveCastOp::ToFloat, Type::Float),
            "Boolean" => (PrimitiveCastOp::ToBool, Type::Bool),
            "parseInt" => (PrimitiveCastOp::ToInt, Type::Float),
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
        if !matches!(
            operand_type,
            Some(Type::Bool | Type::Int | Type::Float | Type::String)
        ) {
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
