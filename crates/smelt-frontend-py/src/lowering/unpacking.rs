// Starred assignment targets: `first, *rest = xs`, `*init, last = xs`,
// `a, *mid, z = xs`.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// Python binds the targets before the starred one to the first items, the
// ones after it to the last items, and the starred one to a list of what is
// left (possibly empty); fewer items than the unstarred targets raise
// `ValueError: not enough values to unpack (expected at least N, got M)`.
//
// The statement is desugared into the Python it means and each piece lowers
// through the ordinary statement rules:
//
//     __smelt_unpack = xs
//     if len(__smelt_unpack) < N:
//         raise ValueError(f"not enough values to unpack (expected at least N, got {len(__smelt_unpack)})")
//     first = __smelt_unpack[0]
//     rest = __smelt_unpack[1:len(__smelt_unpack) - K]     # K targets after the star
//     last = __smelt_unpack[len(__smelt_unpack) - K + j]
//
// so the starred target is a list of the value's item type, and every
// target, nested or not, is assigned as a plain assignment target would be.
// The value must be a list: a tuple's starred part would be a list of its
// remaining (possibly mixed) item types, which is not this shape.

impl ModuleBuilder<'_> {
    /// Whether `target` is a tuple or list target with a starred element.
    fn is_starred_unpacking(target: &Expr) -> bool {
        let elements = match target {
            Expr::Tuple(tuple) => &tuple.elts,
            Expr::List(list) => &list.elts,
            _ => return false,
        };
        elements.iter().any(|element| matches!(element, Expr::Starred(_)))
    }

    /// Lower `target = value` for a starred `target` (see the file header).
    #[expect(clippy::too_many_arguments, reason = "the target, the value, the statement and where it lowers")]
    fn starred_unpacking_assign(
        &mut self,
        target: &Expr,
        value: &Expr,
        range: TextRange,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(range);
        let elements = match target {
            Expr::Tuple(tuple) => &tuple.elts,
            Expr::List(list) => &list.elts,
            _ => return Err(SmeltError::unsupported(span, "a starred assignment needs a tuple or list target")),
        };
        let star_positions: Vec<usize> = elements
            .iter()
            .enumerate()
            .filter_map(|(index, element)| matches!(element, Expr::Starred(_)).then_some(index))
            .collect();
        let &[star] = star_positions.as_slice() else {
            return Err(SmeltError::unsupported(span, "multiple starred expressions in assignment"));
        };
        if self.program_binds_name("len") || self.program_binds_name("ValueError") {
            return Err(SmeltError::unsupported(
                span,
                "a starred assignment in a program that rebinds `len` or `ValueError` is not supported",
            ));
        }
        let before = star;
        let after = elements.len().saturating_sub(star).saturating_sub(1);
        let required = before.saturating_add(after);
        let temp = format!("__smelt_unpack_{}", range.start().to_u32());

        let mut binding = self.synthesized_stmt(&format!("{temp} = 0"), range)?;
        if let Stmt::Assign(assign) = &mut binding {
            *assign.value = value.clone();
        }
        self.statement_shape_in_block(&binding, body, block)?;
        let value_local = self.scope.locals.get(temp.as_str()).copied();
        if !value_local.is_some_and(|local| matches!(self.ctx.krate.types.get(Self::local_ty(body, local)), Some(Type::List(_)))) {
            return Err(SmeltError::unsupported(
                self.span(value.range()),
                "a starred assignment needs a list value",
            ));
        }
        let check = self.synthesized_stmt(
            &format!(
                "if len({temp}) < {required}:\n    raise ValueError(f\"not enough values to unpack (expected at least {required}, got {{len({temp})}})\")"
            ),
            range,
        )?;
        self.statement_shape_in_block(&check, body, block)?;
        for (index, element) in elements.iter().enumerate() {
            let (element_target, subscript) = match index.cmp(&star) {
                std::cmp::Ordering::Less => (element, format!("{index}")),
                std::cmp::Ordering::Equal => {
                    let Expr::Starred(starred) = element else {
                        continue;
                    };
                    let end = if after == 0 { String::new() } else { format!("len({temp}) - {after}") };
                    (starred.value.as_ref(), format!("{before}:{end}"))
                }
                std::cmp::Ordering::Greater => (element, format!("len({temp}) - {}", elements.len().saturating_sub(index))),
            };
            let mut assign = self.synthesized_stmt(&format!("__smelt_target = {temp}[{subscript}]"), range)?;
            if let Stmt::Assign(statement) = &mut assign
                && let Some(slot) = statement.targets.first_mut()
            {
                *slot = element_target.clone();
            }
            self.statement_shape_in_block(&assign, body, block)?;
        }
        Ok(())
    }

    /// Parse one synthesized Python statement, relocated to `range` so its
    /// diagnostics point at the source statement it stands for.
    fn synthesized_stmt(&self, source: &str, range: TextRange) -> Result<Stmt, SmeltError> {
        let span = self.span(range);
        let parsed = ruff_python_parser::parse_module(source).map_err(|error| {
            SmeltError::unsupported(span, format!("internal: synthesized statement does not parse: {error}"))
        })?;
        let mut stmt = parsed
            .into_syntax()
            .body
            .into_iter()
            .next()
            .ok_or_else(|| SmeltError::unsupported(span, "internal: empty synthesized statement"))?;
        Self::relocate_synthesized(&mut stmt, range);
        Ok(stmt)
    }

    /// Move a synthesized statement (an assignment, an `if`, a `raise`) and
    /// everything in it to `range`.
    fn relocate_synthesized(stmt: &mut Stmt, range: TextRange) {
        /// Relocates every expression a statement holds.
        struct Exprs(TextRange);
        impl ruff_python_ast::visitor::transformer::Transformer for Exprs {
            fn visit_expr(&self, expr: &mut Expr) {
                ruff_python_ast::relocate::relocate_expr(expr, self.0);
            }
        }
        ruff_python_ast::visitor::transformer::Transformer::visit_stmt(&Exprs(range), stmt);
        match stmt {
            Stmt::Assign(assign) => assign.range = range,
            Stmt::Raise(raise) => raise.range = range,
            Stmt::If(if_stmt) => {
                if_stmt.range = range;
                for nested in &mut if_stmt.body {
                    Self::relocate_synthesized(nested, range);
                }
            }
            _ => {}
        }
    }
}

