impl ModuleBuilder<'_> {
    /// Lower supported Python dictionary projection calls into HIR map operations.
    fn dict_projection_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let op = match attr.attr.as_str() {
            "keys" => DictProjectionOp::Keys,
            "values" => DictProjectionOp::Values,
            "items" => DictProjectionOp::Entries,
            _ => return Ok(None),
        };
        let span = self.span(call.range());
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "dict keys/values/items methods require no arguments",
            ));
        }
        let dict = self.expression(&attr.value, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(key_type, value_type)) = self.ctx.krate.types.get(dict_ty) else {
            return Err(SmeltError::unsupported(
                span,
                "dict keys/values/items methods require a dict receiver",
            ));
        };
        let key_ty = *key_type;
        let value_ty = *value_type;
        let ty = match op {
            DictProjectionOp::FromEntries => return Ok(None),
            DictProjectionOp::Keys | DictProjectionOp::ForInKeys => self.intern_type(Type::List(key_ty)),
            // A symbol-keyed property list holds symbol VALUES, not their
            // descriptions: the property key an erased record stores is
            // `__smelt_symbol:<description>`, so handing back the bare
            // description made `source[sym]` miss and `target[sym] = v` write a
            // plain string key. `Type::Unknown` is the representation a symbol
            // already has everywhere else (`Literal::Symbol` ->
            // `SmeltUnknown::Symbol`), and the dynamic index paths map that tag
            // back to the prefixed key. Interned only on this arm — interning
            // `Unknown`/`List<Unknown>` for every projection would change which
            // record backing the other arms pick.
            DictProjectionOp::Symbols => {
                let symbol_key_ty = self.intern_type(Type::Unknown);
                self.intern_type(Type::List(symbol_key_ty))
            }
            DictProjectionOp::Values => self.intern_type(Type::List(value_ty)),
            DictProjectionOp::Entries => {
                let entry_ty = self.intern_type(Type::Tuple(vec![key_ty, value_ty]));
                self.intern_type(Type::List(entry_ty))
            }
        };
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::DictProjection { op, dict },
            ty,
            span,
        })))
    }

    /// Lower direct Python `requests.get(url)` calls to response text.
    fn requests_get_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if stdlib_dispatch::call_rule(call) != Some(RuleId::PyRequestsGet) {
            return Ok(None);
        }
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "get" {
            return Ok(None);
        }
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "requests" {
            return Ok(None);
        }
        let span = self.span(call.range());
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                span,
                "requests.get() supports exactly one string URL argument",
            ));
        }
        let [url_expr] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "requests.get() supports exactly one string URL argument",
            ));
        };
        let url = self.expression(url_expr, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, url)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                span,
                "requests.get() requires a str URL argument",
            ));
        }
        let ty = self.intern_type(Type::String);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::HttpGetText { url },
            ty,
            span,
        })))
    }

    /// Returns true when Python `len(...)` can lower directly to Rust `.len()`.
    fn supports_stdlib_len(&self, ty: TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::List(_) | Type::Dict(_, _) | Type::Tuple(_) | Type::String)
        )
    }

    /// Lower supported `asyncio.*` calls into shared async runtime operations.
    fn asyncio_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "asyncio" {
            return Ok(None);
        }

        let span = self.span(call.range());
        match smelt_asyncio::classify_attr(attr.attr.as_str()) {
            smelt_asyncio::AsyncioApi::Gather => {
                let args = call
                    .arguments
                    .args
                    .iter()
                    .map(|arg| self.expression(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                let outputs = args
                    .iter()
                    .map(|arg| {
                        self.future_inner_type(Self::expr_ty(body, *arg))
                            .ok_or_else(|| {
                                SmeltError::unsupported(
                                    span,
                                    "asyncio.gather arguments must be awaitable",
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let output_ty = self.intern_type(Type::Tuple(outputs));
                let ty = self.intern_type(Type::Future(output_ty));
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::All,
                        args,
                    },
                    ty,
                    span,
                })))
            }
            smelt_asyncio::AsyncioApi::Sleep => {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "asyncio.sleep requires exactly one duration argument",
                    ));
                }
                let [duration_expr] = call.arguments.args.as_ref() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "asyncio.sleep requires exactly one duration argument",
                    ));
                };
                let duration = self.expression(duration_expr, body)?;
                let none_ty = self.intern_type(Type::None);
                let ty = self.intern_type(Type::Future(none_ty));
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::Sleep,
                        args: vec![duration],
                    },
                    ty,
                    span,
                })))
            }
            smelt_asyncio::AsyncioApi::CreateTask => {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "asyncio.create_task requires exactly one awaitable argument",
                    ));
                }
                let [future_expr] = call.arguments.args.as_ref() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "asyncio.create_task requires exactly one awaitable argument",
                    ));
                };
                let future = self.expression(future_expr, body)?;
                let inner = self
                    .future_inner_type(Self::expr_ty(body, future))
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            span,
                            "asyncio.create_task argument must be awaitable",
                        )
                    })?;
                let ty = self.intern_type(Type::Future(inner));
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::CreateTask,
                        args: vec![future],
                    },
                    ty,
                    span,
                })))
            }
            smelt_asyncio::AsyncioApi::WaitFor => {
                if call.arguments.args.len() < 2 {
                    return Err(SmeltError::unsupported(
                        span,
                        "asyncio.wait_for requires an awaitable and timeout",
                    ));
                }
                let [future_expr, timeout_expr, ..] = call.arguments.args.as_ref() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "asyncio.wait_for requires an awaitable and timeout",
                    ));
                };
                let future = self.expression(future_expr, body)?;
                let timeout = self.expression(timeout_expr, body)?;
                let inner = self
                    .future_inner_type(Self::expr_ty(body, future))
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            span,
                            "asyncio.wait_for first argument must be awaitable",
                        )
                    })?;
                let ty = self.intern_type(Type::Future(inner));
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::WaitFor,
                        args: vec![future, timeout],
                    },
                    ty,
                    span,
                })))
            }
            smelt_asyncio::AsyncioApi::Queue | smelt_asyncio::AsyncioApi::Lock => {
                Err(SmeltError::unsupported(
                    span,
                    format!(
                        "asyncio.{} needs runtime object support and is not lowered in this slice",
                        attr.attr
                    ),
                ))
            }
            smelt_asyncio::AsyncioApi::LowerLevelLoop => Err(SmeltError::unsupported(
                span,
                format!(
                    "asyncio.{} is a lower-level event-loop API and is not supported",
                    attr.attr
                ),
            )),
            smelt_asyncio::AsyncioApi::Unknown => Err(SmeltError::unsupported(
                span,
                format!("asyncio.{} is not lowered yet", attr.attr),
            )),
            _ => Err(SmeltError::unsupported(
                span,
                format!("asyncio.{} is not recognized by this frontend", attr.attr),
            )),
        }
    }

    /// Resolve a name to a local variable or module-level item.
    fn identifier_expression(
        &mut self,
        name: &str,
        range: TextRange,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);

        if let Some(callback) = self.local_callbacks.get(name).cloned() {
            return self.callback_expr_to_closure(
                &callback.callback,
                &callback.params,
                span,
                body,
            );
        }

        if let Some(&local) = self.locals.get(name) {
            let ty = Self::local_ty(body, local);
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::Local(local),
                ty,
                span,
            }));
        }

        if let Some(&item_id) = self.items.get(name) {
            if let Some(expr) = self.const_item_expression(item_id, body, span)? {
                return Ok(expr);
            }
            let ty = self.item_expr_type(item_id);
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::Item(item_id),
                ty,
                span,
            }));
        }

        if let Some(value) = self.module_dunder_value(name) {
            let ty = self.intern_type(Type::String);
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::Literal(Literal::String(value)),
                ty,
                span,
            }));
        }

        Err(SmeltError::unsupported(
            span,
            format!("unresolved name '{name}'"),
        ))
    }

    /// Return the source-level value for supported module dunder variables.
    fn module_dunder_value(&self, name: &str) -> Option<String> {
        match name {
            "__file__" => Some(self.path.clone()),
            "__name__" => Some(module_name_from_path(&self.path)),
            _ => None,
        }
    }

    /// Lower `EnumClass.MEMBER` and `module.EnumClass.MEMBER` for targeted `IntEnum`s.
    fn enum_member_expression(
        &mut self,
        attr: &ruff_python_ast::ExprAttribute,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some((class_name, member_name)) = self.enum_member_path(attr) else {
            return Ok(None);
        };
        let Some(value) = self
            .enum_members
            .get(class_name)
            .and_then(|members| members.get(member_name))
            .copied()
        else {
            return Ok(None);
        };
        let ty = self.intern_type(Type::Int);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::Literal(Literal::Int(value)),
            ty,
            span: self.span(attr.range),
        })))
    }

    // Member lowering continues in `member.rs`.
}
