//! Building arrays through the global: `Array.from`, `new Array(..)` and `Array(..)`.

use crate::lowering::{
    Argument, Body, DictProjectionOp, Expr, ExprKind, Expression, ModuleBuilder, ObjectPropertyKind,
    PrimitiveCastOp, PropertyKey, SetProjectionOp, SmeltError, Span, Type,
};
use crate::lowering::js_text::js_str_text;
use oxc::span::GetSpan;
use smelt_hir::ErasureReason;
use std::borrow::Cow;

impl ModuleBuilder<'_> {
    /// Lower static `Array.from({ length }, mapper)` calls into indexed list construction.
    pub(in crate::lowering) fn array_from_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if !matches!(&member.object, Expression::Identifier(object) if object.name == "Array")
            || member.property.name != "from"
        {
            return Ok(None);
        }
        let (source_arg, mapper_arg) = match call.arguments.as_slice() {
            [source_arg] => (source_arg, None),
            [source_arg, mapper_arg] => (source_arg, Some(mapper_arg)),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Array.from currently requires an array-like source and optional mapper callback",
                ));
            }
        };
        if !matches!(source_arg, Argument::ObjectExpression(_)) {
            let source = self.argument(source_arg, body)?;
            let source_ty = self.type_param_constraint_or_self(Self::expr_ty(body, source));
            // `Array.from(view)` is the view's elements, decoded by the family
            // rather than read back out of an erased record. Asked before the
            // arms below, whose class fallback erases.
            if mapper_arg.is_none()
                && let span = self.span(call.span.start, call.span.end)
                && let Some(elements) = self.typed_array_spread_list(source, span, body)
            {
                return Ok(Some(elements));
            }
            if mapper_arg.is_none()
                && let Some(list_ty) = self.sync_generator_list_ty(source_ty)
            {
                let span = self.span(call.span.start, call.span.end);
                return Ok(Some(Self::generator_items_expr(source, list_ty, span, body)));
            }
            let list_ty = match self.ctx.krate.types.get(source_ty).cloned() {
                Some(Type::List(_)) if mapper_arg.is_none() => return Ok(Some(source)),
                Some(Type::List(item_ty)) => self.ctx.krate.types.intern(Type::List(item_ty)),
                Some(Type::Set(item_ty)) => {
                    let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::SetProjection {
                            op: SetProjectionOp::Values,
                            set: source,
                        },
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                }
                Some(Type::Dict(key_ty, _)) => {
                    let ty = self.ctx.krate.types.intern(Type::List(key_ty));
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::DictProjection {
                            op: DictProjectionOp::Keys,
                            dict: source,
                        },
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                }
                _ => {
                    let unknown_ty =
                        self.erase_with(ErasureReason::UntypedBuiltin, "Array.from(iterable)");
                    self.ctx.krate.types.intern(Type::List(unknown_ty))
                }
            };
            if let Some(mapper_arg) = mapper_arg {
                let _ = self.argument(mapper_arg, body)?;
            }
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: source,
                    target: list_ty,
                },
                ty: list_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let mut length = self.array_from_length_argument(source_arg, body)?;
        let length_ty = Self::expr_ty(body, length);
        if !matches!(
            self.ctx.krate.types.get(length_ty),
            Some(Type::Int | Type::Float)
        ) {
            // Accept numeric-like, optional-numeric, and erased length surfaces
            // (e.g. `{ length: n }` where `n` is `number | undefined`), coercing
            // them to a JS number so the allocation count is concrete.
            if self.is_numeric_like_type(length_ty)
                || self.optional_numeric_surface(length_ty)
                || self.erased_or_union_surface(length_ty)
            {
                let float_ty = self.ctx.krate.types.intern(Type::Float);
                length = body.push_expr(Expr {
                    kind: ExprKind::PrimitiveCast {
                        op: PrimitiveCastOp::ToJsNumber,
                        operand: length,
                    },
                    ty: float_ty,
                    span: self.span(source_arg.span().start, source_arg.span().end),
                });
            } else {
                return Err(SmeltError::unsupported(
                    self.span(source_arg.span().start, source_arg.span().end),
                    "Array.from({ length }, mapper) length must be numeric",
                ));
            }
        }
        let Some(mapper_arg) = mapper_arg else {
            let item_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Array.from of a length object");
            let ty = self.ctx.krate.types.intern(Type::List(item_ty));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ListFromLength { length },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        };
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Array.from mapper element");
        let index_ty = self.ctx.krate.types.intern(Type::Float);
        let callback = self.callback_argument_with_return_hint(
            mapper_arg,
            &[unknown_ty, index_ty],
            index_ty,
            "Array.from mapper",
            body,
        )?;
        let ty = self.ctx.krate.types.intern(Type::List(callback.return_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListFromLengthMap {
                length,
                callback: callback.expr,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// The `List<yield>` type an iteration of a SYNCHRONOUS generator produces.
    ///
    /// `Array.from(gen)` and `[...gen]` drain the generator's iterator protocol,
    /// so their element type is the generator's own `yield` type; `None` for
    /// every other source (an async generator is not sync-iterable).
    pub(in crate::lowering) fn sync_generator_list_ty(
        &mut self,
        source_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let Some(Type::Generator {
            is_async: false,
            yield_ty,
            ..
        }) = self.ctx.krate.types.get(source_ty).cloned()
        else {
            return None;
        };
        Some(self.ctx.krate.types.intern(Type::List(yield_ty)))
    }

    /// Drain a synchronous generator into a fresh list of its yielded values.
    ///
    /// Lowered as a checked `UnknownCast` from `Generator<Y, ..>` to `List<Y>`:
    /// the emitter recognises the typed generator source and collects it through
    /// the prelude's `SmeltGenerator::collect_yields`, so the items keep their
    /// static `Y` and never cross the erased iterator protocol.
    pub(in crate::lowering) fn generator_items_expr(
        generator: smelt_hir::ExprId,
        list_ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: generator,
                target: list_ty,
            },
            ty: list_ty,
            span,
        })
    }

    /// Extract the numeric `length` expression from `Array.from`'s source argument.
    pub(in crate::lowering) fn array_from_length_argument(
        &mut self,
        source_arg: &Argument<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Argument::ObjectExpression(object) = source_arg else {
            return Err(SmeltError::unsupported(
                self.span(source_arg.span().start, source_arg.span().end),
                "Array.from currently supports object sources shaped as { length }",
            ));
        };
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(source_arg.span().start, source_arg.span().end),
                    "Array.from({ length }, mapper) does not support spread properties",
                ));
            };
            let key_text = match &property.key {
                PropertyKey::StaticIdentifier(identifier) => {
                    Cow::Borrowed(identifier.name.as_str())
                }
                PropertyKey::StringLiteral(literal) => js_str_text(literal.value),
                _ => continue,
            };
            if key_text == "length" {
                return self.object_property_value_expr(property, body, None);
            }
        }
        Err(SmeltError::unsupported(
            self.span(source_arg.span().start, source_arg.span().end),
            "Array.from object source must provide a length property",
        ))
    }

    /// Lower `new Array<T>(length)` through the shared `Array(...)` construction,
    /// which emits `ListFromLength` with item metadata.
    ///
    /// JavaScript creates a sparse array here; Smelt models the later indexed
    /// writes and only needs the list container type at construction time.
    pub(in crate::lowering) fn array_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.lower_array_construction(
            &new_expr.arguments,
            new_expr.type_arguments.as_deref(),
            new_expr.span.start,
            new_expr.span.end,
            body,
        )
    }

    /// Lower a bare `Array(length)` / `Array(element)` call as a value-returning
    /// constructor expression.
    ///
    /// In ECMAScript the `Array` global produces an identical array whether
    /// invoked as `Array(...)` or `new Array(...)` (the spec routes both through
    /// the same constructor behavior). Reusing the `new Array` lowering keeps the
    /// two spellings in lockstep instead of special-casing the call form. The
    /// es-toolkit corpus relies heavily on `Array(n)` to preallocate a list that
    /// is then filled by indexed writes.
    pub(in crate::lowering) fn array_constructor_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "Array" {
            return Ok(None);
        }
        self.lower_array_construction(
            &call.arguments,
            call.type_arguments.as_deref(),
            call.span.start,
            call.span.end,
            body,
        )
        .map(Some)
    }

    /// Shared core for `Array(...)` and `new Array(...)` construction.
    ///
    /// ECMAScript gives the two spellings identical behaviour and splits on the
    /// ARGUMENT LIST, not on the callee: exactly one numeric argument is a
    /// LENGTH, and every other argument list is an ELEMENT list. So `Array(3)`
    /// is a length-3 array of holes, while `Array('a')` is `['a']`,
    /// `Array(1, 2, 3)` is `[1, 2, 3]`, and `Array()` is `[]`.
    ///
    /// The length form lowers to `ListFromLength`, which allocates `n` slots
    /// holding the element type's missing value — the very value an
    /// out-of-range read of the same list answers. Lowering it to an empty list
    /// instead (the previous behaviour) lost the length: every consumer that
    /// drives a loop off `.length` (`fill`, `zip`, `zipWith`, `unzip`) then ran
    /// zero iterations and returned an empty array.
    ///
    /// A single array-literal argument (`Array([1, 2])`) keeps its established
    /// literal lowering. An optional type argument supplies the element type for
    /// either form.
    pub(in crate::lowering) fn lower_array_construction(
        &mut self,
        arguments: &[Argument<'_>],
        type_arguments: Option<&oxc::ast::ast::TSTypeParameterInstantiation<'_>>,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let [Argument::ArrayExpression(array)] = arguments {
            return self.array_expression(array, body, None);
        }
        let annotated_item_ty = if let Some(type_args) = type_arguments {
            let [item] = type_args.params.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(start, end),
                    "Array(...) supports exactly one type argument",
                ));
            };
            Some(self.ts_type_to_hir(item)?)
        } else {
            None
        };
        if let [length_arg] = arguments {
            let length = self.argument(length_arg, body)?;
            // Accept any numeric-like type plus the erased / optional-numeric
            // surfaces that flow from JS `number | undefined` parameters. A
            // clearly non-numeric single argument is an element, not a length.
            let length_ty = Self::expr_ty(body, length);
            let is_length = self.is_numeric_like_type(length_ty)
                || matches!(
                    self.ctx.krate.types.get(length_ty),
                    Some(Type::Int | Type::Float)
                )
                || self.optional_numeric_surface(length_ty)
                || self.erased_or_union_surface(length_ty);
            if is_length {
                // The allocation count must reach the emitter as a JS number;
                // optional and erased numeric surfaces are cast the same way
                // `Array.from({ length })` casts them.
                let length = if matches!(
                    self.ctx.krate.types.get(length_ty),
                    Some(Type::Int | Type::Float)
                ) {
                    length
                } else {
                    let float_ty = self.ctx.krate.types.intern(Type::Float);
                    body.push_expr(Expr {
                        kind: ExprKind::PrimitiveCast {
                            op: PrimitiveCastOp::ToJsNumber,
                            operand: length,
                        },
                        ty: float_ty,
                        span: self.span(start, end),
                    })
                };
                let item_ty = annotated_item_ty
                    .unwrap_or_else(|| self.erase_with(ErasureReason::MissingAnnotation, "Array(n)"));
                let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::ListFromLength { length },
                    ty,
                    span: self.span(start, end),
                }));
            }
            let item_ty = annotated_item_ty.unwrap_or(length_ty);
            let ty = self.ctx.krate.types.intern(Type::List(item_ty));
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ListLit(vec![length]),
                ty,
                span: self.span(start, end),
            }));
        }
        let mut items = Vec::with_capacity(arguments.len());
        for argument in arguments {
            if matches!(argument, Argument::SpreadElement(_)) {
                return Err(SmeltError::unsupported(
                    self.span(start, end),
                    "Array(...) does not support spread arguments",
                ));
            }
            items.push(self.argument(argument, body)?);
        }
        let item_ty = annotated_item_ty.unwrap_or_else(|| {
            if items.is_empty() {
                self.erase_with(ErasureReason::MissingAnnotation, "Array()")
            } else {
                self.array_literal_item_type(&items, body)
            }
        });
        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
        Ok(body.push_expr(Expr {
            kind: ExprKind::ListLit(items),
            ty,
            span: self.span(start, end),
        }))
    }

    /// Let a length-only list allocation adopt the contextual list type.
    ///
    /// `Array(n)` names no element type of its own, so it lowers as
    /// `list[unknown]`. When the value flows into a position that already knows
    /// the list type (`const rows: number[] = Array(n)`), adopting that type
    /// keeps the allocation concrete: its holes are then built as the element
    /// type's own missing value (`0.0`) instead of erased `SmeltUnknown`
    /// holes a later coercion has to map back element by element — which is
    /// both an erasure round-trip and a disagreement with what an out-of-range
    /// read of the same list answers. Array literals already take contextual
    /// types this way; a length-only allocation is the same shape with the
    /// elements left implicit.
    ///
    /// Only an untyped (`list[unknown]`) allocation adopts, and only from a list
    /// hint; every other expression and hint is returned untouched.
    pub(in crate::lowering) fn adopt_contextual_list_allocation_type(
        &mut self,
        value: smelt_hir::ExprId,
        type_hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let Some(hint) = type_hint else {
            return value;
        };
        if !matches!(self.ctx.krate.types.get(hint), Some(Type::List(_))) {
            return value;
        }
        let unknown_list_ty = {
            // not a new erasure: only compared against below
            let unknown_ty = self.unknown_type();
            self.ctx.krate.types.intern(Type::List(unknown_ty))
        };
        if let Some(expr) = body.exprs.get_mut(usize::try_from(value.0).unwrap_or(usize::MAX))
            && matches!(expr.kind, ExprKind::ListFromLength { .. })
            && expr.ty == unknown_list_ty
        {
            expr.ty = hint;
        }
        value
    }
}
