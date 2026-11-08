// Member access and qualified call lowering helpers for `ModuleBuilder`.
// Included into lowering.rs via include!, so this file uses plain comments.
impl ModuleBuilder<'_> {
    /// Lower a qualified static method call `Class.staticMethod(args)`.
    ///
    /// Resolves the class name to its declared `@staticmethod` items and, when a
    /// match is found, lowers the call as a direct call to the receiver-free
    /// associated function so codegen emits `Class::staticMethod(args)`. Returns
    /// `None` when the callee is not a static-method call.
    fn class_static_method_call_expression(
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
        let Some(item_id) = self.class_static_method_item(class_name, method_name) else {
            return Ok(None);
        };
        let span = self.span(call.range());
        let return_ty = match self.item_ref(item_id) {
            Item::Function(function) => function.return_ty,
            Item::Class(_) | Item::Interface(_) | Item::TypeAlias(_) | Item::Const(_) | Item::MutableGlobal(_) => {
                return Ok(None);
            }
        };
        let callee = body.push_expr(HirExpr {
            kind: ExprKind::Item(item_id),
            ty: self.item_expr_type(item_id),
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

    /// Lower a qualified static-field read `Class.NAME` to its literal value.
    ///
    /// Returns `None` when the attribute does not name a class-level static
    /// field, so the caller can fall through to the ordinary attribute path.
    fn class_static_member_expression(
        &self,
        attr: &ruff_python_ast::ExprAttribute,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some((class_name, member)) = self.class_member_path(attr) else {
            return Ok(None);
        };
        let Some(class_item) = self.items.get(class_name).copied() else {
            return Ok(None);
        };
        let Item::Class(class) = self.item_ref(class_item) else {
            return Ok(None);
        };
        let Some(field) = class
            .static_fields
            .iter()
            .find(|field| self.ctx.krate.symbols.get(field.name) == Some(member))
        else {
            return Ok(None);
        };
        let Some(value) = field.value.clone() else {
            return Ok(None);
        };
        let ty = field.ty;
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::Literal(value),
            ty,
            span: self.span(attr.range),
        })))
    }

    /// Resolve a class's `@staticmethod` item by name from its class item.
    fn class_static_method_item(
        &self,
        class_name: &str,
        method_name: &str,
    ) -> Option<ItemId> {
        let class_item = self.items.get(class_name).copied()?;
        let Item::Class(class) = self.item_ref(class_item) else {
            return None;
        };
        class.static_methods.iter().copied().find(|item| {
            matches!(
                self.item_ref(*item),
                Item::Function(function)
                    if self.ctx.krate.symbols.get(function.name) == Some(method_name)
            )
        })
    }

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
        let span = self.span(call.range());
        let return_ty = match self.item_ref(item_id) {
            Item::Function(function) => function.return_ty,
            Item::Class(_) | Item::Interface(_) | Item::TypeAlias(_) | Item::Const(_) | Item::MutableGlobal(_) => {
                return Ok(None);
            }
        };
        let callee = body.push_expr(HirExpr {
            kind: ExprKind::Item(item_id),
            ty: self.item_expr_type(item_id),
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
                self.span(call.range()),
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
            span: self.span(call.range()),
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
        let span = self.span(call.range());
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
                let function_item = function.clone();
                let return_ty = function_item.return_ty;
                let callee = body.push_expr(HirExpr {
                    kind: ExprKind::Item(item_id),
                    ty: self.function_item_type(&function_item),
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
            Item::Interface(_)
            | Item::TypeAlias(_)
            | Item::Const(_)
            | Item::MutableGlobal(_) => Err(SmeltError::unsupported(
                span,
                "module namespace member is not callable",
            )),
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
            Item::Function(function) => {
                let function_item = function.clone();
                self.function_item_type(&function_item)
            }
            Item::Class(class) => {
                let sym = class.name;
                self.intern_type(Type::Class {
                    name: sym,
                    args: vec![],
                })
            }
            Item::Interface(_)
            | Item::TypeAlias(_)
            | Item::Const(_)
            | Item::MutableGlobal(_) => match self.item_ref(item_id) {
                Item::Const(const_item) => const_item.ty,
                Item::MutableGlobal(global_item) => global_item.ty,
                _ => self.intern_type(Type::None),
            },
        }
    }

    /// Return the first-class function type for a HIR function item reference.
    fn function_item_type(&mut self, function: &Function) -> TypeId {
        self.intern_type(Type::Function(FunctionType {
            params: function.params.iter().map(|param| param.ty).collect(),
            rest: function.rest,
            required_params: function.required_params,
            mutable_params: Vec::new(),
            return_ty: function.return_ty,
            is_async: function.is_async,
            may_throw: false,
        }))
    }

    /// Inline an importable constant expression into the current body when supported.
    fn const_item_expression(
        &self,
        item_id: ItemId,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        // A module global (a constructed module-level binding) is one shared
        // object: read its cell, never re-run its initializer.
        if let Item::MutableGlobal(global) = self.item_ref(item_id) {
            return Ok(Some(body.push_expr(HirExpr {
                kind: ExprKind::GlobalGet { item: item_id },
                ty: global.ty,
                span,
            })));
        }
        let Item::Const(const_item) = self.item_ref(item_id) else {
            return Ok(None);
        };
        let const_body = self
            .ctx
            .krate
            .bodies
            .get(usize::try_from(const_item.body.0).unwrap_or(usize::MAX))
            .ok_or_else(|| SmeltError::unsupported(span, "constant body is missing"))?;
        let value = Self::copy_pure_const_expr(const_body, const_item.value, body, span)
            .ok_or_else(|| {
                SmeltError::unsupported(
                    span,
                    "only constants built from literals, collection literals and zero-argument constructions can be imported",
                )
            })?;
        // The use takes the constant's declared type (a `number[]` const is
        // `List<Float>` even when its literal elements are narrower).
        let index = usize::try_from(value.0).unwrap_or(usize::MAX);
        if let Some(expr) = body.exprs.get_mut(index) {
            expr.ty = const_item.ty;
        }
        Ok(Some(value))
    }

    /// Copy the side-effect-free expression `expr` of a constant's `source`
    /// body into `body`, or `None` when any node of it could run code.
    ///
    /// A HIR constant has no runtime cell: every frontend inlines its value at
    /// each read, as TypeScript re-lowers an imported `export const PRIMES =
    /// [...]`. Copying is only sound for a tree that evaluates to the same
    /// value each time without effects: literals, list/set/dict/tuple
    /// literals of such trees, type assertions and unary operators over them,
    /// and a zero-argument construction. A call or any other node refuses.
    fn copy_pure_const_expr(
        source: &Body,
        expr: smelt_hir::ExprId,
        body: &mut Body,
        span: Span,
    ) -> Option<smelt_hir::ExprId> {
        let node = source.exprs.get(usize::try_from(expr.0).ok()?)?;
        let ty = node.ty;
        let copy_all = |items: &[smelt_hir::ExprId], target: &mut Body| {
            items
                .iter()
                .map(|item| Self::copy_pure_const_expr(source, *item, target, span))
                .collect::<Option<Vec<_>>>()
        };
        let kind = match &node.kind {
            ExprKind::Literal(literal) => ExprKind::Literal(literal.clone()),
            ExprKind::ListLit(items) => ExprKind::ListLit(copy_all(items, body)?),
            ExprKind::SetLit(items) => ExprKind::SetLit(copy_all(items, body)?),
            ExprKind::TupleLit(items) => ExprKind::TupleLit(copy_all(items, body)?),
            ExprKind::DictLit(entries) => ExprKind::DictLit(
                entries
                    .iter()
                    .map(|(key, value)| {
                        Some((
                            Self::copy_pure_const_expr(source, *key, body, span)?,
                            Self::copy_pure_const_expr(source, *value, body, span)?,
                        ))
                    })
                    .collect::<Option<Vec<_>>>()?,
            ),
            ExprKind::TypeAssert { value } => ExprKind::TypeAssert {
                value: Self::copy_pure_const_expr(source, *value, body, span)?,
            },
            ExprKind::UnaryOp { op, operand } => ExprKind::UnaryOp {
                op: *op,
                operand: Self::copy_pure_const_expr(source, *operand, body, span)?,
            },
            ExprKind::New { class, args } if args.is_empty() => ExprKind::New {
                class: *class,
                args: Vec::new(),
            },
            _ => return None,
        };
        Some(body.push_expr(HirExpr { kind, ty, span }))
    }
}
