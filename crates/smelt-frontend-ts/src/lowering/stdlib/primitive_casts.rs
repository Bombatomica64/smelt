//! Primitive casts: `String(x)`, `Number(x)`, `Boolean(x)` called as functions.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::Expression;
use oxc::span::GetSpan;
use smelt_hir::{Body, Expr, ExprKind, Literal, PrimitiveCastOp, Type};

impl ModuleBuilder<'_> {
    /// Lower TypeScript global primitive conversion and numeric parse calls.
    pub(in crate::lowering) fn primitive_cast_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        // These names (`parseInt`, `String`, `Number`, `parseFloat`, `BigInt`,
        // `Boolean`) are recognized as JavaScript globals by identifier name. A
        // value import, module item, or local binding of the same name shadows
        // the global — e.g. es-toolkit's `import { parseInt } from './parseInt'`
        // accepts a value and an optional `undefined` radix. Defer to the
        // ordinary call path so the shadowing binding is called instead of the
        // global primitive-conversion op.
        if self.builtin_call_identifier_is_shadowed(callee.name.as_str()) {
            return Ok(None);
        }
        if callee.name == "parseInt" {
            let (operand, radix) = self.parse_int_operand("parseInt", call, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            let kind = radix.map_or(
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToInt,
                    operand,
                },
                |radix| ExprKind::ParseIntRadix { operand, radix },
            );
            return Ok(Some(body.push_expr(Expr {
                kind,
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if callee.name == "BigInt" {
            return self.bigint_conversion_call(call, body).map(Some);
        }
        if callee.name == "parseFloat" {
            let operand = self.parse_float_operand("parseFloat", call, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ParseFloat,
                    operand,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let (op, result_ty) = match callee.name.as_str() {
            "String" => (PrimitiveCastOp::ToString, Type::String),
            "Number" => (PrimitiveCastOp::ToJsNumber, Type::Float),
            "Boolean" => (PrimitiveCastOp::ToBool, Type::Bool),
            _ => return Ok(None),
        };
        if call.arguments.is_empty() {
            // Zero-argument primitive conversions are legal JavaScript and return
            // the type's default primitive: `Boolean()` -> `false`, `Number()` ->
            // `0`, `String()` -> `""`. (`parseFloat`/`BigInt` are not
            // default-value coercions and keep the arity error.)
            let default_literal = match callee.name.as_str() {
                "Boolean" => Some(Literal::Bool(false)),
                "Number" => Some(Literal::Float(0.0)),
                "String" => Some(Literal::String(String::new())),
                _ => None,
            };
            if let Some(literal) = default_literal {
                let ty = self.ctx.krate.types.intern(result_ty);
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(literal),
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
        }
        let [arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{} requires exactly one argument", callee.name),
            ));
        };
        let operand = self.argument(arg, body)?;
        // `String(x)` spells an absent `null` as `"null"` (`lowering::null_absent`).
        let operand = if op == PrimitiveCastOp::ToString {
            self.null_absent_string_operand(operand, arg.span().start, arg.span().end, body)
        } else {
            operand
        };
        let operand_ty = Self::expr_ty(body, operand);
        let operand_type = self.ctx.krate.types.get(operand_ty);
        if !self.primitive_cast_accepts_operand(op, operand_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{} requires a primitive argument", callee.name),
            ));
        }
        if callee.name == "String" && matches!(operand_type, Some(Type::Bool)) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "String currently supports number and string arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(result_ty);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast { op, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether a global primitive conversion accepts a lowered operand type.
    pub(in crate::lowering) fn primitive_cast_accepts_operand(
        &self,
        op: PrimitiveCastOp,
        operand_ty: smelt_hir::TypeId,
    ) -> bool {
        if op == PrimitiveCastOp::ToString && self.erased_or_union_surface(operand_ty) {
            return true;
        }
        if matches!(op, PrimitiveCastOp::ToFloat | PrimitiveCastOp::ToJsNumber) {
            return !matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Never));
        }
        match self.ctx.krate.types.get(operand_ty) {
            Some(Type::Bool | Type::String | Type::Int | Type::BigInt | Type::Float | Type::Unknown) => true,
            Some(Type::TypeParam { .. } | Type::Class { .. }) => {
                matches!(
                    op,
                    PrimitiveCastOp::ToBool
                        | PrimitiveCastOp::ToFloat
                        | PrimitiveCastOp::ToJsNumber
                )
            }
            Some(Type::Union(items)) => items.iter().copied().all(|item| {
                matches!(self.ctx.krate.types.get(item), Some(Type::None))
                    || self.primitive_cast_accepts_operand(op, item)
            }),
            Some(Type::Optional(item)) => self.primitive_cast_accepts_operand(op, *item),
            Some(_) if self.is_numeric_like_type(operand_ty) => true,
            _ => false,
        }
    }
}
