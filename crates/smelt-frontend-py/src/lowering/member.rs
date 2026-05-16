impl ModuleBuilder<'_> {
    /// Lower supported class method calls into their HIR runtime operations.
    fn class_method_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Some((class_name, method_name)) = self.class_member_path(attr) else {
            return Ok(None);
        };
        let Some(item_id) = self
            .class_methods
            .get(class_name)
            .and_then(|methods| methods.get(method_name))
            .copied()
            .or_else(|| self.class_method_item_by_name(class_name, method_name))
        else {
            return Ok(None);
        };
        let span = self.span(call.range);
        let return_ty = match self.item_ref(item_id) {
            Item::Function(function) => function.return_ty,
            Item::Class(_) | Item::Interface(_) | Item::TypeAlias(_) | Item::Const(_) => {
                return Ok(None);
            }
        };
        let callee = body.push_expr(HirExpr {
            kind: ExprKind::Item(item_id),
            ty: return_ty,
            span: self.span(attr.range),
        });
        let args = call
            .arguments
            .args
            .iter()
            .map(|arg| self.expression(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::Call { callee, args },
            ty: return_ty,
            span,
        })))
    }

    /// Lower the narrow `int.__new__(cls, value)` pattern used by `IntEnum.__new__`.
    fn int_new_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "__new__"
            || !matches!(attr.value.as_ref(), Expr::Name(name) if name.id.as_str() == "int")
        {
            return Ok(None);
        }
        let [cls_expr, value_expr] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "int.__new__ currently supports cls and integer value arguments",
            ));
        };
        let cls = self.expression(cls_expr, body)?;
        let value = self.expression(value_expr, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, value)) != Some(&Type::Int) {
            return Err(SmeltError::unsupported(
                self.span(value_expr.range()),
                "int.__new__ value must be an integer",
            ));
        }
        let ty = Self::expr_ty(body, cls);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::Call {
                callee: cls,
                args: vec![value],
            },
            ty,
            span: self.span(call.range),
        })))
    }

    /// Resolve the `(class, member)` pair in an enum member access path.
    fn enum_member_path<'a>(
        &'a self,
        attr: &'a ruff_python_ast::ExprAttribute,
    ) -> Option<(&'a str, &'a str)> {
        let member = attr.attr.as_str();
        let (class_name, _) = self.class_member_path(attr)?;
        Some((class_name, member))
    }

    /// Resolve the `(class, member)` pair in direct or module-qualified class access.
    fn class_member_path<'a>(
        &'a self,
        attr: &'a ruff_python_ast::ExprAttribute,
    ) -> Option<(&'a str, &'a str)> {
        let member = attr.attr.as_str();
        match attr.value.as_ref() {
            Expr::Name(class_name) if self.items.contains_key(class_name.id.as_str()) => {
                Some((class_name.id.as_str(), member))
            }
            Expr::Attribute(class_attr) => {
                let class_item = self.module_member_item(class_attr)?;
                let Item::Class(class) = self.item_ref(class_item) else {
                    return None;
                };
                let class_name = self.ctx.krate.symbols.get(class.name)?;
                Some((class_name, member))
            }
            _ => None,
        }
    }

    /// Lower `module.member` when `module` is an imported package/module namespace.
    fn module_member_expression(
        &mut self,
        attr: &ruff_python_ast::ExprAttribute,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(item_id) = self.module_member_item(attr) else {
            return Ok(None);
        };
        let span = self.span(attr.range);
        if let Some(expr) = self.const_item_expression(item_id, body, span)? {
            return Ok(Some(expr));
        }
        let ty = self.item_expr_type(item_id);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::Item(item_id),
            ty,
            span,
        })))
    }

    /// Lower `module.member(...)` for imported package/module namespace members.
    fn module_member_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Some(item_id) = self.module_member_item(attr) else {
            return Ok(None);
        };
        let span = self.span(call.range);
        match self.item_ref(item_id) {
            Item::Class(class) => {
                let class_sym = class.name;
                let class_ty = self.intern_type(Type::Class {
                    name: class_sym,
                    args: vec![],
                });
                let args = call
                    .arguments
                    .args
                    .iter()
                    .map(|arg| self.expression(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::New {
                        class: class_sym,
                        args,
                    },
                    ty: class_ty,
                    span,
                })))
            }
            Item::Function(function) => {
                let return_ty = function.return_ty;
                let callee = body.push_expr(HirExpr {
                    kind: ExprKind::Item(item_id),
                    ty: return_ty,
                    span: self.span(attr.range),
                });
                let args = call
                    .arguments
                    .args
                    .iter()
                    .map(|arg| self.expression(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::Call { callee, args },
                    ty: return_ty,
                    span,
                })))
            }
            Item::Interface(_) | Item::TypeAlias(_) | Item::Const(_) => Err(
                SmeltError::unsupported(span, "module namespace member is not callable"),
            ),
        }
    }

    /// Resolve one item from an imported package/module namespace member access.
    fn module_member_item(&self, attr: &ruff_python_ast::ExprAttribute) -> Option<ItemId> {
        let Expr::Name(module) = attr.value.as_ref() else {
            return None;
        };
        self.module_namespaces
            .get(module.id.as_str())
            .and_then(|members| members.get(attr.attr.as_str()))
            .copied()
    }

    /// Return the HIR type to attach when an item appears as an expression.
    fn item_expr_type(&mut self, item_id: ItemId) -> TypeId {
        match self.item_ref(item_id) {
            Item::Function(function) => function.return_ty,
            Item::Class(class) => {
                let sym = class.name;
                self.intern_type(Type::Class {
                    name: sym,
                    args: vec![],
                })
            }
            Item::Interface(_) | Item::TypeAlias(_) | Item::Const(_) => {
                match self.item_ref(item_id) {
                    Item::Const(const_item) => const_item.ty,
                    _ => self.intern_type(Type::None),
                }
            }
        }
    }

    /// Inline an importable constant expression into the current body when supported.
    fn const_item_expression(
        &self,
        item_id: ItemId,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Item::Const(const_item) = self.item_ref(item_id) else {
            return Ok(None);
        };
        let const_body = self
            .ctx
            .krate
            .bodies
            .get(usize::try_from(const_item.body.0).unwrap_or(usize::MAX))
            .ok_or_else(|| SmeltError::unsupported(span, "constant body is missing"))?;
        let const_expr = const_body
            .exprs
            .get(usize::try_from(const_item.value.0).unwrap_or(usize::MAX))
            .ok_or_else(|| SmeltError::unsupported(span, "constant value is missing"))?
            .clone();
        match const_expr.kind {
            ExprKind::Literal(literal) => Ok(Some(body.push_expr(HirExpr {
                kind: ExprKind::Literal(literal),
                ty: const_item.ty,
                span,
            }))),
            ExprKind::New { class, args } if args.is_empty() => Ok(Some(body.push_expr(HirExpr {
                kind: ExprKind::New {
                    class,
                    args: Vec::new(),
                },
                ty: const_item.ty,
                span,
            }))),
            _ => Err(SmeltError::unsupported(
                span,
                "only literal and zero-argument constructed constants can be imported",
            )),
        }
    }
}
