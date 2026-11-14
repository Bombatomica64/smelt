//! Generic functions whose declared type-parameter return is an unchecked
//! promise.
//!
//! `function f<T>(obj: T, g: (v: any) => any): T { return g(obj) }` returns
//! whatever `g` returns, typed `any`, as its `T`: TypeScript accepts it and
//! never checks it. A call `f(record, customizer)` instantiates `T` to the
//! record's type, but the value can be anything the customizer made (es-toolkit
//! `cloneDeepWith`, whose customizer may replace a number field with `null`).
//! Converting the definition's erased result to the instantiation at the call
//! would lose exactly what the program relies on, so such a call keeps the
//! erased result.
//!
//! A function is recorded when one of its `return`s yields the erased result
//! of calling one of its own callback PARAMETERS (through casts, narrowing and
//! the `let` that holds it), or the result of a call to a recorded function,
//! while its declared return mentions its own type parameters. Only a value
//! the CALLER supplies can break the promise: a helper that erases internally
//! (`cloneDeep`, whose clone keeps its input's shape) keeps its instantiation. The set is closed over the module's functions once the module
//! is lowered, so a wrapper declared before the helper it returns from is
//! recorded too; call sites in importing modules consult it.

use oxc::ast::ast::{Declaration, Statement};
use smelt_hir::{BodyId, ExprId, ExprKind, Item, ItemId, LocalId, Pattern, Stmt, Type};

