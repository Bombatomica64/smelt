//! Array methods: mutation, sorting, slicing and the modern copying forms.

use crate::lowering::erasure::NotNewErasure;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::{ModuleBuilder, SmeltError};
use oxc::ast::ast::{Argument, CallExpression, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, ErasureReason, Expr, ExprKind, ListCallbackOp, ListProjectionOp, ListSpliceItem,
    Stmt, Type,
};

impl ModuleBuilder<'_> {
    /// Lower direct TypeScript `Array.prototype.shift` calls.
    pub(in crate::lowering) fn list_shift_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "shift" {
            return Ok(None);
        }
        // A utility namespace can export a free function named
        // `shift(list, amount)`. Defer it before applying the zero-argument
        // contract of `Array.prototype.shift`.
        if self.imported_utility_object(&member.object) {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array shift requires no arguments",
            ));
        }
        let list = self.present_list_receiver(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let ty = self.ctx.krate.types.intern(Type::Optional(element_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListShift { list },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.pop` calls.
    pub(in crate::lowering) fn list_pop_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "pop" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array pop requires no arguments",
            ));
        }
        let list = self.present_list_receiver(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let ty = self.ctx.krate.types.intern(Type::Optional(*element_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListPop { list, raises_when_empty: false },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.reverse` calls.
    pub(in crate::lowering) fn list_reverse_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "reverse" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array reverse requires no arguments",
            ));
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(list_ty), Some(Type::List(_))) {
            return Ok(None);
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListReverse { list },
            ty: list_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.sort` calls with an optional comparator.
    pub(in crate::lowering) fn list_sort_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "sort" {
            return Ok(None);
        }
        // A star-import/object utility namespace may export a free function
        // named `sort(list, getter, descending)`. It is not an invocation of
        // `Array.prototype.sort`, so defer before validating instance-method
        // arity or lowering a comparator.
        if self.imported_utility_object(&member.object) {
            return Ok(None);
        }
        let comparator_argument = match call.arguments.as_slice() {
            [] => None,
            [argument] => Some(argument),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array sort requires at most one comparator argument",
                ));
            }
        };
        let list = self.present_list_receiver(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        // ECMA-262 `Array.prototype.sort` step 1: passing `undefined` as the
        // comparator is exactly the same as passing nothing at all — elements
        // are ordered by their `ToString` coercion. An *optional* comparator
        // (`compare?: (a, b) => number`, or any variable of an optional
        // function type) therefore must not be lowered as an ordinary erased
        // callback: the erased wrapper turns "the callback is absent" into an
        // `undefined` RESULT, the numeric coercion reads that as `0`, every
        // comparison answers `Equal`, and the sort silently becomes a no-op.
        // Keep the optional callable itself as the comparator operand so the
        // emitter can branch on its presence and fall back to the default
        // ordering, the way a hand-written `match cmp { Some(f) => .., None =>
        // .. }` would.
        if let Some(argument) = comparator_argument
            && let Some(comparator) = self.optional_sort_comparator(argument, body)?
        {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ListSort {
                    list,
                    comparator: Some(comparator),
                    key: None,
                    reverse: false,
                },
                ty: list_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let comparator = if let Some(argument) = comparator_argument {
            let number_ty = self.ctx.krate.types.intern(Type::Float);
            // Use the body-fallback variant so a comparator whose body uses
            // statement forms the side-effect-free callback IR cannot represent
            // (e.g. a `for` loop scanning sort criteria) retries through full
            // closure-body lowering instead of failing the whole file.
            let callback = self.callback_argument_with_return_hint(
                argument,
                &[element_ty, element_ty],
                number_ty,
                "array sort",
                body,
            )?;
            // A named/optional comparator lowered through an erased wrapper
            // returns `unknown`; the emitter coerces the comparison result
            // numerically, so only reject genuinely non-numeric returns.
            if callback.return_ty != number_ty
                && !matches!(
                    self.ctx.krate.types.get(callback.return_ty),
                    Some(Type::Unknown | Type::TypeParam { .. })
                )
                && !self.erased_or_union_surface(callback.return_ty)
            {
                self.require_callback_ty(callback.return_ty, number_ty, call, "array sort")?;
            }
            Some(callback.expr)
        } else {
            None
        };
        if comparator.is_some() {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ListSort {
                    list,
                    comparator,
                    key: None,
                    reverse: false,
                },
                ty: list_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        // Comparator-less `sort()` uses JavaScript's default ordering: elements
        // are compared by their `ToString` coercion. Scalars compare directly;
        // erased (`unknown`) and union elements go through the emitter's
        // string-coercion comparison, so mixed-value lists (`values(object)`,
        // `Array<T[keyof T]>` results) sort with real JS semantics instead of
        // being rejected. A leaked/unbound type parameter (e.g. the `T[keyof T]`
        // element of a generic `values<T>(...)` result reached through an erased
        // call) renders as `SmeltUnknown` at codegen (a non-scoped `TypeParam`
        // erases to `SmeltUnknown`), so it sorts through the same string
        // coercion; this mirrors the erased-surface treatment already applied to
        // the sort comparator return type. Structured concrete shapes (nested
        // lists, records) stay unsupported because their JS `ToString` (e.g.
        // `1,2` for arrays) is not modeled yet.
        if !matches!(
            self.ctx.krate.types.get(element_ty),
            Some(
                Type::Bool
                    | Type::Int
                    | Type::Float
                    | Type::String
                    | Type::Unknown
                    | Type::Union(_)
                    | Type::TypeParam { .. }
            )
        ) {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "array sort supports boolean, number, and string arrays for now",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListSort {
                list,
                comparator,
                key: None,
                reverse: false,
            },
            ty: list_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Recognize a `sort` argument that is a variable of *optional* function
    /// type and lower it as-is.
    ///
    /// Returns `None` (so the caller falls back to the ordinary callback
    /// lowering) unless the argument is an identifier bound to a local whose
    /// type is `Optional(Function)`. Only that shape can be inspected without
    /// lowering the argument expression, and lowering it speculatively would
    /// duplicate the side effects of a non-identifier argument such as
    /// `xs.sort(makeComparator())`.
    ///
    /// The returned expression keeps its `Optional(Function)` type all the way
    /// into MIR; `emitter::list_mutation` branches on the `Option` at runtime.
    fn optional_sort_comparator(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Argument::Identifier(identifier) = argument else {
            return Ok(None);
        };
        let Some(local) = self.scope.lookup(identifier.name.as_str()) else {
            return Ok(None);
        };
        let local_ty = Self::local_ty(body, local);
        let Some(Type::Optional(inner)) = self.ctx.krate.types.get(local_ty).cloned() else {
            return Ok(None);
        };
        let Some(Type::Function(function)) = self.ctx.krate.types.get(inner).cloned() else {
            return Ok(None);
        };
        // A comparator takes two elements and returns a number. Anything else
        // is not `Array.prototype.sort`'s comparator contract, so leave it to
        // the ordinary callback path (which reports a precise error).
        if function.params.len() != 2 {
            return Ok(None);
        }
        let expr = self.identifier_expression(
            identifier.name.as_str(),
            identifier.span.start,
            identifier.span.end,
            body,
        )?;
        Ok(Some(expr))
    }

    /// Lower the receiver of an in-place array method (`push`, `pop`,
    /// `shift`, `unshift`, `reverse`, `sort`), asserting an OPTIONAL array
    /// present.
    ///
    /// Under strict null checks `tsc` rejects these calls on a possibly-absent
    /// receiver, so an `Optional<List<T>>` receiver here is one a guard
    /// already proved present (`if (!this.#routes) throw ..` before
    /// `this.#routes.push(..)`) whose narrowing did not reach the member path.
    /// The `TypeAssert` keeps the receiver a mutation place: MIR writes a
    /// `TypeAssert` over a field back like the field itself.
    fn present_list_receiver(
        &mut self,
        object: &Expression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let list = self.expression(object, body)?;
        let list_ty = Self::expr_ty(body, list);
        // Only an array that is itself declared optional: a member read whose
        // optionality comes from an optional BASE (`child.#values` with
        // `child: Node | undefined`) keeps the path it already had, which
        // writes the array back through the base.
        let optional_from_base = body.expr_at(list)
            .is_some_and(|expr| match &expr.kind {
                ExprKind::Field { receiver, .. } | ExprKind::OptionalField { receiver, .. } => {
                    matches!(
                        self.ctx.krate.types.get(Self::expr_ty(body, *receiver)),
                        Some(Type::Optional(_))
                    )
                }
                _ => false,
            });
        if !optional_from_base
            && let Some(Type::Optional(inner)) = self.ctx.krate.types.get(list_ty).cloned()
            && matches!(self.ctx.krate.types.get(inner), Some(Type::List(_)))
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: list },
                ty: inner,
                span: self.span(object.span().start, object.span().end),
            }));
        }
        Ok(list)
    }

    /// Lower direct TypeScript `Array.prototype.push` calls.
    pub(in crate::lowering) fn list_push_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "push" {
            return Ok(None);
        }
        if call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array push currently requires at least one item argument",
            ));
        }
        let list = self.present_list_receiver(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        // `Array.prototype.push` accepts any number of arguments and appends each
        // in order, returning the new length. Each spread argument extends the
        // list, each scalar pushes one element; the value of the call is the last
        // mutation's `Float` length result.
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        let mut result = None;
        for item_argument in &call.arguments {
            // Every push before the last is a sequenced side effect; the call's
            // value is the final push's `Float` length result.
            if let Some(previous) = result.take() {
                body.push_stmt(Stmt::Expr(previous));
            }
            let pushed = if let Argument::SpreadElement(spread) = item_argument {
                let other = self.expression(&spread.argument, body)?;
                let other_ty = Self::expr_ty(body, other);
                let other = if matches!(self.ctx.krate.types.get(other_ty), Some(Type::List(_))) {
                    other
                } else {
                    let list_value_ty = self.ctx.krate.types.intern(Type::List(element_ty));
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: other },
                        ty: list_value_ty,
                        span: self.span(spread.span.start, spread.span.end),
                    })
                };
                body.push_expr(Expr {
                    kind: ExprKind::ListExtend { list, other },
                    ty: number_ty,
                    span: self.span(call.span.start, call.span.end),
                })
            } else {
                // Lower the pushed value with the array's element type as a
                // contextual hint. This lets a bare literal argument adopt the
                // element's concrete shape at construction time instead of
                // defaulting to an erased surface — e.g. `result.push([value,
                // key])` into an `Array<[number, string]>` lowers `[value, key]`
                // as a `(Float, String)` tuple literal rather than a
                // `List<Unknown>` that could never be re-typed into the tuple.
                let item = self.list_element_argument(item_argument, element_ty, "push", body)?;
                body.push_expr(Expr {
                    kind: ExprKind::ListPush { list, item },
                    ty: number_ty,
                    span: self.span(call.span.start, call.span.end),
                })
            };
            result = Some(pushed);
        }
        Ok(result)
    }

    /// Lower modern TypeScript array APIs that materialize lists directly.
    pub(in crate::lowering) fn modern_array_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(
            method,
            "splice"
                | "toSpliced"
                | "fill"
                | "copyWithin"
                | "with"
                | "flat"
                | "flatMap"
                | "toSorted"
                | "toReversed"
                | "findLast"
                | "findLastIndex"
                | "keys"
                | "values"
                | "entries"
        ) {
            return Ok(None);
        }
        let list = self.present_list_receiver(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let span = self.span(call.span.start, call.span.end);
        match method {
            "splice" | "toSpliced" => {
                let [start_arg, rest @ ..] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array splice requires a start argument",
                    ));
                };
                let start = self.slice_index_argument(start_arg, body)?;
                let delete_count = rest
                    .first()
                    .map(|argument| self.slice_index_argument(argument, body))
                    .transpose()?;
                let item_args = if delete_count.is_some() {
                    rest.get(1..).unwrap_or(&[])
                } else {
                    rest
                };
                let mut items = Vec::with_capacity(item_args.len());
                for argument in item_args {
                    let (item, spread) = match argument {
                        Argument::SpreadElement(spread) => {
                            let item = self.expression(&spread.argument, body)?;
                            if Self::expr_ty(body, item) != list_ty {
                                return Err(SmeltError::unsupported(
                                    self.span(argument.span().start, argument.span().end),
                                    "array splice replacement items must match the array element type",
                                ));
                            }
                            (item, true)
                        }
                        _ => (
                            self.list_element_argument(argument, element_ty, "splice", body)?,
                            false,
                        ),
                    };
                    items.push(ListSpliceItem {
                        value: item,
                        spread,
                    });
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListSplice {
                        list,
                        start,
                        delete_count,
                        items,
                        mutate: method == "splice",
                    },
                    ty: list_ty,
                    span,
                })))
            }
            "fill" => {
                let [value_arg, rest @ ..] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array fill requires a value argument",
                    ));
                };
                if rest.len() > 2 {
                    return Err(SmeltError::unsupported(
                        span,
                        "array fill supports value, start, and end arguments",
                    ));
                }
                let mut value = self.argument(value_arg, body)?;
                let value_ty = Self::expr_ty(body, value);
                if value_ty != element_ty {
                    // Coerce a fill value that is assignment-compatible with the
                    // element type (numeric widening, erased/union surfaces, type
                    // params) via a type assertion, instead of requiring an exact
                    // type match.
                    let compatible = self.array_item_type_compatible(value_ty, element_ty)
                        || self.ctx.krate.types.get(element_ty) == Some(&Type::Unknown)
                        || self.type_contains_unknown(value_ty)
                        || self.type_contains_unknown(element_ty)
                        || self.numeric_type_compatible(element_ty, value_ty)
                        || self.erased_or_union_surface(value_ty)
                        || self.erased_or_union_surface(element_ty)
                        || matches!(
                            (
                                self.ctx.krate.types.get(element_ty),
                                self.ctx.krate.types.get(value_ty)
                            ),
                            (Some(Type::TypeParam { .. }), _) | (_, Some(Type::TypeParam { .. }))
                        );
                    if compatible {
                        value = body.push_expr(Expr {
                            kind: ExprKind::TypeAssert { value },
                            ty: element_ty,
                            span: self.span(value_arg.span().start, value_arg.span().end),
                        });
                    } else {
                        return Err(SmeltError::unsupported(
                            self.span(value_arg.span().start, value_arg.span().end),
                            "array fill value must match the array element type",
                        ));
                    }
                }
                let start = rest
                    .first()
                    .map(|argument| self.slice_index_argument(argument, body))
                    .transpose()?;
                let end = rest
                    .get(1)
                    .map(|argument| self.slice_index_argument(argument, body))
                    .transpose()?;
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListFill {
                        list,
                        value,
                        start,
                        end,
                    },
                    ty: list_ty,
                    span,
                })))
            }
            "copyWithin" => {
                let [target_arg, start_arg, rest @ ..] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array copyWithin requires target and start arguments",
                    ));
                };
                if rest.len() > 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "array copyWithin supports target, start, and end arguments",
                    ));
                }
                let target = self.slice_index_argument(target_arg, body)?;
                let start = self.slice_index_argument(start_arg, body)?;
                let end = rest
                    .first()
                    .map(|argument| self.slice_index_argument(argument, body))
                    .transpose()?;
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListCopyWithin {
                        list,
                        target,
                        start,
                        end,
                    },
                    ty: list_ty,
                    span,
                })))
            }
            "with" => {
                let [index_arg, value_arg] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array with requires index and value arguments",
                    ));
                };
                let index = self.slice_index_argument(index_arg, body)?;
                let value = self.argument(value_arg, body)?;
                if Self::expr_ty(body, value) != element_ty {
                    return Err(SmeltError::unsupported(
                        self.span(value_arg.span().start, value_arg.span().end),
                        "array with value must match the array element type",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListWith { list, index, value },
                    ty: list_ty,
                    span,
                })))
            }
            "flat" => {
                let list = self.narrowed_list_receiver(list, body);
                if call.arguments.len() > 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "array flat accepts at most one depth argument",
                    ));
                }
                let depth = call
                    .arguments
                    .first()
                    .map(|argument| self.argument(argument, body))
                    .transpose()?;
                let flat_item_ty = match self
                    .ctx
                    .krate
                    .types
                    .get(self.type_param_constraint_or_self(element_ty))
                {
                    Some(Type::List(flat_item_ty)) => *flat_item_ty,
                    Some(Type::Tuple(items)) => self.flattened_tuple_item_type(items.clone()),
                    // not a new erasure: the element is already erased
                    Some(Type::Unknown) => self.unknown_type(NotNewErasure::ErasedUpstream),
                    // `T[]`'s flattened item is the type-level `FlatArray<T, D>`.
                    Some(Type::TypeParam { .. }) => {
                        self.erase_with(ErasureReason::TypeLevel, "FlatArray")
                    }
                    // A union element with array arms (`number | number[]`)
                    // flattens arm by arm; see `lowering::union_flat`.
                    Some(Type::Union(_)) => match self.union_flat_item_type(element_ty, call.arguments.first()) {
                        Some(item_ty) => item_ty,
                        None => return Ok(None),
                    },
                    // An item that is not an array stays as it is.
                    _ => element_ty,
                };
                let ty = self.ctx.krate.types.intern(Type::List(flat_item_ty));
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListFlat { list, depth },
                    ty,
                    span,
                })))
            }
            "flatMap" => {
                let list = self.narrowed_list_receiver(list, body);
                let [callback_arg] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array flatMap requires one callback argument",
                    ));
                };
                let index_ty = self.ctx.krate.types.intern(Type::Float);
                let callback = self.callback_argument(
                    callback_arg,
                    &[element_ty, index_ty, list_ty],
                    "array flatMap",
                    body,
                )?;
                let flat_item_ty = self
                    .flat_map_callback_item_type(callback.return_ty)
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(callback_arg.span().start, callback_arg.span().end),
                            "array flatMap callback must return an array or flattened item",
                        )
                    })?;
                let ty = self.ctx.krate.types.intern(Type::List(flat_item_ty));
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListCallback {
                        op: ListCallbackOp::FlatMap,
                        list,
                        callback: callback.expr,
                    },
                    ty,
                    span,
                })))
            }
            "toSorted" => {
                let comparator_argument = match call.arguments.as_slice() {
                    [] => None,
                    [argument] => Some(argument),
                    _ => {
                        return Err(SmeltError::unsupported(
                            span,
                            "array toSorted requires at most one comparator argument",
                        ));
                    }
                };
                let comparator = if let Some(argument) = comparator_argument {
                    let callback = self.callback_argument(
                        argument,
                        &[element_ty, element_ty],
                        "array toSorted",
                        body,
                    )?;
                    let number_ty = self.ctx.krate.types.intern(Type::Float);
                    self.require_callback_ty(
                        callback.return_ty,
                        number_ty,
                        call,
                        "array toSorted",
                    )?;
                    Some(callback.expr)
                } else {
                    None
                };
                let sorted = body.push_expr(Expr {
                    kind: ExprKind::ListCopy { list },
                    ty: list_ty,
                    span,
                });
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListSort {
                        list: sorted,
                        comparator,
                        key: None,
                        reverse: false,
                    },
                    ty: list_ty,
                    span,
                })))
            }
            "toReversed" => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        span,
                        "array toReversed requires no arguments",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListReversed { list },
                    ty: list_ty,
                    span,
                })))
            }
            "findLast" | "findLastIndex" => {
                let [callback_arg] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "array findLast/findLastIndex requires one callback argument",
                    ));
                };
                let index_ty = self.ctx.krate.types.intern(Type::Float);
                // findLast/findLastIndex are truthiness predicates like
                // find/filter: a named callback returning an erased/`any`
                // value is coerced to bool by the truthy wrapper instead of
                // being rejected for not returning `boolean` literally.
                let callback = self.truthy_callback_argument_with_return_hint(
                    callback_arg,
                    &[element_ty, index_ty, list_ty],
                    "array findLast/findLastIndex",
                    body,
                )?;
                let op = if method == "findLast" {
                    ListCallbackOp::FindLast
                } else {
                    ListCallbackOp::FindLastIndex
                };
                let ty = if method == "findLast" {
                    self.ctx.krate.types.intern(Type::Optional(element_ty))
                } else {
                    index_ty
                };
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListCallback {
                        op,
                        list,
                        callback: callback.expr,
                    },
                    ty,
                    span,
                })))
            }
            "keys" | "values" | "entries" => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        span,
                        "array projection methods require no arguments",
                    ));
                }
                let op = match method {
                    "keys" => ListProjectionOp::Keys,
                    "values" => ListProjectionOp::Values,
                    "entries" => ListProjectionOp::Entries,
                    _ => return Ok(None),
                };
                let ty = match op {
                    ListProjectionOp::Keys => {
                        let int_ty = self.ctx.krate.types.intern(Type::Int);
                        self.ctx.krate.types.intern(Type::List(int_ty))
                    }
                    ListProjectionOp::Values => list_ty,
                    ListProjectionOp::Entries => {
                        let int_ty = self.ctx.krate.types.intern(Type::Int);
                        let tuple_ty = self
                            .ctx
                            .krate
                            .types
                            .intern(Type::Tuple(vec![int_ty, element_ty]));
                        self.ctx.krate.types.intern(Type::List(tuple_ty))
                    }
                };
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListProjection { op, list },
                    ty,
                    span,
                })))
            }
            _ => Ok(None),
        }
    }

    /// Infer the output item type for JavaScript `Array.prototype.flatMap`.
    fn flat_map_callback_item_type(
        &mut self,
        return_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(return_ty))
            .cloned()
        {
            Some(Type::List(item_ty)) => Some(item_ty),
            Some(Type::Tuple(items)) => Some(self.flattened_tuple_item_type(items)),
            Some(Type::Union(items)) => {
                let mut item_tys = Vec::new();
                for item in items {
                    match self
                        .ctx
                        .krate
                        .types
                        .get(self.type_param_constraint_or_self(item))
                    {
                        Some(Type::List(list_item)) if !item_tys.contains(list_item) => {
                            item_tys.push(*list_item);
                        }
                        Some(Type::Tuple(tuple_items)) => {
                            let tuple_item = self.flattened_tuple_item_type(tuple_items.clone());
                            if !item_tys.contains(&tuple_item) {
                                item_tys.push(tuple_item);
                            }
                        }
                        Some(Type::List(_) | Type::Never) => {}
                        _ if !item_tys.contains(&item) => item_tys.push(item),
                        _ => {}
                    }
                }
                match item_tys.as_slice() {
                    [] => None,
                    [single] => Some(*single),
                    _ => Some(self.ctx.krate.types.intern(Type::Union(item_tys))),
                }
            }
            // not a new erasure: the callback already returns an erased value
            Some(Type::Unknown) => Some(self.unknown_type(NotNewErasure::ErasedUpstream)),
            // A generic callback result flattens through the type-level
            // `FlatArray<U, 1>`, which Smelt does not evaluate.
            Some(Type::TypeParam { .. }) => {
                Some(self.erase_with(ErasureReason::TypeLevel, "FlatArray"))
            }
            Some(_) | None => None,
        }
    }

    /// Collapse a tuple used as an array element into the item type produced by flattening it.
    pub(in crate::lowering) fn flattened_tuple_item_type(
        &mut self,
        items: Vec<smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        match items.as_slice() {
            [] => self.ctx.krate.types.intern(Type::Never),
            [single] => *single,
            _ => self.ctx.krate.types.intern(Type::Union(items)),
        }
    }

    /// Lower one value an array method inserts (`push`, `unshift`, a `splice`
    /// replacement item) at the array's element type.
    ///
    /// The element type is the argument's contextual type, exactly as an
    /// assignment target's type is: `users.unshift({ username, password })`
    /// builds the declared element record, and `conditions.unshift((c) => ..)`
    /// types the arrow's parameter from the element's callable type. Without
    /// the hint the literal lowered to an anonymous shape that could never
    /// match. The value must then be assignable to the element type; an
    /// erased or union surface on either side is reconciled with an explicit
    /// assertion, and anything else is a named blocker.
    pub(in crate::lowering) fn list_element_argument(
        &mut self,
        argument: &Argument<'_>,
        element_ty: smelt_hir::TypeId,
        method: &str,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let item = self.argument_with_hint(argument, body, Some(element_ty))?;
        let item_ty = Self::expr_ty(body, item);
        let compatible = self.array_item_type_compatible(item_ty, element_ty)
            || self.ctx.krate.types.get(element_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(item_ty)
            || self.type_contains_unknown(element_ty)
            || self.numeric_type_compatible(element_ty, item_ty);
        if compatible {
            return Ok(item);
        }
        if self.erased_or_union_surface(item_ty) || self.erased_or_union_surface(element_ty) {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: item },
                ty: element_ty,
                span: self.span(argument.span().start, argument.span().end),
            }));
        }
        // The per-method wording, which diagnostics are matched on.
        let message = match method {
            "unshift" => "array unshift arguments must match the array element type",
            "splice" => "array splice replacement items must match the array element type",
            _ => "array push argument must match the array element type",
        };
        Err(SmeltError::unsupported(
            self.span(argument.span().start, argument.span().end),
            message,
        ))
    }

    /// Lower direct TypeScript `Array.prototype.unshift` calls.
    pub(in crate::lowering) fn list_unshift_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "unshift" {
            return Ok(None);
        }
        let Expression::Identifier(_) = &member.object else {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "array unshift currently requires a local array receiver",
            ));
        };
        let list = self.present_list_receiver(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let mut items = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            items.push(self.list_element_argument(argument, element_ty, "unshift", body)?);
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListUnshift { list, items },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript collection and string slicing calls.
    ///
    /// `subarray` joins `slice` here: `TypedArray.prototype.subarray` and Node's
    /// `Buffer.prototype.subarray` take the same `(begin, end)` index arguments and
    /// differ only in sharing the source's memory instead of copying it. Smelt
    /// models byte buffers by value, so a copy is the faithful lowering — and it is
    /// the *distinct object* the `isBuffer(v) ? v.subarray() : ...` clone idiom is
    /// after. `subarray` is not a string method, so a string receiver declines.
    pub(in crate::lowering) fn collection_slice_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "slice" | "substring" | "substr" | "subarray") {
            return Ok(None);
        }
        // `slice`-shaped methods: same index arguments, same collection dispatch.
        let slice_like = matches!(method, "slice" | "subarray");
        if call.arguments.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "slice/subarray/substring/substr currently support only omitted, start, and end arguments",
            ));
        }
        let mut operand = self.expression(&member.object, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        let mut effective_operand_ty = self.type_param_constraint_or_self(operand_ty);
        // `slice` is defined on arrays and typed arrays / array buffers as well
        // as strings. When the receiver is erased or otherwise not a plain
        // string (a generic `T`, `unknown`, or a union that mixes arrays with
        // buffers), route to a `ListSlice` whose codegen runtime-dispatches on
        // the value tag instead of coercing the value to a string — a string
        // coercion would ToString an array receiver to "[object Object]" and
        // then char-slice that. `substring`/`substr` stay string-only.
        let slice_erased_receiver = slice_like
            && (matches!(
                self.ctx.krate.types.get(effective_operand_ty),
                Some(Type::List(_) | Type::Unknown | Type::TypeParam { .. })
            ) || self.type_contains_unknown(effective_operand_ty)
                || matches!(
                    self.ctx.krate.types.get(effective_operand_ty),
                    Some(Type::Union(items)) if items.iter().any(|item| matches!(
                        self.ctx.krate.types.get(*item),
                        Some(Type::List(_) | Type::Unknown | Type::TypeParam { .. })
                    ))
                ));
        // `subarray` has no string form, so a receiver that is not a list or an
        // erased value is not ours; decline rather than string-coercing it.
        if method == "subarray" && !slice_erased_receiver {
            return Ok(None);
        }
        if !slice_erased_receiver
            && (matches!(
                self.ctx.krate.types.get(effective_operand_ty),
                Some(Type::Unknown | Type::TypeParam { .. })
            ) || self.type_contains_unknown(effective_operand_ty)
                || matches!(
                    self.ctx.krate.types.get(effective_operand_ty),
                    Some(Type::Optional(inner)) if self.is_string_compatible_type(*inner)
                )
                || matches!(
                    self.ctx.krate.types.get(effective_operand_ty),
                    Some(Type::Union(items)) if items.iter().any(|item| self.is_string_compatible_type(*item))
                ))
        {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            operand = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: operand },
                ty: string_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
            effective_operand_ty = string_ty;
        }
        if method == "substring"
            && self.ctx.krate.types.get(effective_operand_ty) != Some(&Type::String)
        {
            return Ok(None);
        }
        let start = call
            .arguments
            .first()
            .map(|argument| self.slice_index_argument(argument, body))
            .transpose()?;
        let end = call
            .arguments
            .get(1)
            .map(|argument| self.slice_index_argument(argument, body))
            .transpose()?;
        let slice_kind = match method {
            "substring" => smelt_hir::StringSliceKind::Substring,
            "substr" => smelt_hir::StringSliceKind::Substr,
            _ => smelt_hir::StringSliceKind::Slice,
        };

        match self.ctx.krate.types.get(effective_operand_ty) {
            Some(Type::String) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::StringSlice {
                        operand,
                        start,
                        end,
                        kind: slice_kind,
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            Some(Type::List(_)) if slice_like => Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ListSlice {
                    list: operand,
                    start,
                    end,
                },
                ty: operand_ty,
                span: self.span(call.span.start, call.span.end),
            }))),
            _ if slice_erased_receiver => Ok(Some(body.push_expr(Expr {
                // Erased/array-ish `slice`: `ListSlice` codegen routes the
                // erased receiver through the list-slice helper, which
                // dispatches on the runtime value tag (array vs string vs
                // forwarded marker buffer).
                kind: ExprKind::ListSlice {
                    list: operand,
                    start,
                    end,
                },
                ty: operand_ty,
                span: self.span(call.span.start, call.span.end),
            }))),
            _ => Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "slice/substring/substr requires a string receiver, or an array receiver for slice",
            )),
        }
    }

    /// Lower and validate a slice index argument.
    pub(in crate::lowering) fn slice_index_argument(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let index = self.argument(argument, body)?;
        if !self.slice_index_type_is_number(Self::expr_ty(body, index)) {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "slice indexes must be numbers",
            ));
        }
        Ok(index)
    }

    /// Return whether a type can be used as a JavaScript slice index.
    pub(in crate::lowering) fn slice_index_type_is_number(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Int | Type::Float | Type::Unknown | Type::TypeParam { .. }) => true,
            Some(Type::Optional(item)) => self.slice_index_type_is_number(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.slice_index_type_is_number(item)),
            _ => false,
        }
    }

    /// Return whether a value type is compatible with an array element type.
    pub(in crate::lowering) fn array_item_type_compatible(
        &mut self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        if self.type_assignable_to(actual, expected) {
            return true;
        }
        if self.erased_or_union_surface(actual) || self.erased_or_union_surface(expected) {
            return true;
        }
        if matches!(
            (
                self.ctx.krate.types.get(actual),
                self.ctx.krate.types.get(expected)
            ),
            (Some(Type::Union(_)), Some(Type::Union(_)))
        ) {
            return true;
        }
        let members = self.non_nullish_member_types(actual);
        !members.is_empty()
            && if matches!(self.ctx.krate.types.get(expected), Some(Type::Union(_))) {
                members
                    .into_iter()
                    .any(|item| self.type_assignable_to(item, expected))
            } else {
                members
                    .into_iter()
                    .all(|item| self.type_assignable_to(item, expected))
            }
    }

    /// Return union members after removing nullish wrappers from each member.
    fn non_nullish_member_types(&mut self, ty: smelt_hir::TypeId) -> Vec<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::Union(items)) => items
                .into_iter()
                .flat_map(|item| self.non_nullish_member_types(item))
                .collect(),
            Some(Type::Optional(item)) => vec![item],
            Some(Type::None) => Vec::new(),
            _ => vec![ty],
        }
    }

    /// Lower direct TypeScript array containment.
    pub(in crate::lowering) fn list_contains_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "includes" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Ok(None);
        }
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
        let Some(item_argument) = call.arguments.first() else {
            return Ok(None);
        };
        let item = self.argument(item_argument, body)?;
        if !self.array_item_type_compatible(Self::expr_ty(body, item), item_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array includes argument must match the array element type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListContains { list, item },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return the concrete list and item type for list-like receiver surfaces.
    pub(in crate::lowering) fn list_surface_type(
        &self,
        ty: smelt_hir::TypeId,
    ) -> Option<(smelt_hir::TypeId, smelt_hir::TypeId)> {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::List(item_ty)) => Some((ty, *item_ty)),
            Some(Type::Optional(inner)) => match self.ctx.krate.types.get(*inner) {
                Some(Type::List(item_ty)) => Some((*inner, *item_ty)),
                _ => None,
            },
            Some(Type::Union(items)) => {
                let mut list_surface = None;
                for item in items {
                    if let Some(surface) = self.list_surface_type(*item) {
                        if list_surface.replace(surface).is_some() {
                            return None;
                        }
                    }
                }
                list_surface
            }
            _ => None,
        }
    }

    /// Lower direct TypeScript `Array.prototype.entries` calls.
    pub(in crate::lowering) fn list_entries_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "entries" {
            return Ok(None);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array entries requires no arguments",
            ));
        }
        let list = self.expression(&member.object, body)?;
        let source_ty = Self::expr_ty(body, list);
        if !matches!(
            self.ctx.krate.types.get(source_ty),
            Some(Type::TypeParam { .. })
        ) {
            return Ok(None);
        }
        let list_ty = self.type_param_constraint_or_self(source_ty);
        let Some(Type::List(list_item_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let item_ty = *list_item_ty;
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        let entry_ty = self
            .ctx
            .krate
            .types
            .intern(Type::Tuple(vec![index_ty, item_ty]));
        let ty = self.ctx.krate.types.intern(Type::List(entry_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListEnumerate { list },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `.at(index)` on arrays and strings to optional HIR indexing.
    ///
    /// JavaScript `.at` accepts negative indexes, but out-of-range positions
    /// return `undefined` rather than raising. Model that with `OptionalIndex`
    /// so generated Rust uses checked normalized indexes for misses.
    pub(in crate::lowering) fn collection_at_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "at" {
            return Ok(None);
        }
        let [index_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array/string at requires exactly one numeric index",
            ));
        };
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let item_ty = match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(item_ty)) => *item_ty,
            Some(Type::String) => self.ctx.krate.types.intern(Type::String),
            _ => return Ok(None),
        };
        let index = self.argument(index_argument, body)?;
        self.optional_at_index_expr(receiver, item_ty, index, call.span.start, call.span.end, body)
            .map(Some)
    }

    /// Lower direct TypeScript `Array.prototype.concat` calls with any number of arguments, widening element types.
    ///
    /// Two shapes reach this rule. `ns.concat(collection, ...values)` on a utility
    /// *namespace* object (a `import * as _` star import or a registered object
    /// namespace) is the lodash free-function form: its first argument is the base
    /// array and the rest are appended, so the receiver identifier is not a value
    /// itself. Everything else — including an ordinary named value import that
    /// resolves to an erased array, e.g. `import { falsey } from './falsey'` used
    /// as `falsey.concat(true, 1)` — is real `Array.prototype.concat`, where the
    /// member object is the receiver array and every argument is concatenated onto
    /// it. Only genuine namespace/utility objects take the free-function path;
    /// plain value imports flow through the erased-receiver arms of
    /// [`Self::finish_list_concat_call`], which already coerce an `Unknown`/union
    /// receiver into a concrete list instead of rejecting it.
    pub(in crate::lowering) fn list_concat_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "concat" {
            return Ok(None);
        }
        if self.imported_utility_object(&member.object) {
            let Some((left_argument, right_arguments)) = call.arguments.split_first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static array concat requires a receiver argument",
                ));
            };
            let left = self.argument(left_argument, body)?;
            return self.finish_list_concat_call(call, left, right_arguments, true, body);
        }
        let left = self.expression(&member.object, body)?;
        self.finish_list_concat_call(call, left, &call.arguments, false, body)
    }
}
