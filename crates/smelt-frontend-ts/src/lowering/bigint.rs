//! Lowering of JavaScript `bigint` operations.
//!
//! A `bigint` is its own HIR type ([`Type::BigInt`]), separate from the bounded
//! [`Type::Int`]: a literal (`123n`), a `bigint` annotation, a `BigInt(x)`
//! result and bigint arithmetic all carry it, so ordinary integer and index
//! typing is untouched and the backend can give the value an unbounded Rust
//! type.
//!
//! Most bigint operators reuse the ordinary `BinOp` lowering, because their
//! operand types already say "bigint" to the backend. The operations here are
//! the ones that need a different HIR shape:
//!
//! * `/`, `%`, `**`, `<<` and `>>` over bigints throw a catchable
//!   `RangeError` (a zero divisor, a negative exponent, a result past the
//!   size cap), so they become a fallible [`ExprKind::BigIntCall`] rather than
//!   a `BinOp` rvalue; so does `bigint.toString(radix)`, whose radix is
//!   range-checked;
//! * `BigInt(x)` converts with ECMA-262 `ToBigInt` semantics (a fractional
//!   number is a `RangeError`, a malformed string a `SyntaxError`) instead of
//!   the `number` cast it used to share with `Number(x)`;
//! * `BigInt.asIntN` / `BigInt.asUintN` are fallible on their width;
//! * `~x` over a bigint is `x ^ -1n` at bigint type, not the int32 form.

use smelt_hir::{BigIntOp, BinOp, Body, Expr, ExprId, ExprKind, Literal, Span, Type, TypeId};

use super::ModuleBuilder;
use crate::SmeltError;