use super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// The body of `item` when it is a generic function whose declared return
    /// mentions its own type parameters, the only kind that can be recorded.
    fn generic_return_body(&self, item: ItemId) -> Option<BodyId> {
        let Some(Item::Function(function)) =
            self.ctx.krate.items.get(usize::try_from(item.0).ok()?)
        else {
            return None;
        };
        let body = function.body?;
        (!function.type_params.is_empty()
            && self.mentions_type_params(function.return_ty, &function.type_params))
        .then_some(body)
    }

    /// Examine the top-level function `statement` declares, now that its body
    /// is lowered; see the module docs. `module_items` are the module's items.
    ///
    /// Called after each top-level statement, so a call later in the same
    /// module sees a function declared above it. Each function is examined
    /// once; only when one is recorded are the module's other functions
    /// examined again (a wrapper above the helper it forwards to), which keeps
    /// a module of `n` functions linear.
    pub(in crate::lowering) fn note_lowered_function(
        &mut self,
        statement: &Statement<'_>,
        module_items: &[ItemId],
    ) {
        let function: &oxc::ast::ast::Function<'_> = match statement {
            Statement::FunctionDeclaration(function) => function,
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::FunctionDeclaration(function) => function,
                _ => return,
            },
            _ => return,
        };
        let Some(item) = function
            .id
            .as_ref()
            .and_then(|id| self.items.get(id.name.as_str()).copied())
        else {
            return;
        };
        let recorded = self
            .generic_return_body(item)
            .is_some_and(|body| self.body_returns_unchecked(body))
            && self.ctx.unchecked_generic_returns.insert(item);
        if recorded {
            self.close_unchecked_generic_returns(module_items);
        }
    }

    /// Record every function of `items` that returns through a recorded one,
    /// until nothing changes.
    pub(in crate::lowering) fn close_unchecked_generic_returns(&mut self, items: &[ItemId]) {
        loop {
            let found: Vec<ItemId> = items
                .iter()
                .copied()
                .filter(|item| !self.ctx.unchecked_generic_returns.contains(item))
                .filter(|&item| {
                    self.generic_return_body(item)
                        .is_some_and(|body| self.body_returns_unchecked(body))
                })
                .collect();
            if found.is_empty() {
                break;
            }
            self.ctx.unchecked_generic_returns.extend(found);
        }
    }

    /// Whether some `return` of `body` yields an unchecked value.
    fn body_returns_unchecked(&self, body: BodyId) -> bool {
        let Some(body) = self
            .ctx
            .krate
            .bodies
            .get(usize::try_from(body.0).unwrap_or(usize::MAX))
        else {
            return false;
        };
        body.stmts.iter().any(|stmt| match stmt {
            Stmt::Return(Some(value)) => self.yields_unchecked(body, *value, 0),
            _ => false,
        })
    }

    /// The initializer of `local` in `body`, when a `let` binds it directly.
    fn local_initializer(body: &smelt_hir::Body, local: LocalId) -> Option<ExprId> {
        body.stmts.iter().find_map(|stmt| match stmt {
            Stmt::Let {
                pat,
                value: Some(value),
                ..
            } => matches!(
                body.patterns.get(usize::try_from(pat.0).ok()?),
                Some(Pattern::Binding(bound)) if *bound == local
            )
            .then_some(*value),
            _ => None,
        })
    }

    /// The expression `expr` of `body`.
    fn expr_of(body: &smelt_hir::Body, expr: ExprId) -> Option<&smelt_hir::Expr> {
        body.exprs.get(usize::try_from(expr.0).ok()?)
    }

    /// Whether `expr` yields the erased result of calling one of the body's own
    /// parameters, or of a call to a recorded function; see the module docs.
    fn yields_unchecked(&self, body: &smelt_hir::Body, expr: ExprId, depth: usize) -> bool {
        // Bounds a chain of `let` initializers; valid source has no cycle.
        if depth > 32 {
            return false;
        }
        let Some(node) = Self::expr_of(body, expr) else {
            return false;
        };
        match &node.kind {
            ExprKind::UnknownCast { value, .. } | ExprKind::TypeAssert { value } => {
                self.yields_unchecked(body, *value, depth + 1)
            }
            ExprKind::Local(local) => Self::local_initializer(body, *local)
                .is_some_and(|value| self.yields_unchecked(body, value, depth + 1)),
            ExprKind::ClosureCall { callee, .. } => {
                let erased = match self.ctx.krate.types.get(node.ty) {
                    Some(Type::Unknown) => true,
                    Some(Type::Optional(inner)) => {
                        self.ctx.krate.types.get(*inner) == Some(&Type::Unknown)
                    }
                    _ => false,
                };
                erased && Self::is_parameter(body, *callee, 0)
            }
            // A recorded function passes its unchecked result on only when this
            // body hands it a callback of its OWN caller: `cloneDeep(obj)`
            // calls the same helper as `cloneDeepWith(obj, customizer)` but
            // passes no customizer, so its clone keeps its shape.
            ExprKind::Call { callee, args } => {
                let recorded = Self::expr_of(body, *callee).is_some_and(|callee| {
                    matches!(callee.kind, ExprKind::Item(item) if self.ctx.unchecked_generic_returns.contains(&item))
                });
                recorded
                    && args
                        .iter()
                        .any(|arg| self.forwards_callback_parameter(body, *arg))
            }
            _ => false,
        }
    }

    /// Whether `arg` forwards one of `body`'s own function-typed parameters.
    fn forwards_callback_parameter(&self, body: &smelt_hir::Body, arg: ExprId) -> bool {
        let Some(node) = Self::expr_of(body, arg) else {
            return false;
        };
        let callable = match self.ctx.krate.types.get(node.ty) {
            Some(Type::Function(_)) => true,
            Some(Type::Optional(inner)) => {
                matches!(self.ctx.krate.types.get(*inner), Some(Type::Function(_)))
            }
            _ => false,
        };
        callable && Self::is_parameter(body, arg, 0)
    }

    /// Whether `expr` reads one of `body`'s parameters, through casts.
    fn is_parameter(body: &smelt_hir::Body, expr: ExprId, depth: usize) -> bool {
        if depth > 8 {
            return false;
        }
        match Self::expr_of(body, expr).map(|node| &node.kind) {
            Some(ExprKind::Local(local)) => body.params.contains(local),
            Some(ExprKind::UnknownCast { value, .. } | ExprKind::TypeAssert { value }) => {
                Self::is_parameter(body, *value, depth + 1)
            }
            _ => false,
        }
    }
}
