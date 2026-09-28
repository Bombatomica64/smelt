//! A member read through a record-view assertion on an erased value.
//!
//! A type assertion changes nothing at runtime, so `(value as { then?: Function }).then`
//! is `value.then`: an ordinary JavaScript property lookup that answers the
//! value's own property or, failing that, whatever its prototype chain holds.
//! When `value` is erased (`unknown`) and the asserted type is an object type
//! literal, Smelt represents the asserted type as a string-keyed record
//! (`Dict<String, V>`). Materializing that record first and reading the key off
//! it is not the same lookup: the record is a copy of the value's OWN entries,
//! so every inherited member is lost — a promise's `then`/`catch`/`finally`, a
//! class instance's prototype methods, `Object.prototype` members. A thenable
//! probe such as Hono's `!!(next() as { then?: Function })?.then` then answered
//! `false` for a real promise.
//!
//! The rule: reading a member through such an assertion reads it off the
//! erased value itself (`ExprKind::Field` on the `unknown` receiver, the same
//! read `smelt_get_unknown_field` performs for any erased value) and converts
//! the result to the member's declared type with a checked `UnknownCast`. This
//! is also what a hand port would write — one property read, not a copy of the
//! whole object. The record view stays what it is for every other use
//! (`Object.keys`, spreading, passing it on), where own entries are exactly
//! right.
//!
//! Only a receiver that is a plain read — a binding, a static member chain over
//! one, or an element at a literal index (`values[0]`) — is taken this way,
//! because its type can be inspected by lowering it without duplicating any
//! side effect; any other operand keeps the ordinary assertion path.

use oxc::ast::ast::{Expression, StaticMemberExpression, TSType};
use smelt_hir::{Body, Expr, ExprKind, Type};

use crate::SmeltError;
use crate::lowering::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Lower `(erased as { .. }).member` as a member read on the erased value.
    ///
    /// Returns `None` (the caller then lowers the member expression normally)
    /// unless the object is an assertion of a binding whose lowered type is
    /// `unknown` to a string-keyed record type that declares the member.
    pub(in crate::lowering) fn asserted_erased_member_read(
        &mut self,
        member: &StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some((operand, annotation)) = asserted_operand(&member.object) else {
            return Ok(None);
        };
        if !is_plain_read(operand) {
            return Ok(None);
        }
        let Ok(target) = self.ts_type_to_hir(annotation) else {
            return Ok(None);
        };
        let Some(Type::Dict(key, _)) = self.ctx.krate.types.get(target).cloned() else {
            return Ok(None);
        };
        if self.ctx.krate.types.get(key) != Some(&Type::String) {
            return Ok(None);
        }
        let field = self.intern_source_name(member.property.name.as_str());
        let Ok(field_ty) = self.class_field_type(target, field) else {
            return Ok(None);
        };
        let exprs_before = body.exprs.len();
        let receiver = self.expression(operand, body)?;
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        if Self::expr_ty(body, receiver) != unknown_ty {
            // A plain read has no effect, so the probe is dropped and the
            // ordinary assertion path lowers the operand itself.
            body.exprs.truncate(exprs_before);
            return Ok(None);
        }
        let span = self.span(member.span.start, member.span.end);
        let read = body.push_expr(Expr {
            kind: ExprKind::Field { receiver, field },
            ty: unknown_ty,
            span,
        });
        let result_ty = if member.optional {
            self.optional_chain_result_type(field_ty)
        } else {
            field_ty
        };
        if result_ty == unknown_ty {
            return Ok(Some(read));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: read,
                target: result_ty,
            },
            ty: result_ty,
            span,
        })))
    }
}

/// The operand and asserted type of `(operand as T)` / `<T>operand`, through parentheses.
fn asserted_operand<'e, 'a>(
    object: &'e Expression<'a>,
) -> Option<(&'e Expression<'a>, &'e TSType<'a>)> {
    match object {
        Expression::ParenthesizedExpression(inner) => asserted_operand(&inner.expression),
        Expression::TSAsExpression(assertion) => {
            Some((&assertion.expression, &assertion.type_annotation))
        }
        Expression::TSTypeAssertion(assertion) => {
            Some((&assertion.expression, &assertion.type_annotation))
        }
        _ => None,
    }
}

/// Whether lowering `expression` only reads: a binding, `this`, or a
/// non-optional static member / literal-index element chain over one.
fn is_plain_read(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::Identifier(_) | Expression::ThisExpression(_) => true,
        Expression::ParenthesizedExpression(inner) => is_plain_read(&inner.expression),
        Expression::StaticMemberExpression(member) => {
            !member.optional && is_plain_read(&member.object)
        }
        Expression::ComputedMemberExpression(member) => {
            !member.optional
                && matches!(
                    member.expression,
                    Expression::NumericLiteral(_) | Expression::StringLiteral(_)
                )
                && is_plain_read(&member.object)
        }
        _ => false,
    }
}
