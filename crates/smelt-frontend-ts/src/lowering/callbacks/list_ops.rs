//! List-iteration callback lowering: `concat`, `map`/`filter`/`forEach`,
//! `reduce`, and the reduce-utility recognizers that carry their semantics
//! through a callback argument.

use crate::lowering::{
    Argument, Body, DictProjectionOp, Expr, ExprKind, Expression, FunctionType, ListCallbackOp,
    Literal, ModuleBuilder, SmeltError, Type,
};
use oxc::span::GetSpan;

impl ModuleBuilder<'_> {
    /// Lower direct TypeScript `Array.prototype.concat` for one same-typed array argument.
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
        call: &oxc::ast::ast::CallExpression<'_>,
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

    /// Finish array concat lowering after the receiver and the list of
    /// `concat(...)` arguments are known.
    ///
    /// JavaScript `Array.prototype.concat` accepts any number of arguments, each
    /// of which is either another array (spread into the result) or a scalar
    /// element (appended). This folds every argument onto the receiver list left
    /// to right so `a.concat(b, c, d)` and `a.concat(x)` both lower through the
    /// same per-argument path.
    pub(in crate::lowering) fn finish_list_concat_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        mut left: smelt_hir::ExprId,
        right_arguments: &[Argument<'_>],
        right_prefers_list: bool,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let mut ty = Self::expr_ty(body, left);
        let mut item_ty = match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::List(list_item_ty)) => list_item_ty,
            Some(Type::Optional(inner)) => {
                let Some(Type::List(list_item_ty)) = self.ctx.krate.types.get(inner).cloned()
                else {
                    return Ok(None);
                };
                ty = inner;
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                });
                list_item_ty
            }
            Some(Type::Tuple(items)) => {
                let item_ty = self.tuple_items_element_type(&items);
                ty = self.ctx.krate.types.intern(Type::List(item_ty));
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                });
                item_ty
            }
            Some(Type::Union(items))
                if items.iter().any(|item| {
                    matches!(
                        self.ctx.krate.types.get(*item),
                        Some(
                            Type::List(_)
                                | Type::Unknown
                                | Type::TypeParam { .. }
                                | Type::Class { .. }
                        )
                    )
                }) =>
            {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                ty = self.ctx.krate.types.intern(Type::List(item_ty));
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                });
                item_ty
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) | None => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                ty = self.ctx.krate.types.intern(Type::List(item_ty));
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                });
                item_ty
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array concat requires an array receiver",
                ));
            }
        };
        if right_arguments.is_empty() {
            // `arr.concat()` with no arguments is a shallow copy.
            return Ok(Some(left));
        }
        for right_argument in right_arguments {
            let (right, widened_item_ty) = self.list_concat_argument(
                call,
                right_argument,
                ty,
                item_ty,
                right_prefers_list,
                body,
            )?;
            // JavaScript `concat` widens: `A[].concat(B[])` yields
            // `Array<A | B>`. When the argument's element type is not
            // assignable to the receiver's, the result list element widens
            // (using the same nullable/erased unification as array literals)
            // and the accumulated left side re-types into the widened list so
            // the emitter injects each element at use.
            if let Some(widened) = widened_item_ty
                && widened != item_ty
            {
                item_ty = widened;
                ty = self.ctx.krate.types.intern(Type::List(item_ty));
                left = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: left },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                });
            }
            left = body.push_expr(Expr {
                kind: ExprKind::ListConcat { left, right },
                ty,
                span: self.span(call.span.start, call.span.end),
            });
        }
        Ok(Some(left))
    }

    /// Unify a concat argument element type with the receiver's element type
    /// the way array-literal inference does, or `None` when the pair has no
    /// concrete widened shape.
    ///
    /// JavaScript `concat` produces `Array<A | B>`; internally a `T`/`null`
    /// mix widens to `Optional<T>`, matching how `[0, 1, null]` lowers.
    /// Pairs that would only unify by erasing to `unknown` return `None` so
    /// the caller keeps its explicit unsupported diagnostic instead of
    /// silently introducing a tagged dynamic value.
    fn concat_widened_element_type(
        &mut self,
        item_ty: smelt_hir::TypeId,
        right_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        if right_ty == item_ty {
            return Some(item_ty);
        }
        let none_ty = self.ctx.krate.types.intern(Type::None);
        if right_ty == none_ty {
            if matches!(self.ctx.krate.types.get(item_ty), Some(Type::Optional(_))) {
                return Some(item_ty);
            }
            return Some(self.ctx.krate.types.intern(Type::Optional(item_ty)));
        }
        if item_ty == none_ty {
            if matches!(self.ctx.krate.types.get(right_ty), Some(Type::Optional(_))) {
                return Some(right_ty);
            }
            return Some(self.ctx.krate.types.intern(Type::Optional(right_ty)));
        }
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(item_ty)
            && *inner == right_ty
        {
            return Some(item_ty);
        }
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(right_ty)
            && *inner == item_ty
        {
            return Some(right_ty);
        }
        None
    }

    /// Normalize a single `concat(...)` argument into a list of the receiver's
    /// element type so it can be appended with [`ExprKind::ListConcat`].
    ///
    /// `concat` accepts both arrays (spread) and scalar elements (wrapped into a
    /// singleton list); this picks between the two based on the argument's lowered
    /// type, matching the receiver's element type (`item_ty`) and list type (`ty`).
    /// When the argument's element type only combines with the receiver's by
    /// widening (e.g. appending `null` onto `number[]`), the widened element type
    /// is returned alongside the value so the caller can re-type the result list.
    pub(in crate::lowering) fn list_concat_argument(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        right_argument: &Argument<'_>,
        ty: smelt_hir::TypeId,
        item_ty: smelt_hir::TypeId,
        right_prefers_list: bool,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, Option<smelt_hir::TypeId>), SmeltError> {
        let mut right = self.argument(right_argument, body)?;
        let right_ty = Self::expr_ty(body, right);
        let right = if right_ty == ty {
            right
        } else if let Some(Type::List(right_item_ty)) = self.ctx.krate.types.get(right_ty).cloned()
            && (right_item_ty == item_ty
                || self.erased_or_union_surface(right_item_ty)
                || self.erased_or_union_surface(item_ty)
                || self.type_assignable_to(right_item_ty, item_ty))
        {
            // An array argument whose element type is (or is assignable to) the
            // receiver's element type — including a concrete-union element such
            // as `number | string` — is spread into the result. The re-type is a
            // no-op at runtime; the emitter injects each value into the element
            // type at use as needed.
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: right },
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else if right_ty == item_ty {
            body.push_expr(Expr {
                kind: ExprKind::ListLit(vec![right]),
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else if self.type_assignable_to(right_ty, item_ty) {
            // A scalar argument that is a member of the receiver's element type —
            // e.g. concatenating a `number` onto a `Array<number | string>`.
            // Coerce it into the element type (the emitter injects the concrete
            // union member) and wrap it in a singleton list to append.
            right = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: right },
                ty: item_ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            });
            body.push_expr(Expr {
                kind: ExprKind::ListLit(vec![right]),
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else if self.erased_or_union_surface(item_ty) {
            right = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: right },
                ty: item_ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            });
            body.push_expr(Expr {
                kind: ExprKind::ListLit(vec![right]),
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else if right_prefers_list && self.erased_or_union_surface(right_ty) {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: right },
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else if self.erased_or_union_surface(right_ty)
            || self.ctx.krate.types.get(right_ty) == Some(&Type::None)
        {
            if self.ctx.krate.types.get(right_ty) != Some(&Type::List(item_ty)) {
                right = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: right },
                    ty: item_ty,
                    span: self.span(right_argument.span().start, right_argument.span().end),
                });
            }
            body.push_expr(Expr {
                kind: ExprKind::ListLit(vec![right]),
                ty,
                span: self.span(right_argument.span().start, right_argument.span().end),
            })
        } else {
            // The argument neither matches the receiver's element type nor is
            // erased: JavaScript still concatenates by widening the result
            // element type (`A[].concat(B[])` is `Array<A | B>`). Compute the
            // widened element shape (a `T`/`null` mix widens to `Optional<T>`
            // like array literals) and hand it back so the caller re-types the
            // accumulated result list.
            let right_element_ty =
                if let Some(Type::List(right_item_ty)) = self.ctx.krate.types.get(right_ty) {
                    Some(*right_item_ty)
                } else {
                    None
                };
            let Some(widened) =
                self.concat_widened_element_type(item_ty, right_element_ty.unwrap_or(right_ty))
            else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array concat requires an array or element argument matching the receiver",
                ));
            };
            let widened_list_ty = self.ctx.krate.types.intern(Type::List(widened));
            let span = self.span(right_argument.span().start, right_argument.span().end);
            let widened_right = if right_element_ty.is_some() {
                // An array argument spreads into the widened result list.
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: right },
                    ty: widened_list_ty,
                    span,
                })
            } else {
                // A scalar argument appends as a singleton of the widened
                // element type.
                let element = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: right },
                    ty: widened,
                    span,
                });
                body.push_expr(Expr {
                    kind: ExprKind::ListLit(vec![element]),
                    ty: widened_list_ty,
                    span,
                })
            };
            return Ok((widened_right, Some(widened)));
        };
        Ok((right, None))
    }

    /// Lower callback-heavy TypeScript array methods.
    pub(in crate::lowering) fn list_callback_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "map" => ListCallbackOp::Map,
            "filter" => ListCallbackOp::Filter,
            "find" => ListCallbackOp::Find,
            "findIndex" => ListCallbackOp::FindIndex,
            "some" => ListCallbackOp::Some,
            "every" => ListCallbackOp::Every,
            "forEach" => ListCallbackOp::ForEach,
            _ => return Ok(None),
        };
        // A method-style callback on an imported utility object is the lodash
        // free-function form `_.map(collection, iteratee)` (and `filter`, `some`,
        // etc.), not a `Array.prototype.map` receiver. The collection is the
        // first argument and the iteratee the second; the older single-argument
        // shape (`_.map(iteratee)` as a partially-applied factory) is also
        // accepted. The receiver is an opaque imported lodash value, so the call
        // result stays a placeholder — but every argument is still lowered so
        // captures and side effects are honored.
        if self.imported_utility_object(&member.object) {
            let (collection_argument, callback_argument) = match call.arguments.as_slice() {
                [callback_argument] => (None, callback_argument),
                [collection_argument, callback_argument] => {
                    (Some(collection_argument), callback_argument)
                }
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "lodash collection callbacks accept a collection and one iteratee",
                    ));
                }
            };
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
            let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
            let return_ty = match op {
                ListCallbackOp::Map | ListCallbackOp::Filter => list_ty,
                ListCallbackOp::Some | ListCallbackOp::Every => {
                    self.ctx.krate.types.intern(Type::Bool)
                }
                ListCallbackOp::ForEach => self.ctx.krate.types.intern(Type::None),
                _ => unknown_ty,
            };
            if let Some(collection_argument) = collection_argument {
                let _ = self.argument(collection_argument, body)?;
            }
            let index_ty = self.ctx.krate.types.intern(Type::Int);
            let _ = self.callback_argument_with_body_fallback(
                callback_argument,
                &[unknown_ty, index_ty, list_ty],
                unknown_ty,
                "array callback",
                body,
            )?;
            let ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: vec![list_ty],
                rest: None,
                required_params: None,
                mutable_params: Vec::new(),
                return_ty,
                is_async: false,
                may_throw: false,
            }));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let [callback_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array callback methods require exactly one callback argument",
            ));
        };
        let mut list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let list_ty = match self.ctx.krate.types.get(list_ty).cloned() {
            Some(Type::List(_)) => list_ty,
            Some(Type::Tuple(items)) => {
                let item_ty = self.tuple_items_element_type(&items);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                asserted_ty
            }
            Some(Type::Union(items))
                if items.iter().any(|item| {
                    matches!(
                        self.ctx.krate.types.get(*item),
                        Some(Type::List(_) | Type::Unknown | Type::TypeParam { .. })
                    )
                }) =>
            {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                asserted_ty
            }
            _ => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                asserted_ty
            }
        };
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array callback method receiver must be an array",
            ));
        };
        let element_ty = *list_element_ty;
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let fallback_return_ty = match op {
            ListCallbackOp::Map | ListCallbackOp::FlatMap => unknown_ty,
            ListCallbackOp::Filter
            | ListCallbackOp::Find
            | ListCallbackOp::FindIndex
            | ListCallbackOp::FindLast
            | ListCallbackOp::FindLastIndex
            | ListCallbackOp::Some
            | ListCallbackOp::Every => bool_ty,
            ListCallbackOp::ForEach => self.ctx.krate.types.intern(Type::None),
        };
        let callback_param_tys = [element_ty, index_ty, list_ty];
        let callback = if matches!(
            op,
            ListCallbackOp::Filter
                | ListCallbackOp::Find
                | ListCallbackOp::FindIndex
                | ListCallbackOp::FindLast
                | ListCallbackOp::FindLastIndex
                | ListCallbackOp::Some
                | ListCallbackOp::Every
        ) {
            self.truthy_callback_argument_with_body_fallback(
                callback_argument,
                &callback_param_tys,
                "array callback",
                body,
            )?
        } else {
            self.callback_argument_with_body_fallback(
                callback_argument,
                &callback_param_tys,
                fallback_return_ty,
                "array callback",
                body,
            )?
        };
        let ty = match op {
            ListCallbackOp::Map => self.ctx.krate.types.intern(Type::List(callback.return_ty)),
            ListCallbackOp::Filter => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array filter")?;
                list_ty
            }
            ListCallbackOp::Find => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array find")?;
                self.ctx.krate.types.intern(Type::Optional(element_ty))
            }
            ListCallbackOp::FindIndex => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array findIndex")?;
                self.ctx.krate.types.intern(Type::Float)
            }
            ListCallbackOp::FindLast => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array findLast")?;
                self.ctx.krate.types.intern(Type::Optional(element_ty))
            }
            ListCallbackOp::FindLastIndex => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array findLastIndex")?;
                self.ctx.krate.types.intern(Type::Float)
            }
            ListCallbackOp::Some => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array some")?;
                bool_ty
            }
            ListCallbackOp::Every => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array every")?;
                bool_ty
            }
            ListCallbackOp::ForEach => self.ctx.krate.types.intern(Type::None),
            ListCallbackOp::FlatMap => {
                let Some(Type::List(item_ty)) = self.ctx.krate.types.get(callback.return_ty) else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "array flatMap callback must return an array",
                    ));
                };
                self.ctx.krate.types.intern(Type::List(*item_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListCallback {
                op,
                list,
                callback: callback.expr,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower lodash `_.forEach(collection, callback)` over array-like or object-like inputs.
    pub(in crate::lowering) fn lodash_for_each_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "forEach" {
            return Ok(None);
        }
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "_" || !self.value_imports.contains("_") {
            return Ok(None);
        }
        let [collection_arg, callback_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "lodash forEach requires collection and callback arguments",
            ));
        };
        let mut list = self.argument(collection_arg, body)?;
        let list_ty = Self::expr_ty(body, list);
        let (list_ty, item_ty) = match self.ctx.krate.types.get(list_ty).cloned() {
            Some(Type::List(item_ty)) => (list_ty, item_ty),
            Some(Type::Tuple(items)) => {
                let item_ty = self.tuple_items_element_type(&items);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(collection_arg.span().start, collection_arg.span().end),
                });
                (asserted_ty, item_ty)
            }
            Some(Type::Dict(_, value_ty)) => {
                let projected_ty = self.ctx.krate.types.intern(Type::List(value_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::DictProjection {
                        op: DictProjectionOp::Values,
                        dict: list,
                    },
                    ty: projected_ty,
                    span: self.span(collection_arg.span().start, collection_arg.span().end),
                });
                (projected_ty, value_ty)
            }
            _ => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(collection_arg.span().start, collection_arg.span().end),
                });
                (asserted_ty, item_ty)
            }
        };
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let callback_param_tys = [item_ty, index_ty, list_ty];
        let callback_expr = if let Argument::ArrowFunctionExpression(arrow) = callback_arg {
            self.arrow_closure_body_expr(arrow, &callback_param_tys, none_ty, body)?
        } else {
            self.callback_argument_with_body_fallback(
                callback_arg,
                &callback_param_tys,
                none_ty,
                "lodash forEach",
                body,
            )?
            .expr
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListCallback {
                op: ListCallbackOp::ForEach,
                list,
                callback: callback_expr,
            },
            ty: none_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower Strapi's imported `async.map(collection, callback, options?)` helper.
    pub(in crate::lowering) fn strapi_async_map_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "map" {
            return Ok(None);
        }
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "async" || !self.value_imports.contains("async") {
            return Ok(None);
        }
        let [collection_arg, callback_arg, trailing_args @ ..] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Strapi async.map requires collection and callback arguments",
            ));
        };
        let mut list = self.argument(collection_arg, body)?;
        let list_ty = Self::expr_ty(body, list);
        let (list_ty, item_ty) = match self.ctx.krate.types.get(list_ty).cloned() {
            Some(Type::List(item_ty)) => (list_ty, item_ty),
            Some(Type::Tuple(items)) => {
                let item_ty = self.tuple_items_element_type(&items);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(collection_arg.span().start, collection_arg.span().end),
                });
                (asserted_ty, item_ty)
            }
            _ => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(collection_arg.span().start, collection_arg.span().end),
                });
                (asserted_ty, item_ty)
            }
        };
        for arg in trailing_args {
            let _ = self.argument(arg, body)?;
        }
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let callback_param_tys = [item_ty, index_ty, list_ty];
        let callback = self.callback_argument_with_body_fallback(
            callback_arg,
            &callback_param_tys,
            unknown_ty,
            "Strapi async.map",
            body,
        )?;
        let list_result_ty = self.ctx.krate.types.intern(Type::List(callback.return_ty));
        let ty = self.ctx.krate.types.intern(Type::Future(list_result_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListCallback {
                op: ListCallbackOp::Map,
                list,
                callback: callback.expr,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `Array.prototype.reduce`, including element-typed calls without an initial value.
    pub(in crate::lowering) fn list_reduce_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "reduce" {
            return Ok(None);
        }
        if Self::is_static_reduce_utility_call(call) {
            return self.static_reduce_utility_call(call, body, member);
        }
        let ([callback_argument] | [callback_argument, _]) = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array reduce requires callback and at most one initial value",
            ));
        };
        let list = self.expression(&member.object, body)?;
        let list_span = member.object.span();
        self.lower_list_reduce(
            call,
            body,
            list,
            list_span.start,
            list_span.end,
            callback_argument,
            call.arguments.get(1),
        )
    }

    /// Check whether a `reduce` call is the utility form `ns.reduce(value, callback, initial?)`.
    pub(in crate::lowering) fn is_static_reduce_utility_call(call: &oxc::ast::ast::CallExpression<'_>) -> bool {
        match call.arguments.as_slice() {
            [first, _, _] => !Self::argument_is_callback_like(first),
            [first, second] => {
                !Self::argument_is_callback_like(first) && Self::argument_is_callback_like(second)
            }
            _ => false,
        }
    }

    /// Return true for argument nodes that represent callback values directly.
    pub(in crate::lowering) fn argument_is_callback_like(argument: &Argument<'_>) -> bool {
        match argument {
            Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_) => true,
            Argument::TSAsExpression(as_expr) => {
                Self::expression_is_callback_like(&as_expr.expression)
            }
            Argument::TSTypeAssertion(assertion) => {
                Self::expression_is_callback_like(&assertion.expression)
            }
            Argument::TSSatisfiesExpression(satisfies) => {
                Self::expression_is_callback_like(&satisfies.expression)
            }
            Argument::TSNonNullExpression(non_null) => {
                Self::expression_is_callback_like(&non_null.expression)
            }
            _ => false,
        }
    }

    /// Return true for expression nodes that represent callback values directly.
    pub(in crate::lowering) fn expression_is_callback_like(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => true,
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::expression_is_callback_like(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::expression_is_callback_like(&as_expr.expression)
            }
            Expression::TSTypeAssertion(assertion) => {
                Self::expression_is_callback_like(&assertion.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::expression_is_callback_like(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::expression_is_callback_like(&non_null.expression)
            }
            _ => false,
        }
    }

    /// Lower utility-style `reduce(collection, callback, initial?)` calls.
    pub(in crate::lowering) fn static_reduce_utility_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let [collection_arg, callback_arg, rest @ ..] = call.arguments.as_slice() else {
            return Ok(None);
        };
        if rest.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array reduce requires callback and at most one initial value",
            ));
        }
        let collection = self.argument(collection_arg, body)?;
        let collection_ty = Self::expr_ty(body, collection);
        if matches!(
            self.ctx.krate.types.get(collection_ty),
            Some(Type::List(_) | Type::Tuple(_))
        ) {
            return self.lower_list_reduce(
                call,
                body,
                collection,
                collection_arg.span().start,
                collection_arg.span().end,
                callback_arg,
                rest.first(),
            );
        }
        let initial = if let Some(initial_arg) = rest.first() {
            self.argument(initial_arg, body)?
        } else {
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(call.span.start, call.span.end),
            })
        };
        let _ = self.argument(callback_arg, body)?;
        let _ = self.expression(&member.object, body)?;
        Ok(Some(initial))
    }

    /// Lower a normalized array-reduce receiver and callback pair.
    pub(in crate::lowering) fn lower_list_reduce(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        mut list: smelt_hir::ExprId,
        list_span_start: u32,
        list_span_end: u32,
        callback_argument: &Argument<'_>,
        initial_argument: Option<&Argument<'_>>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let mut list_ty = Self::expr_ty(body, list);
        let element_ty =
            if let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) {
                *list_element_ty
            } else if let Some(Type::Tuple(items)) = self.ctx.krate.types.get(list_ty).cloned() {
                let item_ty = self.flattened_tuple_item_type(items);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(list_span_start, list_span_end),
                });
                list_ty = asserted_ty;
                item_ty
            } else {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let asserted_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                list = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: list },
                    ty: asserted_ty,
                    span: self.span(list_span_start, list_span_end),
                });
                list_ty = asserted_ty;
                item_ty
            };
        let mut initial = if let Some(initial_argument) = initial_argument {
            Some(self.argument(initial_argument, body)?)
        } else {
            None
        };
        // The accumulator seed type: the initial value's type, or (with no
        // initial) the element type, which seeds the fold from the first element.
        let seed_ty = initial.map_or(element_ty, |initial| Self::expr_ty(body, initial));
        let index_ty = self.ctx.krate.types.intern(Type::Int);
        // Contextual callback parameter types `(acc, cur, index, array)`. The
        // arrow path lowers its body with `seed_ty` as the return hint; the
        // named path resolves the accumulator type from the callback's own
        // declared signature via `reduce_accumulator_ty` below.
        let callback_param_tys = [seed_ty, element_ty, index_ty, list_ty];
        let (callback_expr, accumulator_ty) =
            if let Argument::ArrowFunctionExpression(arrow) = callback_argument {
                let callback_expr =
                    self.arrow_closure_body_expr(arrow, &callback_param_tys, seed_ty, body)?;
                (callback_expr, seed_ty)
            } else {
                let callback = self.callback_argument(
                    callback_argument,
                    &callback_param_tys,
                    "array reduce",
                    body,
                )?;
                // TypeScript threads the accumulator type `U` through
                // `reduce<U>(cb: (acc: U, cur, i, arr) => U, initial: U): U`,
                // unifying it from the callback's declared accumulator parameter,
                // its return type, and the initial value. When a named/opaque
                // callback's declared return type is not identical to the seed
                // but the two statically reconcile (concrete unions, optionals,
                // `unknown`/generic surfaces, numeric widening; see #71's shared
                // reconciler), pick `U` and coerce the seed/callback result into
                // it in the emitter, rather than rejecting the reduce.
                let declared_acc_ty = self.callback_declared_accumulator_ty(callback.expr, body);
                let accumulator_ty = self.reduce_accumulator_ty(
                    seed_ty,
                    declared_acc_ty,
                    callback.return_ty,
                    call,
                    initial.is_some(),
                )?;
                (callback.expr, accumulator_ty)
            };
        // When the accumulator type was widened past the seed's own type, coerce
        // the initial value into it so the emitted fold seeds with the exact
        // destination type (the emitter injects concrete-union members, boxes
        // into optionals, etc. through the same `TypeAssert` coercion seam other
        // array callbacks use for element-type coercions).
        if let (Some(initial_expr), Some(initial_argument)) = (initial.as_mut(), initial_argument)
            && Self::expr_ty(body, *initial_expr) != accumulator_ty
        {
            *initial_expr = body.push_expr(Expr {
                kind: ExprKind::TypeAssert {
                    value: *initial_expr,
                },
                ty: accumulator_ty,
                span: self.span(initial_argument.span().start, initial_argument.span().end),
            });
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListReduce {
                list,
                initial,
                callback: callback_expr,
            },
            ty: accumulator_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return a callback expression's declared accumulator (first) parameter type.
    ///
    /// A named or local reduce callback lowers to a value whose HIR type is its
    /// declared `Type::Function`; its first parameter is the accumulator slot
    /// `acc`. This surfaces that type so [`Self::reduce_accumulator_ty`] can use
    /// the callback's declared accumulator as the reduce result type `U` when it
    /// is wider than the seed. Returns `None` when the callback is not a plain
    /// function value (e.g. an opaque erased value with no static signature).
    fn callback_declared_accumulator_ty(
        &self,
        callback_expr: smelt_hir::ExprId,
        body: &Body,
    ) -> Option<smelt_hir::TypeId> {
        let ty = Self::expr_ty(body, callback_expr);
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::Function(function)) => function.params.first().copied(),
            _ => None,
        }
    }

    /// Reconcile the reduce accumulator type from the seed and callback types.
    ///
    /// `reduce` threads an accumulator of a single type `U` through the fold: the
    /// initial value, every callback result, and the final result all have type
    /// `U`. `U` is unified from the seed (the initial value's type, or the
    /// element type when there is no initial), the callback's declared
    /// accumulator (first) parameter type, and the callback's return type. This
    /// picks `U` and lets the emitter coerce the seed and each callback result
    /// into it:
    ///
    /// - When the seed already equals the callback return type (the common case),
    ///   `U` is that type unchanged.
    /// - Otherwise, when the callback declares an accumulator parameter type that
    ///   both the seed and the return type are assignable to — e.g.
    ///   `step(acc: string | number, x): number` folded from `"seed"` — that
    ///   declared type is `U`. This matches how TypeScript resolves `reduce<U>`.
    /// - Failing a usable declared parameter, the seed and return type are
    ///   reconciled through the shared conditional-branch reconciler
    ///   ([`Self::conditional_branch_type`]) used by #71 (numeric widening, list
    ///   element unification, concrete unions). A genuinely irreconcilable pair
    ///   surfaces the existing "array reduce callback returns an unsupported
    ///   type" error.
    ///
    /// With no initial value the fold seeds from the first element, so the
    /// accumulator must stay the element type: a widening callback return is
    /// rejected because there is no seed to coerce into the wider type.
    fn reduce_accumulator_ty(
        &mut self,
        seed_ty: smelt_hir::TypeId,
        declared_acc_ty: Option<smelt_hir::TypeId>,
        callback_return_ty: smelt_hir::TypeId,
        call: &oxc::ast::ast::CallExpression<'_>,
        has_initial: bool,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if seed_ty == callback_return_ty {
            return Ok(seed_ty);
        }
        if has_initial {
            // Prefer the callback's declared accumulator type when both the seed
            // and the return type flow into it: that is exactly TypeScript's `U`.
            if let Some(declared_acc_ty) = declared_acc_ty
                && declared_acc_ty != seed_ty
                && self.type_assignable_to(seed_ty, declared_acc_ty)
                && self.type_assignable_to(callback_return_ty, declared_acc_ty)
            {
                return Ok(declared_acc_ty);
            }
            // A callback with no distinct declared accumulator (or an opaque one)
            // whose return type simply subsumes the seed uses the return type.
            if self.type_assignable_to(seed_ty, callback_return_ty) {
                return Ok(callback_return_ty);
            }
            // Fall back to the shared reconciler; only accept a result the
            // callback result can be coerced into (its own return type flows in
            // by construction, so require the seed to as well).
            if let Ok(reconciled) = self.conditional_branch_type(
                seed_ty,
                callback_return_ty,
                None,
                call.span.start,
                call.span.end,
            ) && self.type_assignable_to(callback_return_ty, reconciled)
                && self.type_assignable_to(seed_ty, reconciled)
            {
                return Ok(reconciled);
            }
        }
        self.require_callback_ty(callback_return_ty, seed_ty, call, "array reduce")?;
        Ok(seed_ty)
    }

}
