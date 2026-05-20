impl ModuleBuilder<'_> {
    /// Lower supported `Map` projection calls into HIR collection operations.
    fn map_projection_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "keys" => DictProjectionOp::Keys,
            "values" => DictProjectionOp::Values,
            "entries" => DictProjectionOp::Entries,
            _ => return Ok(None),
        };
        if let [dict_argument] = call.arguments.as_slice() {
            return self.static_dict_projection_utility_call(call, body, op, dict_argument);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map keys/values/entries require no arguments",
            ));
        }
        let dict = self.expression(&member.object, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let value_ty = *dict_value_ty;
        let ty = match op {
            DictProjectionOp::Keys => self.ctx.krate.types.intern(Type::List(key_ty)),
            DictProjectionOp::Values => self.ctx.krate.types.intern(Type::List(value_ty)),
            DictProjectionOp::Entries => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![key_ty, value_ty]));
                self.ctx.krate.types.intern(Type::List(entry_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictProjection { op, dict },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower utility-style `keys(value)`, `values(value)`, and `entries(value)` calls.
    ///
    /// Libraries such as Lodash expose these as namespace functions instead of
    /// receiver methods. The callee namespace is intentionally ignored here:
    /// once TypeScript has accepted a static member call with a single value
    /// argument, the frontend can lower the projection through the same record
    /// operation used by `Object.keys`.
    fn static_dict_projection_utility_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        op: DictProjectionOp,
        dict_argument: &Argument<'_>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let mut dict = self.argument(dict_argument, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let (key_ty, value_ty) = match self.ctx.krate.types.get(dict_ty) {
            Some(Type::Dict(key_ty, value_ty)) => (*key_ty, *value_ty),
            Some(
                Type::Unknown
                | Type::TypeParam { .. }
                | Type::Class { .. }
                | Type::String
                | Type::Bool,
            ) => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let target = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                dict = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: dict,
                        target,
                    },
                    ty: target,
                    span: self.span(dict_argument.span().start, dict_argument.span().end),
                });
                (key_ty, value_ty)
            }
            Some(Type::Union(items)) if items.iter().all(|item| self.object_keys_compatible_type(*item)) => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let target = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                dict = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: dict,
                        target,
                    },
                    ty: target,
                    span: self.span(dict_argument.span().start, dict_argument.span().end),
                });
                (key_ty, value_ty)
            }
            _ => return Ok(None),
        };
        let ty = match op {
            DictProjectionOp::Keys => self.ctx.krate.types.intern(Type::List(key_ty)),
            DictProjectionOp::Values => self.ctx.krate.types.intern(Type::List(value_ty)),
            DictProjectionOp::Entries => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![key_ty, value_ty]));
                self.ctx.krate.types.intern(Type::List(entry_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictProjection { op, dict },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Set` projection methods.
    fn set_projection_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "keys" | "values" => SetProjectionOp::Values,
            "entries" => SetProjectionOp::Entries,
            _ => return Ok(None),
        };
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Set keys/values/entries require no arguments",
            ));
        }
        let set = self.expression(&member.object, body)?;
        let set_ty = Self::expr_ty(body, set);
        let Some(Type::Set(set_item_ty)) = self.ctx.krate.types.get(set_ty) else {
            return Ok(None);
        };
        let item_ty = *set_item_ty;
        let ty = match op {
            SetProjectionOp::Values => self.ctx.krate.types.intern(Type::List(item_ty)),
            SetProjectionOp::Entries => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![item_ty, item_ty]));
                self.ctx.krate.types.intern(Type::List(entry_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::SetProjection { op, set },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.concat` for one same-typed array argument.
    fn list_concat_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "concat" {
            return Ok(None);
        }
        let [right_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array concat currently requires exactly one array argument",
            ));
        };
        let mut left = self.expression(&member.object, body)?;
        let mut ty = Self::expr_ty(body, left);
        let item_ty = match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::List(list_item_ty)) => list_item_ty,
            Some(Type::Tuple(items)) => {
                let item_ty = self.tuple_items_element_type(&items);
                ty = self.ctx.krate.types.intern(Type::List(item_ty));
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                item_ty
            }
            Some(Type::Union(items))
                if items.iter().any(|item| {
                    matches!(
                        self.ctx.krate.types.get(*item),
                        Some(Type::List(_) | Type::Unknown | Type::TypeParam { .. } | Type::Class { .. })
                    )
                }) =>
            {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                ty = self.ctx.krate.types.intern(Type::List(item_ty));
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                item_ty
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) | None => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                ty = self.ctx.krate.types.intern(Type::List(item_ty));
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                item_ty
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array concat requires an array receiver",
                ));
            }
        };
        let mut right = self.argument(right_argument, body)?;
        let right_ty = Self::expr_ty(body, right);
        let right = if right_ty == ty {
            right
        } else if right_ty == item_ty {
            body.push_expr(Expr {
                kind: ExprKind::ListLit(vec![right]),
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else if self.erased_or_union_surface(right_ty)
            || self.ctx.krate.types.get(right_ty) == Some(&Type::None)
        {
            if self.ctx.krate.types.get(right_ty) != Some(&Type::List(item_ty)) {
                right = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert {
                        value: right,
                    },
                    ty: item_ty,
                    span: self.span(right_argument.span().start, right_argument.span().end),
                });
            }
            body.push_expr(Expr {
                kind: ExprKind::ListLit(vec![right]),
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array concat requires an array or element argument matching the receiver",
            ));
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListConcat { left, right },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower callback-heavy TypeScript array methods.
    fn list_callback_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "map" => ListCallbackOp::Map,
            "filter" => ListCallbackOp::Filter,
            "find" => ListCallbackOp::Find,
            "findIndex" => ListCallbackOp::FindIndex,
            "some" => ListCallbackOp::Some,
            "every" => ListCallbackOp::Every,
            "forEach" => ListCallbackOp::ForEach,
            _ => return Ok(None),
        };
        let [callback_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array callback methods require exactly one callback argument",
            ));
        };
        let mut list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let list_ty = match self.ctx.krate.types.get(list_ty).cloned() {
            Some(Type::List(_)) => list_ty,
            Some(Type::Tuple(items)) => {
                let item_ty = self.tuple_items_element_type(&items);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                asserted_ty
            }
            Some(Type::Union(items))
                if items.iter().any(|item| {
                    matches!(
                        self.ctx.krate.types.get(*item),
                        Some(Type::List(_) | Type::Unknown | Type::TypeParam { .. })
                    )
                }) =>
            {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                asserted_ty
            }
            _ => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                asserted_ty
            }
        };
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array callback method receiver must be an array",
            ));
        };
        let element_ty = *list_element_ty;
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let fallback_return_ty = match op {
            ListCallbackOp::Map | ListCallbackOp::FlatMap => unknown_ty,
            ListCallbackOp::Filter
            | ListCallbackOp::Find
            | ListCallbackOp::FindIndex
            | ListCallbackOp::FindLast
            | ListCallbackOp::FindLastIndex
            | ListCallbackOp::Some
            | ListCallbackOp::Every => bool_ty,
            ListCallbackOp::ForEach => self.ctx.krate.types.intern(Type::None),
        };
        let callback_param_tys = [element_ty, index_ty, list_ty];
        let callback = if matches!(
            op,
            ListCallbackOp::Filter
                | ListCallbackOp::Find
                | ListCallbackOp::FindIndex
                | ListCallbackOp::FindLast
                | ListCallbackOp::FindLastIndex
                | ListCallbackOp::Some
                | ListCallbackOp::Every
        ) {
            self.truthy_callback_argument_with_body_fallback(
                callback_argument,
                &callback_param_tys,
                "array callback",
                body,
            )?
        } else {
            self.callback_argument_with_body_fallback(
                callback_argument,
                &callback_param_tys,
                fallback_return_ty,
                "array callback",
                body,
            )?
        };
        let ty = match op {
            ListCallbackOp::Map => self.ctx.krate.types.intern(Type::List(callback.return_ty)),
            ListCallbackOp::Filter => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array filter")?;
                list_ty
            }
            ListCallbackOp::Find => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array find")?;
                self.ctx.krate.types.intern(Type::Optional(element_ty))
            }
            ListCallbackOp::FindIndex => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array findIndex")?;
                self.ctx.krate.types.intern(Type::Float)
            }
            ListCallbackOp::FindLast => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array findLast")?;
                self.ctx.krate.types.intern(Type::Optional(element_ty))
            }
            ListCallbackOp::FindLastIndex => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array findLastIndex")?;
                self.ctx.krate.types.intern(Type::Float)
            }
            ListCallbackOp::Some => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array some")?;
                bool_ty
            }
            ListCallbackOp::Every => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array every")?;
                bool_ty
            }
            ListCallbackOp::ForEach => self.ctx.krate.types.intern(Type::None),
            ListCallbackOp::FlatMap => {
                let Some(Type::List(item_ty)) = self.ctx.krate.types.get(callback.return_ty) else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "array flatMap callback must return an array",
                    ));
                };
                self.ctx.krate.types.intern(Type::List(*item_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListCallback {
                op,
                list,
                callback: callback.expr,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower lodash `_.forEach(collection, callback)` over array-like or object-like inputs.
    fn lodash_for_each_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "forEach" {
            return Ok(None);
        }
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "_" || !self.value_imports.contains("_") {
            return Ok(None);
        }
        let [collection_arg, callback_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "lodash forEach requires collection and callback arguments",
            ));
        };
        let mut list = self.argument(collection_arg, body)?;
        let list_ty = Self::expr_ty(body, list);
        let (list_ty, item_ty) = match self.ctx.krate.types.get(list_ty).cloned() {
            Some(Type::List(item_ty)) => (list_ty, item_ty),
            Some(Type::Tuple(items)) => {
                let item_ty = self.tuple_items_element_type(&items);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(collection_arg.span().start, collection_arg.span().end),
                });
                (asserted_ty, item_ty)
            }
            Some(Type::Dict(_, value_ty)) => {
                let projected_ty = self.ctx.krate.types.intern(Type::List(value_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::DictProjection {
                        op: DictProjectionOp::Values,
                        dict: list,
                    },
                    ty: projected_ty,
                    span: self.span(collection_arg.span().start, collection_arg.span().end),
                });
                (projected_ty, value_ty)
            }
            _ => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(collection_arg.span().start, collection_arg.span().end),
                });
                (asserted_ty, item_ty)
            }
        };
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let callback_param_tys = [item_ty, index_ty, list_ty];
        let callback_expr = if let Argument::ArrowFunctionExpression(arrow) = callback_arg {
            self.arrow_closure_body_expr(arrow, &callback_param_tys, none_ty, body)?
        } else {
            self.callback_argument_with_body_fallback(
                callback_arg,
                &callback_param_tys,
                none_ty,
                "lodash forEach",
                body,
            )?
            .expr
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListCallback {
                op: ListCallbackOp::ForEach,
                list,
                callback: callback_expr,
            },
            ty: none_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower Strapi's imported `async.map(collection, callback, options?)` helper.
    fn strapi_async_map_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "map" {
            return Ok(None);
        }
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "async" || !self.value_imports.contains("async") {
            return Ok(None);
        }
        let [collection_arg, callback_arg, trailing_args @ ..] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Strapi async.map requires collection and callback arguments",
            ));
        };
        let mut list = self.argument(collection_arg, body)?;
        let list_ty = Self::expr_ty(body, list);
        let (list_ty, item_ty) = match self.ctx.krate.types.get(list_ty).cloned() {
            Some(Type::List(item_ty)) => (list_ty, item_ty),
            Some(Type::Tuple(items)) => {
                let item_ty = self.tuple_items_element_type(&items);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(collection_arg.span().start, collection_arg.span().end),
                });
                (asserted_ty, item_ty)
            }
            _ => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(collection_arg.span().start, collection_arg.span().end),
                });
                (asserted_ty, item_ty)
            }
        };
        for arg in trailing_args {
            let _ = self.argument(arg, body)?;
        }
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let callback_param_tys = [item_ty, index_ty, list_ty];
        let callback = self.callback_argument_with_body_fallback(
            callback_arg,
            &callback_param_tys,
            unknown_ty,
            "Strapi async.map",
            body,
        )?;
        let list_result_ty = self.ctx.krate.types.intern(Type::List(callback.return_ty));
        let ty = self.ctx.krate.types.intern(Type::Future(list_result_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListCallback {
                op: ListCallbackOp::Map,
                list,
                callback: callback.expr,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `Array.prototype.reduce`, including element-typed calls without an initial value.
    fn list_reduce_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "reduce" {
            return Ok(None);
        }
        if Self::is_static_reduce_utility_call(call) {
            return self.static_reduce_utility_call(call, body, member);
        }
        let ([callback_argument] | [callback_argument, _]) = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array reduce requires callback and at most one initial value",
            ));
        };
        let list = self.expression(&member.object, body)?;
        let list_span = member.object.span();
        self.lower_list_reduce(
            call,
            body,
            list,
            list_span.start,
            list_span.end,
            callback_argument,
            call.arguments.get(1),
        )
    }

    /// Check whether a `reduce` call is the utility form `ns.reduce(value, callback, initial?)`.
    fn is_static_reduce_utility_call(call: &oxc::ast::ast::CallExpression<'_>) -> bool {
        match call.arguments.as_slice() {
            [first, _, _] => !Self::argument_is_callback_like(first),
            [first, second] => {
                !Self::argument_is_callback_like(first) && Self::argument_is_callback_like(second)
            }
            _ => false,
        }
    }

    /// Return true for argument nodes that represent callback values directly.
    fn argument_is_callback_like(argument: &Argument<'_>) -> bool {
        match argument {
            Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_) => true,
            Argument::TSAsExpression(as_expr) => Self::expression_is_callback_like(&as_expr.expression),
            Argument::TSTypeAssertion(assertion) => {
                Self::expression_is_callback_like(&assertion.expression)
            }
            Argument::TSSatisfiesExpression(satisfies) => {
                Self::expression_is_callback_like(&satisfies.expression)
            }
            Argument::TSNonNullExpression(non_null) => {
                Self::expression_is_callback_like(&non_null.expression)
            }
            _ => false,
        }
    }

    /// Return true for expression nodes that represent callback values directly.
    fn expression_is_callback_like(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => true,
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::expression_is_callback_like(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => Self::expression_is_callback_like(&as_expr.expression),
            Expression::TSTypeAssertion(assertion) => {
                Self::expression_is_callback_like(&assertion.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::expression_is_callback_like(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::expression_is_callback_like(&non_null.expression)
            }
            _ => false,
        }
    }

    /// Lower utility-style `reduce(collection, callback, initial?)` calls.
    fn static_reduce_utility_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let [collection_arg, callback_arg, rest @ ..] = call.arguments.as_slice() else {
            return Ok(None);
        };
        if rest.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array reduce requires callback and at most one initial value",
            ));
        }
        let collection = self.argument(collection_arg, body)?;
        let collection_ty = Self::expr_ty(body, collection);
        if matches!(
            self.ctx.krate.types.get(collection_ty),
            Some(Type::List(_) | Type::Tuple(_))
        ) {
            return self.lower_list_reduce(
                call,
                body,
                collection,
                collection_arg.span().start,
                collection_arg.span().end,
                callback_arg,
                rest.first(),
            );
        }
        let initial = if let Some(initial_arg) = rest.first() {
            self.argument(initial_arg, body)?
        } else {
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(call.span.start, call.span.end),
            })
        };
        let _ = self.argument(callback_arg, body)?;
        let _ = self.expression(&member.object, body)?;
        Ok(Some(initial))
    }

    /// Lower a normalized array-reduce receiver and callback pair.
    fn lower_list_reduce(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        mut list: smelt_hir::ExprId,
        list_span_start: u32,
        list_span_end: u32,
        callback_argument: &Argument<'_>,
        initial_argument: Option<&Argument<'_>>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let mut list_ty = Self::expr_ty(body, list);
        let element_ty = if let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty)
        {
            *list_element_ty
        } else if let Some(Type::Tuple(items)) = self.ctx.krate.types.get(list_ty).cloned() {
            let item_ty = self.flattened_tuple_item_type(items);
            let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
            list = body.push_expr(Expr {
                kind: ExprKind::TypeAssert {
                    value: list,
                },
                ty: asserted_ty,
                span: self.span(list_span_start, list_span_end),
            });
            list_ty = asserted_ty;
            item_ty
        } else {
            let item_ty = self.ctx.krate.types.intern(Type::Unknown);
            let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
            list = body.push_expr(Expr {
                kind: ExprKind::TypeAssert {
                    value: list,
                },
                ty: asserted_ty,
                span: self.span(list_span_start, list_span_end),
            });
            list_ty = asserted_ty;
            item_ty
        };
        let initial = if let Some(initial_argument) = initial_argument {
            Some(self.argument(initial_argument, body)?)
        } else {
            None
        };
        let accumulator_ty = initial.map_or(element_ty, |initial| Self::expr_ty(body, initial));
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        let callback_param_tys = [accumulator_ty, element_ty, index_ty, list_ty];
        let callback_expr = if let Argument::ArrowFunctionExpression(arrow) = callback_argument {
            self.arrow_closure_body_expr(arrow, &callback_param_tys, accumulator_ty, body)?
        } else {
            let callback =
                self.callback_argument(callback_argument, &callback_param_tys, "array reduce", body)?;
            self.require_callback_ty(callback.return_ty, accumulator_ty, call, "array reduce")?;
            callback.expr
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListReduce {
                list,
                initial,
                callback: callback_expr,
            },
            ty: accumulator_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Validate the inferred callback return type for an array method.
    fn require_callback_ty(
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
    fn callback_expr_to_closure(
        &mut self,
        callback: CallbackExpr,
        params: &[smelt_hir::TypeId],
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        self.callback_expr_to_closure_with_return_ty(callback.ty, callback, params, span, body)
    }

    /// Store a lowered callback expression as a closure with an explicit return type.
    fn callback_expr_to_closure_with_return_ty(
        &mut self,
        return_ty: smelt_hir::TypeId,
        callback: CallbackExpr,
        params: &[smelt_hir::TypeId],
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
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
        let body_id = self.ctx.krate.push_body(closure_body);
        let captures = self.callback_captures(&callback, body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: params.to_vec(),
            return_ty,
            is_async: false,
        }));
        body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                return_ty,
                captures,
                body: body_id,
                callback_body: Some(callback),
                span,
            }),
            ty: closure_ty,
            span,
        })
    }

    /// Collect explicit captures from a callback expression tree.
    fn callback_captures(&mut self, callback: &CallbackExpr, body: &Body) -> Vec<ClosureCapture> {
        let mut captures = HashMap::new();
        self.collect_callback_captures(callback, body, &mut captures);
        captures.into_values().collect()
    }

    /// Instantiate a stored local-callback default expression at one call site.
    fn local_callback_default_expr(
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
    fn callback_expr_to_body_expr(
        &mut self,
        callback: &CallbackExpr,
        args: &[smelt_hir::ExprId],
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match &callback.kind {
            CallbackExprKind::Param(index) => args.get(*index).copied().ok_or_else(|| {
                SmeltError::unsupported(span, "default argument references an unavailable parameter")
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
            CallbackExprKind::DictLit(_)
            | CallbackExprKind::Sequence { .. }
            | CallbackExprKind::Throw { .. }
            | CallbackExprKind::HasDynamicField { .. }
            | CallbackExprKind::DynamicIndex { .. }
            | CallbackExprKind::Function(_)
            | CallbackExprKind::FunctionTableLookup { .. }
            | CallbackExprKind::AssignCapture { .. }
            | CallbackExprKind::HasField { .. }
            | CallbackExprKind::FieldTruthy { .. }
            | CallbackExprKind::UnknownIs { .. } => Err(SmeltError::unsupported(
                span,
                "this callback default expression is not lowered at call sites yet",
            )),
            CallbackExprKind::Index { receiver, index } => {
                let receiver = self.callback_expr_to_body_expr(receiver, args, body, span)?;
                let index_ty = self.ctx.krate.types.intern(Type::Float);
                let index_value = index.to_string().parse::<f64>().map_err(|err| {
                    SmeltError::unsupported(
                        span,
                        format!("callback tuple index cannot be represented as a number: {err}"),
                    )
                })?;
                let index_expr = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(index_value)),
                    ty: index_ty,
                    span,
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Index {
                        receiver,
                        index: index_expr,
                    },
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
                let callee = self.callback_expr_to_body_expr(callee, args, body, span)?;
                let call_args = self.callback_call_args_to_body_exprs(call_args, args, body, span)?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Call {
                        callee,
                        args: call_args,
                    },
                    ty: callback.ty,
                    span,
                }))
            }
            CallbackExprKind::MethodCall {
                receiver,
                method,
                args: call_args,
            } => {
                let receiver = self.callback_expr_to_body_expr(receiver, args, body, span)?;
                let call_args = self.callback_call_args_to_body_exprs(call_args, args, body, span)?;
                if self.ctx.krate.symbols.get(*method) == Some("has")
                    && call_args.len() == 1
                    && matches!(
                        self.ctx.krate.types.get(Self::expr_ty(body, receiver)),
                        Some(Type::Set(_))
                    )
                {
                    let item = *call_args.first().ok_or_else(|| {
                        SmeltError::unsupported(span, "Set.has callback call requires one argument")
                    })?;
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::SetContains {
                            set: receiver,
                            item,
                        },
                        ty: callback.ty,
                        span,
                    }));
                }
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Method {
                        receiver,
                        method: *method,
                        args: call_args,
                    },
                    ty: callback.ty,
                    span,
                }))
            }
        }
    }

    /// Convert stored callback call arguments into normal HIR argument expressions.
    fn callback_call_args_to_body_exprs(
        &mut self,
        call_args: &[CallbackCallArg],
        args: &[smelt_hir::ExprId],
        body: &mut Body,
        span: Span,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        call_args
            .iter()
            .map(|arg| {
                if arg.spread {
                    return Err(SmeltError::unsupported(
                        span,
                        "spread arguments in callback defaults are not lowered yet",
                    ));
                }
                self.callback_expr_to_body_expr(&arg.expr, args, body, span)
            })
            .collect()
    }

    /// Recursively collect captures and upgrade assigned captures to mutable mode.
    fn collect_callback_captures(
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
            CallbackExprKind::MethodCall { receiver, args, .. } => {
                self.collect_callback_captures(receiver, body, captures);
                for arg in args {
                    self.collect_callback_captures(&arg.expr, body, captures);
                }
            }
            CallbackExprKind::FunctionTableLookup { key, .. } => {
                self.collect_callback_captures(key, body, captures);
            }
            CallbackExprKind::Param(_) | CallbackExprKind::Function(_) | CallbackExprKind::Literal(_) => {}
        }
    }

    /// Lower a supported arrow callback to a typed expression tree.
    fn arrow_callback(
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
    fn known_callback_factory_predicate(
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
    fn is_identifier_callee(callee: &Expression<'_>, expected: &str) -> bool {
        matches!(callee, Expression::Identifier(identifier) if identifier.name == expected)
    }

    /// Return whether a call callee is `object.property`.
    fn is_static_member_callee(
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
    fn asserted_arrow_callback(
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
            Argument::TSSatisfiesExpression(satisfies) => Ok(Some(self.arrow_callback_expression(
                &satisfies.expression,
                expected_param_tys,
                body,
            )?)),
            Argument::TSNonNullExpression(non_null) => Ok(Some(self.arrow_callback_expression(
                &non_null.expression,
                expected_param_tys,
                body,
            )?)),
            _ => Ok(None),
        }
    }

    /// Lower the expression inside a TypeScript assertion when it is a callback.
    fn arrow_callback_expression(
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
            Expression::ParenthesizedExpression(parenthesized) => self.arrow_callback_expression(
                &parenthesized.expression,
                expected_param_tys,
                body,
            ),
            Expression::TSAsExpression(as_expr) => {
                self.arrow_callback_expression(&as_expr.expression, expected_param_tys, body)
            }
            Expression::TSSatisfiesExpression(satisfies) => self.arrow_callback_expression(
                &satisfies.expression,
                expected_param_tys,
                body,
            ),
            Expression::TSNonNullExpression(non_null) => {
                self.arrow_callback_expression(&non_null.expression, expected_param_tys, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(expression.span().start, expression.span().end),
                "array callback methods currently require arrow function callbacks",
            )),
        }
    }

    /// Lower a function-expression callback after expected parameter types are known.
    fn function_callback_from_params(
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
        if function.params.rest.is_some() || function.params.items.len() > expected_param_tys.len() {
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
    fn arrow_callback_from_params(
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

    /// Lower a terminating block-bodied callback into a nested callback expression.
    fn callback_block_expression<'a>(
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
                if if_stmt.alternate.is_some() {
                    return Err(SmeltError::unsupported(
                        self.span(if_stmt.span.start, if_stmt.span.end),
                        "callback if/else blocks need direct conditional expression lowering",
                    ));
                }
                let cond = self.callback_truthy_expression(&if_stmt.test, params, body)?;
                let then_expr = match self.callback_terminating_statement(&if_stmt.consequent, params, body) {
                    Ok(expr) => expr,
                    Err(error) if !rest.is_empty() => {
                        drop(error);
                        let side_effect = self.callback_side_effect_statement(
                            &if_stmt.consequent,
                            params,
                            body,
                        )?;
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
    fn callback_side_effect_statement<'a>(
        &mut self,
        side_effect_statement: &'a Statement<'a>,
        params: &HashMap<&'a str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        let none_ty = self.ctx.krate.types.intern(Type::None);
        match side_effect_statement {
            Statement::ExpressionStatement(expr_stmt) => self.callback_expression(&expr_stmt.expression, params, body),
            Statement::BlockStatement(block) => {
                let mut effects = Vec::new();
                for block_statement in &block.body {
                    let effect = match block_statement {
                        Statement::ExpressionStatement(expr_stmt) => {
                            self.callback_expression(&expr_stmt.expression, params, body)?
                        }
                        Statement::ThrowStatement(throw_stmt) => {
                            self.callback_throw_expression(Some(&throw_stmt.argument), params, body)?
                        }
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

    /// Lower a callback statement that must terminate the current branch.
    fn callback_terminating_statement<'a>(
        &mut self,
        statement: &'a Statement<'a>,
        params: &mut HashMap<&'a str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        match statement {
            Statement::BlockStatement(block) => self.callback_block_expression(&block.body, params, body),
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
    fn callback_params_with_guard_narrowing<'a>(
        &mut self,
        params: &HashMap<&'a str, CallbackExpr>,
        expression: &Expression<'_>,
    ) -> HashMap<&'a str, CallbackExpr> {
        let mut narrowed = params.clone();
        self.apply_callback_guard_narrowing(&mut narrowed, expression);
        narrowed
    }

    /// Apply simple `value !== undefined` callback type facts to a parameter map.
    fn apply_callback_guard_narrowing(
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
    fn callback_truthy_expression(
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
    fn callback_function_table_has_key(
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
    fn callback_throw_expression(
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

    /// Extract the message from common thrown error constructors.
    fn callback_throw_message(
        &mut self,
        argument: &Expression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        if let Expression::NewExpression(new_expr) = argument
            && matches!(&new_expr.callee, Expression::Identifier(callee) if matches!(callee.name.as_str(), "Error" | "TypeError" | "RangeError"))
            && let Some(first) = new_expr.arguments.first()
            && let Some(expression) = first.as_expression()
        {
            return self.callback_expression(expression, params, body);
        }
        if matches!(argument, Expression::NewExpression(_)) {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::String(String::new())),
                ty,
            });
        }
        self.callback_expression(argument, params, body)
    }

    /// Bind names from a callback parameter pattern to callback expressions.
    fn bind_callback_param_pattern<'a>(
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
                        Some(Type::Dict(_, value)) => *value,
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
    fn arrow_callback_param_types_with_hint(
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
                        .and_then(|function| function.params.get(index).copied())
                });
            let ty = match (&param.pattern, ty) {
                (_, Some(ty)) => ty,
                (BindingPattern::BindingIdentifier(_), None) => {
                    self.infer_unannotated_arrow_param_type(arrow, index)
                }
                (_, None) => self.ctx.krate.types.intern(Type::Unknown),
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
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(rest.span.start, rest.span.end),
                        "rest closure parameters must have explicit array type annotations",
                    )
                })?;
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

    /// Infer a conservative type for an unannotated arrow parameter.
    ///
    /// TypeScript normally gets these from contextual typing. When Smelt loses
    /// that context through imported generic helpers, arithmetic use inside the
    /// callback is still enough to recover a numeric parameter; other cases use
    /// `unknown` so typed library code can keep lowering.
    fn infer_unannotated_arrow_param_type(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        index: usize,
    ) -> smelt_hir::TypeId {
        let Some(param_name) = arrow.params.items.get(index).and_then(|param| {
            Self::simple_binding_pattern_name(&param.pattern)
        }) else {
            return self.ctx.krate.types.intern(Type::Unknown);
        };
        if self.arrow_param_used_as_number(arrow, param_name) {
            self.ctx.krate.types.intern(Type::Float)
        } else {
            self.ctx.krate.types.intern(Type::Unknown)
        }
    }

    /// Return a simple identifier name from a binding or defaulted binding pattern.
    fn simple_binding_pattern_name<'a>(pattern: &'a BindingPattern<'a>) -> Option<&'a str> {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => Some(binding.name.as_str()),
            BindingPattern::AssignmentPattern(assign) => {
                Self::simple_binding_pattern_name(&assign.left)
            }
            _ => None,
        }
    }

    /// Return true when an arrow parameter participates in arithmetic.
    fn arrow_param_used_as_number(
        &self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        param_name: &str,
    ) -> bool {
        self.arrow_return_expression(arrow)
            .ok()
            .is_some_and(|expr| Self::expression_uses_identifier_in_arithmetic(expr, param_name))
    }

    /// Scan an expression for arithmetic involving a named identifier.
    fn expression_uses_identifier_in_arithmetic(expression: &Expression<'_>, name: &str) -> bool {
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
                matches!(&new_expr.callee, Expression::Identifier(identifier) if identifier.name == "Date")
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
    fn expression_contains_identifier(expression: &Expression<'_>, name: &str) -> bool {
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
    fn arrow_closure_body_expr(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        param_tys: &[smelt_hir::TypeId],
        return_ty: smelt_hir::TypeId,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(arrow.span.start, arrow.span.end);
        let mut closure_body = Body::new(None, span);
        let mut closure_params = Vec::new();
        let mut param_names = HashSet::new();
        let mut saved_locals = Vec::new();

        for (index, param) in arrow.params.items.iter().enumerate() {
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
                        self.locals.insert(binding.name.as_str().to_owned(), local),
                    ));
                }
                pattern => {
                    let mut names = Vec::new();
                    Self::binding_pattern_names(pattern, &mut names);
                    for name in &names {
                        param_names.insert(name.clone());
                        saved_locals.push((name.clone(), self.locals.get(name.as_str()).copied()));
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

        if let Some(rest) = &arrow.params.rest {
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
                self.locals.insert(binding.name.as_str().to_owned(), local),
            ));
        }

        let mut capture_names = Vec::new();
        if arrow.expression {
            let return_expression = self.arrow_return_expression(arrow)?;
            self.collect_expression_capture_names(
                return_expression,
                &param_names,
                &mut capture_names,
            );
        } else {
            for statement in &arrow.body.statements {
                self.collect_statement_capture_names(statement, &param_names, &mut capture_names);
            }
        }
        capture_names.sort();
        capture_names.dedup();

        let mut captures = Vec::new();
        for name in capture_names {
            let Some(source_local) = saved_locals
                .iter()
                .find_map(|(saved_name, prior)| (saved_name == &name).then_some(*prior).flatten())
                .or_else(|| self.locals.get(name.as_str()).copied())
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
            saved_locals.push((name.clone(), self.locals.insert(name, body_local)));
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
        self.current_async = arrow.r#async;
        self.current_return_ty = Some(return_ty);
        let lowering_result = if arrow.expression {
            match self.arrow_return_expression(arrow) {
                Ok(return_expression) => self
                    .expression_with_hint(return_expression, &mut closure_body, Some(return_ty))
                    .map(|value| {
                        closure_body.push_stmt(Stmt::Return(Some(value)));
                    }),
                Err(error) => Err(error),
            }
        } else {
            let mut result = Ok(());
            for statement in &arrow.body.statements {
                if let Err(error) = self.statement(statement, &mut closure_body) {
                    result = Err(error);
                    break;
                }
            }
            result
        };
        if arrow.r#async {
            closure_body.build_async_state_machine();
        }
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        for (name, prior) in saved_locals.into_iter().rev() {
            if let Some(local) = prior {
                self.locals.insert(name, local);
            } else {
                self.locals.remove(name.as_str());
            }
        }
        lowering_result?;
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: param_tys.to_vec(),
            return_ty,
            is_async: arrow.r#async,
        }));
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                return_ty,
                captures,
                body: body_id,
                callback_body: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Lower an arrow function expression as a first-class closure value.
    fn arrow_function_expression(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.arrow_function_expression_with_hint(arrow, body, None)
    }

    /// Lower an arrow function expression using an optional contextual function type.
    fn arrow_function_expression_with_hint(
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
        let params = self.arrow_callback_param_types_with_hint(arrow, contextual_function.as_ref())?;
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
            return Ok(self.callback_expr_to_closure(callback, &params, span, body));
        }
        let mut return_ty = explicit_return_ty
            .or_else(|| contextual_function.as_ref().map(|function| function.return_ty))
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
        if arrow.r#async && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_))) {
            return_ty = self.ctx.krate.types.intern(Type::Future(return_ty));
        }
        self.arrow_closure_body_expr(arrow, &params, return_ty, body)
        })();
        self.pop_type_parameter_scope();
        result
    }

    /// Return the expression produced by an arrow function body.
    fn arrow_return_expression<'a>(
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
    fn collect_expression_capture_names(
        &self,
        expression: &Expression<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match expression {
            Expression::Identifier(identifier) => {
                let name = identifier.name.as_str();
                if !param_names.contains(name) && self.locals.contains_key(name) {
                    captures.push(name.to_owned());
                }
            }
            Expression::CallExpression(call) => {
                self.collect_expression_capture_names(&call.callee, param_names, captures);
                for arg in &call.arguments {
                    match arg {
                        Argument::SpreadElement(spread) => self.collect_expression_capture_names(
                            &spread.argument,
                            param_names,
                            captures,
                        ),
                        other => {
                            if let Some(arg_expression) = other.as_expression() {
                                self.collect_expression_capture_names(
                                    arg_expression,
                                    param_names,
                                    captures,
                                );
                            }
                        }
                    }
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
                self.collect_expression_capture_names(&conditional.consequent, param_names, captures);
                self.collect_expression_capture_names(&conditional.alternate, param_names, captures);
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
                        match arg {
                            Argument::SpreadElement(spread) => self.collect_expression_capture_names(
                                &spread.argument,
                                param_names,
                                captures,
                            ),
                            other => {
                                if let Some(arg_expression) = other.as_expression() {
                                    self.collect_expression_capture_names(
                                        arg_expression,
                                        param_names,
                                        captures,
                                    );
                                }
                            }
                        }
                    }
                }
                ChainElement::StaticMemberExpression(member) => {
                    self.collect_expression_capture_names(&member.object, param_names, captures);
                }
                ChainElement::ComputedMemberExpression(member) => {
                    self.collect_expression_capture_names(&member.object, param_names, captures);
                    self.collect_expression_capture_names(&member.expression, param_names, captures);
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
            _ => {}
        }
    }

    /// Collect captured locals referenced by an assignment target.
    fn collect_assignment_target_capture_names(
        &self,
        target: &AssignmentTarget<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let name = identifier.name.as_str();
                if !param_names.contains(name) && self.locals.contains_key(name) {
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
    fn collect_statement_capture_names(
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
            Statement::ThrowStatement(statement) => {
                self.collect_expression_capture_names(&statement.argument, param_names, captures);
            }
            _ => {}
        }
    }

    /// Lower either an inline arrow callback or a local closure callback value.
    fn callback_argument(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        context: &'static str,
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        if let Argument::Identifier(identifier) = argument {
            if let Some(local) = self.locals.get(identifier.name.as_str()).copied() {
                let local_ty = Self::local_ty(body, local);
                if let Some(Type::Function(function)) = self.ctx.krate.types.get(local_ty).cloned() {
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
                        format!("{context} callback item `{}` is not a function", identifier.name),
                    ));
                };
                if function.params.is_empty() || function.params.len() > expected_param_tys.len() {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("{context} callback item parameter count is not supported"),
                    ));
                }
                let return_ty = function.return_ty;
                let expr = self.item_function_closure_expression(
                    item,
                    identifier.span.start,
                    identifier.span.end,
                    body,
                )?;
                return Ok(ClosureCallback { expr, return_ty });
            }
            if matches!(
                identifier.name.as_str(),
                "Boolean" | "isEmpty" | "isArray" | "isString" | "isObject" | "trim"
            )
                && (identifier.name == "Boolean"
                    || self.value_imports.contains(identifier.name.as_str()))
            {
                let param_ty = expected_param_tys
                    .first()
                    .copied()
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                let return_ty = self.ctx.krate.types.intern(Type::Bool);
                let function_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params: vec![param_ty],
                    return_ty,
                    is_async: false,
                }));
                let expr = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: function_ty,
                    span: self.span(identifier.span.start, identifier.span.end),
                });
                return Ok(ClosureCallback { expr, return_ty });
            }
            let Some(callback) = self.local_callbacks.get(identifier.name.as_str()).cloned() else {
                return Err(SmeltError::unsupported(
                    self.span(identifier.span.start, identifier.span.end),
                    format!("{context} local callback `{}` is not defined", identifier.name),
                ));
            };
            if callback.params.is_empty() || callback.params.len() > expected_param_tys.len() {
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
                callback.callback,
                &callback.params,
                self.span(identifier.span.start, identifier.span.end),
                body,
            );
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
            if let Some(Type::Function(function)) =
                self.ctx.krate.types.get(Self::expr_ty(body, direct_expr)).cloned()
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
            callback,
            expected_param_tys,
            self.span(argument.span().start, argument.span().end),
            body,
        );
        Ok(ClosureCallback { expr, return_ty })
    }

    /// Lower an array predicate callback, coercing JavaScript truthy returns into booleans.
    fn truthy_callback_argument_with_body_fallback(
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
                    callback,
                    expected_param_tys,
                    span,
                    body,
                );
                Ok(ClosureCallback {
                    expr,
                    return_ty: bool_ty,
                })
            }
            Err(error)
                if Self::should_fallback_to_closure_body_for_callback(&error)
                    && matches!(argument, Argument::ArrowFunctionExpression(_)) =>
            {
                let Argument::ArrowFunctionExpression(arrow) = argument else {
                    return Err(error);
                };
                let expr = self.arrow_closure_body_expr(arrow, expected_param_tys, bool_ty, body)?;
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
                } else {
                    Err(SmeltError::unsupported(
                        self.span(argument.span().start, argument.span().end),
                        format!("{context} callback returns an unsupported type"),
                    ))
                }
            }
        }
    }

    /// Convert a callback expression result into the boolean value used by JS predicates.
    fn coerce_callback_expr_to_truthy(
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
            kind
                if self.is_nullishable_type(callback.ty)
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
    fn callback_argument_with_body_fallback(
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
                    && matches!(argument, Argument::ArrowFunctionExpression(_)) =>
            {
                let Argument::ArrowFunctionExpression(arrow) = argument else {
                    return Err(error);
                };
                let expr =
                    self.arrow_closure_body_expr(arrow, expected_param_tys, fallback_return_ty, body)?;
                Ok(ClosureCallback {
                    expr,
                    return_ty: fallback_return_ty,
                })
            }
            Err(error) => Err(error),
        }
    }

    /// Return whether compact callback lowering should retry as a normal closure.
    fn should_fallback_to_closure_body_for_callback(error: &SmeltError) -> bool {
        error.message == "callback expression kind is not supported yet"
            || error.message == "callback expression statements must be followed by a return or throw"
            || error.message == "callback side-effect blocks only support expression statements"
            || error.message == "callback block declarations require simple bindings"
            || error.message == "async callbacks need closure-body lowering"
            || error.message.starts_with("unresolved callback identifier `")
            || error
                .message
                .contains("resolves outside the current callback body")
    }

    /// Collapse tuple item types into the element type used by array callbacks.
    fn tuple_items_element_type(&mut self, items: &[smelt_hir::TypeId]) -> smelt_hir::TypeId {
        match items {
            [] => self.ctx.krate.types.intern(Type::Unknown),
            [single] => *single,
            [first, rest @ ..] if rest.iter().all(|item| item == first) => *first,
            _ => self.ctx.krate.types.intern(Type::Union(items.to_vec())),
        }
    }

    /// Lower a supported callback expression.
    fn callback_expression(
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
                        kind: CallbackExprKind::Literal(Literal::None),
                        ty: self.ctx.krate.types.intern(Type::None),
                    });
                }
                if let Some(value) = self.const_literals.get(identifier.name.as_str()) {
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Literal(value.literal.clone()),
                        ty: value.ty,
                    });
                }
                if let Some(item) = self.items.get(identifier.name.as_str()).copied() {
                    let span = self.span(identifier.span.start, identifier.span.end);
                    let ty = self.item_expr_type(item, span)?;
                    let Item::Function(function) = self.item_ref(item) else {
                        return Err(SmeltError::unsupported(
                            span,
                            "callback item references must resolve to functions",
                        ));
                    };
                    return Ok(CallbackExpr {
                        kind: CallbackExprKind::Function(function.name),
                        ty,
                    });
                }
                if let Some((name, ty)) = self
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
                    return Err(SmeltError::unsupported(
                        self.span(identifier.span.start, identifier.span.end),
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
                                return Err(SmeltError::unsupported(
                                    self.span(identifier.span.start, identifier.span.end),
                                    format!(
                                        "unresolved callback identifier `{}`",
                                        identifier.name
                                    ),
                                ));
                            }
                        }
                        ArrayExpressionElement::BinaryExpression(binary) => {
                            let op = self.callback_binary_op(
                                binary.operator,
                                binary.span.start,
                                binary.span.end,
                            )?;
                            let lhs = self.callback_expression(&binary.left, params, body)?;
                            let rhs = self.callback_expression(&binary.right, params, body)?;
                            let ty = if matches!(
                                op,
                                BinOp::Eq
                                    | BinOp::NotEq
                                    | BinOp::Lt
                                    | BinOp::Lte
                                    | BinOp::Gt
                                    | BinOp::Gte
                            ) {
                                self.ctx.krate.types.intern(Type::Bool)
                            } else {
                                lhs.ty
                            };
                            CallbackExpr {
                                kind: CallbackExprKind::Binary {
                                    op,
                                    lhs: Box::new(lhs),
                                    rhs: Box::new(rhs),
                                },
                                ty,
                            }
                        }
                        ArrayExpressionElement::ComputedMemberExpression(member) => {
                            let receiver =
                                self.callback_expression(&member.object, params, body)?;
                            let index =
                                self.callback_expression(&member.expression, params, body)?;
                            if self.ctx.krate.types.get(self.type_param_constraint_or_self(index.ty))
                                != Some(&Type::Float)
                                && self.ctx.krate.types.get(self.type_param_constraint_or_self(index.ty))
                                    != Some(&Type::Int)
                                && self.ctx.krate.types.get(self.type_param_constraint_or_self(index.ty))
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
                                            Some(Type::List(_) | Type::Unknown | Type::TypeParam { .. })
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
                let Some(first) = items.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "callback empty arrays require type context",
                    ));
                };
                let item_ty = if items.iter().all(|item| item.ty == first.ty) {
                    first.ty
                } else {
                    let mut item_tys = Vec::new();
                    for item in &items {
                        if !item_tys.contains(&item.ty) {
                            item_tys.push(item.ty);
                        }
                    }
                    self.ctx.krate.types.intern(Type::Union(item_tys))
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
                            entries.push((self.intern_source_name("__computed"), value));
                            continue;
                        }
                    };
                    let value = self.callback_expression(&property.value, params, body)?;
                    entries.push((self.intern_source_name(key_text), value));
                }
                Ok(CallbackExpr {
                    kind: CallbackExprKind::DictLit(entries),
                    ty: self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty)),
                })
            }
            Expression::NewExpression(new_expr)
                if matches!(&new_expr.callee, Expression::Identifier(callee) if callee.name == "RegExp") =>
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
                        "entries" => {
                            self.ctx.krate.types.intern(Type::Tuple(vec![string_ty, unknown_ty]))
                        }
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
                    if self.ctx.krate.types.get(value.ty) == Some(&Type::Unknown) {
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
                                (self.callback_expression(arg_expression, params, body)?, false)
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
                if let Some(expr) = self.callback_regex_replace_uppercase_call(call, params, body)?
                {
                    return Ok(expr);
                }
                if let Expression::StaticMemberExpression(member) = &call.callee
                    && matches!(member.property.name.as_str(), "trim" | "trimStart" | "trimEnd")
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
                    let return_ty = match member.property.name.as_str() {
                        "toString" => self.ctx.krate.types.intern(Type::String),
                        "match" => self.ctx.krate.types.intern(Type::Bool),
                        "has" if matches!(self.ctx.krate.types.get(receiver.ty), Some(Type::Set(_))) => {
                            self.ctx.krate.types.intern(Type::Bool)
                        }
                        "getFullYear" | "getMonth" | "getDate" | "getHours" | "getMinutes"
                        | "getSeconds" | "getMilliseconds" | "getTime" => {
                            self.ctx.krate.types.intern(Type::Float)
                        }
                        _ => self.ctx.krate.types.intern(Type::Unknown),
                    };
                    let mut args = Vec::new();
                    for arg in &call.arguments {
                        let (expr, spread) = match arg {
                            Argument::SpreadElement(spread) => {
                                (self.callback_expression(&spread.argument, params, body)?, true)
                            }
                            other => {
                                let Some(arg_expression) = other.as_expression() else {
                                    return Err(SmeltError::unsupported(
                                        self.span(other.span().start, other.span().end),
                                        "callback method argument kind is not supported yet",
                                    ));
                                };
                                (self.callback_expression(arg_expression, params, body)?, false)
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
                        Argument::SpreadElement(spread) => {
                            (self.callback_expression(&spread.argument, params, body)?, true)
                        }
                        other => {
                            let Some(arg_expression) = other.as_expression() else {
                                return Err(SmeltError::unsupported(
                                    self.span(other.span().start, other.span().end),
                                    "callback call argument kind is not supported yet",
                                ));
                            };
                            (self.callback_expression(arg_expression, params, body)?, false)
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
                    Some(Type::Dict(_, value)) => *value,
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
                    && let Some(namespace) =
                        self.object_namespaces.get(receiver_ident.name.as_str()).cloned()
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
                    let index_usize =
                        index
                            .value
                            .to_string()
                            .parse::<usize>()
                            .map_err(|err| {
                                SmeltError::unsupported(
                                    self.span(index.span.start, index.span.end),
                                    format!("callback computed access index is invalid: {err}"),
                                )
                            })?;
                    let item_ty =
                        match self.ctx.krate.types.get(self.type_param_constraint_or_self(receiver.ty))
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
                let index = self.callback_expression(&member.expression, params, body)?;
                let receiver_ty = self.type_param_constraint_or_self(receiver.ty);
                let index_ty = self.type_param_constraint_or_self(index.ty);
                let numeric_index = matches!(
                    self.ctx.krate.types.get(index_ty),
                    Some(Type::Float | Type::Int | Type::Unknown)
                );
                let string_key_index = (self.ctx.krate.types.get(index_ty) == Some(&Type::String)
                    || self.erased_or_union_surface(index_ty))
                    && matches!(
                        self.ctx.krate.types.get(receiver_ty),
                        Some(Type::Dict(_, _) | Type::Class { .. } | Type::Unknown | Type::TypeParam { .. })
                    );
                if !numeric_index && !string_key_index
                {
                    return Err(SmeltError::unsupported(
                        self.span(member.expression.span().start, member.expression.span().end),
                        "callback dynamic computed access index must be numeric or a string record key",
                    ));
                }
                let item_ty = match self.ctx.krate.types.get(receiver_ty) {
                    Some(Type::List(item_ty)) => *item_ty,
                    Some(Type::String) => self.ctx.krate.types.intern(Type::String),
                    Some(Type::Dict(_, value_ty)) => *value_ty,
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => {
                        self.ctx.krate.types.intern(Type::Unknown)
                    }
                    Some(Type::Union(items))
                        if items.iter().any(|item| {
                            matches!(
                                self.ctx.krate.types.get(*item),
                                Some(Type::List(_) | Type::Dict(_, _) | Type::Unknown | Type::TypeParam { .. })
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
                let then_params = self.callback_params_with_guard_narrowing(
                    params,
                    &conditional.test,
                );
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
                    ) && assign.operator == AssignmentOperator::Assign
                    {
                        return self.callback_expression(&assign.right, params, body);
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
                    return Err(SmeltError::unsupported(
                        self.span(target.span.start, target.span.end),
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
                            format!(
                                "callback assignment operator is not supported yet: {other:?}"
                            ),
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
                        && let Some(namespace) = self.object_namespaces.get(receiver_ident.name.as_str())
                    {
                        let case_keys = namespace.keys().cloned().collect::<Vec<_>>();
                        let key = self.callback_expression(&binary.left, params, body)?;
                        if self.ctx.krate.types.get(key.ty) != Some(&Type::String) {
                            return Err(SmeltError::unsupported(
                                self.span(binary.left.span().start, binary.left.span().end),
                                "callback namespace `in` checks require a string key",
                            ));
                        }
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
                let ty = if matches!(
                    op,
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte
                ) {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    lhs.ty
                };
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
                let op = match logical.operator {
                    LogicalOperator::And => BinOp::And,
                    LogicalOperator::Or => BinOp::Or,
                    LogicalOperator::Coalesce => BinOp::Or,
                };
                let lhs = self.callback_expression(&logical.left, params, body)?;
                let rhs = self.callback_expression(&logical.right, params, body)?;
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty: self.ctx.krate.types.intern(Type::Bool),
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

    /// Lower `value.replace(/.../, (match) => match.toUpperCase())` inside callbacks.
    ///
    /// Date-fns builds locale distance tokens by uppercasing a regex match
    /// within a `reduce` callback. The callback-expression path is still a
    /// compact IR while it is migrated to regular closure bodies, so this
    /// recognizes that public `String.prototype.replace` API shape directly.
    fn callback_regex_replace_uppercase_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<Option<CallbackExpr>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "replace" {
            return Ok(None);
        }
        let [Argument::RegExpLiteral(pattern), Argument::ArrowFunctionExpression(replacement)] =
            call.arguments.as_slice()
        else {
            return Ok(None);
        };
        if !self.arrow_callback_returns_param_uppercase(replacement)? {
            return Ok(None);
        }

        let receiver = self.callback_expression(&member.object, params, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let pattern = CallbackExpr {
            kind: CallbackExprKind::Literal(Literal::String(Self::regex_literal_pattern_text(
                pattern,
            ))),
            ty: string_ty,
        };
        Ok(Some(CallbackExpr {
            kind: CallbackExprKind::MethodCall {
                receiver: Box::new(receiver),
                method: self.intern_source_name("__smelt_replace_first_match_uppercase"),
                args: vec![CallbackCallArg {
                    expr: pattern,
                    spread: false,
                }],
            },
            ty: string_ty,
        }))
    }

    /// Return whether a replacement callback is `(m) => m.toUpperCase()`.
    fn arrow_callback_returns_param_uppercase(
        &self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
    ) -> Result<bool, SmeltError> {
        if arrow.params.items.len() != 1 || arrow.params.rest.is_some() {
            return Ok(false);
        }
        let Some(param) = arrow.params.items.first() else {
            return Ok(false);
        };
        let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
            return Ok(false);
        };
        let returned = self.arrow_return_expression(arrow)?;
        let Expression::CallExpression(call) = returned else {
            return Ok(false);
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(false);
        };
        let Expression::Identifier(receiver) = &member.object else {
            return Ok(false);
        };
        Ok(receiver.name == binding.name
            && member.property.name == "toUpperCase"
            && call.arguments.is_empty())
    }

    /// Lower a callback template literal as string concatenation.
    fn callback_template_literal(
        &mut self,
        template: &oxc::ast::ast::TemplateLiteral<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let Some(first_quasi) = template.quasis.first() else {
            return Err(SmeltError::unsupported(
                self.span(template.span.start, template.span.end),
                "callback template literals must contain at least one quasi",
            ));
        };
        let first_text = first_quasi
            .value
            .cooked
            .as_ref()
            .map_or_else(|| first_quasi.value.raw.as_str(), |cooked| cooked.as_str())
            .to_owned();
        let mut acc = CallbackExpr {
            kind: CallbackExprKind::Literal(Literal::String(first_text)),
            ty: string_ty,
        };
        for (index, expression) in template.expressions.iter().enumerate() {
            let part = self.callback_expression(expression, params, body)?;
            acc = CallbackExpr {
                kind: CallbackExprKind::Binary {
                    op: BinOp::Add,
                    lhs: Box::new(acc),
                    rhs: Box::new(part),
                },
                ty: string_ty,
            };
            if let Some(quasi) = template.quasis.get(index.saturating_add(1)) {
                let text = quasi
                    .value
                    .cooked
                    .as_ref()
                    .map_or_else(|| quasi.value.raw.as_str(), |cooked| cooked.as_str());
                if !text.is_empty() {
                    let literal = CallbackExpr {
                        kind: CallbackExprKind::Literal(Literal::String(text.to_owned())),
                        ty: string_ty,
                    };
                    acc = CallbackExpr {
                        kind: CallbackExprKind::Binary {
                            op: BinOp::Add,
                            lhs: Box::new(acc),
                            rhs: Box::new(literal),
                        },
                        ty: string_ty,
                    };
                }
            }
        }
        Ok(acc)
    }

    /// Lower callback `typeof value` expressions to string literals.
    fn callback_typeof_unary(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        let operand = self.callback_expression(&unary.argument, params, body)?;
        let kind = self.typeof_type_name(operand.ty).unwrap_or("object");
        Ok(CallbackExpr {
            kind: CallbackExprKind::Literal(Literal::String(kind.to_owned())),
            ty: self.ctx.krate.types.intern(Type::String),
        })
    }

    /// Compute the result type of a callback conditional expression.
    fn callback_unify_conditional_exprs(
        &mut self,
        then_expr: CallbackExpr,
        else_expr: CallbackExpr,
        start: u32,
        end: u32,
    ) -> Result<(CallbackExpr, CallbackExpr, smelt_hir::TypeId), SmeltError> {
        if let Ok(ty) = self.callback_conditional_type(then_expr.ty, else_expr.ty, start, end) {
            return Ok((then_expr, else_expr, ty));
        }
        if let Some(coerced_then) =
            self.coerce_callback_object_literal_to_type(then_expr.clone(), else_expr.ty)
        {
            let ty = else_expr.ty;
            return Ok((coerced_then, else_expr, ty));
        }
        if let Some(coerced_else) =
            self.coerce_callback_object_literal_to_type(else_expr.clone(), then_expr.ty)
        {
            let ty = then_expr.ty;
            return Ok((then_expr, coerced_else, ty));
        }
        self.callback_conditional_type(then_expr.ty, else_expr.ty, start, end)
            .map(|ty| (then_expr, else_expr, ty))
    }

    /// Coerce a callback object literal to a structural object type when its fields fit.
    fn coerce_callback_object_literal_to_type(
        &mut self,
        mut expr: CallbackExpr,
        target_ty: smelt_hir::TypeId,
    ) -> Option<CallbackExpr> {
        let CallbackExprKind::DictLit(entries) = &expr.kind else {
            return None;
        };
        if !self.callback_object_literal_assignable_to(entries, target_ty) {
            return None;
        }
        expr.ty = target_ty;
        Some(expr)
    }

    /// Return whether callback object-literal entries are compatible with a structural type.
    fn callback_object_literal_assignable_to(
        &mut self,
        entries: &[(smelt_hir::Symbol, CallbackExpr)],
        target_ty: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(target_ty).cloned() {
            Some(Type::Class { .. }) => entries.iter().all(|(field, value)| {
                self.class_field_type(target_ty, *field)
                    .is_ok_and(|field_ty| self.callback_type_assignable(value.ty, field_ty))
            }),
            Some(Type::Dict(_, value_ty)) => entries
                .iter()
                .all(|(_, value)| self.callback_type_assignable(value.ty, value_ty)),
            _ => false,
        }
    }

    /// Lightweight callback assignment compatibility for contextual branch typing.
    fn callback_type_assignable(
        &self,
        source_ty: smelt_hir::TypeId,
        target_ty: smelt_hir::TypeId,
    ) -> bool {
        source_ty == target_ty
            || matches!(self.ctx.krate.types.get(source_ty), Some(Type::Unknown))
            || matches!(self.ctx.krate.types.get(target_ty), Some(Type::Unknown))
    }

    /// Compute the result type of a callback conditional expression.
    fn callback_conditional_type(
        &mut self,
        then_ty: smelt_hir::TypeId,
        else_ty: smelt_hir::TypeId,
        start: u32,
        end: u32,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if then_ty == else_ty {
            return Ok(then_ty);
        }
        if self.ctx.krate.types.get(then_ty) == Some(&Type::Never) {
            return Ok(else_ty);
        }
        if self.ctx.krate.types.get(else_ty) == Some(&Type::Never) {
            return Ok(then_ty);
        }
        if self.ctx.krate.types.get(then_ty) == Some(&Type::Unknown)
            || self.ctx.krate.types.get(else_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(then_ty)
            || self.type_contains_unknown(else_ty)
            || self.erased_or_union_surface(then_ty)
            || self.erased_or_union_surface(else_ty)
        {
            return Ok(self.ctx.krate.types.intern(Type::Unknown));
        }
        if let Some(inner) = self.non_nullish_type(then_ty)
            && inner == else_ty
        {
            return Ok(then_ty);
        }
        if let Some(inner) = self.non_nullish_type(else_ty)
            && inner == then_ty
        {
            return Ok(else_ty);
        }
        if self.ctx.krate.types.get(else_ty) == Some(&Type::None) {
            return Ok(self.ctx.krate.types.intern(Type::Optional(then_ty)));
        }
        if self.ctx.krate.types.get(then_ty) == Some(&Type::None) {
            return Ok(self.ctx.krate.types.intern(Type::Optional(else_ty)));
        }
        Err(SmeltError::unsupported(
            self.span(start, end),
            "callback conditional expression branches must have compatible lowered types",
        ))
    }

    /// Lower `typeof value === "kind"` checks inside callback expressions.
    fn callback_typeof_binary(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<Option<CallbackExpr>, SmeltError> {
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality
                | BinaryOperator::Equality
                | BinaryOperator::StrictInequality
                | BinaryOperator::Inequality
        ) {
            return Ok(None);
        }
        let Expression::UnaryExpression(unary) = &binary.left else {
            return Ok(None);
        };
        if unary.operator != UnaryOperator::Typeof {
            return Ok(None);
        }
        let Expression::StringLiteral(kind_literal) = &binary.right else {
            return Ok(None);
        };
        let Some(kind) = unknown_kind_from_typeof(kind_literal.value.as_str()) else {
            return Ok(None);
        };
        let value = self.callback_expression(&unary.argument, params, body)?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let mut expr = if self.ctx.krate.types.get(value.ty) == Some(&Type::Unknown) {
            CallbackExpr {
                kind: CallbackExprKind::UnknownIs {
                    value: Box::new(value),
                    kind,
                },
                ty: bool_ty,
            }
        } else {
            CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::Bool(
                    self.type_matches_typeof(value.ty, kind_literal.value.as_str()),
                )),
                ty: bool_ty,
            }
        };
        if matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ) {
            expr = CallbackExpr {
                kind: CallbackExprKind::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(expr),
                },
                ty: bool_ty,
            };
        }
        Ok(Some(expr))
    }

    /// Maps supported TypeScript callback binary operators to HIR operators.
    fn callback_binary_op(
        &self,
        operator: BinaryOperator,
        start: u32,
        end: u32,
    ) -> Result<BinOp, SmeltError> {
        match operator {
            BinaryOperator::Addition => Ok(BinOp::Add),
            BinaryOperator::Subtraction => Ok(BinOp::Sub),
            BinaryOperator::Multiplication => Ok(BinOp::Mul),
            BinaryOperator::Division => Ok(BinOp::Div),
            BinaryOperator::Remainder => Ok(BinOp::Rem),
            BinaryOperator::StrictEquality | BinaryOperator::Equality => Ok(BinOp::Eq),
            BinaryOperator::StrictInequality | BinaryOperator::Inequality => Ok(BinOp::NotEq),
            BinaryOperator::LessThan => Ok(BinOp::Lt),
            BinaryOperator::LessEqualThan => Ok(BinOp::Lte),
            BinaryOperator::GreaterThan => Ok(BinOp::Gt),
            BinaryOperator::GreaterEqualThan => Ok(BinOp::Gte),
            BinaryOperator::ShiftLeft => Ok(BinOp::Shl),
            BinaryOperator::ShiftRight => Ok(BinOp::Shr),
            BinaryOperator::ShiftRightZeroFill => Ok(BinOp::UShr),
            _ => Err(SmeltError::unsupported(
                self.span(start, end),
                "callback binary operator is not supported yet",
            )),
        }
    }

    /// Lower direct TypeScript `Array.prototype.indexOf` and `lastIndexOf`.
    fn list_search_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "indexOf" => ListSearchOp::Find,
            "lastIndexOf" => ListSearchOp::RFind,
            _ => return Ok(None),
        };
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let [item_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf currently require exactly one item argument",
            ));
        };
        let item_ty = *element_ty;
        let item = self.argument(item_argument, body)?;
        if Self::expr_ty(body, item) != item_ty
            && self.ctx.krate.types.get(item_ty) != Some(&Type::Unknown)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf argument must match the array element type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListSearch { op, list, item },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.entries` calls.
    fn list_entries_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "entries" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array entries requires no arguments",
            ));
        }
        let list = self.expression(&member.object, body)?;
        let source_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(source_ty), Some(Type::TypeParam { .. })) {
            return Ok(None);
        }
        let list_ty = self.type_param_constraint_or_self(source_ty);
        let Some(Type::List(list_item_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let item_ty = *list_item_ty;
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        let entry_ty = self
            .ctx
            .krate
            .types
            .intern(Type::Tuple(vec![index_ty, item_ty]));
        let ty = self.ctx.krate.types.intern(Type::List(entry_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListEnumerate { list },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `.at(index)` on arrays and strings to Python-style HIR indexing.
    fn collection_at_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "at" {
            return Ok(None);
        }
        let [index_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array/string at requires exactly one numeric index",
            ));
        };
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let ty = match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(item_ty)) => *item_ty,
            Some(Type::String) => self.ctx.krate.types.intern(Type::String),
            _ => return Ok(None),
        };
        let index = self.argument(index_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, index)) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array/string at index must be a number",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Index { receiver, index },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    // Continued in the next split builder file.
}
