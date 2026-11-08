// Module-level variables as module globals.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// A Python module's top-level `NAME = value` (or `NAME: T = value`) binds a
// name in the module's namespace: every function of the module reads it, a
// function that declares `global NAME` writes it, and another module imports
// it (`from config import LIMIT`), from Python or from TypeScript. A local of
// the module body can do none of that, so such a binding is a module global:
// an `Item::MutableGlobal`, read with `GlobalGet` and written with
// `GlobalSet`, exactly as the TypeScript frontend lifts a module slot.
//
// Functions are lowered (pass 1) before the module body (pass 2), so the
// globals are declared first, in pass 0, from what a statement says without
// lowering it: the annotation's type, or the type of a literal value. The
// value itself is lowered when the module body reaches the statement, into a
// lazy initializer the body touches there, so it runs in source order; a
// literal is stored inline. A binding whose type pass 0 cannot tell stays a
// module-body local, as before.

/// What pass 0 declared for one module variable.
#[derive(Debug, Clone, Copy)]
struct ModuleVariable {
    /// The `Item::MutableGlobal`.
    item: ItemId,
    /// Whether the module body has reached the binding's first assignment.
    reached: bool,
}

impl ModuleBuilder<'_> {
    /// Pass 0 of [`Self::module`]: declare a module global for every
    /// top-level binding whose type is known before lowering; see the file
    /// header.
    fn declare_module_variables(&mut self, module: &ModModule, hir_module: &mut Module) {
        let defined = module
            .body
            .iter()
            .filter_map(|stmt| match stmt {
                Stmt::FunctionDef(function) => Some(function.name.as_str()),
                Stmt::ClassDef(class) => Some(class.name.as_str()),
                _ => None,
            })
            .collect::<HashSet<_>>();
        for stmt in &module.body {
            let Some((name, annotation, value)) = Self::module_binding(stmt) else {
                continue;
            };
            if defined.contains(name)
                || self.module_variables.contains_key(name)
                || (name.starts_with("__") && name.ends_with("__"))
                || is_module_all_assignment(stmt)
                || matches!(stmt, Stmt::Assign(assign) if Self::constructed_constant_shape(assign).is_some())
            {
                continue;
            }
            let ty = match annotation {
                Some(annotation) => match self.annotation_to_hir(annotation) {
                    Ok(ty) => ty,
                    Err(_) => continue,
                },
                None => match self.literal_value_type(value) {
                    Some(ty) => ty,
                    None => continue,
                },
            };
            let literal = Self::scalar_literal(value).filter(|literal| self.literal_has_type(literal, ty));
            let init = literal.map_or(smelt_hir::MutableGlobalInit::Pending, smelt_hir::MutableGlobalInit::Literal);
            let span = self.span(stmt.range());
            let name_sym = self.intern_name(name);
            let item = self.ctx.krate.push_item(Item::MutableGlobal(smelt_hir::MutableGlobalItem {
                name: name_sym,
                ty,
                init,
                visibility: Visibility::Public,
                span,
            }));
            self.items.insert(name.to_owned(), item);
            self.exports.insert(name.to_owned(), item);
            hir_module.items.push(item);
            self.module_variables.insert(name.to_owned(), ModuleVariable { item, reached: false });
        }
    }

    /// The `(name, annotation, value)` of a top-level `NAME = value` or
    /// `NAME: T = value`.
    fn module_binding(stmt: &Stmt) -> Option<(&str, Option<&Expr>, &Expr)> {
        match stmt {
            Stmt::Assign(assign) => match assign.targets.as_slice() {
                [Expr::Name(target)] => Some((target.id.as_str(), None, &assign.value)),
                _ => None,
            },
            Stmt::AnnAssign(ann) => match (ann.target.as_ref(), ann.value.as_deref()) {
                (Expr::Name(target), Some(value)) => Some((target.id.as_str(), Some(&ann.annotation), value)),
                _ => None,
            },
            _ => None,
        }
    }

    /// The type of a literal value (`3`, `"x"`, `[1, 2]`, `{"a": 1.5}`), or
    /// `None` when the value is not one or its items disagree.
    fn literal_value_type(&mut self, value: &Expr) -> Option<TypeId> {
        let ty = match value {
            Expr::NumberLiteral(number) => match number.value {
                Number::Int(_) => Type::Int,
                Number::Float(_) => Type::Float,
                Number::Complex { .. } => return None,
            },
            Expr::StringLiteral(_) => Type::PyStr,
            Expr::BooleanLiteral(_) => Type::Bool,
            Expr::List(list) => Type::List(self.literal_items_type(&list.elts)?),
            Expr::Set(set) => Type::Set(self.literal_items_type(&set.elts)?),
            Expr::Tuple(tuple) => Type::Tuple(
                tuple.elts.iter().map(|item| self.literal_value_type(item)).collect::<Option<Vec<_>>>()?,
            ),
            Expr::Dict(dict) => {
                let keys = dict.items.iter().map(|item| item.key.as_ref()).collect::<Option<Vec<_>>>()?;
                let values = dict.items.iter().map(|item| &item.value).collect::<Vec<_>>();
                let key = self.literal_items_type(keys.into_iter())?;
                let value = self.literal_items_type(values.into_iter())?;
                Type::Dict(key, value)
            }
            _ => return None,
        };
        Some(self.intern_type(ty))
    }

    /// The one literal type every item of a non-empty collection has.
    fn literal_items_type<'expr>(&mut self, items: impl IntoIterator<Item = &'expr Expr>) -> Option<TypeId> {
        let mut found = None;
        for item in items {
            let ty = self.literal_value_type(item)?;
            if found.is_some_and(|known| known != ty) {
                return None;
            }
            found = Some(ty);
        }
        found
    }

    /// A scalar literal value stored inline as a global's initializer.
    fn scalar_literal(value: &Expr) -> Option<Literal> {
        match value {
            Expr::NumberLiteral(number) => match &number.value {
                Number::Int(int) => int.as_i64().map(Literal::Int),
                Number::Float(float) => Some(Literal::Float(*float)),
                Number::Complex { .. } => None,
            },
            Expr::StringLiteral(text) => Some(Literal::String(text.value.to_str().to_owned())),
            Expr::BooleanLiteral(boolean) => Some(Literal::Bool(boolean.value)),
            _ => None,
        }
    }

    /// Whether `literal` is a value of `ty` (`x: float = 3` stores a float).
    fn literal_has_type(&self, literal: &Literal, ty: TypeId) -> bool {
        matches!(
            (literal, self.ctx.krate.types.get(ty)),
            (Literal::Int(_), Some(Type::Int))
                | (Literal::Float(_), Some(Type::Float))
                | (Literal::String(_), Some(Type::PyStr))
                | (Literal::Bool(_), Some(Type::Bool))
        )
    }

    /// The module global `name` names where it is written from here: at the
    /// module's top level, or in a function that declared it `global`.
    fn written_module_variable(&self, name: &str) -> Option<ItemId> {
        let variable = self.module_variables.get(name)?;
        (self.scope.kind == FnKind::Module || self.scope.globals.contains(name)).then_some(variable.item)
    }

    /// Lower `NAME = value` (or `NAME: T = value`) to a module global, or
    /// `None` when `name` is not one written from here.
    ///
    /// The module body's first assignment of a global with a pending
    /// initializer becomes that initializer, touched here so it runs in source
    /// order; every other write stores with `GlobalSet`.
    fn module_variable_assign(
        &mut self,
        name: &str,
        value: &Expr,
        stmt_range: TextRange,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Option<Result<(), SmeltError>> {
        let item = self.written_module_variable(name)?;
        let span = self.span(stmt_range);
        let Item::MutableGlobal(global) = self.item_ref(item) else {
            return None;
        };
        let (ty, pending) = (global.ty, matches!(global.init, smelt_hir::MutableGlobalInit::Pending));
        let first = self.scope.kind == FnKind::Module && self.module_variables.get(name).is_some_and(|variable| !variable.reached);
        if first {
            if let Some(variable) = self.module_variables.get_mut(name) {
                variable.reached = true;
            }
            if !pending {
                // The literal initializer already is the value.
                return Some(Ok(()));
            }
            return Some(self.module_variable_initializer(name, item, ty, value, span, body, block));
        }
        let value = match self.expression_with_hint(value, body, Some(ty)) {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        let set = body.push_expr(HirExpr { kind: ExprKind::GlobalSet { item, value }, ty, span });
        body.push_stmt_to_block(block, HirStmt::Expr(set));
        Some(Ok(()))
    }

    /// Lower a global's first value into its lazy initializer and touch the
    /// global at the statement, so the value is computed in source order.
    #[expect(clippy::too_many_arguments, reason = "the binding, its global and where the touch goes")]
    fn module_variable_initializer(
        &mut self,
        name: &str,
        item: ItemId,
        ty: TypeId,
        value: &Expr,
        span: Span,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let mut init_body = Body::new(None, span);
        let lowered = self.with_fn_scope(FnKind::Sync, |this| this.expression_with_hint(value, &mut init_body, Some(ty)))?;
        init_body.push_stmt(HirStmt::Return(Some(lowered)));
        let body_id = self.ctx.krate.push_body(init_body);
        let init_name = self.intern_synthesized_name(&format!("smelt_global_init${name}${}", item.0));
        let init_item = self.ctx.krate.push_item(Item::Function(Function {
            name: init_name,
            span,
            type_params: Vec::new(),
            params: Vec::new(),
            rest: None,
            required_params: Some(0),
            return_ty: ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        if let Some(Item::MutableGlobal(global)) =
            self.ctx.krate.items.get_mut(usize::try_from(item.0).unwrap_or(usize::MAX))
        {
            global.init = smelt_hir::MutableGlobalInit::Initializer(init_item);
        }
        self.pending_module_items.push(init_item);
        let unit = self.intern_type(Type::None);
        let touch = body.push_expr(HirExpr { kind: ExprKind::GlobalGet { item }, ty: unit, span });
        body.push_stmt_to_block(block, HirStmt::Expr(touch));
        Ok(())
    }

    /// Lower `NAME op= value` on a module global written from here, or `None`.
    fn module_variable_aug_assign(
        &mut self,
        aug: &StmtAugAssign,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Option<Result<(), SmeltError>> {
        let Expr::Name(target) = aug.target.as_ref() else {
            return None;
        };
        let item = self.written_module_variable(target.id.as_str())?;
        let Item::MutableGlobal(global) = self.item_ref(item) else {
            return None;
        };
        let ty = global.ty;
        let span = self.span(aug.range);
        let result = (|| {
            let op = Self::python_arithmetic_op(aug.op).ok_or_else(|| {
                SmeltError::unsupported(span, format!("augmented assignment operator '{:?}' is not supported", aug.op))
            })?;
            let current = body.push_expr(HirExpr { kind: ExprKind::GlobalGet { item }, ty, span });
            let rhs = self.expression(&aug.value, body)?;
            let compound = self.python_arithmetic(op, current, rhs, span, body)?;
            if Self::expr_ty(body, compound) != ty {
                return Err(SmeltError::unsupported(
                    span,
                    "this augmented assignment changes the type of a module variable; declare its wider type",
                ));
            }
            let set = body.push_expr(HirExpr { kind: ExprKind::GlobalSet { item, value: compound }, ty, span });
            body.push_stmt_to_block(block, HirStmt::Expr(set));
            Ok(())
        })();
        Some(result)
    }

    /// `global NAME, …` in a function: later writes of those names store the
    /// module globals. A name that is not a module global Smelt declared is a
    /// named blocker, never a silently local write.
    fn global_statement(&mut self, global: &ruff_python_ast::StmtGlobal) -> Result<(), SmeltError> {
        for name in &global.names {
            if !self.module_variables.contains_key(name.as_str()) {
                return Err(SmeltError::unsupported(
                    self.span(global.range),
                    format!(
                        "`global {name}` names no module variable Smelt can type; annotate its module-level assignment"
                    ),
                ));
            }
            self.scope.globals.insert(name.as_str().to_owned());
        }
        Ok(())
    }
}