impl ModuleBuilder<'_> {
    /// Whether `ty` is the JavaScript `bigint` type.
    pub(in crate::lowering) fn is_bigint_type(&self, ty: TypeId) -> bool {
        matches!(self.ctx.krate.types.get(ty), Some(Type::BigInt))
    }

    /// Whether a value of `ty` can be a bigint: a bigint, or a union (or
    /// optional) with a bigint member.
    pub(in crate::lowering) fn type_may_hold_bigint(&self, ty: TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::BigInt) => true,
            Some(Type::Union(items)) => items.iter().any(|item| self.type_may_hold_bigint(*item)),
            Some(Type::Optional(inner)) => self.type_may_hold_bigint(*inner),
            _ => false,
        }
    }

    /// The `1` that `++` / `--` add to a value of `ty`: `1n` for a bigint
    /// (`i++` on a bigint is `i + 1n`; adding the number `1` would be the
    /// mixed-type `TypeError`), the number `1` otherwise.
    pub(in crate::lowering) fn unit_step_literal(&self, ty: TypeId) -> Literal {
        if self.is_bigint_type(ty) {
            Literal::BigInt("1".to_owned())
        } else {
            Literal::Float(1.0)
        }
    }

    /// Push a fallible bigint operation, typed `string` for
    /// [`BigIntOp::returns_string`] and `bigint` otherwise.
    fn push_bigint_call(
        &mut self,
        op: BigIntOp,
        args: Vec<ExprId>,
        span: Span,
        body: &mut Body,
    ) -> ExprId {
        let ty = self.ctx.krate.types.intern(if op.returns_string() {
            Type::String
        } else {
            Type::BigInt
        });
        body.push_expr(Expr {
            kind: ExprKind::BigIntCall { op, args },
            ty,
            span,
        })
    }

    /// The fallible form of `lhs op rhs`, when both operands are bigints and
    /// the operator can throw (`/` and `%` on a zero divisor, `<<` and `>>`
    /// past the size cap).
    ///
    /// `None` leaves the ordinary `BinOp` lowering in charge, which is right for
    /// every other bigint operator: their operand types already route them to
    /// the backend's bigint arithmetic.
    pub(in crate::lowering) fn bigint_fallible_binary(
        &mut self,
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<ExprId> {
        let call_op = match op {
            BinOp::Div => BigIntOp::Div,
            BinOp::Rem => BigIntOp::Rem,
            BinOp::Shl => BigIntOp::Shl,
            BinOp::Shr => BigIntOp::Shr,
            _ => return None,
        };
        if !self.is_bigint_type(Self::expr_ty(body, lhs))
            || !self.is_bigint_type(Self::expr_ty(body, rhs))
        {
            return None;
        }
        Some(self.push_bigint_call(call_op, vec![lhs, rhs], span, body))
    }

    /// `base ** exponent` over two bigints, which throws on a negative
    /// exponent; `None` for the `number` power.
    pub(in crate::lowering) fn bigint_power(
        &mut self,
        base: ExprId,
        exponent: ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<ExprId> {
        if !self.is_bigint_type(Self::expr_ty(body, base))
            || !self.is_bigint_type(Self::expr_ty(body, exponent))
        {
            return None;
        }
        Some(self.push_bigint_call(BigIntOp::Pow, vec![base, exponent], span, body))
    }

    /// `operand.toString(radix)` over a bigint, which throws on a radix
    /// outside `2..=36`; `None` for any other receiver.
    pub(in crate::lowering) fn bigint_to_string_radix(
        &mut self,
        operand: ExprId,
        radix: ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<ExprId> {
        if !self.is_bigint_type(Self::expr_ty(body, operand)) {
            return None;
        }
        Some(self.push_bigint_call(BigIntOp::ToStringRadix, vec![operand, radix], span, body))
    }

    /// `JSON.stringify(value)` over a value whose type can hold a bigint,
    /// which throws a catchable `TypeError` when one is present at run time.
    pub(in crate::lowering) fn bigint_json_stringify(
        &mut self,
        value: ExprId,
        span: Span,
        body: &mut Body,
    ) -> ExprId {
        self.push_bigint_call(BigIntOp::JsonStringify, vec![value], span, body)
    }

    /// `~operand` over a bigint: `operand ^ -1n`, which is the bitwise NOT of
    /// an infinite two's-complement value (`~5n` is `-6n`).
    pub(in crate::lowering) fn bigint_bitwise_not(
        &mut self,
        operand: ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<ExprId> {
        if !self.is_bigint_type(Self::expr_ty(body, operand)) {
            return None;
        }
        let ty = self.ctx.krate.types.intern(Type::BigInt);
        let all_ones = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::BigInt("-1".to_owned())),
            ty,
            span,
        });
        Some(body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::BitXor,
                lhs: operand,
                rhs: all_ones,
            },
            ty,
            span,
        }))
    }

    /// Lower the global `BigInt(value)` conversion.
    ///
    /// A bigint argument is already the result. Anything else goes through the
    /// fallible `ToBigInt` builtin: an integral number or a numeric string
    /// converts, a fractional number throws `RangeError` and a malformed string
    /// throws `SyntaxError`, exactly as in JavaScript.
    pub(in crate::lowering) fn bigint_conversion_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<ExprId, SmeltError> {
        let span = self.span(call.span.start, call.span.end);
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                span,
                "BigInt requires exactly one argument",
            ));
        };
        let operand = self.argument(argument, body)?;
        if self.is_bigint_type(Self::expr_ty(body, operand)) {
            return Ok(operand);
        }
        Ok(self.push_bigint_call(BigIntOp::From, vec![operand], span, body))
    }

    /// Lower `BigInt.asIntN(bits, value)` / `BigInt.asUintN(bits, value)`.
    pub(in crate::lowering) fn bigint_static_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<ExprId>, SmeltError> {
        if self.builtin_call_identifier_is_shadowed("BigInt") {
            return Ok(None);
        }
        let Some(member) = call.callee.as_member_expression() else {
            return Ok(None);
        };
        let op = match member.static_property_name() {
            Some("asIntN") => BigIntOp::AsIntN,
            Some("asUintN") => BigIntOp::AsUintN,
            _ => return Ok(None),
        };
        let span = self.span(call.span.start, call.span.end);
        let [bits, value] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                span,
                format!("{} requires exactly two arguments", op.name()),
            ));
        };
        let bits = self.argument(bits, body)?;
        let value = self.argument(value, body)?;
        Ok(Some(self.push_bigint_call(op, vec![bits, value], span, body)))
    }
}