impl ModuleBuilder<'_> {
    /// Whether the tuple or list target `target` stores to anything that
    /// already exists: a bound local, a module variable past its first
    /// assignment, an attribute or a subscript. Such an assignment writes
    /// those places (`a, b = b, a + b` in a loop updates `a` and `b`); one
    /// made only of fresh names declares them.
    fn destructuring_stores(&self, target: &Expr) -> bool {
        match target {
            Expr::Name(name) => {
                self.scope.locals.contains_key(name.id.as_str())
                    || (self.written_module_variable(name.id.as_str()).is_some()
                        && !(self.scope.kind == FnKind::Module
                            && self.module_variables.get(name.id.as_str()).is_some_and(|variable| !variable.reached)))
            }
            Expr::Attribute(_) | Expr::Subscript(_) => true,
            Expr::Tuple(tuple) => tuple.elts.iter().any(|element| self.destructuring_stores(element)),
            Expr::List(list) => list.elts.iter().any(|element| self.destructuring_stores(element)),
            _ => false,
        }
    }

    /// `a, b = value` where a target already exists (`destructuring_stores`):
    /// the value is evaluated once into a temporary, then each element is
    /// stored left to right, as Python assigns: into the existing local, the
    /// module variable, the attribute or the subscript it names, or a fresh
    /// local for a name not bound yet. A nested target unpacks its element
    /// the same way. The value must be a tuple (its elements' static types are
    /// known one by one).
    fn destructuring_assign(
        &mut self,
        target: &Expr,
        value: smelt_hir::ExprId,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(target.range());
        let value_ty = Self::expr_ty(body, value);
        let elements: Vec<&Expr> = match target {
            Expr::Tuple(tuple) => tuple.elts.iter().collect(),
            Expr::List(list) => list.elts.iter().collect(),
            _ => return Err(SmeltError::unsupported(span, "internal error: expected a tuple or list target")),
        };
        let Some(Type::Tuple(items)) = self.ctx.krate.types.get(value_ty).cloned() else {
            return Err(SmeltError::unsupported(
                span,
                "an unpacking assignment to existing names, attributes or items needs a tuple value",
            ));
        };
        if items.len() != elements.len() {
            return Err(SmeltError::unsupported(
                span,
                format!("cannot unpack {} values into {} targets", items.len(), elements.len()),
            ));
        }
        let temp = body.push_local(self.synthetic_local("__smelt_unpacked", value_ty, false, span));
        let pat = body.push_pattern(HirPattern::Binding(temp));
        body.push_stmt_to_block(block, HirStmt::Let { pat, ty: value_ty, value: Some(value) });
        for (index, (element, item_ty)) in elements.into_iter().zip(items).enumerate() {
            let element_span = self.span(element.range());
            let tuple = body.push_expr(HirExpr { kind: ExprKind::Local(temp), ty: value_ty, span: element_span });
            let item = body.push_expr(HirExpr { kind: ExprKind::TupleIndex { tuple, index }, ty: item_ty, span: element_span });
            self.store_unpacked(element, item, body, block)?;
        }
        Ok(())
    }

    /// Store the unpacked `item` into one element target of
    /// [`Self::destructuring_assign`].
    fn store_unpacked(
        &mut self,
        element: &Expr,
        item: smelt_hir::ExprId,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(element.range());
        let item_ty = Self::expr_ty(body, item);
        match element {
            Expr::Tuple(_) | Expr::List(_) => self.destructuring_assign(element, item, body, block),
            Expr::Name(name) if self.scope.locals.contains_key(name.id.as_str()) => {
                let target = self.expression(element, body)?;
                body.push_stmt_to_block(block, HirStmt::Assign { target, value: item });
                Ok(())
            }
            Expr::Name(name) if let Some(global) = self.written_module_variable(name.id.as_str()) => {
                let Item::MutableGlobal(declared) = self.item_ref(global) else {
                    return Err(SmeltError::unsupported(span, "internal error: a module variable is not a global"));
                };
                let ty = declared.ty;
                let set = body.push_expr(HirExpr { kind: ExprKind::GlobalSet { item: global, value: item }, ty, span });
                body.push_stmt_to_block(block, HirStmt::Expr(set));
                Ok(())
            }
            Expr::Name(_) => {
                let pat = self.binding_pattern_from_target(element, body, Some(item_ty))?;
                body.push_stmt_to_block(block, HirStmt::Let { pat, ty: item_ty, value: Some(item) });
                Ok(())
            }
            Expr::Attribute(_) | Expr::Subscript(_) => {
                let target = self.expression(element, body)?;
                let target = Self::subscript_store_target(target, body);
                body.push_stmt_to_block(block, HirStmt::Assign { target, value: item });
                Ok(())
            }
            _ => Err(SmeltError::unsupported(span, "an unpacking target must be a name, an attribute, an item, or a tuple of them")),
        }
    }
}
