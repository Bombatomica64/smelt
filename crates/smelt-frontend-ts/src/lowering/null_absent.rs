//! Which nullish value an optional's absent case is.
//!
//! Smelt stores `T | null` and `T | undefined` alike, as `Type::Optional(T)`
//! (a Rust `Option<T>`): the storage has one absent case. JavaScript has two,
//! and a program can observe which one it holds in three places:
//!
//! - a string conversion: `` `${x}` ``, `String(x)` and `"a" + x` spell `null`
//!   as `"null"` and `undefined` as `"undefined"`;
//! - a strict comparison: `x === undefined` is `false` for a `null`;
//! - `console.log(x)`, which prints the word.
//!
//! The TypeScript 7 checker types every expression, so at each of those places
//! the frontend asks it which nullish value the expression's type admits.
//! When the answer is `null` and never `undefined` (a `string | null`), the
//! absent case IS `null` and is lowered that way. Every other answer (both, or
//! only `undefined`, or no checker) keeps the `undefined` reading the storage
//! has always had. The storage type itself does not change.

use smelt_hir::{BinOp, Body, Expr, ExprKind, Literal, PrimitiveCastOp, Type};
use smelt_ts_model::TsTypeKind;

use super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Apply the string-conversion rule to the operands of a `+`.
    ///
    /// `+` with a string on one side converts the other side to a string. Both
    /// binary lowerings (with and without a type hint) call this, so the rule
    /// is stated once.
    pub(in crate::lowering) fn null_absent_concat_operands(
        &mut self,
        op: BinOp,
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> (smelt_hir::ExprId, smelt_hir::ExprId) {
        use oxc::span::GetSpan as _;
        if op != BinOp::Add {
            return (lhs, rhs);
        }
        let is_string =
            |builder: &Self, expr| builder.ctx.krate.types.get(Self::expr_ty(body, expr)) == Some(&Type::String);
        if is_string(self, lhs) {
            let span = binary.right.span();
            (lhs, self.null_absent_string_operand(rhs, span.start, span.end, body))
        } else if is_string(self, rhs) {
            let span = binary.left.span();
            (self.null_absent_string_operand(lhs, span.start, span.end, body), rhs)
        } else {
            (lhs, rhs)
        }
    }

    /// Whether the TS7 checker types the expression at `start..end` as an
    /// array or a tuple (every member, for a union). `false` without facts.
    pub(in crate::lowering) fn checker_types_as_array(&self, start: u32, end: u32) -> bool {
        let Some(facts) = self.ts_facts else {
            return false;
        };
        let Some(ty) = facts.type_at(&self.path, start, end) else {
            return false;
        };
        match &ty.kind {
            TsTypeKind::Array { .. } | TsTypeKind::Tuple { .. } => true,
            TsTypeKind::Union { members, .. } => {
                !members.is_empty()
                    && members.iter().all(|member| {
                        matches!(
                            facts.types.get(*member).map(|member| &member.kind),
                            Some(TsTypeKind::Array { .. } | TsTypeKind::Tuple { .. })
                        )
                    })
            }
            _ => false,
        }
    }

    /// Whether the checker types the expression at `start..end` as admitting
    /// `null` but not `undefined` (nor `void`).
    pub(in crate::lowering) fn checker_absent_is_null(&self, start: u32, end: u32) -> bool {
        let Some(facts) = self.ts_facts else {
            return false;
        };
        let Some(ty) = facts.type_at(&self.path, start, end) else {
            return false;
        };
        // A read the checker narrowed to `null` itself (`let x: T | null =
        // null; String(x)`) holds the absent case and it is `null`.
        let members = match &ty.kind {
            TsTypeKind::Null => return true,
            TsTypeKind::Union { members, .. } => members,
            _ => return false,
        };
        let mut null = false;
        for member in members {
            match facts.types.get(*member).map(|member| &member.kind) {
                Some(TsTypeKind::Null) => null = true,
                Some(TsTypeKind::Undefined | TsTypeKind::Void) => return false,
                _ => {}
            }
        }
        null
    }

    /// Lower the operand of a string conversion so an absent `null` reads as
    /// `"null"`.
    ///
    /// `operand` is the lowered expression and `start..end` its source span.
    /// For an `Optional` whose checker type admits only `null`, the result is
    /// `x ?? "null"` for a string and `x !== undefined ? String(x) : "null"`,
    /// with `x` bound once, otherwise; any other operand is returned
    /// unchanged.
    pub(in crate::lowering) fn null_absent_string_operand(
        &mut self,
        operand: smelt_hir::ExprId,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let operand_ty = Self::expr_ty(body, operand);
        if !matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Optional(_)))
            || !self.checker_absent_is_null(start, end)
        {
            return operand;
        }
        let span = self.span(start, end);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        // A `string | null` needs no conversion when present: `x ?? "null"`.
        if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Optional(inner)) if *inner == string_ty) {
            let fallback = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("null".to_owned())),
                ty: string_ty,
                span,
            });
            return body.push_expr(Expr {
                kind: ExprKind::OptionalCoalesce {
                    optional: operand,
                    fallback,
                },
                ty: string_ty,
                span,
            });
        }
        let name = self.intern_source_name("__smelt_nullable");
        let local = self.capture_bind_value(operand, operand_ty, name, span, body);
        let read = |body: &mut Body| {
            body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty: operand_ty,
                span,
            })
        };
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let tested = read(body);
        let absent = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Undefined),
            ty: none_ty,
            span,
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::JsStrictNotEq,
                lhs: tested,
                rhs: absent,
            },
            ty: bool_ty,
            span,
        });
        let present = read(body);
        let then_expr = body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToString,
                operand: present,
            },
            ty: string_ty,
            span,
        });
        let else_expr = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("null".to_owned())),
            ty: string_ty,
            span,
        });
        body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            },
            ty: string_ty,
            span,
        })
    }
}
