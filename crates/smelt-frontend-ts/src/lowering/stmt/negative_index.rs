//! JavaScript negative / out-of-range bracket-index lowering.
//!
//! In JavaScript an element bracket access such as `arr[-1]` or `str[-1]` is a
//! *property* lookup, not an element lookup. A negative numeric key never names
//! an array element or string character, so the read evaluates to `undefined`
//! and a write (`arr[-1] = x`) stores an ordinary string-keyed property that
//! does not change the array's elements or `length`.
//!
//! This is deliberately distinct from `Array.prototype.at(-1)` / `String.at`,
//! which wrap negative offsets to count from the end. Smelt therefore does not
//! rewrite `arr[-1]` into `.at(-1)`; it models the true JavaScript property
//! semantics: reads become `undefined` and writes become no-ops on the
//! collection (their right-hand side is still evaluated for side effects).
//!
//! Only *statically* negative literal indexes are handled here — the same class
//! the frontend previously rejected (`-1`, `-(2)`). A fully dynamic computed
//! index whose sign is unknown at compile time is not in scope and keeps its
//! existing runtime lowering.

use oxc::ast::ast::{AssignmentTarget, Expression};

use smelt_hir::{Body, Expr, ExprKind, Literal, Stmt, Type};

use crate::lowering::ModuleBuilder;
use crate::SmeltError;

impl ModuleBuilder<'_> {
    /// Returns true when a receiver type uses fixed sequence element indexing.
    ///
    /// Arrays, strings, and tuples index by element position, so a negative
    /// numeric bracket key on any of them is a JavaScript property lookup rather
    /// than an element access. Records/maps and dynamic (`unknown`) receivers use
    /// key lookups where a negative number is a perfectly valid key, so they are
    /// intentionally excluded.
    pub(in crate::lowering) fn is_sequence_indexing_receiver(
        &self,
        receiver_ty: smelt_hir::TypeId,
    ) -> bool {
        matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::List(_) | Type::String | Type::Tuple(_))
        )
    }

    /// Returns true when a sequence bracket access uses a statically negative
    /// literal index and therefore has JavaScript property-lookup semantics.
    pub(in crate::lowering) fn is_negative_sequence_bracket_index(
        &self,
        receiver_ty: smelt_hir::TypeId,
        index: smelt_hir::ExprId,
        body: &Body,
    ) -> bool {
        self.is_sequence_indexing_receiver(receiver_ty)
            && Self::is_negative_numeric_expr(body, index)
    }

    /// Lower a statically-negative sequence bracket *read* to `undefined`.
    ///
    /// JavaScript yields `undefined` for any out-of-range property lookup, so the
    /// value is statically known. The result is a `None` literal typed as an
    /// optional of the collection's element type, keeping the lowered value's
    /// type honest (undefined-able) instead of pretending the element is present.
    pub(in crate::lowering) fn lower_negative_sequence_bracket_read(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let element_ty = self.index_type(receiver_ty)?;
        let ty = self.optional_chain_result_type(element_ty);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(span.start, span.end),
        }))
    }

    /// Try to lower a statement-level assignment whose target is a statically
    /// negative sequence bracket index.
    ///
    /// `arr[-1] = value` in JavaScript sets a string-keyed property on the array
    /// object; it does not grow the array or change any element. On Smelt's
    /// `Vec`-backed sequences there is no such property slot, so the faithful
    /// lowering is a no-op on the collection. The right-hand side is still
    /// lowered and emitted as a discarded expression statement so its side
    /// effects (calls, updates) are preserved and evaluation order is unchanged.
    ///
    /// Returns `Ok(true)` when the target matched and a statement was emitted,
    /// `Ok(false)` when the target is not a negative sequence bracket write and
    /// normal assignment lowering should proceed.
    pub(in crate::lowering) fn try_lower_negative_bracket_write_statement(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        // Only a plain `=` write mirrors the property-store semantics cleanly. A
        // compound write (`arr[-1] += x`) would first read the (undefined)
        // property; that case is rare and left to normal lowering.
        if assign.operator != oxc::syntax::operator::AssignmentOperator::Assign {
            return Ok(false);
        }
        let Some(member) = Self::assignment_target_computed_member(&assign.left) else {
            return Ok(false);
        };
        if !Self::computed_member_has_negative_literal_index(member) {
            return Ok(false);
        }
        // Resolve the receiver type to confirm it is a fixed sequence before
        // treating the negative index as a property write rather than a key.
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let access_receiver_ty = self.optional_receiver_inner_type(receiver_ty);
        if !self.is_sequence_indexing_receiver(access_receiver_ty) {
            return Ok(false);
        }
        // Preserve right-hand-side side effects and evaluation order; the write
        // itself is a no-op on the collection.
        let value = self.expression(&assign.right, body)?;
        body.push_stmt_to_block(block, Stmt::Expr(value));
        Ok(true)
    }

    /// Extract the computed member expression from an assignment target, if any.
    ///
    /// `AssignmentTarget` flattens `SimpleAssignmentTarget`'s member-expression
    /// variants, so a computed member target such as `arr[-1]` appears directly
    /// as `AssignmentTarget::ComputedMemberExpression`.
    fn assignment_target_computed_member<'a>(
        target: &'a AssignmentTarget<'a>,
    ) -> Option<&'a oxc::ast::ast::ComputedMemberExpression<'a>> {
        match target {
            AssignmentTarget::ComputedMemberExpression(member) => Some(member),
            _ => None,
        }
    }

    /// Returns true when a computed member's index is a statically negative
    /// numeric literal (`arr[-1]`, `arr[-(2)]`).
    fn computed_member_has_negative_literal_index(
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
    ) -> bool {
        Self::source_expression_is_negative_numeric_literal(&member.expression)
    }

    /// Returns true when a source expression is a negative numeric literal,
    /// including a unary negation of a numeric literal.
    fn source_expression_is_negative_numeric_literal(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::UnaryExpression(unary) => {
                unary.operator == oxc::syntax::operator::UnaryOperator::UnaryNegation
                    && matches!(&unary.argument, Expression::NumericLiteral(literal) if literal.value != 0.0)
            }
            Expression::NumericLiteral(literal) => literal.value < 0.0,
            _ => false,
        }
    }
}
