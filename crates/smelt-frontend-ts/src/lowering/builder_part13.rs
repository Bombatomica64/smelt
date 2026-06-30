impl ModuleBuilder<'_> {

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
        if let Expression::Identifier(object) = &member.object
            && (self.namespace_imports.contains(object.name.as_str())
                || self.value_imports.contains(object.name.as_str()))
        {
            let Some((left_argument, right_arguments)) = call.arguments.split_first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static array concat requires a receiver argument",
                ));
            };
            let left = self.argument(left_argument, body)?;
            return self.finish_list_concat_call(call, left, right_arguments, true, body);
        }
        let left = self.expression(&member.object, body)?;
        self.finish_list_concat_call(call, left, &call.arguments, false, body)
    }

    /// Finish array concat lowering after the receiver and the list of
    /// `concat(...)` arguments are known.
    ///
    /// JavaScript `Array.prototype.concat` accepts any number of arguments, each
    /// of which is either another array (spread into the result) or a scalar
    /// element (appended). This folds every argument onto the receiver list left
    /// to right so `a.concat(b, c, d)` and `a.concat(x)` both lower through the
    /// same per-argument path.
    fn finish_list_concat_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        mut left: smelt_hir::ExprId,
        right_arguments: &[Argument<'_>],
        right_prefers_list: bool,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let mut ty = Self::expr_ty(body, left);
        let item_ty = match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::List(list_item_ty)) => list_item_ty,
            Some(Type::Optional(inner)) => {
                let Some(Type::List(list_item_ty)) = self.ctx.krate.types.get(inner).cloned()
                else {
                    return Ok(None);
                };
                ty = inner;
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                });
                list_item_ty
            }
            Some(Type::Tuple(items)) => {
                let item_ty = self.tuple_items_element_type(&items);
                ty = self.ctx.krate.types.intern(Type::List(item_ty));
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                });
                item_ty
            }
            Some(Type::Union(items))
                if items.iter().any(|item| {
                    matches!(
                        self.ctx.krate.types.get(*item),
                        Some(
                            Type::List(_)
                                | Type::Unknown
                                | Type::TypeParam { .. }
                                | Type::Class { .. }
                        )
                    )
                }) =>
            {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                ty = self.ctx.krate.types.intern(Type::List(item_ty));
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                });
                item_ty
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) | None => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                ty = self.ctx.krate.types.intern(Type::List(item_ty));
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(call.span.start, call.span.end),
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
        if right_arguments.is_empty() {
            // `arr.concat()` with no arguments is a shallow copy.
            return Ok(Some(left));
        }
        for right_argument in right_arguments {
            let right = self.list_concat_argument(
                call,
                right_argument,
                ty,
                item_ty,
                right_prefers_list,
                body,
            )?;
            left = body.push_expr(Expr {
                kind: ExprKind::ListConcat { left, right },
                ty,
                span: self.span(call.span.start, call.span.end),
            });
        }
        Ok(Some(left))
    }

    /// Normalize a single `concat(...)` argument into a list of the receiver's
    /// element type so it can be appended with [`ExprKind::ListConcat`].
    ///
    /// `concat` accepts both arrays (spread) and scalar elements (wrapped into a
    /// singleton list); this picks between the two based on the argument's lowered
    /// type, matching the receiver's element type (`item_ty`) and list type (`ty`).
    fn list_concat_argument(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        right_argument: &Argument<'_>,
        ty: smelt_hir::TypeId,
        item_ty: smelt_hir::TypeId,
        right_prefers_list: bool,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut right = self.argument(right_argument, body)?;
        let right_ty = Self::expr_ty(body, right);
        let right = if right_ty == ty {
            right
        } else if let Some(Type::List(right_item_ty)) = self.ctx.krate.types.get(right_ty)
            && (*right_item_ty == item_ty
                || self.erased_or_union_surface(*right_item_ty)
                || self.erased_or_union_surface(item_ty))
        {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: right },
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else if right_ty == item_ty {
            body.push_expr(Expr {
                kind: ExprKind::ListLit(vec![right]),
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else if self.erased_or_union_surface(item_ty) {
            right = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: right },
                ty: item_ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            });
            body.push_expr(Expr {
                kind: ExprKind::ListLit(vec![right]),
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else if right_prefers_list && self.erased_or_union_surface(right_ty) {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: right },
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else if self.erased_or_union_surface(right_ty)
            || self.ctx.krate.types.get(right_ty) == Some(&Type::None)
        {
            if self.ctx.krate.types.get(right_ty) != Some(&Type::List(item_ty)) {
                right = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: right },
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
        Ok(right)
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
        // A method-style callback on an imported utility object is the lodash
        // free-function form `_.map(collection, iteratee)` (and `filter`, `some`,
        // etc.), not a `Array.prototype.map` receiver. The collection is the
        // first argument and the iteratee the second; the older single-argument
        // shape (`_.map(iteratee)` as a partially-applied factory) is also
        // accepted. The receiver is an opaque imported lodash value, so the call
        // result stays a placeholder — but every argument is still lowered so
        // captures and side effects are honored.
        if self.imported_utility_object(&member.object) {
            let (collection_argument, callback_argument) = match call.arguments.as_slice() {
                [callback_argument] => (None, callback_argument),
                [collection_argument, callback_argument] => {
                    (Some(collection_argument), callback_argument)
                }
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "lodash collection callbacks accept a collection and one iteratee",
                    ));
                }
            };
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
            let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
            let return_ty = match op {
                ListCallbackOp::Map | ListCallbackOp::Filter => list_ty,
                ListCallbackOp::Some | ListCallbackOp::Every => {
                    self.ctx.krate.types.intern(Type::Bool)
                }
                ListCallbackOp::ForEach => self.ctx.krate.types.intern(Type::None),
                _ => unknown_ty,
            };
            if let Some(collection_argument) = collection_argument {
                let _ = self.argument(collection_argument, body)?;
            }
            let index_ty = self.ctx.krate.types.intern(Type::Int);
            let _ = self.callback_argument_with_body_fallback(
                callback_argument,
                &[unknown_ty, index_ty, list_ty],
                unknown_ty,
                "array callback",
                body,
            )?;
            let ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: vec![list_ty],
                rest: None,
                required_params: None,
                mutable_params: Vec::new(),
                return_ty,
                is_async: false,
                may_throw: false,
            }));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
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
            Argument::TSAsExpression(as_expr) => {
                Self::expression_is_callback_like(&as_expr.expression)
            }
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
            Expression::TSAsExpression(as_expr) => {
                Self::expression_is_callback_like(&as_expr.expression)
            }
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
        let element_ty =
            if let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) {
                *list_element_ty
            } else if let Some(Type::Tuple(items)) = self.ctx.krate.types.get(list_ty).cloned() {
                let item_ty = self.flattened_tuple_item_type(items);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(list_span_start, list_span_end),
                });
                list_ty = asserted_ty;
                item_ty
            } else {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
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
            let callback = self.callback_argument(
                callback_argument,
                &callback_param_tys,
                "array reduce",
                body,
            )?;
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
    fn callback_expr_to_closure_with_return_ty(
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
    fn remap_callback_captures(
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
    fn callback_captures(&mut self, callback: &CallbackExpr, body: &Body) -> Vec<ClosureCapture> {
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
    fn callback_method_call_to_body_expr(
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
                && matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::Dict(_, _))) =>
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
            "map" | "flatMap"
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
    fn callback_callable_field_method_to_body_expr(
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
    fn callback_call_method_to_body_expr(
        &mut self,
        receiver: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        raw_args: &[CallbackCallArg],
        ty: smelt_hir::TypeId,
        callback_args: &[smelt_hir::ExprId],
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
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

    /// Convert a callback `concat` argument into the list operand expected by HIR.
    fn callback_concat_right_to_body_expr(
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
    fn callback_method_receiver_is_list_like(&self, receiver_ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(receiver_ty)),
            Some(Type::List(_) | Type::Tuple(_))
        )
    }

    /// Infer the result type for a callback-body `Array.prototype.flat` call.
    fn callback_flat_result_type(
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
    fn callback_string_case_to_body_expr(
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
    fn callback_coerce_to_string(
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
    fn callback_at_call_to_body_expr(
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
    fn callback_join_call_to_body_expr(
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

    /// Resolve a callback function symbol back to its normal HIR item.
    fn callback_function_item(
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
    fn callback_function_item_closure(
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
                self.callback_expr_to_body_expr(&arg.expr, args, body, span)
            })
            .collect()
    }

    /// Expand spread callback arguments into fixed function parameters and an optional rest list.
    fn callback_spread_call_args_to_body_exprs(
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
    fn callback_packed_spread_call_args(
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
    fn callback_local_value_is_callable_surface(&self, ty: smelt_hir::TypeId) -> bool {
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
    fn opaque_local_callback(
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
    fn opaque_member_callback(&mut self, expected_param_tys: &[smelt_hir::TypeId]) -> CallbackExpr {
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
    fn is_opaque_callback_value(&self, name: &str) -> bool {
        !self.locals.contains_key(name)
            && !self.items.contains_key(name)
            && (self.value_imports.contains(name) || self.source_contains_forward_callable(name))
    }

    /// Return whether an expression is an imported utility namespace/object.
    fn imported_utility_object(&self, expression: &Expression<'_>) -> bool {
        matches!(
            expression,
            Expression::Identifier(object)
                if self.namespace_imports.contains(object.name.as_str())
                    || self.object_namespaces.contains_key(object.name.as_str())
        )
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

    /// Lower a callback statement that must terminate the current branch.
    fn callback_terminating_statement<'a>(
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
                    contextual_function.and_then(|function| function.params.get(index).copied())
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
        let saved_narrowed_locals = std::mem::take(&mut self.narrowed_locals);
        let infer_expression_return =
            arrow.expression && matches!(self.ctx.krate.types.get(return_ty), Some(Type::Unknown));
        self.current_async = arrow.r#async;
        self.current_return_ty = Some(return_ty);
        let mut actual_return_ty = return_ty;
        let predeclare_result = if arrow.expression {
            Ok(())
        } else {
            self.predeclare_local_function_declarations(&arrow.body.statements, &mut closure_body)
                .and_then(|()| {
                    self.predeclare_local_arrow_callbacks(&arrow.body.statements, &mut closure_body)
                })
        };
        let lowering_result = if let Err(error) = predeclare_result {
            Err(error)
        } else if arrow.expression {
            match self.arrow_return_expression(arrow) {
                Ok(return_expression) => {
                    let hint = (!infer_expression_return).then_some(return_ty);
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
        self.narrowed_locals = saved_narrowed_locals;
        for (name, prior) in saved_locals.into_iter().rev() {
            if let Some(local) = prior {
                self.locals.insert(name, local);
            } else {
                self.locals.remove(name.as_str());
            }
        }
        lowering_result?;
        let may_throw = Self::body_contains_uncaught_throw(&closure_body);
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: param_tys.to_vec(),
            rest: arrow.params.rest.as_ref().map(|_| arrow.params.items.len()),
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: actual_return_ty,
            is_async: arrow.r#async,
            may_throw,
        }));
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest: arrow.params.rest.as_ref().map(|_| arrow.params.items.len()),
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
    fn callback_expr_contains_throw(callback: &CallbackExpr) -> bool {
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

    /// Returns whether a lowered closure body can throw past its own boundary.
    ///
    /// Try bodies with a catch handler are considered locally handled here: the
    /// MIR lowering attaches exception edges for nested calls and explicit
    /// throws, so only statements that can escape the closure need to widen the
    /// closure ABI to `Result`.
    fn body_contains_uncaught_throw(body: &Body) -> bool {
        Self::block_contains_uncaught_throw(body, body.root)
    }

    /// Returns whether a HIR block contains a throw not protected by catch.
    fn block_contains_uncaught_throw(body: &Body, block: smelt_hir::BlockId) -> bool {
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
    fn stmt_contains_uncaught_throw(body: &Body, stmt: &Stmt) -> bool {
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
                return self.callback_expr_to_closure_with_return_ty(
                    return_ty,
                    &callback,
                    &params,
                    rest,
                    contextual_function
                        .as_ref()
                        .and_then(|function| function.required_params),
                    span,
                    body,
                );
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
            Expression::ThisExpression(_) => {
                if !param_names.contains("this") && self.locals.contains_key("this") {
                    captures.push("this".to_owned());
                }
            }
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
            Expression::NewExpression(new_expr) => {
                self.collect_expression_capture_names(&new_expr.callee, param_names, captures);
                for arg in &new_expr.arguments {
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
                            Argument::SpreadElement(spread) => self
                                .collect_expression_capture_names(
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
                for expression in &sequence.expressions {
                    self.collect_expression_capture_names(expression, param_names, captures);
                }
            }
            _ => {}
        }
    }

    /// Collect captured locals referenced by a simple assignment target
    /// (the target of `x++`, `--y`, or `obj[i]++`).
    ///
    /// `++counter` and `startIndex++` in the curry/bind/after family mutate a
    /// captured enclosing local; without traversing the update target the
    /// mutated local is never recorded as a capture and the closure body fails
    /// with `unresolved identifier`.
    fn collect_simple_assignment_target_capture_names(
        &self,
        target: &SimpleAssignmentTarget<'_>,
        param_names: &HashSet<String>,
        captures: &mut Vec<String>,
    ) {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let name = identifier.name.as_str();
                if !param_names.contains(name) && self.locals.contains_key(name) {
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
    fn collect_for_left_binding_names(left: &ForStatementLeft<'_>, names: &mut HashSet<String>) {
        let ForStatementLeft::VariableDeclaration(decl) = left else {
            return;
        };
        for declarator in &decl.declarations {
            let mut binding_names = Vec::new();
            Self::binding_pattern_names(&declarator.id, &mut binding_names);
            names.extend(binding_names);
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
                    && matches!(argument, Argument::ArrowFunctionExpression(_)) =>
            {
                let Argument::ArrowFunctionExpression(arrow) = argument else {
                    return Err(error);
                };
                let expr =
                    self.arrow_closure_body_expr(arrow, expected_param_tys, bool_ty, body)?;
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
                let expr = self.arrow_closure_body_expr(
                    arrow,
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

    /// Return whether compact callback lowering should retry as a normal closure.
    fn should_fallback_to_closure_body_for_callback(error: &SmeltError) -> bool {
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
                        ArrayExpressionElement::BinaryExpression(binary) => {
                            let op = self.callback_binary_op(
                                binary.operator,
                                binary.span.start,
                                binary.span.end,
                            )?;
                            let lhs = self.callback_expression(&binary.left, params, body)?;
                            let rhs = self.callback_expression(&binary.right, params, body)?;
                            let ty = self.binary_result_type(op, lhs.ty, rhs.ty);
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
                    Some(Type::Dict(_, value)) => *value,
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
                        Some(
                            Type::Dict(_, _)
                                | Type::Class { .. }
                                | Type::Unknown
                                | Type::TypeParam { .. }
                        )
                    );
                if !numeric_index && !string_key_index {
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
                    ) && assign.operator == AssignmentOperator::Assign
                    {
                        // A member-target store (`obj[k] = v` / `obj.k = v`) mutates the
                        // receiver, but the side-effect-free callback expression IR cannot
                        // represent the store — only its right-hand value. Bail so the caller
                        // re-lowers this arrow through full closure-body lowering, which keeps
                        // the mutation. (Previously the store was silently dropped, leaving
                        // mutating reducers like `(acc, x) => { acc[x] = x; return acc; }` as
                        // identity functions.)
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
        let [
            Argument::RegExpLiteral(pattern),
            Argument::ArrowFunctionExpression(replacement),
        ] = call.arguments.as_slice()
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
        let ty = self.ctx.krate.types.intern(Type::String);
        if self.typeof_type_name(operand.ty).is_none() {
            return Ok(CallbackExpr {
                kind: CallbackExprKind::TypeofValue {
                    value: Box::new(operand),
                },
                ty,
            });
        }
        let kind = self.typeof_type_name(operand.ty).unwrap_or("object");
        Ok(CallbackExpr {
            kind: CallbackExprKind::Literal(Literal::String(kind.to_owned())),
            ty,
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

    /// Lower `value === undefined/null` checks inside callback expressions.
    fn callback_nullish_binary(
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
        let Some((value_expr, nullish_expr)) =
            Self::nullish_comparison_parts(&binary.left, &binary.right)
        else {
            return Ok(None);
        };
        let is_undefined_comparison = Self::is_undefined_expression(nullish_expr);
        let is_strict = matches!(
            binary.operator,
            BinaryOperator::StrictEquality | BinaryOperator::StrictInequality
        );
        let value = self.callback_expression(value_expr, params, body)?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let is_inequality = matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        );
        if self.ctx.krate.types.get(value.ty) == Some(&Type::Unknown) {
            let check = if is_strict {
                CallbackExpr {
                    kind: CallbackExprKind::UnknownIs {
                        value: Box::new(value),
                        kind: if is_undefined_comparison {
                            UnknownKind::Undefined
                        } else {
                            UnknownKind::Null
                        },
                    },
                    ty: bool_ty,
                }
            } else {
                let null_check = CallbackExpr {
                    kind: CallbackExprKind::UnknownIs {
                        value: Box::new(value.clone()),
                        kind: UnknownKind::Null,
                    },
                    ty: bool_ty,
                };
                let undefined_check = CallbackExpr {
                    kind: CallbackExprKind::UnknownIs {
                        value: Box::new(value),
                        kind: UnknownKind::Undefined,
                    },
                    ty: bool_ty,
                };
                CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op: BinOp::Or,
                        lhs: Box::new(null_check),
                        rhs: Box::new(undefined_check),
                    },
                    ty: bool_ty,
                }
            };
            if is_inequality {
                return Ok(Some(CallbackExpr {
                    kind: CallbackExprKind::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(check),
                    },
                    ty: bool_ty,
                }));
            }
            return Ok(Some(check));
        }
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let op = match binary.operator {
            BinaryOperator::StrictEquality => BinOp::JsStrictEq,
            BinaryOperator::StrictInequality => BinOp::JsStrictNotEq,
            BinaryOperator::Equality => BinOp::Eq,
            BinaryOperator::Inequality => BinOp::NotEq,
            _ => unreachable!("callback nullish comparison operators are filtered above"),
        };
        Ok(Some(CallbackExpr {
            kind: CallbackExprKind::Binary {
                op,
                lhs: Box::new(value),
            rhs: Box::new(CallbackExpr {
                    kind: CallbackExprKind::Literal(if is_undefined_comparison {
                        Literal::Undefined
                    } else {
                        Literal::None
                    }),
                    ty: none_ty,
                }),
            },
            ty: bool_ty,
        }))
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
            // `===`/`!==` keep JS reference semantics (`JsStrictEq`); `==`/`!=`
            // stay structural (`Eq`). See builder_part08's mapping.
            BinaryOperator::StrictEquality => Ok(BinOp::JsStrictEq),
            BinaryOperator::Equality => Ok(BinOp::Eq),
            BinaryOperator::StrictInequality => Ok(BinOp::JsStrictNotEq),
            BinaryOperator::Inequality => Ok(BinOp::NotEq),
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
        let mut list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some((list_ty, item_ty)) = self.list_surface_type(list_ty) else {
            return Ok(None);
        };
        if Self::expr_ty(body, list) != list_ty {
            list = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: list },
                ty: list_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
        }
        let [item_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf currently require exactly one item argument",
            ));
        };
        let item = self.argument(item_argument, body)?;
        if !self.array_item_type_compatible(Self::expr_ty(body, item), item_ty) {
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
        if !matches!(
            self.ctx.krate.types.get(source_ty),
            Some(Type::TypeParam { .. })
        ) {
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

    /// Lower TypeScript `.at(index)` on arrays and strings to optional HIR indexing.
    ///
    /// JavaScript `.at` accepts negative indexes, but out-of-range positions
    /// return `undefined` rather than raising. Model that with `OptionalIndex`
    /// so generated Rust uses checked normalized indexes for misses.
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
        let item_ty = match self.ctx.krate.types.get(receiver_ty) {
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
        let ty = self.ctx.krate.types.intern(Type::Optional(item_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::OptionalIndex { receiver, index },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    // Continued in the next split builder file.
}
