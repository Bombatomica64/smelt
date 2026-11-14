//! `super.method(args)`: the base class's own method, run on `this`.
//!
//! A read of `this.method` would find the override the subclass declares, so
//! the call names the base declaration's item directly and passes `this` as its
//! receiver. Codegen renders such a call through an inherited copy of the base
//! method on the subclass, since the subclass's own method of that name is the
//! override.

use oxc::ast::ast::{CallExpression, Expression};
use smelt_hir::{Body, Expr, ExprKind, Item, Type};

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::support::source_spelling;

impl ModuleBuilder<'_> {
    /// Lower `super.method(args)` as a call of the nearest ancestor's `method`
    /// item with `this` as its receiver; `None` for any other call.
    pub(in crate::lowering) fn super_method_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Super(super_expr) = &member.object else {
            return Ok(None);
        };
        let this =
            self.identifier_expression("this", super_expr.span.start, super_expr.span.end, body)?;
        let Some(Type::Class { name: own, .. }) =
            self.ctx.krate.types.get(Self::expr_ty(body, this)).cloned()
        else {
            return Ok(None);
        };
        let Some(item) = self.ancestor_method_item(own, member.property.name.as_str()) else {
            return Ok(None);
        };
        let Item::Function(function) = self.item_ref(item) else {
            return Ok(None);
        };
        let return_ty = function.return_ty;
        let span = self.span(call.span.start, call.span.end);
        let callee_ty = self.item_expr_type(item, span)?;
        let callee = body.push_expr(Expr {
            kind: ExprKind::Item(item),
            ty: callee_ty,
            span,
        });
        let mut args = vec![this];
        for argument in &call.arguments {
            args.push(self.argument(argument, body)?);
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Call { callee, args },
            ty: return_ty,
            span,
        })))
    }

    /// The item of the method `name` declared by the nearest ancestor of the
    /// class `own`.
    fn ancestor_method_item(
        &self,
        own: smelt_hir::Symbol,
        name: &str,
    ) -> Option<smelt_hir::ItemId> {
        let mut class = self
            .classes
            .base_of_symbol(own)
            .map(|base| base.0)
            .or_else(|| self.class_by_symbol(own).and_then(|class| class.base))?;
        // A chain is finite, but a malformed one must not loop.
        for _ in 0..64_u32 {
            let declared = self.class_by_symbol(class)?;
            let found = declared.methods.iter().copied().find(|method| {
                matches!(
                    self.item_ref(*method),
                    Item::Function(function)
                        if source_spelling(&self.ctx.krate, function.name) == Some(name)
                )
            });
            if found.is_some() {
                return found;
            }
            class = declared.base?;
        }
        None
    }
}
