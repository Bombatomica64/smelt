//! Which nullish value an optional's absent case is.
//!
//! The type of an optional says which absent value its `None` is
//! (`smelt_hir::Absence`), and codegen prints, compares and erases by it.
//! This module is the older, checker-driven rule, kept for the places where
//! the HIR type carries the default `undefined` tag while the checker proves
//! the expression `null` (an optional Smelt itself introduced). JavaScript
//! observes which absent value an expression holds in three places:
//!
//! - a string conversion: `` `${x}` ``, `String(x)` and `"a" + x` spell `null`
//!   as `"null"` and `undefined` as `"undefined"`;
//! - a strict comparison: `x === undefined` is `false` for a `null`;
//! - `console.log(x)`, which prints the word;
//! - `typeof x`, which is `"object"` for `null` and `"undefined"` for
//!   `undefined`.
//!
//! The TypeScript 7 checker types every expression, so at each of those places
//! the frontend asks it which nullish value the expression's type admits.
//! When the answer is `null` and never `undefined` (a `string | null`), the
//! absent case IS `null` and is lowered that way. Every other answer (both, or
//! only `undefined`, or no checker) keeps the `undefined` reading the storage
//! has always had. The storage type itself does not change.

use crate::lowering::body_arena::BodyArena;
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
        let is_string = |builder: &Self, expr| {
            builder.ctx.krate.types.get(Self::expr_ty(body, expr)) == Some(&Type::String)
        };
        if is_string(self, lhs) {
            let span = binary.right.span();
            (
                lhs,
                self.string_conversion_operand(rhs, span.start, span.end, body),
            )
        } else if is_string(self, rhs) {
            let span = binary.left.span();
            (
                self.string_conversion_operand(lhs, span.start, span.end, body),
                rhs,
            )
        } else {
            (lhs, rhs)
        }
    }

    /// Whether the TS7 checker has a type for the expression at `start..end`
    /// and no value of it can be a string. `false` without facts.
    pub(in crate::lowering) fn checker_types_as_non_string(&self, start: u32, end: u32) -> bool {
        let Some(facts) = self.ts_facts else {
            return false;
        };
        facts
            .type_id_at(&self.path, start, end)
            .is_some_and(|id| !ts_type_may_be_string(facts, id, 0))
    }

    /// Whether the TS7 checker types the expression at `start..end` as a
    /// number or a bigint (a literal of either included). `false` without
    /// facts.
    pub(in crate::lowering) fn checker_types_as_numeric(&self, start: u32, end: u32) -> bool {
        self.ts_facts
            .and_then(|facts| facts.type_at(&self.path, start, end))
            .is_some_and(|ty| {
                matches!(
                    ty.kind,
                    TsTypeKind::Number
                        | TsTypeKind::NumberLiteral(_)
                        | TsTypeKind::BigInt
                        | TsTypeKind::BigIntLiteral(_)
                )
            })
    }

    /// Whether the TS7 checker types the expression at `start..end` as a string.
    /// `false` without facts.
    pub(in crate::lowering) fn checker_types_as_string(&self, start: u32, end: u32) -> bool {
        self.ts_facts
            .and_then(|facts| facts.type_at(&self.path, start, end))
            .is_some_and(|ty| matches!(ty.kind, TsTypeKind::String | TsTypeKind::StringLiteral(_)))
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

    /// Lower the operand of a string conversion (a template literal part, a
    /// `+` operand beside a string, `String(x)`) through the steps of
    /// JavaScript `ToString` that need source-level facts; codegen spells the
    /// per-type rest (`string_like_operand_rust_text`), so all three
    /// conversions agree.
    ///
    /// `operand` is the lowered expression and `start..end` its source span.
    /// - A value only ever absent (`null`, `undefined`) is its word.
    /// - A class instance whose class has a `toString` method of its own (or
    ///   inherited) is converted by calling it, as `ToPrimitive` does: the
    ///   result is `x.toString()`.
    /// - A list of such class instances converts as the list of their
    ///   strings (`${[p, q]}` is `p.toString() + "," + q.toString()`).
    /// - An `Optional` whose checker type admits only `null` reads its absent
    ///   case as `"null"`: `x ?? "null"` for a string and
    ///   `x !== undefined ? String(x) : "null"`, with `x` bound once,
    ///   otherwise.
    ///
    /// Any other operand is returned unchanged.
    pub(in crate::lowering) fn string_conversion_operand(
        &mut self,
        operand: smelt_hir::ExprId,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let operand_ty = Self::expr_ty(body, operand);
        // A value that is only ever absent is the word for which absent
        // value it is: `String(null)` is `"null"`, `String(undefined)` is
        // `"undefined"`.
        if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::None)) {
            let null_literal = matches!(
                body.expr_at(operand).map(|expr| &expr.kind),
                Some(ExprKind::Literal(Literal::None))
            );
            let word = if null_literal || self.checker_absent_is_null(start, end) {
                "null"
            } else {
                "undefined"
            };
            let string_ty = self.ctx.krate.types.intern(Type::String);
            return body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(word.to_owned())),
                ty: string_ty,
                span: self.span(start, end),
            });
        }
        if matches!(
            self.ctx.krate.types.get(operand_ty),
            Some(Type::Class { .. })
        ) && self.declares_to_string(operand_ty)
        {
            let method = self.intern_source_name("toString");
            let string_ty = self.ctx.krate.types.intern(Type::String);
            let return_ty = self
                .resolve_method(operand_ty, method, oxc::span::Span::new(start, end))
                .map_or(string_ty, |(return_ty, _)| return_ty);
            return body.push_expr(Expr {
                kind: ExprKind::Method {
                    receiver: operand,
                    method,
                    args: Vec::new(),
                },
                ty: return_ty,
                span: self.span(start, end),
            });
        }
        // An array's string is its elements' strings joined with `,`, so a
        // list of such class instances converts each through its `toString`.
        if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(operand_ty).cloned()
            && matches!(self.ctx.krate.types.get(item_ty), Some(Type::Class { .. }))
            && self.declares_to_string(item_ty)
        {
            return self.list_items_to_string(
                operand,
                item_ty,
                oxc::span::Span::new(start, end),
                body,
            );
        }
        if !matches!(
            self.ctx.krate.types.get(operand_ty),
            Some(Type::Optional(..))
        ) || !self.checker_absent_is_null(start, end)
        {
            return operand;
        }
        let span = self.span(start, end);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        // A `string | null` needs no conversion when present: `x ?? "null"`.
        if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Optional(inner, _)) if *inner == string_ty)
        {
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
        let name = self.intern_synthesized_name("__smelt_nullable");
        let local = self.capture_bind_value(operand, operand_ty, name, span, body);
        let read = |reads: &mut Body| {
            reads.push_expr(Expr {
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

    /// `typeof` of an operand whose absent case the checker types as `null`.
    ///
    /// `typeof null` is `"object"`; the storage's single absent case reads as
    /// `undefined`, which made `typeof null` answer `"undefined"`. `operand`
    /// is the lowered expression and `start..end` its source span. A `null`
    /// operand (`typeof null`, a read narrowed to `null`) is `"object"`; an
    /// `Optional` admitting `null` and not `undefined` is
    /// `x !== undefined ? typeof x : "object"`, with `x` bound once. `None`
    /// for every other operand, which keeps the ordinary `typeof` rule.
    pub(in crate::lowering) fn null_absent_typeof(
        &mut self,
        operand: smelt_hir::ExprId,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let operand_ty = Self::expr_ty(body, operand);
        let absent_only = matches!(self.ctx.krate.types.get(operand_ty), Some(Type::None));
        let optional = matches!(
            self.ctx.krate.types.get(operand_ty),
            Some(Type::Optional(..))
        );
        if !(absent_only || optional) || !self.checker_absent_is_null(start, end) {
            return None;
        }
        let span = self.span(start, end);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let object = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("object".to_owned())),
            ty: string_ty,
            span,
        });
        if absent_only {
            return Some(object);
        }
        let name = self.intern_synthesized_name("__smelt_nullable");
        let local = self.capture_bind_value(operand, operand_ty, name, span, body);
        let read = |reads: &mut Body| {
            reads.push_expr(Expr {
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
            kind: ExprKind::TypeofValue { value: present },
            ty: string_ty,
            span,
        });
        Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr: object,
            },
            ty: string_ty,
            span,
        }))
    }
}

/// Whether a value of the checker type `id` can be a string: a string, a type
/// that says nothing (`any`, `unknown`, a free parameter, a deferred operator),
/// or a union or intersection with such a member.
fn ts_type_may_be_string(
    facts: &smelt_ts_model::TsTypeFacts,
    id: smelt_ts_model::TsTypeId,
    depth: usize,
) -> bool {
    if depth > 16 {
        return true;
    }
    let Some(ty) = facts.types.get(id) else {
        return true;
    };
    match &ty.kind {
        TsTypeKind::String
        | TsTypeKind::StringLiteral(_)
        | TsTypeKind::Any
        | TsTypeKind::Unknown
        | TsTypeKind::TypeParameter { .. }
        | TsTypeKind::Deferred(_)
        | TsTypeKind::Truncated => true,
        TsTypeKind::Union { members, .. } => members
            .iter()
            .any(|member| ts_type_may_be_string(facts, *member, depth + 1)),
        TsTypeKind::Intersection(members) => members
            .iter()
            .any(|member| ts_type_may_be_string(facts, *member, depth + 1)),
        _ => false,
    }
}
