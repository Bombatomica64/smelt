// `type(x).__name__` and `x.__class__.__name__`: the name of `x`'s runtime
// class.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// * A builtin value's class is fixed by its static type: `int`, `float`,
//   `str`, `bool`, `NoneType`, `list`, `dict`, `set`, `tuple`.
// * A class instance may be any class at or below its static class. The name
//   is decided by `isinstance` tests (`ExprKind::InstanceOf`) against the
//   class's descendants, deepest first, so the first that holds is the exact
//   class; when none holds it is the static class itself. The descendants
//   come from the module's class hierarchy as scanned before lowering
//   (`ClassAttributeStores::class_bases`), so a base-class method lowered
//   before its subclasses still names them.
//
// `x` is evaluated once (bound to a temporary unless it is a local). Any other
// static type (a union, a callable) is refused.

impl ModuleBuilder<'_> {
    /// Lower `type(x).__name__` / `x.__class__.__name__`, or `None` when
    /// `attr` is not one of those spellings.
    fn type_name_expression(
        &mut self,
        attr: &ruff_python_ast::ExprAttribute,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if attr.attr.as_str() != "__name__" {
            return Ok(None);
        }
        let subject = match attr.value.as_ref() {
            Expr::Call(call)
                if matches!(call.func.as_ref(), Expr::Name(name) if name.id.as_str() == "type")
                    && !self.program_binds_name("type")
                    && call.arguments.keywords.is_empty() =>
            {
                let [subject] = call.arguments.args.as_ref() else {
                    return Ok(None);
                };
                subject
            }
            Expr::Attribute(class_attr) if class_attr.attr.as_str() == "__class__" => class_attr.value.as_ref(),
            _ => return Ok(None),
        };
        let span = self.span(attr.range);
        let str_ty = self.intern_type(Type::PyStr);
        let value = self.expression(subject, body)?;
        let value_ty = Self::expr_ty(body, value);
        let fixed = match self.ctx.krate.types.get(value_ty) {
            Some(Type::PyInt) => Some("int"),
            Some(Type::Float) => Some("float"),
            Some(Type::PyStr) => Some("str"),
            Some(Type::Bool) => Some("bool"),
            Some(Type::None) => Some("NoneType"),
            Some(Type::List(_)) => Some("list"),
            Some(Type::Dict(_, _)) => Some("dict"),
            Some(Type::Set(_)) => Some("set"),
            Some(Type::Tuple(_)) => Some("tuple"),
            Some(Type::Class { .. }) => None,
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "type(x).__name__ is supported for builtin values and class instances",
                ));
            }
        };
        let block = body.push_block(span);
        let name_text = |name: &str, arena: &mut Body| {
            arena.push_expr(HirExpr { kind: ExprKind::Literal(Literal::String(name.to_owned())), ty: str_ty, span })
        };
        let tail = if let Some(name) = fixed {
            // The static type decides; the operand is still evaluated.
            body.push_stmt_to_block(block, HirStmt::Expr(value));
            name_text(name, body)
        } else {
            let Some(&Type::Class { name: class_sym, .. }) = self.ctx.krate.types.get(value_ty) else {
                return Ok(None);
            };
            let class_name = self.ctx.krate.symbols.get(class_sym).unwrap_or_default().to_owned();
            let simple_name = class_name.rsplit('.').next().unwrap_or(&class_name).to_owned();
            // A local (`self`) is read where it is tested; any other subject
            // is bound once to a temporary.
            let value_index = usize::try_from(value.0).unwrap_or(usize::MAX);
            let local = if let ExprKind::Local(local) = body.exprs[value_index].kind {
                local
            } else {
                let local_name = self.intern_synthesized_name("__smelt_type_subject");
                let local = body.push_local(LocalDecl { name: Some(local_name), ty: value_ty, mutable: false, span });
                let pat = body.push_pattern(HirPattern::Binding(local));
                body.push_stmt_to_block(block, HirStmt::Let { pat, ty: value_ty, value: Some(value) });
                local
            };
            // Deepest descendants first, each tested before its ancestors.
            let mut chain = name_text(&simple_name, body);
            for descendant in self.class_descendants_shallow_first(&simple_name) {
                let descendant_sym = self.intern_name(&descendant);
                let bool_ty = self.intern_type(Type::Bool);
                let read = body.push_expr(HirExpr { kind: ExprKind::Local(local), ty: value_ty, span });
                let test = body.push_expr(HirExpr {
                    kind: ExprKind::InstanceOf { value: read, class: descendant_sym },
                    ty: bool_ty,
                    span,
                });
                let named = name_text(&descendant, body);
                chain = body.push_expr(HirExpr {
                    kind: ExprKind::Conditional { cond: test, then_expr: named, else_expr: chain },
                    ty: str_ty,
                    span,
                });
            }
            chain
        };
        let block_index = usize::try_from(block.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[block_index].tail = Some(tail);
        Ok(Some(body.push_expr(HirExpr { kind: ExprKind::Block(block), ty: str_ty, span })))
    }

    /// The module's classes below `class_name`, every class after its
    /// ancestors (so the conditional chain built from the end tests the
    /// deepest first).
    fn class_descendants_shallow_first(&self, class_name: &str) -> Vec<String> {
        let mut ordered: Vec<String> = Vec::new();
        let mut frontier = vec![class_name.to_owned()];
        while let Some(parent) = frontier.pop() {
            for (child, bases) in &self.module_class_bases {
                if bases.iter().any(|base| *base == parent) && !ordered.contains(child) && child != class_name {
                    ordered.push(child.clone());
                    frontier.push(child.clone());
                }
            }
        }
        // A breadth-or-depth walk may list a grandchild before a sibling;
        // ordering by depth keeps every class after all of its ancestors.
        let depth = |name: &str| {
            let mut depth = 0_usize;
            let mut current = name.to_owned();
            while current != class_name && depth < 64 {
                let Some((_, bases)) = self.module_class_bases.iter().find(|(class, _)| *class == current) else {
                    break;
                };
                let Some(parent) = bases.first() else {
                    break;
                };
                current = parent.clone();
                depth = depth.saturating_add(1);
            }
            depth
        };
        ordered.sort_by_key(|name| depth(name));
        ordered
    }
}
