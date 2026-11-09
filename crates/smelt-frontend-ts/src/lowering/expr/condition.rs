//! Conditions: an expression in test position (`if`, `while`, `?:`, `!`)
//! lowered to a `bool` by JavaScript truthiness, including keyed-read presence
//! tests and types whose truthiness is constant.

use crate::SmeltError;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::peel::{Wrappers, peel};
use oxc::ast::ast::Expression;
use smelt_hir::{BinOp, Body, Expr, ExprKind, Literal, PrimitiveCastOp, Span, Type};
use super::super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Lower a JavaScript condition to a boolean expression.
    ///
    /// TypeScript permits optional values in truthiness positions. Smelt models
    /// the common `value ? a : b` and `if (value)` optional-object/string cases
    /// as a `value != None` check once the expression has lowered to
    /// `Optional<T>`.
    pub(in crate::lowering) fn condition_expression(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // `&&`/`||` yield an OPERAND, not a boolean, so in a value position they
        // lower to a union-typed selection (see
        // `logical_operand_value_expression`). A condition only observes that
        // operand's truthiness, which distributes over the operator, so lower
        // the condition form directly to a boolean instead of building a union
        // value just to test it.
        if let Expression::LogicalExpression(logical) = peel(expression, Wrappers::Parens)
            && let Some(cond) = self.logical_condition_expression(logical, body)?
        {
            return Ok(cond);
        }
        let cond = self.expression(expression, body)?;
        self.lowered_condition_expression(cond, self.expression_span(expression), body)
    }

    /// Coerce an already lowered JavaScript value into its boolean truthiness result.
    ///
    /// Assignment operators such as `||=` already lower their target as a
    /// writable place. Reusing the resulting expression here avoids lowering a
    /// computed receiver solely to form the condition that selects its value.
    pub(in crate::lowering) fn lowered_condition_expression(
        &mut self,
        cond: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let cond_ty = Self::expr_ty(body, cond);
        if self.ctx.krate.types.get(cond_ty) == Some(&Type::Bool) {
            return Ok(cond);
        }
        // A function or class value is always truthy in JavaScript, and so is a
        // type parameter whose constraint pins it to an object surface. An
        // unconstrained `T`, however, can hold `0`, `-0`, `NaN`, `""`, `false`,
        // `null` or `undefined`, so it must be tested, not folded: it falls
        // through to the `type_is_truthy_condition_surface` cast below.
        if matches!(
            self.ctx.krate.types.get(cond_ty),
            Some(Type::Function(_) | Type::Class { .. })
        ) || (matches!(self.ctx.krate.types.get(cond_ty), Some(Type::TypeParam { .. }))
            && self.type_is_always_truthy_object_surface(cond_ty))
        {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty,
                span,
            }));
        }
        if self.ctx.krate.types.get(cond_ty) == Some(&Type::String) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            let empty = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: string_ty,
                span,
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: cond,
                    rhs: empty,
                },
                ty: bool_ty,
                span,
            }));
        }
        if matches!(self.ctx.krate.types.get(cond_ty), Some(Type::Int | Type::Float | Type::BigInt)) {
            let zero = body.push_expr(Expr {
                kind: match self.ctx.krate.types.get(cond_ty) {
                    Some(Type::Int) => ExprKind::Literal(Literal::Int(0)),
                    // `0n` is the one falsy bigint.
                    Some(Type::BigInt) => ExprKind::Literal(Literal::BigInt("0".to_owned())),
                    _ => ExprKind::Literal(Literal::Float(0.0)),
                },
                ty: cond_ty,
                span,
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: cond,
                    rhs: zero,
                },
                ty: bool_ty,
                span,
            }));
        }
        if let Some(condition) = self.optional_known_date_presence_condition(cond, span, body) {
            return Ok(condition);
        }
        if let Some(condition) = self.keyed_read_presence_condition(cond, span, body) {
            return Ok(condition);
        }
        // A type that cannot hold a nullish value AND whose every inhabitant is
        // an object is truthy for every value it can take, so the guard is the
        // constant `true` and needs no runtime test at all. Comparing against
        // `none` instead only worked because the emitter folds `tuple != none`
        // to `false`; for a union it emits a real presence check, which a
        // generated union enum cannot answer (`matches!(v, SmeltUnknown::Null)`
        // over a `SmeltUnion3`). Saying `true` is both the precise answer and
        // the one no representation has to support.
        //
        // A UNION of object arms is the exception: the type says every value is
        // truthy, but a union whose arms have no concrete Rust spelling is
        // stored as an erased `SmeltUnknown`, and an erased value can be
        // `undefined` at run time even where TypeScript's type cannot — a
        // `Record<string, Result<T>>` read of a missing key is exactly that
        // (Hono's `const staticMatch = matcher[2][path]; if (staticMatch)`,
        // which folded to `if true` and returned `undefined` for every dynamic
        // route). Which representation the union gets is the emitter's
        // decision, so the union keeps a `ToBool` and the emitter folds it to
        // `true` only for a concrete generated union enum, which cannot hold an
        // absent value .
        if !self.is_nullishable_type(cond_ty) && self.type_is_constantly_truthy(cond_ty) {
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            let kind = if matches!(self.ctx.krate.types.get(cond_ty), Some(Type::Union(_))) {
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToBool,
                    operand: cond,
                }
            } else {
                ExprKind::Literal(Literal::Bool(true))
            };
            return Ok(body.push_expr(Expr {
                kind,
                ty: bool_ty,
                span,
            }));
        }
        if self
            .non_nullish_type(cond_ty)
            .is_some_and(|inner_ty| self.type_is_always_truthy_object_surface(inner_ty))
        {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            let none = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: none_ty,
                span,
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: cond,
                    rhs: none,
                },
                ty: bool_ty,
                span,
            }));
        }
        if self.is_nullishable_type(cond_ty) || self.type_is_truthy_condition_surface(cond_ty) {
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToBool,
                    operand: cond,
                },
                ty: bool_ty,
                span,
            }));
        }
        Err(SmeltError::unsupported(
            span,
            format!(
                "condition expression must be boolean or optional (got {:?})",
                self.ctx.krate.types.get(cond_ty)
            ),
        ))
    }

    /// Lower the truthiness of a keyed record read as a presence test.
    ///
    /// Without `noUncheckedIndexedAccess`, TypeScript types `record[key]` as
    /// the record's value type even though a missing key reads `undefined`,
    /// and the source's own guard is how a program asks which it got:
    /// `if (!routes[m][path]) { routes[m][path] = [] }` (Hono's reg-exp router
    /// `add`). The Rust read defaults a missing entry to the value type's
    /// default, and when that type is an object surface (list, record, class,
    /// tuple, ...) the constant-truthiness fold below answers `true` — the
    /// guard never fired and no route was ever inserted.
    ///
    /// Every value of such a type is truthy, so the read is truthy exactly
    /// when the key is present, and the honest condition is the containment
    /// check itself. Primitive value types need nothing: their Rust defaults
    /// (`0`, `""`, `false`) are already falsy, like the `undefined` they stand
    /// in for.
    pub(in crate::lowering) fn keyed_read_presence_condition(
        &mut self,
        cond: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let cond_ty = Self::expr_ty(body, cond);
        if self.is_nullishable_type(cond_ty) || !self.type_is_constantly_truthy(cond_ty) {
            return None;
        }
        let mut read = cond;
        while let Some(ExprKind::TypeAssert { value }) = body.expr_at(read)
            .map(|expr| expr.kind.clone())
        {
            read = value;
        }
        let Some(ExprKind::Index { receiver, index }) = body.expr_at(read)
            .map(|expr| expr.kind.clone())
        else {
            return None;
        };
        let Some(Type::Dict(key_ty, _)) = self.ctx.krate.types.get(Self::expr_ty(body, receiver))
        else {
            return None;
        };
        if *key_ty != Self::expr_ty(body, index) {
            return None;
        }
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::DictContainsKey {
                dict: receiver,
                key: index,
                lookup: smelt_hir::PropertyLookup::Own,
            },
            ty: bool_ty,
            span,
        }))
    }

    /// `record.has(key)` for a keyed record read `record[key]` whose value type
    /// is not itself nullable, or `None` for any other expression.
    ///
    /// The read then answers `undefined` exactly when the key is absent, so
    /// this containment test is how a presence assertion on it is asked (see
    /// `expect_to_be_none_statement`). `TypeAssert` wrappers are looked
    /// through, as in [`Self::keyed_read_presence_condition`].
    pub(in crate::lowering) fn keyed_read_contains_key(
        &mut self,
        read: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if self.is_nullishable_type(Self::expr_ty(body, read)) {
            return None;
        }
        let mut inner = read;
        while let Some(ExprKind::TypeAssert { value }) = body.expr_at(inner)
            .map(|expr| expr.kind.clone())
        {
            inner = value;
        }
        let Some(ExprKind::Index { receiver, index }) = body.expr_at(inner)
            .map(|expr| expr.kind.clone())
        else {
            return None;
        };
        let Some(Type::Dict(key_ty, value_ty)) =
            self.ctx.krate.types.get(Self::expr_ty(body, receiver)).cloned()
        else {
            return None;
        };
        if key_ty != Self::expr_ty(body, index)
            || self.is_nullishable_type(value_ty)
            || matches!(self.ctx.krate.types.get(value_ty), Some(Type::Unknown))
        {
            return None;
        }
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::DictContainsKey {
                dict: receiver,
                key: index,
                lookup: smelt_hir::PropertyLookup::Own,
            },
            ty: bool_ty,
            span,
        }))
    }

    /// Keep a keyed record read's absence when it flows into an optional slot.
    ///
    /// `record[key]` is typed as the record's value type (TypeScript without
    /// `noUncheckedIndexedAccess`), and read at that type a missing key yields
    /// the value type's Rust default. Written into a slot typed
    /// `V | undefined` — an annotated optional local, a local widened by
    /// `presence_tested_locals`, an optional field or parameter — that default
    /// was then wrapped as PRESENT (`Some(default)`), so the slot could never
    /// hold the `undefined` a missing key reads in JavaScript. Retyping the
    /// read to `Optional<V>` makes it the lookup it is (`get(key)`), and the
    /// slot receives exactly what JavaScript stores.
    pub(in crate::lowering) fn keyed_read_into_optional_slot(
        &self,
        read: smelt_hir::ExprId,
        type_hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let Some(hint) = type_hint else {
            return read;
        };
        let Some(Type::Optional(slot_value)) = self.ctx.krate.types.get(hint).cloned() else {
            return read;
        };
        let read_ty = Self::expr_ty(body, read);
        if read_ty != slot_value || self.is_nullishable_type(read_ty) {
            return read;
        }
        let Some(ExprKind::Index { receiver, .. }) = body.expr_at(read)
            .map(|expr| expr.kind.clone())
        else {
            return read;
        };
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, receiver)),
            Some(Type::Dict(_, _))
        ) {
            return read;
        }
        if let Some(expr) = body.expr_at_mut(read)
        {
            expr.ty = hint;
        }
        read
    }

    /// Lower truthiness for optional Date values as object presence.
    ///
    /// Date instances are represented by timestamps in Rust, but source
    /// truthiness depends on the Date object existing, not on its timestamp.
    pub(in crate::lowering) fn optional_known_date_presence_condition(
        &mut self,
        value: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let value_ty = Self::expr_ty(body, value);
        if !self.is_nullishable_type(value_ty)
            || !self.expression_is_known_date_value(value, body)
        {
            return None;
        }
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let none = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: none_ty,
            span,
        });
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: value,
                rhs: none,
            },
            ty: bool_ty,
            span,
        }))
    }

    /// Return whether a present optional value is always truthy in JavaScript.
    pub(in crate::lowering) fn type_is_always_truthy_object_surface(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(ty)),
            Some(
                Type::Class { .. }
                    | Type::Function(_)
                    | Type::List(_)
                    | Type::Set(_)
                    | Type::Dict(_, _)
                    | Type::Tuple(_)
                    | Type::Future(_)
            )
        )
    }

    /// Return whether EVERY value of `ty` is truthy in JavaScript.
    ///
    /// Only seven values are falsy — `false`, `0`, `-0`, `NaN`, `''`, `null`,
    /// `undefined` (and `0n`) — and none of them is an object, so a type whose
    /// every inhabitant is an object has no falsy inhabitant. That composes
    /// through a union: `A | B` with both arms objects adds no falsy value
    /// either.
    ///
    /// Deliberately SEPARATE from [`Self::type_is_always_truthy_object_surface`],
    /// which answers the narrower question "is a *present optional's* payload
    /// always truthy" and is consulted by the presence-test rung. Teaching that
    /// helper about unions would also change the presence-test rung for
    /// optional unions of functions — turning es-toolkit's `getCacheKey ? …`
    /// from a full erased `ToBool` match into a presence test. That is an
    /// equivalent and cheaper emission, but it is a change to a corpus this
    /// family does not need to touch, so the union knowledge stays local to the
    /// constant-fold rung that needs it.
    fn type_is_constantly_truthy(&self, ty: smelt_hir::TypeId) -> bool {
        let resolved = self.type_param_constraint_or_self(ty);
        if let Some(Type::Union(items)) = self.ctx.krate.types.get(resolved) {
            let items = items.clone();
            return !items.is_empty()
                && items.iter().all(|item| self.type_is_constantly_truthy(*item));
        }
        self.type_is_always_truthy_object_surface(resolved)
    }

    /// Return whether a non-boolean type can appear in a JavaScript truthiness guard.
    pub(in crate::lowering) fn type_is_truthy_condition_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Function(_)
                | Type::Class { .. }
                | Type::TypeParam { .. }
                | Type::Unknown
                | Type::Never,
            ) => true,
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| {
                    matches!(
                        self.ctx.krate.types.get(item),
                        Some(
                            Type::Bool
                                | Type::Int
                                | Type::Float
                                | Type::BigInt
                                | Type::String
                                | Type::None
                        )
                    ) || self.type_is_truthy_condition_surface(item)
                }),
            _ => false,
        }
    }
}
