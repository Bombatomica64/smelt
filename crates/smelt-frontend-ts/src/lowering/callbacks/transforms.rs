//! Callback value transforms: string/template/typeof/binary/conditional
//! callback expressions and the list search/entries/`at` collection calls.

use crate::lowering::{
    Body, Expr, ExprKind, Expression, ListSearchOp, ModuleBuilder,
    PrimitiveCastOp, SmeltError, Type, };
use oxc::span::GetSpan;

impl ModuleBuilder<'_> {
    /// Lower direct TypeScript `Array.prototype.indexOf` and `lastIndexOf`.
    pub(in crate::lowering) fn list_search_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "indexOf" => ListSearchOp::Find,
            "lastIndexOf" => ListSearchOp::RFind,
            _ => return Ok(None),
        };
        let mut list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some((list_ty, item_ty)) = self.list_surface_type(list_ty) else {
            return Ok(None);
        };
        if Self::expr_ty(body, list) != list_ty {
            list = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: list },
                ty: list_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
        }
        // JavaScript `indexOf`/`lastIndexOf` accept the search item plus an
        // optional `fromIndex`. The item argument is required; the second
        // argument is a numeric start/end position handled by the emitter.
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf require one item and an optional fromIndex argument",
            ));
        }
        let Some(item_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf require one item and an optional fromIndex argument",
            ));
        };
        let item = self.argument(item_argument, body)?;
        if !self.array_item_type_compatible(Self::expr_ty(body, item), item_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf argument must match the array element type",
            ));
        }
        let from_index = if let Some(from_index_argument) = call.arguments.get(1) {
            let from_index = self.argument(from_index_argument, body)?;
            if !self.slice_index_type_is_number(Self::expr_ty(body, from_index)) {
                return Err(SmeltError::unsupported(
                    self.span(
                        from_index_argument.span().start,
                        from_index_argument.span().end,
                    ),
                    "array indexOf/lastIndexOf fromIndex must be numeric",
                ));
            }
            Some(from_index)
        } else {
            None
        };
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListSearch {
                op,
                list,
                item,
                from_index,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Build `receiver.at(index)` on a list or string whose element type is
    /// `item_ty`: the index coerced by [`Self::coerce_at_index_operand`], then
    /// an `OptionalIndex` typed `Optional<item_ty>`.
    pub(in crate::lowering) fn optional_at_index_expr(
        &mut self,
        receiver: smelt_hir::ExprId,
        item_ty: smelt_hir::TypeId,
        index: smelt_hir::ExprId,
        span_start: u32,
        span_end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let index = self.coerce_at_index_operand(index, span_start, span_end, body)?;
        let ty = self.ctx.krate.types.intern(Type::Optional(item_ty));
        Ok(body.push_expr(Expr {
            kind: ExprKind::OptionalIndex { receiver, index },
            ty,
            span: self.span(span_start, span_end),
        }))
    }

    /// Coerce a `.at(index)` argument to the `Float` runtime type the optional
    /// indexing path expects, or reject a genuinely non-numeric index.
    ///
    /// JavaScript's `Array.prototype.at`/`String.prototype.at` run `ToInteger`
    /// on the argument, so any statically numeric-compatible index is valid even
    /// when its Smelt type is not exactly `Float`. An index already typed `Float`
    /// is returned unchanged (the common case). A numeric-like index (`Int`, a
    /// numeric type-parameter constraint, or a union whose arms are all numeric),
    /// or an optional-numeric surface (`number | undefined`), is converted with a
    /// JS `Number(...)` cast so the emitted normalized-index arithmetic sees a
    /// concrete `f64`. Erased (`unknown`), class (which includes a typed object
    /// or interface), and type-parameter indexes, and unions or optionals with
    /// such an arm, are coerced with the same JS `Number(...)` cast; only other
    /// non-numeric types (a string, a list or a `Record`, for example) are
    /// unsupported.
    fn coerce_at_index_operand(
        &mut self,
        index: smelt_hir::ExprId,
        span_start: u32,
        span_end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let index_ty = Self::expr_ty(body, index);
        if self.ctx.krate.types.get(index_ty) == Some(&Type::Float) {
            return Ok(index);
        }
        if self.is_numeric_like_type(index_ty)
            || self.optional_numeric_surface(index_ty)
            || self.erased_or_union_surface(index_ty)
        {
            // A numeric-like, optional-numeric, or erased index (e.g. the
            // cross-module `toInteger(n)` return that lowers to an opaque surface)
            // is coerced with a JS `Number(...)` cast. `Array`/`String.prototype.at`
            // run `ToInteger` on the argument at runtime, so any value is accepted;
            // the cast makes the emitted normalized-index arithmetic see a concrete
            // `f64` instead of aborting on the erased shape.
            let float_ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToJsNumber,
                    operand: index,
                },
                ty: float_ty,
                span: self.span(span_start, span_end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(span_start, span_end),
            "array/string at index must be a number",
        ))
    }
}
