//! A class whose base is chosen at run time: `const Base = c ? X : Y; class D
//! extends Base { .. }`.
//!
//! Rust has no class whose parent is a value, so `D` is specialised once per
//! arm, as a hand port would write `DOverX` and `DOverY`: each is an ordinary
//! class with a static base, so `super`, inherited members and statics resolve
//! the way they do for any class. The condition is evaluated once, into a hidden
//! local, and every expression rooted at `D` (`new D()`, `D.x`, `D.m()`,
//! `x instanceof D`) is lowered once per arm and joined on that local, so its
//! type is the union of the arms' types and nothing is erased.
//!
//! Inside one arm the class registry aliases `Base` to that arm's class and `D`
//! to that arm's specialisation (`ClassRegistry::push_alias`), which is what
//! makes the ordinary lowering reach the right declarations.

use oxc::ast::ast::{BindingPattern, Class, Expression, VariableDeclarator};
use smelt_hir::{Body, Expr, ExprKind, LocalDecl, Pattern, Stmt, Type};

use crate::SmeltError;
use crate::lowering::ModuleBuilder;

/// A binding that holds one of two classes, chosen by a saved condition.
#[derive(Debug, Clone)]
pub(in crate::lowering) struct ConditionalBase {
    /// The hidden local holding the condition.
    cond: smelt_hir::LocalId,
    /// The class names of the arms, the `true` arm first.
    arms: [String; 2],
}

/// A class lowered once per arm of a [`ConditionalBase`].
#[derive(Debug, Clone)]
pub(in crate::lowering) struct ConditionalClass {
    /// The hidden local holding the condition.
    cond: smelt_hir::LocalId,
    /// The specialised class names, the `true` arm first.
    arms: [String; 2],
}

/// The identifier `expression` is, through parentheses.
fn identifier_name<'a>(expression: &'a Expression<'a>) -> Option<&'a str> {
    match expression {
        Expression::Identifier(identifier) => Some(identifier.name.as_str()),
        Expression::ParenthesizedExpression(inner) => identifier_name(&inner.expression),
        _ => None,
    }
}

impl ModuleBuilder<'_> {
    /// Lower `const Base = c ? X : Y` over two classes: the condition into a
    /// hidden local, and `Base` as the class that local selects. Returns false
    /// for any other declarator.
    pub(in crate::lowering) fn conditional_base_declarator(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            return Ok(false);
        };
        let Some(Expression::ConditionalExpression(conditional)) = &declarator.init else {
            return Ok(false);
        };
        let (Some(when_true), Some(when_false)) =
            (identifier_name(&conditional.consequent), identifier_name(&conditional.alternate))
        else {
            return Ok(false);
        };
        if !self.classes.contains(when_true) || !self.classes.contains(when_false) {
            return Ok(false);
        }
        let name = binding.name.as_str();
        let span = self.span(declarator.span.start, declarator.span.end);
        let cond_value = self.expression(&conditional.test, body)?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let symbol = self.intern_source_name(&format!("smelt_{name}_arm"));
        let cond = body.push_local(LocalDecl { name: Some(symbol), ty: bool_ty, mutable: false, span });
        let pat = body.push_pattern(Pattern::Binding(cond));
        body.push_stmt_to_block(block, Stmt::Let { pat, ty: bool_ty, value: Some(cond_value) });
        let selected = self.select_on(cond, span, body, |this, arm, body| {
            let class = [when_true, when_false][arm];
            this.identifier_expression(class, span.start, span.end, body)
        })?;
        self.binding_declaration(&declarator.id, Some(selected), None, false, body, block)?;
        self.conditional_bases.insert(
            name.to_owned(),
            ConditionalBase { cond, arms: [when_true.to_owned(), when_false.to_owned()] },
        );
        Ok(true)
    }

    /// Lower `class D extends Base` over a [`ConditionalBase`] once per arm.
    /// Returns false for any other class.
    pub(in crate::lowering) fn conditional_class_declaration(
        &mut self,
        class: &Class<'_>,
    ) -> Result<bool, SmeltError> {
        let (Some(id), Some(heritage)) = (&class.id, &class.heritage) else {
            return Ok(false);
        };
        let heritage = &heritage.expression;
        let Some(base) = identifier_name(heritage).and_then(|name| self.conditional_bases.get(name)).cloned()
        else {
            return Ok(false);
        };
        let Some(base_name) = identifier_name(heritage).map(str::to_owned) else {
            return Ok(false);
        };
        let name = id.name.to_string();
        let specialised = base.arms.clone().map(|arm| format!("{name}Over{arm}"));
        for (arm, specialised_name) in base.arms.iter().zip(&specialised) {
            self.classes.push_alias(base_name.clone(), arm.clone());
            self.classes.push_alias(name.clone(), specialised_name.clone());
            self.forced_class_name = Some(specialised_name.clone());
            let lowered = self.class_declaration(class);
            self.forced_class_name = None;
            self.classes.pop_alias();
            self.classes.pop_alias();
            lowered?;
        }
        self.conditional_classes.insert(name, ConditionalClass { cond: base.cond, arms: specialised });
        Ok(true)
    }

    /// Lower an expression rooted at `root` once per arm when `root` is a
    /// [`ConditionalClass`] not already standing for one of its arms, joined on
    /// the saved condition. `lower` is the very lowering that met the root, run
    /// again with `root` standing for each arm in turn. `None` for any other root.
    pub(in crate::lowering) fn conditional_class_select(
        &mut self,
        root: Option<&Expression<'_>>,
        span: smelt_hir::Span,
        body: &mut Body,
        mut lower: impl FnMut(&mut Self, &mut Body) -> Result<smelt_hir::ExprId, SmeltError>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(root) = root.and_then(identifier_name) else {
            return Ok(None);
        };
        if self.classes.is_aliased(root) {
            return Ok(None);
        }
        let Some(class) = self.conditional_classes.get(root).cloned() else {
            return Ok(None);
        };
        let root = root.to_owned();
        self.select_on(class.cond, span, body, |this, arm, body| {
            this.classes.push_alias(root.clone(), class.arms[arm].clone());
            let lowered = lower(this, body);
            this.classes.pop_alias();
            lowered
        })
        .map(Some)
    }

    /// `cond ? arm(0) : arm(1)`, typed as the union of what the arms answer.
    fn select_on(
        &mut self,
        cond: smelt_hir::LocalId,
        span: smelt_hir::Span,
        body: &mut Body,
        mut arm: impl FnMut(&mut Self, usize, &mut Body) -> Result<smelt_hir::ExprId, SmeltError>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let then_expr = arm(self, 0, body)?;
        let else_expr = arm(self, 1, body)?;
        let then_ty = Self::expr_ty(body, then_expr);
        let else_ty = Self::expr_ty(body, else_expr);
        let ty = if then_ty == else_ty {
            then_ty
        } else {
            self.ctx.krate.types.intern(Type::Union(vec![then_ty, else_ty]))
        };
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let cond = body.push_expr(Expr { kind: ExprKind::Local(cond), ty: bool_ty, span });
        Ok(body.push_expr(Expr { kind: ExprKind::Conditional { cond, then_expr, else_expr }, ty, span }))
    }
}
