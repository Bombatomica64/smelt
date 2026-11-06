//! Flow narrowing from `instanceof`: which union arms a class test keeps on each
//! branch of the guard.

use crate::lowering::ModuleBuilder;
use oxc::ast::ast::Expression;
use oxc::syntax::operator::BinaryOperator;
use smelt_hir::{Body, Type};

impl ModuleBuilder<'_> {
    /// Recognize `value instanceof Class` and retain matching concrete class arms.
    pub(in crate::lowering) fn instanceof_local_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if binary.operator != BinaryOperator::Instanceof {
            return None;
        }
        let (Expression::Identifier(identifier), Expression::Identifier(class)) =
            (&binary.left, &binary.right)
        else {
            return None;
        };
        let local_name = identifier.name.as_str();
        let local = self.scope.lookup(local_name)?;
        let ty = self
            .narrowed_type(local_name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let class_name = class.name.as_str();
        // `instanceof` PROVES the value is present: `null instanceof File` and
        // `undefined instanceof File` are both `false` in JavaScript, so no
        // absent value can reach the true branch. That is what lets an OPTIONAL
        // union narrow to the arm itself rather than to an optional of it —
        // `form.get(name) instanceof File` gives `File`, not
        // `File | undefined`.
        //
        // The unwrap is HERE and not in `filtered_union_members`, which several
        // guards share, because only this guard proves presence.
        // `typeof x !== "string"` does not: `typeof undefined` is
        // `"undefined"`, which is also not `"string"`, so that guard's filter
        // has to keep the optional it was given.
        let ty = match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => ty,
        };
        if let Some(retained) = self.filtered_union_members(ty, |member| {
            matches!(
                member,
                Type::Class {
                    name: class_symbol,
                    ..
                } if self.ctx.krate.symbols.get(*class_symbol) == Some(class_name)
            )
        }) {
            let narrowed = self.intern_filtered_union(retained)?;
            return Some((local_name.to_owned(), narrowed));
        }
        // The local is not a union, so there is nothing to filter — which used
        // to end the narrowing here and leave an ERASED local erased across the
        // guard. `if (x instanceof Headers)` then read `x.get(..)` through the
        // erased member path, which has no `get` on the header record and
        // answered `undefined`: a silently wrong value where the equivalent
        // user type predicate (`x is Headers`) narrowed correctly and emitted
        // the right checked cast (`blocker-logs/standards-instanceof-narrowing.md`).
        //
        // An erased local narrows to the target class whenever that class can be
        // recovered from an erased value — `StdlibClass::narrows_from_erased`,
        // which is true exactly for the classes whose runtime type declares a
        // `SmeltFromUnknown` adapter next to its host marker. Asking the
        // registry that question is what keeps this a general rule: the
        // narrowing is emitted precisely where it can be materialized, and a
        // class with no adapter still keeps its erased type rather than being
        // given an invented conversion.
        //
        // Only an ERASED local narrows this way. A local already typed as some
        // concrete thing is not made into something else by an `instanceof`
        // test: that is either a tautology or dead code, and rewriting its type
        // would discard what the source said it was.
        if !matches!(self.ctx.krate.types.get(ty), Some(Type::Unknown)) {
            return None;
        }
        let target = Self::stdlib_class_for_name(class_name)?;
        if !target.narrows_from_erased() || self.source_shadows_global(class_name) {
            return None;
        }
        let narrowed = self.stdlib_class_type(class_name);
        Some((local_name.to_owned(), narrowed))
    }

    /// Filter a union through a structural flow fact.
    pub(in crate::lowering) fn filtered_union_members(
        &self,
        ty: smelt_hir::TypeId,
        mut keep: impl FnMut(&Type) -> bool,
    ) -> Option<Vec<smelt_hir::TypeId>> {
        let Type::Union(items) = self.ctx.krate.types.get(ty)?.clone() else {
            return None;
        };
        let retained = items
            .into_iter()
            .filter(|item| self.ctx.krate.types.get(*item).is_some_and(&mut keep))
            .collect::<Vec<_>>();
        (!retained.is_empty()).then_some(retained)
    }

    /// Recognize the FALSE side of `local instanceof Class` on a union local.
    ///
    /// `tsc` removes the class's arm: after `if (input instanceof Request)
    /// return ..`, `input` is `string | URL`. Only the union's members are
    /// filtered, and an absent value stays possible (`undefined instanceof C`
    /// is `false`), so an optional union keeps its `Optional`. A local that is
    /// not a union, a union that would lose every arm, and a remainder with an
    /// erased arm are left alone.
    pub(in crate::lowering) fn instanceof_inverse_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if binary.operator != BinaryOperator::Instanceof {
            return None;
        }
        let (Expression::Identifier(identifier), Expression::Identifier(class)) =
            (&binary.left, &binary.right)
        else {
            return None;
        };
        let local_name = identifier.name.as_str();
        let local = self.scope.lookup(local_name)?;
        let ty = self
            .narrowed_type(local_name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let (union_ty, optional) = match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => (*inner, true),
            _ => (ty, false),
        };
        let class_name = class.name.as_str();
        let Some(Type::Union(arms)) = self.ctx.krate.types.get(union_ty) else {
            return None;
        };
        let arm_count = arms.len();
        let retained = self.filtered_union_members(union_ty, |member| {
            !matches!(
                member,
                Type::Class {
                    name: class_symbol,
                    ..
                } if self.ctx.krate.symbols.get(*class_symbol) == Some(class_name)
            )
        })?;
        // Narrowing to an erased remainder proves nothing a reader can use and
        // only copies the value into an erased local, so it declines.
        if retained.len() == arm_count
            || retained
                .iter()
                .any(|arm| matches!(self.ctx.krate.types.get(*arm), Some(Type::Unknown)))
        {
            return None;
        }
        let narrowed = self.intern_filtered_union(retained)?;
        let narrowed = if optional {
            self.ctx.krate.types.intern(Type::Optional(narrowed))
        } else {
            narrowed
        };
        Some((local_name.to_owned(), narrowed))
    }

    /// Intern the canonical type produced by a non-empty union filter.
    pub(in crate::lowering) fn intern_filtered_union(
        &mut self,
        retained: Vec<smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
        match retained.as_slice() {
            [] => None,
            [single] => Some(*single),
            _ => Some(self.ctx.krate.types.intern(Type::Union(retained))),
        }
    }
}
