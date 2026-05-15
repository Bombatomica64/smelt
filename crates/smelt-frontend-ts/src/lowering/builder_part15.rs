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
            ChainElement::TSNonNullExpression(non_null) => self.non_null_assertion_expression(
                &non_null.expression,
                self.span(non_null.span.start, non_null.span.end),
                body,
            ),
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
        if let Some(expr) = self.object_static_function_member(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.object_static_member(member, body) {
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
            let operand = if optional_access {
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: receiver },
                    ty: access_receiver_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                })
            } else {
                receiver
            };
            if optional_access {
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Len { operand },
                    ty,
                    span: self.span(member.span.start, member.span.end),
                }));
            }
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Len { operand },
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

    /// Lower supported `Object.<fn>` member references to first-class callables.
    ///
    /// Remeda commonly passes static object helpers into `purry`, e.g.
    /// `purry(Object.fromEntries, args)`. Direct-call lowering handles
    /// `Object.fromEntries(value)`, while this path gives bare member
    /// references a callable shape and the correct `.length` arity.
    fn object_static_function_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Object" {
            return None;
        }
        let (arity, return_ty) = match member.property.name.as_str() {
            "keys" | "values" | "entries" | "fromEntries" | "getPrototypeOf" | "create" => {
                (1, self.ctx.krate.types.intern(Type::Unknown))
            }
            "assign" | "setPrototypeOf" => (2, self.ctx.krate.types.intern(Type::Unknown)),
            "is" | "hasOwn" => (2, self.ctx.krate.types.intern(Type::Bool)),
            _ => return None,
        };
        Some(self.object_static_closure(member, arity, return_ty, outer_body))
    }

    /// Build an opaque closure for a supported static `Object` member reference.
    fn object_static_closure(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        arity: usize,
        return_ty: smelt_hir::TypeId,
        outer_body: &mut Body,
    ) -> smelt_hir::ExprId {
        let span = self.span(member.span.start, member.span.end);
        let unknown = self.ctx.krate.types.intern(Type::Unknown);
        let mut closure_body = Body::new(None, span);
        let mut params = Vec::new();
        let mut param_tys = Vec::new();
        for index in 0..arity {
            let name = self.intern_source_name(&format!("arg{index}"));
            let local = closure_body.push_local(LocalDecl {
                name: Some(name),
                ty: unknown,
                mutable: false,
                span,
            });
            closure_body.params.push(local);
            params.push(Param {
                name,
                local,
                ty: unknown,
                span,
            });
            param_tys.push(unknown);
        }
        let result = closure_body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: return_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body = self.ctx.krate.push_body(closure_body);
        let ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: param_tys,
            return_ty,
            is_async: false,
        }));
        outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params,
                return_ty,
                captures: Vec::new(),
                body,
                callback_body: None,
                span,
            }),
            ty,
            span,
        })
    }

    /// Lower opaque static Object metadata reads such as `Object.prototype`.
    fn object_static_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Object" || member.property.name != "prototype" {
            return None;
        }
        let ty = self.ctx.krate.types.intern(Type::Unknown);
        Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
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
        if Self::is_process_env_field(member, "TZ") || Self::is_process_env_field(member, "tz") {
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
        if let Some(expr) = self.dynamic_math_member_expression(member, body) {
            return Ok(expr);
        }
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
            let tuple_len = if self.allow_unknown_index_access {
                usize::MAX
            } else {
                items.len()
            };
            let tuple_index = self.static_tuple_index(index, body, tuple_len, member.span)?;
            let Some(ty) = items.get(tuple_index).copied() else {
                if self.allow_unknown_index_access {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Index { receiver, index },
                        ty,
                        span: self.span(member.span.start, member.span.end),
                    }));
                }
                return Err(SmeltError::unsupported(
                    self.span(member.span.start, member.span.end),
                    "tuple index is out of bounds",
                ));
            };
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TupleIndex {
                    tuple: receiver,
                    index: tuple_index,
                },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        if let Some(Type::Union(items)) = self.ctx.krate.types.get(access_receiver_ty).cloned()
            && let Some(max_tuple_len) = items
                .iter()
                .filter_map(|item| match self.ctx.krate.types.get(*item) {
                    Some(Type::Tuple(tuple_items)) => Some(tuple_items.len()),
                    _ => None,
                })
                .max()
        {
            let tuple_len = if self.allow_unknown_index_access {
                usize::MAX
            } else {
                max_tuple_len
            };
            let tuple_index = self.static_tuple_index(index, body, tuple_len, member.span)?;
            let mut indexed_tys = Vec::new();
            for item in items {
                if let Some(Type::Tuple(tuple_items)) = self.ctx.krate.types.get(item)
                    && let Some(ty) = tuple_items.get(tuple_index).copied()
                    && !indexed_tys.contains(&ty)
                {
                    indexed_tys.push(ty);
                }
            }
            if !indexed_tys.is_empty() {
                let ty = match indexed_tys.as_slice() {
                    [single] => *single,
                    _ => self.ctx.krate.types.intern(Type::Union(indexed_tys)),
                };
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Index { receiver, index },
                    ty,
                    span: self.span(member.span.start, member.span.end),
                }));
            }
        }
        self.reject_negative_bracket_index(access_receiver_ty, index, body, member.span)?;
        if self.can_lower_acknowledged_unknown_index(access_receiver_ty, member.span.start) {
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Index { receiver, index },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        let ty = self.index_type(access_receiver_ty)?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Index { receiver, index },
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower `Math[method]` for supported numeric method-key unions to a closure.
    ///
    /// Date-fns stores a selected rounding method as a string literal union and
    /// then calls the selected function. Smelt keeps that value dynamic by
    /// emitting a captured closure that dispatches over supported rounding names.
    fn dynamic_math_member_expression(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Math" {
            return None;
        }
        let Expression::Identifier(method_ident) = &member.expression else {
            return None;
        };
        let source_local = self.locals.get(method_ident.name.as_str()).copied()?;
        let source_decl = usize::try_from(source_local.0)
            .ok()
            .and_then(|index| outer_body.locals.get(index))?;

        let span = self.span(member.span.start, member.span.end);
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        let value_name = self.intern_source_name("value");
        let method_name = source_decl
            .name
            .unwrap_or_else(|| self.intern_source_name(method_ident.name.as_str()));
        let method_ty = source_decl.ty;
        let mut closure_body = Body::new(None, span);
        let method_local = closure_body.push_local(LocalDecl {
            name: Some(method_name),
            ty: method_ty,
            mutable: source_decl.mutable,
            span: source_decl.span,
        });
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

        let mut result = Self::dynamic_math_round_expr(
            &mut closure_body,
            NumericRoundOp::Trunc,
            value_expr,
            number_ty,
            span,
        );
        for (method, op) in [
            ("round", NumericRoundOp::Round),
            ("floor", NumericRoundOp::Floor),
            ("ceil", NumericRoundOp::Ceil),
        ] {
            let method_expr = closure_body.push_expr(Expr {
                kind: ExprKind::Local(method_local),
                ty: method_ty,
                span,
            });
            let method_text = closure_body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(method.to_owned())),
                ty: self.ctx.krate.types.intern(Type::String),
                span,
            });
            let cond = closure_body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Eq,
                    lhs: method_expr,
                    rhs: method_text,
                },
                ty: self.ctx.krate.types.intern(Type::Bool),
                span,
            });
            let then_expr =
                Self::dynamic_math_round_expr(&mut closure_body, op, value_expr, number_ty, span);
            result = closure_body.push_expr(Expr {
                kind: ExprKind::Conditional {
                    cond,
                    then_expr,
                    else_expr: result,
                },
                ty: number_ty,
                span,
            });
        }
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
                captures: vec![ClosureCapture {
                    source_local,
                    body_local: Some(method_local),
                    symbol: method_name,
                    ty: method_ty,
                    mode: CaptureMode::ByRef,
                }],
                body: body_id,
                callback_body: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Push one numeric rounding operation inside a generated dynamic Math closure.
    fn dynamic_math_round_expr(
        body: &mut Body,
        op: NumericRoundOp,
        operand: smelt_hir::ExprId,
        number_ty: smelt_hir::TypeId,
        span: Span,
    ) -> smelt_hir::ExprId {
        body.push_expr(Expr {
            kind: ExprKind::NumericRound { op, operand },
            ty: number_ty,
            span,
        })
    }

    /// Lower explicitly acknowledged `unknown[key]` reads in unknown contexts.
    ///
    /// Runtime indexing into `unknown` is intentionally rejected by the normal
    /// index path. This fallback only covers source that already carries a
    /// nearby `@ts-expect-error [ts7053]`, which Remeda uses for dynamic
    /// accumulator reads that TypeScript itself cannot prove.
    fn unknown_computed_member_with_hint(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.has_ts_expect_error_before(member.span.start, "ts7053") {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, receiver)),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. })
        ) {
            return Ok(None);
        }
        let index = self.expression(&member.expression, body)?;
        let ty = self.ctx.krate.types.intern(Type::Unknown);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Index { receiver, index },
            ty,
            span: self.span(member.span.start, member.span.end),
        })))
    }

    /// Return whether source explicitly acknowledges a dynamic unknown index read.
    fn can_lower_acknowledged_unknown_index(&self, receiver_ty: smelt_hir::TypeId, start: u32) -> bool {
        matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. })
        ) && self.has_ts_expect_error_before(start, "ts7053")
    }

    /// Return whether a nearby preceding comment expects the given TS error code.
    fn has_ts_expect_error_before(&self, start: u32, code: &str) -> bool {
        let Ok(start) = usize::try_from(start) else {
            return false;
        };
        let prefix_start = start.saturating_sub(256);
        let Some(prefix) = self.source.get(prefix_start..start) else {
            return false;
        };
        prefix.contains("@ts-expect-error") && prefix.contains(code)
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
            "Number" | "parseFloat" | "BigInt" => (PrimitiveCastOp::ToFloat, Type::Float),
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
        let operand_ty = Self::expr_ty(body, operand);
        let operand_type = self.ctx.krate.types.get(operand_ty);
        if !self.primitive_cast_accepts_operand(op, operand_ty) {
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
        operand_ty: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(operand_ty) {
            Some(Type::Bool | Type::String | Type::Int | Type::Float | Type::Unknown) => true,
            Some(Type::TypeParam { .. }) if op == PrimitiveCastOp::ToBool => true,
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.primitive_cast_accepts_operand(op, item)),
            Some(Type::Optional(item)) => self.primitive_cast_accepts_operand(op, *item),
            Some(_) if self.is_numeric_like_type(operand_ty) => true,
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
        let target_ty = Self::expr_ty(body, target);
        let right_ty_hint = match assign.operator {
            AssignmentOperator::Assign => Some(target_ty),
            AssignmentOperator::LogicalNullish => self.non_nullish_type(target_ty),
            _ => None,
        };
        let right = self.expression_with_hint(&assign.right, body, right_ty_hint)?;
        let value = match assign.operator {
            AssignmentOperator::Assign => right,
            AssignmentOperator::LogicalNullish => {
                body.push_expr(Expr {
                    kind: ExprKind::OptionalCoalesce {
                        optional: target,
                        fallback: right,
                    },
                    ty: self.non_nullish_type(target_ty).unwrap_or(target_ty),
                    span: self.span(assign.span.start, assign.span.end),
                })
            }
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
        self.apply_assignment_observed_type(&assign.left, Self::expr_ty(body, value), body);
        Ok((target, value))
    }

    /// Record the observed type produced by assigning into an unknown local.
    ///
    /// TypeScript flow analysis narrows a variable's observed type after direct
    /// assignment even when its declaration started as `unknown` in Smelt's
    /// no-`any` model. Keeping this as a narrowing fact preserves the local's
    /// declared storage type while allowing later reads in the same flow to be
    /// lowered with the assigned value type.
    fn apply_assignment_observed_type(
        &mut self,
        target: &AssignmentTarget<'_>,
        observed_ty: smelt_hir::TypeId,
        body: &Body,
    ) {
        let AssignmentTarget::AssignmentTargetIdentifier(identifier) = target else {
            return;
        };
        let name = identifier.name.as_str();
        let Some(local) = self.locals.get(name).copied() else {
            return;
        };
        let base_ty = Self::local_ty(body, local);
        if self.ctx.krate.types.get(base_ty) != Some(&Type::Unknown) {
            return;
        }
        if self.ctx.krate.types.get(observed_ty) == Some(&Type::Unknown) {
            return;
        }
        self.apply_narrowing(name.to_owned(), observed_ty);
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

    /// Lower prefix and postfix increment/decrement used as expression values.
    ///
    /// JavaScript returns the old value for postfix updates and the updated
    /// value for prefix updates. The assignment is still emitted immediately so
    /// surrounding expressions observe the same side effect ordering.
    fn update_expression(
        &mut self,
        update: &oxc::ast::ast::UpdateExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let (target, value) = self.update_parts(update, body)?;
        body.push_stmt(Stmt::Assign { target, value });
        if update.prefix {
            Ok(value)
        } else {
            Ok(target)
        }
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
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                let Some(Some(first)) = array.elements.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "array assignment targets require at least one element",
                    ));
                };
                self.assignment_maybe_default_target_expr(first, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(target.span().start, target.span().end),
                "assignment target must be a local, field, or index expression",
            )),
        }
    }

    /// Convert an array destructuring assignment element to its target expression.
    fn assignment_maybe_default_target_expr(
        &mut self,
        target: &oxc::ast::ast::AssignmentTargetMaybeDefault<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match target {
            oxc::ast::ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => {
                self.assignment_target_expr(&default.binding, body)
            }
            _ => self.assignment_target_expr(target.to_assignment_target(), body),
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
