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

/// The names a module's top-level code binds, functions and classes aside.
///
/// `all` holds every such name; `other` the ones some statement binds
/// otherwise than as the single name target of an assignment, annotated
/// assignment or augmented assignment (a `for` or `with` target, an
/// unpacking, a walrus, a `del`). Only the plain forms write a module global,
/// so a name in `other` is never made one.
#[derive(Default)]
struct ModuleStores<'a> {
    /// Every name bound at module level.
    all: HashSet<&'a str>,
    /// The names bound at module level by a form other than a plain one.
    other: HashSet<&'a str>,
    /// Every plain binding statement of each name, in source order.
    plain: HashMap<&'a str, Vec<&'a Stmt>>,
}

impl<'a> ruff_python_ast::visitor::Visitor<'a> for ModuleStores<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            // Their names are items, and their bodies are other scopes.
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) | Stmt::Import(_) | Stmt::ImportFrom(_) => {}
            Stmt::Assign(assign) if matches!(assign.targets.as_slice(), [Expr::Name(_)]) => {
                if let [Expr::Name(target)] = assign.targets.as_slice() {
                    self.all.insert(target.id.as_str());
                    self.plain.entry(target.id.as_str()).or_default().push(stmt);
                }
                self.visit_expr(&assign.value);
            }
            Stmt::AnnAssign(ann) if matches!(ann.target.as_ref(), Expr::Name(_)) => {
                if let Expr::Name(target) = ann.target.as_ref() {
                    self.all.insert(target.id.as_str());
                    self.plain.entry(target.id.as_str()).or_default().push(stmt);
                }
                if let Some(value) = &ann.value {
                    self.visit_expr(value);
                }
            }
            Stmt::AugAssign(aug) if matches!(aug.target.as_ref(), Expr::Name(_)) => {
                if let Expr::Name(target) = aug.target.as_ref() {
                    self.all.insert(target.id.as_str());
                    self.plain.entry(target.id.as_str()).or_default().push(stmt);
                }
                self.visit_expr(&aug.value);
            }
            _ => ruff_python_ast::visitor::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Name(name) = expr
            && !matches!(name.ctx, ruff_python_ast::ExprContext::Load)
        {
            self.all.insert(name.id.as_str());
            self.other.insert(name.id.as_str());
        }
        ruff_python_ast::visitor::walk_expr(self, expr);
    }
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
        let mut stores = ModuleStores::default();
        for stmt in &module.body {
            ruff_python_ast::visitor::Visitor::visit_stmt(&mut stores, stmt);
        }
        // Candidates in source order: the first plain binding of each name.
        let mut candidates: Vec<(&str, TypeId, &Expr, Option<Literal>, TextRange)> = Vec::new();
        for stmt in &module.body {
            let Some((name, annotation, value)) = Self::module_binding(stmt) else {
                continue;
            };
            if defined.contains(name)
                || stores.other.contains(name)
                || candidates.iter().any(|(seen, ..)| *seen == name)
                || self.module_variables.contains_key(name)
                || (name.starts_with("__") && name.ends_with("__"))
                || is_module_all_assignment(stmt)
                || matches!(stmt, Stmt::Assign(assign) if Self::constructed_constant_shape(assign).is_some())
                || Self::lambda_with_call_metadata(value)
            {
                continue;
            }
            let ty = match annotation {
                Some(annotation_expr) => match self.annotation_to_hir(annotation_expr) {
                    Ok(ty) => ty,
                    Err(_) => continue,
                },
                None => match self.literal_value_type(value) {
                    Some(ty) => ty,
                    None => continue,
                },
            };
            // An unannotated name's type is its first value's; Python lets a
            // later binding change it (`z = 9`, then `z /= 3` is a `float`),
            // which a module-body local follows and a typed global cannot.
            if annotation.is_none()
                && stores.plain.get(name).is_some_and(|bindings| {
                    bindings
                        .iter()
                        .filter(|binding| binding.range() != stmt.range())
                        .any(|binding| !self.rebind_keeps_type(binding, ty))
                })
            {
                continue;
            }
            let literal = Self::scalar_literal(value).filter(|literal| self.literal_has_type(literal, ty));
            candidates.push((name, ty, value, literal, stmt.range()));
        }
        // A global's value lowers in its own initializer, which sees globals,
        // items and builtins but not the module body's locals. So a
        // candidate whose value reads a module-level name that stays a local
        // stays a local itself, until no candidate changes.
        loop {
            let globals = candidates.iter().map(|(name, ..)| *name).collect::<HashSet<_>>();
            let before = candidates.len();
            candidates.retain(|(_, _, value, literal, _)| {
                literal.is_some() || {
                    let mut read = NamesRead::default();
                    ruff_python_ast::visitor::Visitor::visit_expr(&mut read, value);
                    !read
                        .names
                        .iter()
                        .any(|name| stores.all.contains(name) && !globals.contains(name))
                }
            });
            if candidates.len() == before {
                break;
            }
        }
        for (name, ty, _, literal, range) in candidates {
            let init = literal.map_or(smelt_hir::MutableGlobalInit::Pending, smelt_hir::MutableGlobalInit::Literal);
            let span = self.span(range);
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
                Type::Dict(self.literal_items_type(keys)?, self.literal_items_type(values)?)
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

    /// Whether a later module-level binding of an unannotated name provably
    /// leaves it the type `ty` its first value gave it: a value of that
    /// literal type, or an augmented assignment that stays in it (`+=`, `-=`,
    /// `*=`, `//=`, `%=` by an `int` literal on an `int`; by a number literal
    /// on a `float`; `+=` of a string or `*=` by an `int` on a `str`). Any
    /// other rebinding may change the type.
    fn rebind_keeps_type(&mut self, binding: &Stmt, ty: TypeId) -> bool {
        let ty_kind = self.ctx.krate.types.get(ty).cloned();
        match binding {
            Stmt::Assign(assign) => self.literal_value_type(&assign.value) == Some(ty),
            Stmt::AugAssign(aug) => {
                let operand = match aug.value.as_ref() {
                    Expr::NumberLiteral(number) => match number.value {
                        Number::Int(_) => Some(Type::Int),
                        Number::Float(_) => Some(Type::Float),
                        Number::Complex { .. } => None,
                    },
                    Expr::StringLiteral(_) => Some(Type::PyStr),
                    _ => None,
                };
                matches!(
                    (ty_kind, aug.op, operand),
                    (
                        Some(Type::Int),
                        Operator::Add | Operator::Sub | Operator::Mult | Operator::FloorDiv | Operator::Mod,
                        Some(Type::Int),
                    ) | (
                        Some(Type::Float),
                        Operator::Add
                            | Operator::Sub
                            | Operator::Mult
                            | Operator::Div
                            | Operator::FloorDiv
                            | Operator::Mod,
                        Some(Type::Int | Type::Float),
                    ) | (Some(Type::PyStr), Operator::Add, Some(Type::PyStr))
                        | (Some(Type::PyStr), Operator::Mult, Some(Type::Int))
                )
            }
            // An annotated rebinding declares the type; a different one is a
            // checker error, not a rebinding.
            _ => true,
        }
    }

    /// Whether `value` is a lambda whose calls need what only its defining
    /// body has: default values (lowered there, once) or `*args`/`**kwargs`
    /// packing. Such a binding stays a module-body local, where its calls
    /// find that metadata; a plain lambda is an ordinary callable global.
    fn lambda_with_call_metadata(value: &Expr) -> bool {
        let Expr::Lambda(lambda) = value else {
            return false;
        };
        lambda.parameters.as_ref().is_some_and(|params| {
            params.vararg.is_some()
                || params.kwarg.is_some()
                || params
                    .posonlyargs
                    .iter()
                    .chain(&params.args)
                    .chain(&params.kwonlyargs)
                    .any(|param| param.default.is_some())
        })
    }

    /// Lower `NAME = value` (or `NAME: T = value`) to a module global, or
    /// `None` when `name` is not one written from here.
    ///
    /// The module body's first assignment of a global with a pending
    /// initializer becomes that initializer, touched here so it runs in source
    /// order; every other write stores with `GlobalSet`.
    #[expect(clippy::too_many_arguments, reason = "the binding, its value and where the write goes")]
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
        let stored = match self.expression_with_hint(value, body, Some(ty)) {
            Ok(stored) => stored,
            Err(error) => return Some(Err(error)),
        };
        let set = body.push_expr(HirExpr { kind: ExprKind::GlobalSet { item, value: stored }, ty, span });
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
            let current = body.push_expr(HirExpr { kind: ExprKind::GlobalGet { item }, ty, span });
            let rhs = self.expression(&aug.value, body)?;
            if let Some(update) = self.set_in_place_update(aug.op, current, rhs, span, body)? {
                body.push_stmt_to_block(block, HirStmt::Expr(update));
                return Ok(());
            }
            let op = Self::python_arithmetic_op(aug.op).ok_or_else(|| {
                SmeltError::unsupported(span, format!("augmented assignment operator '{:?}' is not supported", aug.op))
            })?;
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
