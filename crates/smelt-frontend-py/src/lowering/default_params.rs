// Default parameter values, applied by the callee.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// Python evaluates a default once, when the `def` runs, and every call that
// leaves the argument out gets that value: a mutable default is shared by all
// such calls. A caller in TypeScript knows nothing of Python's defaults, so the
// default cannot live at the call site: as the TypeScript frontend does for its
// own defaults, the parameter is `Optional[T]`, a caller that leaves it out
// passes `None`, and the body begins by putting the default in its place. A
// literal default is that literal; any other is a module global holding the
// value, initialized once, so its identity is the one Python shares.

/// A parameter with a default, as its function lowers it.
#[derive(Debug, Clone, Copy)]
struct DefaultedParam {
    /// The declared type, which the body sees.
    declared_ty: TypeId,
    /// `Optional[declared]`, which callers pass (`None` when they leave it out).
    optional_ty: TypeId,
    /// The global holding a non-literal default's one value.
    shared: Option<ItemId>,
}

impl ModuleBuilder<'_> {
    /// How the parameter `name: declared = default` lowers, or `None` when it
    /// needs nothing (a `None` default of a type that already holds `None`).
    ///
    /// # Errors
    ///
    /// A type that holds `None` with a default other than `None`: a caller
    /// passing `None` and a caller leaving the argument out would look alike.
    fn defaulted_param(
        &mut self,
        name: &str,
        declared: TypeId,
        default: &Expr,
        span: Span,
    ) -> Result<Option<DefaultedParam>, SmeltError> {
        let admits_none = self.type_admits_none(declared);
        if admits_none {
            if matches!(default, Expr::NoneLiteral(_)) {
                return Ok(None);
            }
            return Err(SmeltError::unsupported(
                self.span(default.range()),
                format!(
                    "parameter '{name}' admits None and defaults to something else; Smelt cannot tell an omitted argument from None there"
                ),
            ));
        }
        let optional_ty = self.intern_type(Type::Optional(declared));
        let shared = if Self::default_is_literal(default) {
            None
        } else {
            Some(self.shared_default(name, declared, default, span)?)
        };
        Ok(Some(DefaultedParam { declared_ty: declared, optional_ty, shared }))
    }

    /// Whether `ty` holds `None` (an optional, `None` itself, a union with it).
    fn type_admits_none(&self, ty: TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(_) | Type::None | Type::Unknown) => true,
            Some(Type::Union(members)) => {
                members.iter().any(|member| matches!(self.ctx.krate.types.get(*member), Some(Type::None)))
            }
            _ => false,
        }
    }

    /// Whether a default is an immutable literal, which every call may
    /// evaluate afresh without anyone telling.
    fn default_is_literal(default: &Expr) -> bool {
        match default {
            Expr::NumberLiteral(_) | Expr::StringLiteral(_) | Expr::BooleanLiteral(_) | Expr::NoneLiteral(_) => true,
            Expr::UnaryOp(unary) => Self::default_is_literal(&unary.operand),
            Expr::Tuple(tuple) => tuple.elts.iter().all(Self::default_is_literal),
            _ => false,
        }
    }

    /// The module global holding a non-literal default's one value.
    fn shared_default(&mut self, name: &str, ty: TypeId, default: &Expr, span: Span) -> Result<ItemId, SmeltError> {
        let mut init_body = Body::new(None, span);
        let value = self.with_fn_scope(FnKind::Sync, |this| this.expression_with_hint(default, &mut init_body, Some(ty)))?;
        init_body.push_stmt(HirStmt::Return(Some(value)));
        let body_id = self.ctx.krate.push_body(init_body);
        let global_name = self.intern_synthesized_name(&format!("smelt_default${name}${}", self.ctx.krate.items.len()));
        let item = self.ctx.krate.push_item(Item::MutableGlobal(smelt_hir::MutableGlobalItem {
            name: global_name,
            ty,
            init: smelt_hir::MutableGlobalInit::Pending,
            visibility: Visibility::Private,
            span,
        }));
        let init_name = self.intern_synthesized_name(&format!("smelt_global_init$default${name}${}", item.0));
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
        self.pending_module_items.push(item);
        Ok(item)
    }

    /// Rebind `name` at the top of the body to the argument, or the default
    /// when the caller left it out.
    #[expect(clippy::too_many_arguments, reason = "the parameter, its default and where the prologue goes")]
    fn default_prologue(
        &mut self,
        name: &str,
        param: smelt_hir::LocalId,
        defaulted: &DefaultedParam,
        default: &Expr,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let span = self.span(default.range());
        let optional = body.push_expr(HirExpr { kind: ExprKind::Local(param), ty: defaulted.optional_ty, span });
        let fallback = match defaulted.shared {
            Some(item) => body.push_expr(HirExpr { kind: ExprKind::GlobalGet { item }, ty: defaulted.declared_ty, span }),
            None => self.expression_with_hint(default, body, Some(defaulted.declared_ty))?,
        };
        let value = body.push_expr(HirExpr {
            kind: ExprKind::OptionalCoalesce { optional, fallback },
            ty: defaulted.declared_ty,
            span,
        });
        let name_sym = self.intern_name(name);
        let local = body.push_local(LocalDecl { name: Some(name_sym), ty: defaulted.declared_ty, mutable: true, span });
        let pat = body.push_pattern(HirPattern::Binding(local));
        body.push_stmt(HirStmt::Let { pat, ty: defaulted.declared_ty, value: Some(value) });
        self.scope.locals.insert(name.to_owned(), local);
        Ok(())
    }
}
