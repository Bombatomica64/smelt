//! Collection constructors, projections, and object/class-shape lowering.
//!
//! This split of the lowering builder lowers the remaining collection-shaped
//! surfaces: `Array.from` and other list/set/dict constructors, set and dict
//! projection operations, and the object-literal and class-member shapes that
//! feed construction. The helpers classify receiver and argument types and
//! emit the concrete HIR construction and projection kinds.

use crate::RestParam;
use crate::lowering::{
    Argument, ArrayExpressionElement, AsyncOp, BinOp, BinaryOperator, BindingPattern, Body,
    CaptureMode, ClosureCapture, DictProjectionOp, Expr, ExprKind, Expression, Field, FunctionType,
    HashSet, Item, Literal, LocalDecl, LogicalOperator, ModuleBuilder, ObjectPropertyKind, Param,
    Pattern, PrimitiveCastOp, PropertyKey, PropertyKind, SetProjectionOp, SmeltError, Span,
    Statement, Stmt, Type, UnaryOp, UnaryOperator,
};
use oxc::span::GetSpan;

/// One lowered element of a spread-containing array literal.
///
/// `array_expression_with_spread` lowers all elements before assembling the
/// `ListLit`/`ListConcat` chain so the literal's item type can be unified from
/// the lowered piece types.
enum SpreadPiece {
    /// A `...operand` spread with its lowered operand and source span.
    Spread(smelt_hir::ExprId, oxc::span::Span),
    /// A plain element expression.
    Item(smelt_hir::ExprId),
}

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
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
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
                _ => self.ctx.krate.types.intern(Type::List(unknown_ty)),
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
            let item_ty = self.ctx.krate.types.intern(Type::Unknown);
            let ty = self.ctx.krate.types.intern(Type::List(item_ty));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ListFromLength { length },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        };
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let index_ty = self.ctx.krate.types.intern(Type::Float);
        let callback = self.callback_argument_with_body_fallback(
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
                PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str(),
                PropertyKey::StringLiteral(literal) => literal.value.as_str(),
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

    /// Lower `new Array<T>(length)` to an empty list with item metadata.
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
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
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
                self.ctx.krate.types.intern(Type::Unknown)
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
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
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

    /// Lower supported string split calls into HIR string runtime calls.
    pub(in crate::lowering) fn string_split_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "split" {
            return Ok(None);
        }
        if let Expression::Identifier(object) = &member.object
            && (self.imports.is_namespace(object.name.as_str())
                || self.imports.is_value(object.name.as_str()))
        {
            if call.arguments.len() < 2 || call.arguments.len() > 3 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static string split requires value, separator, and optional limit arguments",
                ));
            }
            let Some(haystack_argument) = call.arguments.first() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static string split requires value, separator, and optional limit arguments",
                ));
            };
            let Some(separator_argument) = call.arguments.get(1) else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "static string split requires value, separator, and optional limit arguments",
                ));
            };
            let haystack = self.argument(haystack_argument, body)?;
            let separator = self.argument(separator_argument, body)?;
            let limit = call
                .arguments
                .get(2)
                .map(|argument| self.argument(argument, body))
                .transpose()?;
            return self.finish_string_split_call(call, haystack, separator, limit, body);
        }
        if call.arguments.is_empty() || call.arguments.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires a separator and optional limit argument",
            ));
        }
        let haystack = self.expression(&member.object, body)?;
        let Some(separator_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires a separator argument",
            ));
        };
        let separator = self.argument(separator_argument, body)?;
        let limit = call
            .arguments
            .get(1)
            .map(|argument| self.argument(argument, body))
            .transpose()?;
        self.finish_string_split_call(call, haystack, separator, limit, body)
    }

    /// Finish string split lowering after the receiver-style or helper-style arguments are known.
    pub(in crate::lowering) fn finish_string_split_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        haystack: smelt_hir::ExprId,
        separator: smelt_hir::ExprId,
        limit: Option<smelt_hir::ExprId>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let haystack_ty = Self::expr_ty(body, haystack);
        let separator_ty = Self::expr_ty(body, separator);
        if !(self.is_string_compatible_type(haystack_ty)
            || self.type_contains_unknown(haystack_ty)
            || self.erased_or_union_surface(haystack_ty))
            || !self.string_split_separator_type_is_supported(separator_ty)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires string receiver and separator",
            ));
        }
        if let Some(limit) = limit {
            let limit_ty = Self::expr_ty(body, limit);
            if !self.string_split_limit_type_is_supported(limit_ty) {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "string split limit must be numeric or undefined",
                ));
            }
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let ty = self.ctx.krate.types.intern(Type::List(string_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSplit {
                haystack,
                separator,
                limit,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether a type can act as a JavaScript string split separator.
    pub(in crate::lowering) fn string_split_separator_type_is_supported(
        &self,
        ty: smelt_hir::TypeId,
    ) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::String | Type::Unknown | Type::TypeParam { .. }) => true,
            Some(Type::Class { name, .. }) => self
                .ctx
                .krate
                .symbols
                .get(*name)
                .is_some_and(|name| name == "RegExp"),
            Some(Type::Optional(item)) => self.string_split_separator_type_is_supported(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.string_split_separator_type_is_supported(item)),
            _ => false,
        }
    }

    /// Return whether a type can act as a JavaScript string split limit.
    pub(in crate::lowering) fn string_split_limit_type_is_supported(
        &self,
        ty: smelt_hir::TypeId,
    ) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Int | Type::Float | Type::None | Type::Unknown | Type::TypeParam { .. }) => {
                true
            }
            Some(Type::Optional(item)) => self.string_split_limit_type_is_supported(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.string_split_limit_type_is_supported(item)),
            _ => false,
        }
    }

    /// Lower array entries passed to a `Promise.*` combinator.
    pub(in crate::lowering) fn promise_array_args(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        array
            .elements
            .iter()
            .map(|element| match element {
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => {
                    Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "Promise combinator arrays cannot use spread or elision",
                    ))
                }
                _ => {
                    let value = self.array_element(element, body)?;
                    if matches!(
                        self.ctx.krate.types.get(Self::expr_ty(body, value)),
                        Some(Type::Future(_))
                    ) {
                        return Ok(value);
                    }
                    // A combinator element that is not statically a `Future`
                    // is a plain value (or an erased one that may be a promise
                    // at run time), which `Promise.all` adopts as-is. It must
                    // travel in the op's operands: lowering to a bare `Sleep`
                    // kept only its *type*, so the element expression -- and any
                    // side effect in it -- was dropped and the combinator saw
                    // `default_value(ty)`. `Promise.all([f(), g()])` on erased
                    // callables never called `f` or `g` at all.
                    let duration = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::Float(0.0)),
                        ty: self.ctx.krate.types.intern(Type::Float),
                        span: self.span(element.span().start, element.span().start),
                    });
                    let ty = self
                        .ctx
                        .krate
                        .types
                        .intern(Type::Future(Self::expr_ty(body, value)));
                    Ok(body.push_expr(Expr {
                        kind: ExprKind::AsyncOp {
                            op: AsyncOp::Resolve,
                            args: vec![duration, value],
                        },
                        ty,
                        span: self.span(element.span().start, element.span().end),
                    }))
                }
            })
            .collect()
    }

    /// Recover a concrete `Set<T>` type from a contextual type hint.
    ///
    /// A `Set` construction is often assigned to a variable whose annotation
    /// wraps the set in `Optional`/`Union` (`Set<T> | undefined`, common when a
    /// set is conditionally initialized). The set element type is still fully
    /// determined by that annotation, so unwrap those wrappers to find the set
    /// arm rather than treating the wrapped hint as an unannotated construction.
    /// Returns the interned `Type::Set` `TypeId` when exactly one set arm is
    /// present, otherwise `None`.
    fn set_type_from_hint(&self, hint: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(hint) {
            Some(Type::Set(_)) => Some(hint),
            Some(Type::Optional(inner)) => self.set_type_from_hint(*inner),
            Some(Type::Union(items)) => {
                let mut found = None;
                for item in items {
                    if let Some(set_ty) = self.set_type_from_hint(*item) {
                        if found.is_some() {
                            return None;
                        }
                        found = Some(set_ty);
                    }
                }
                found
            }
            _ => None,
        }
    }

    /// Lower `new Set(...)` from an array literal or annotated empty constructor.
    pub(in crate::lowering) fn set_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if !Self::is_ts_stdlib_class_name(callee.name.as_str(), smelt_stdlib::StdlibClass::Set) {
            return Ok(None);
        }
        let (items, ty) = match new_expr.arguments.as_slice() {
            [] => {
                // Prefer an explicit `new Set<T>()` argument, then the element
                // type recovered from the contextual type hint (including hints
                // wrapped in `Optional`/`Union`, e.g. `let s: Set<T> | undefined
                // = new Set()`), then an erased empty set. An empty `new Set()`
                // with no usable element type mirrors the graceful `new Map()`
                // fallback: it never rejects the construction, it produces an
                // empty `Set<unknown>` whose element type is refined by later
                // `.add(...)` calls, rather than forcing an inline annotation.
                let ty = if let Some(type_args) = &new_expr.type_arguments
                    && let [item] = type_args.params.as_slice()
                {
                    let item_ty = self.ts_type_to_hir(item)?;
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                } else if let Some(set_ty) =
                    type_hint.and_then(|hint| self.set_type_from_hint(hint))
                {
                    set_ty
                } else {
                    let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                };
                (Vec::new(), ty)
            }
            [Argument::ArrayExpression(array)] => {
                if array
                    .elements
                    .iter()
                    .any(|element| matches!(element, ArrayExpressionElement::SpreadElement(_)))
                {
                    let list = self.array_expression(array, body, None)?;
                    let list_ty = self.type_param_constraint_or_self(Self::expr_ty(body, list));
                    let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) else {
                        return Err(SmeltError::unsupported(
                            self.span(array.span.start, array.span.end),
                            "new Set([...spread]) requires an array literal argument",
                        ));
                    };
                    // A surrounding non-Set hint (e.g. a spread context hinting
                    // `T[]` in `[...new Set(xs)]`) describes the outer
                    // expression, not this constructor: fall back to the
                    // inferred set type instead of rejecting.
                    let ty = if let Some(hint) = type_hint
                        && matches!(self.ctx.krate.types.get(hint), Some(Type::Set(_)))
                    {
                        hint
                    } else {
                        self.ctx.krate.types.intern(Type::Set(*item_ty))
                    };
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::ListToSet { list },
                        ty,
                        span: self.span(new_expr.span.start, new_expr.span.end),
                    })));
                }
                let items = array
                    .elements
                    .iter()
                    .map(|element| self.array_element(element, body))
                    .collect::<Result<Vec<_>, _>>()?;
                // Only honor the hint when it is a Set; a non-Set hint comes
                // from the surrounding expression (spread, argument slot) and
                // must not reject the constructor.
                let ty = if let Some(hint) = type_hint
                    && matches!(self.ctx.krate.types.get(hint), Some(Type::Set(_)))
                {
                    hint
                } else if let Some(first_item) = items.first().copied() {
                    let item_ty = Self::expr_ty(body, first_item);
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                } else if let Some(type_args) = &new_expr.type_arguments
                    && let [item] = type_args.params.as_slice()
                {
                    let item_ty = self.ts_type_to_hir(item)?;
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                } else {
                    let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                };
                (items, ty)
            }
            [argument] => {
                let mut list = self.argument(argument, body)?;
                let raw_ty = Self::expr_ty(body, list);
                let list_ty = self.type_param_constraint_or_self(raw_ty);
                // `new Set(iterable)` accepts arrays directly. Optional arrays are
                // asserted to their inner list, an existing Set is already in set
                // shape, and erased/union surfaces (e.g. a generic helper return
                // typed `unknown`) are asserted to `List<Unknown>` so the
                // list-to-set conversion can proceed instead of being rejected.
                let item_ty = match self.ctx.krate.types.get(list_ty).cloned() {
                    Some(Type::List(item_ty)) => item_ty,
                    Some(Type::Optional(inner)) => {
                        if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(inner).cloned()
                        {
                            list = body.push_expr(Expr {
                                kind: ExprKind::TypeAssert { value: list },
                                ty: inner,
                                span: self.span(argument.span().start, argument.span().end),
                            });
                            item_ty
                        } else {
                            return Err(SmeltError::unsupported(
                                self.span(argument.span().start, argument.span().end),
                                "new Set(iterable) currently requires an array argument",
                            ));
                        }
                    }
                    Some(Type::Set(item_ty)) => {
                        // `new Set(otherSet)` copies an existing set: keep its
                        // element type and short-circuit (no list conversion).
                        let ty = if let Some(hint) = type_hint
                            && matches!(self.ctx.krate.types.get(hint), Some(Type::Set(_)))
                        {
                            hint
                        } else {
                            self.ctx.krate.types.intern(Type::Set(item_ty))
                        };
                        return Ok(Some(body.push_expr(Expr {
                            kind: ExprKind::TypeAssert { value: list },
                            ty,
                            span: self.span(new_expr.span.start, new_expr.span.end),
                        })));
                    }
                    _ if self.erased_or_union_surface(list_ty) => {
                        let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                        let asserted_list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                        list = body.push_expr(Expr {
                            kind: ExprKind::TypeAssert { value: list },
                            ty: asserted_list_ty,
                            span: self.span(argument.span().start, argument.span().end),
                        });
                        item_ty
                    }
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(argument.span().start, argument.span().end),
                            "new Set(iterable) currently requires an array argument",
                        ));
                    }
                };
                // Same hint rule as the literal forms: only a Set-shaped hint
                // overrides the inferred element type.
                let ty = if let Some(hint) = type_hint
                    && matches!(self.ctx.krate.types.get(hint), Some(Type::Set(_)))
                {
                    hint
                } else {
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                };
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ListToSet { list },
                    ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                })));
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(new_expr.span.start, new_expr.span.end),
                    "new Set currently supports no arguments or one array argument",
                ));
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::SetLit(items),
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// Lower `new Map(...)` to a dictionary literal.
    pub(in crate::lowering) fn map_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &new_expr.callee else {
            return Ok(None);
        };
        if !Self::is_ts_stdlib_class_name(callee.name.as_str(), smelt_stdlib::StdlibClass::Map) {
            return Ok(None);
        }
        let (entries, ty) = match new_expr.arguments.as_slice() {
            [] => {
                // `new Map()` always produces a `Type::JsMap` so erasure can
                // stamp the `__smelt_map` marker. Explicit `new Map<K, V>()`
                // type arguments and a `Map<K, V>` (or interchangeable `Record`)
                // type hint only refine the key/value component types; the
                // source-spelled Map identity is preserved regardless.
                let ty = if let Some(type_args) = &new_expr.type_arguments
                    && let [key, value] = type_args.params.as_slice()
                {
                    let key_ty = self.ts_type_to_hir(key)?;
                    let value_ty = self.ts_type_to_hir(value)?;
                    self.ctx.krate.types.intern(Type::JsMap(key_ty, value_ty))
                } else if let Some(hint) = type_hint
                    && let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
                        self.ctx.krate.types.get(hint)
                {
                    let (key_ty, value_ty) = (*key_ty, *value_ty);
                    self.ctx.krate.types.intern(Type::JsMap(key_ty, value_ty))
                } else {
                    let unknown = self.ctx.krate.types.intern(Type::Unknown);
                    self.ctx.krate.types.intern(Type::JsMap(unknown, unknown))
                };
                (Vec::new(), ty)
            }
            [Argument::ArrayExpression(array)] => {
                let entries = self.map_constructor_entries(array, body)?;
                let ty = if let Some(hint) = type_hint {
                    let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
                        self.ctx.krate.types.get(hint)
                    else {
                        return Err(SmeltError::unsupported(
                            self.span(new_expr.span.start, new_expr.span.end),
                            "new Map([...]) requires a Map<K, V> type annotation when annotated",
                        ));
                    };
                    // The declared `Map<K, V>` annotation states the intended
                    // entry key/value types, so each entry only needs to be
                    // *assignable* to those declared types rather than exactly
                    // equal. This mirrors array literals with a type hint: a
                    // union annotation (`Map<string, string | number>`) accepts
                    // heterogeneous entries, and the emitter coerces each entry
                    // to the declared type (erasing to the union ABI) when the
                    // dict literal is emitted.
                    let (key_ty, value_ty) = (*key_ty, *value_ty);
                    for (key, value) in &entries {
                        let entry_key_ty = Self::expr_ty(body, *key);
                        let entry_value_ty = Self::expr_ty(body, *value);
                        if !self.type_assignable_to(entry_key_ty, key_ty)
                            || !self.type_assignable_to(entry_value_ty, value_ty)
                        {
                            return Err(SmeltError::unsupported(
                                self.span(new_expr.span.start, new_expr.span.end),
                                "new Map entry key and value types must match the Map<K, V> annotation",
                            ));
                        }
                    }
                    // Preserve the source Map identity: even when the annotation
                    // spelled a `Record` (interchangeable with `Map` internally),
                    // a `new Map([...])` value must stamp the `__smelt_map`
                    // marker, so re-intern the declared components as `JsMap`.
                    self.ctx.krate.types.intern(Type::JsMap(key_ty, value_ty))
                } else if entries.is_empty() {
                    let unknown = self.ctx.krate.types.intern(Type::Unknown);
                    self.ctx.krate.types.intern(Type::JsMap(unknown, unknown))
                } else {
                    // Without an annotation, infer the key and value component
                    // types the same way an array literal infers its element
                    // type: a single shared type when the entries are
                    // homogeneous, otherwise the union of the observed types
                    // (falling back to the erased `Unknown` surface for shapes
                    // that do not form a clean union). This matches how
                    // TypeScript widens a mixed `new Map([...])` to a union
                    // entry type and keeps Map inference consistent with the
                    // array-literal path.
                    let keys = entries.iter().map(|(key, _)| *key).collect::<Vec<_>>();
                    let values = entries.iter().map(|(_, value)| *value).collect::<Vec<_>>();
                    let key_ty = self.array_literal_item_type(&keys, body);
                    let value_ty = self.array_literal_item_type(&values, body);
                    self.ctx.krate.types.intern(Type::JsMap(key_ty, value_ty))
                };
                (entries, ty)
            }
            _ => {
                for argument in &new_expr.arguments {
                    let _ = self.argument(argument, body)?;
                }
                let unknown = self.ctx.krate.types.intern(Type::Unknown);
                (
                    Vec::new(),
                    self.ctx.krate.types.intern(Type::JsMap(unknown, unknown)),
                )
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// Lower `new Object()` / `Object(...)` to a concrete record value.
    ///
    /// JavaScript `Object()` is the plain-object constructor. Smelt models plain
    /// objects with the same concrete record representation as an object literal
    /// `{}` (a `Type::Dict` carrying `ExprKind::DictLit`), so no value is routed
    /// through `SmeltUnknown`:
    ///
    /// - `new Object()` / `Object()` / `Object(null)` / `Object(undefined)`
    ///   produce a fresh empty record, exactly like `{}`.
    /// - `Object(value)` where `value` is already an object/record (a `Dict`,
    ///   `Class`, or `unknown` surface) returns that value unchanged, matching
    ///   `Object(obj) === obj`.
    /// - Boxing a primitive (`Object(42)` -> a boxed `Number` object) has no
    ///   concrete Smelt model yet and is rejected as an unsupported lowering
    ///   rather than erased, so the boundary stays explicit.
    pub(in crate::lowering) fn object_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let argument = match new_expr.arguments.as_slice() {
            [] => None,
            [Argument::NullLiteral(_)] => None,
            [Argument::Identifier(ident)] if ident.name == "undefined" => None,
            [argument] => Some(self.argument(argument, body)?),
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "Object constructor supports at most one argument",
                ));
            }
        };
        if let Some(value) = argument {
            let value_ty = self.type_param_constraint_or_self(Self::expr_ty(body, value));
            if matches!(
                self.ctx.krate.types.get(value_ty),
                Some(Type::Dict(_, _) | Type::Class { .. } | Type::Unknown)
            ) {
                return Ok(value);
            }
            return Err(SmeltError::unsupported(
                span,
                "Object(value) boxing of non-object values is not lowered yet",
            ));
        }
        let ty = self.object_literal_type(&[], type_hint, body);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty,
            span,
        }))
    }

    /// Lower the entry array passed to `new Map([[key, value], ...])`.
    pub(in crate::lowering) fn map_constructor_entries(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
    ) -> Result<Vec<(smelt_hir::ExprId, smelt_hir::ExprId)>, SmeltError> {
        let mut entries = Vec::new();
        for element in &array.elements {
            let ArrayExpressionElement::ArrayExpression(pair) = element else {
                return Err(SmeltError::unsupported(
                    self.span(element.span().start, element.span().end),
                    "new Map entries must be [key, value] array pairs",
                ));
            };
            let [key_element, value_element] = pair.elements.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(pair.span.start, pair.span.end),
                    "new Map entries must contain exactly key and value",
                ));
            };
            let key = self.array_element(key_element, body)?;
            let value = self.array_element(value_element, body)?;
            entries.push((key, value));
        }
        Ok(entries)
    }

    /// Lower an array element.
    pub(in crate::lowering) fn array_element(
        &mut self,
        element: &ArrayExpressionElement<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match element {
            ArrayExpressionElement::NumericLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Float);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            ArrayExpressionElement::BigIntLiteral(lit) => {
                self.bigint_literal_expression(lit.value.as_str(), lit.span, body)
            }
            ArrayExpressionElement::StringLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(lit.value.to_string())),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            ArrayExpressionElement::BooleanLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            ArrayExpressionElement::NullLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            ArrayExpressionElement::Identifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            ArrayExpressionElement::BinaryExpression(binary) => {
                self.binary_expression(binary, body)
            }
            ArrayExpressionElement::LogicalExpression(logical) => {
                self.logical_expression(logical, body)
            }
            ArrayExpressionElement::ConditionalExpression(conditional) => {
                self.conditional_expression(conditional, body, None)
            }
            ArrayExpressionElement::UnaryExpression(unary) => self.unary_expression(unary, body),
            ArrayExpressionElement::TSAsExpression(as_expr) => {
                self.expression(&as_expr.expression, body)
            }
            ArrayExpressionElement::TSSatisfiesExpression(satisfies) => {
                self.expression(&satisfies.expression, body)
            }
            ArrayExpressionElement::TSNonNullExpression(non_null) => {
                self.expression(&non_null.expression, body)
            }
            ArrayExpressionElement::ArrayExpression(array) => {
                if let [ArrayExpressionElement::SpreadElement(spread)] = array.elements.as_slice() {
                    return self.expression(&spread.argument, body);
                }
                let mut items = Vec::new();
                for nested_element in &array.elements {
                    let item = match nested_element {
                        ArrayExpressionElement::SpreadElement(_) => {
                            return Err(SmeltError::unsupported(
                                self.span(nested_element.span().start, nested_element.span().end),
                                "array spread elements are not lowered yet",
                            ));
                        }
                        ArrayExpressionElement::Elision(_) => {
                            // A hole reads as `undefined` (see the sibling
                            // elision arm in `array_expression`).
                            let ty = self.ctx.krate.types.intern(Type::Unknown);
                            body.push_expr(Expr {
                                kind: ExprKind::Literal(Literal::Undefined),
                                ty,
                                span: self
                                    .span(nested_element.span().start, nested_element.span().end),
                            })
                        }
                        _ => self.array_element(nested_element, body)?,
                    };
                    items.push(item);
                }
                let Some(first) = items.first().copied() else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "empty nested arrays require an explicit type annotation",
                    ));
                };
                let item_ty = Self::expr_ty(body, first);
                let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                Ok(body.push_expr(Expr {
                    kind: ExprKind::ListLit(items),
                    ty,
                    span: self.span(array.span.start, array.span.end),
                }))
            }
            ArrayExpressionElement::ObjectExpression(object) => {
                self.object_expression(object, body, None)
            }
            ArrayExpressionElement::RegExpLiteral(literal) => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let pattern = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(
                        Self::regex_literal_pattern_text_without_flags(literal),
                    )),
                    ty: string_ty,
                    span: self.span(literal.span.start, literal.span.end),
                });
                let flags = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(literal.regex.flags.to_string())),
                    ty: string_ty,
                    span: self.span(literal.span.start, literal.span.end),
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: self.intern_type_name("RegExp"),
                        args: vec![pattern, flags],
                    },
                    ty: self.regexp_type(),
                    span: self.span(literal.span.start, literal.span.end),
                }))
            }
            ArrayExpressionElement::TemplateLiteral(template) => {
                self.template_literal_expression(template, body)
            }
            ArrayExpressionElement::CallExpression(call) => self.call_expression(call, body),
            ArrayExpressionElement::NewExpression(new_expr) => {
                self.new_expression_with_hint(new_expr, body, None)
            }
            ArrayExpressionElement::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            ArrayExpressionElement::StaticMemberExpression(member) => {
                self.static_member(member, body)
            }
            ArrayExpressionElement::ArrowFunctionExpression(arrow) => {
                self.arrow_function_expression(arrow, body)
            }
            // `ArrayExpressionElement` inherits every `Expression` variant, so any
            // element we do not special-case above (function expressions, `this`,
            // class expressions, etc.) is lowered through the shared expression
            // path. `as_expression` returns `Some` for exactly the inherited
            // variants; only `SpreadElement`/`Elision` fall through to the error.
            other => {
                if let Some(expression) = other.as_expression() {
                    return self.expression(expression, body);
                }
                Err(SmeltError::unsupported(
                    self.span(element.span().start, element.span().end),
                    format!("array element kind is not lowered yet: {element:?}"),
                ))
            }
        }
    }

    /// Lower a binary expression.
    pub(in crate::lowering) fn binary_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // `instanceof` and `in` are not arithmetic/comparison operators that map
        // onto a `BinOp`; they have dedicated lowering (predicate and key-membership
        // respectively) shared with the hinted expression path in builder_part08.
        if binary.operator == BinaryOperator::Instanceof {
            return self.instanceof_expression(binary, body);
        }
        if binary.operator == BinaryOperator::In {
            return self.in_expression(binary, body);
        }
        if binary.operator == BinaryOperator::Exponential {
            let base = self.expression(&binary.left, body)?;
            let exponent = self.expression(&binary.right, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::NumericPow { base, exponent },
                ty,
                span: self.span(binary.span.start, binary.span.end),
            }));
        }
        let op = match binary.operator {
            BinaryOperator::Addition => BinOp::Add,
            BinaryOperator::Subtraction => BinOp::Sub,
            BinaryOperator::Multiplication => BinOp::Mul,
            BinaryOperator::Division => BinOp::Div,
            BinaryOperator::Remainder => BinOp::Rem,
            // `===`/`!==` keep JS reference semantics (`JsStrictEq`); `==`/`!=`
            // stay structural (`Eq`). See builder_part08's mapping.
            BinaryOperator::StrictEquality => BinOp::JsStrictEq,
            BinaryOperator::Equality => BinOp::Eq,
            BinaryOperator::StrictInequality => BinOp::JsStrictNotEq,
            BinaryOperator::Inequality => BinOp::NotEq,
            BinaryOperator::LessThan => BinOp::Lt,
            BinaryOperator::LessEqualThan => BinOp::Lte,
            BinaryOperator::GreaterThan => BinOp::Gt,
            BinaryOperator::GreaterEqualThan => BinOp::Gte,
            BinaryOperator::ShiftLeft => BinOp::Shl,
            BinaryOperator::ShiftRight => BinOp::Shr,
            BinaryOperator::ShiftRightZeroFill => BinOp::UShr,
            BinaryOperator::BitwiseAnd => BinOp::BitAnd,
            BinaryOperator::BitwiseOR => BinOp::BitOr,
            BinaryOperator::BitwiseXOR => BinOp::BitXor,
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(binary.span.start, binary.span.end),
                    format!("binary operator is not lowered yet: {:?}", binary.operator),
                ));
            }
        };
        let mut lhs = self.expression(&binary.left, body)?;
        let mut rhs = self.expression(&binary.right, body)?;
        // JavaScript `===`/`!==` on erased values compares the ORIGINAL erased
        // `SmeltUnknown` (reference identity for objects/arrays/functions, value
        // for primitives). A `typeof`-guard narrows an operand to a concrete
        // type (e.g. `Function`), which lowers the reference as an
        // `UnknownCast` that re-materializes the value through an adapter — for
        // a function that means a FRESH `Rc` whose `Rc::ptr_eq` never matches,
        // so `f === f` wrongly yields `false`. The narrowing carries no benefit
        // for a bare strict-equality operand, so peel it back to the pre-cast
        // erased value and let both sides compare as the untouched originals.
        if matches!(op, BinOp::JsStrictEq | BinOp::JsStrictNotEq) {
            lhs = self.peel_narrowing_cast_for_identity(body, lhs);
            rhs = self.peel_narrowing_cast_for_identity(body, rhs);
        }

        let lhs_ty = Self::expr_ty(body, lhs);
        let rhs_ty = Self::expr_ty(body, rhs);
        let ty = self.binary_result_type(op, lhs_ty, rhs_ty);
        Ok(body.push_expr(Expr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty,
            span: self.span(binary.span.start, binary.span.end),
        }))
    }

    /// Peel a narrowing [`ExprKind::UnknownCast`] off a strict-equality operand.
    ///
    /// A `typeof`/`instanceof` guard narrows an erased local to a concrete type
    /// via an `UnknownCast` whose inner value is still an erased `SmeltUnknown`.
    /// For a bare `===`/`!==` operand the narrowed concrete type is discarded
    /// anyway (the comparison lowers through `js_strict_eq` on the erased
    /// representation), and materializing the concrete shape can re-wrap the
    /// value through an adapter that breaks JS reference identity (a function
    /// becomes a fresh `Rc`, so `f === f` is `false`). Returning the pre-cast
    /// erased value keeps the comparison on the untouched original. Only peels
    /// when the inner value is genuinely erased, so ordinary casts are left
    /// intact.
    pub(in crate::lowering) fn peel_narrowing_cast_for_identity(
        &self,
        body: &mut Body,
        expr: smelt_hir::ExprId,
    ) -> smelt_hir::ExprId {
        let index = usize::try_from(expr.0).expect("expr id should fit into usize");
        let Some(Expr {
            kind: ExprKind::UnknownCast { value, .. },
            span,
            ..
        }) = body.exprs.get(index)
        else {
            return expr;
        };
        let inner = *value;
        let cast_span = *span;
        // The narrowing cast rewraps a `Local` read at the narrowed type; recover
        // the local and re-read it at its erased base type so the comparison sees
        // the untouched original `SmeltUnknown`.
        let inner_index = usize::try_from(inner.0).expect("expr id should fit into usize");
        let Some(Expr {
            kind: ExprKind::Local(local),
            ..
        }) = body.exprs.get(inner_index)
        else {
            return expr;
        };
        let source_local = *local;
        let base_ty = Self::local_ty(body, source_local);
        if matches!(
            self.ctx.krate.types.get(base_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) {
            return body.push_expr(Expr {
                kind: ExprKind::Local(source_local),
                ty: base_ty,
                span: cast_span,
            });
        }
        expr
    }

    /// Lower a logical expression.
    pub(in crate::lowering) fn logical_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // The `(typeof X === 'object' && X) || ...` global-detection chain folds
        // to the global-object value before ordinary `||` lowering, so the dead
        // absent-alias clauses (e.g. `&& window`) are never lowered.
        if let Some(expr) = self.global_detection_chain_expression(logical, body) {
            return Ok(expr);
        }
        // A guard the profile already decides short-circuits the whole
        // expression BEFORE any value-shape helper runs, because several of them
        // lower the right operand first (`logical_and_numeric_value_expression`
        // does) and the dead operand is exactly the one that cannot be lowered.
        if let Some(expr) = self.short_circuited_static_guard(logical, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.logical_or_fallback_expression(logical, body)? {
            return Ok(expr);
        }
        if logical.operator == LogicalOperator::Coalesce {
            return self.nullish_coalesce_expression(logical, body, None);
        }
        if let Some(expr) = self.logical_and_numeric_value_expression(logical, body)? {
            return Ok(expr);
        }
        let op = if logical.operator == LogicalOperator::And {
            BinOp::And
        } else {
            BinOp::Or
        };
        let lhs = self.expression(&logical.left, body)?;
        if let Some(expr) = Self::short_circuited_logical_operand(logical, lhs, body) {
            return Ok(expr);
        }
        let rhs = self.expression(&logical.right, body)?;
        let span = self.span(logical.span.start, logical.span.end);
        if let Some(expr) = self.logical_operand_value_expression(logical, body, lhs, rhs)? {
            return Ok(expr);
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty,
            span,
        }))
    }

    /// Fold `<statically decided guard> && <dead operand>` (and the `||` mirror)
    /// to the guard's constant, without lowering the dead operand at all.
    ///
    /// JavaScript never evaluates the right operand of a `&&` whose left operand
    /// is falsy, nor of a `||` whose left operand is truthy. Smelt folds
    /// existence guards against the target profile to a constant, so the dead
    /// operand is routinely one that CANNOT be lowered — es-toolkit's `isBrowser`
    /// reads `window?.document` behind `typeof window !== 'undefined'`, and the
    /// non-DOM profile provides no `window` binding on purpose. Visiting it
    /// anyway turned a correctly-folded `false` into an `unresolved identifier`
    /// blocker that aborted the whole crate build.
    ///
    /// [`Self::static_guard_value`] answers from the AST alone, so this runs
    /// before the value-shape helpers — several of which lower the right operand
    /// first — and costs one pattern match when it does not apply.
    pub(in crate::lowering) fn short_circuited_static_guard(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let value = self.static_guard_value(&logical.left)?;
        let decides = match logical.operator {
            LogicalOperator::And => !value,
            LogicalOperator::Or => value,
            LogicalOperator::Coalesce => false,
        };
        if !decides {
            return None;
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(value)),
            ty,
            span: self.span(logical.span.start, logical.span.end),
        }))
    }

    /// Return the whole `&&`/`||` result when the LEFT operand already decided it.
    ///
    /// JavaScript's logical operators short-circuit: the right operand of a
    /// `&&` whose left operand is falsy is never evaluated, and neither is the
    /// right operand of a `||` whose left operand is truthy. Smelt folds plenty
    /// of guards to a compile-time literal — a `typeof` probe against the target
    /// profile's absent globals is the common one — and until now it still went
    /// on to LOWER the dead operand, which then had to resolve names that only
    /// exist in the branch JavaScript never takes. es-toolkit's `isBrowser`
    /// (`typeof window !== 'undefined' && window?.document != null`) is the
    /// instance: `typeof window` folds to `false` for the non-DOM profile, and
    /// lowering `window?.document` anyway demands a `window` binding the profile
    /// deliberately does not provide.
    ///
    /// Deliberately narrow: only a `bool` LITERAL left operand short-circuits,
    /// so this folds exactly the guards lowering already reduced to a constant
    /// and never speculates about a runtime value's truthiness. The folded
    /// result is the left operand itself, which is what JavaScript yields.
    fn short_circuited_logical_operand(
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        lhs: smelt_hir::ExprId,
        body: &Body,
    ) -> Option<smelt_hir::ExprId> {
        let index = usize::try_from(lhs.0).ok()?;
        let Some(ExprKind::Literal(Literal::Bool(value))) =
            body.exprs.get(index).map(|expr| &expr.kind)
        else {
            return None;
        };
        let decides = match logical.operator {
            LogicalOperator::And => !value,
            LogicalOperator::Or => *value,
            LogicalOperator::Coalesce => false,
        };
        decides.then_some(lhs)
    }

    /// Lower `a && b` / `a || b` in a VALUE position to the operand JavaScript
    /// actually yields.
    ///
    /// JavaScript's logical operators are selectors, not boolean operators:
    /// `a && b` evaluates to `a` when `a` is falsy and to `b` otherwise, and
    /// `a || b` evaluates to `a` when `a` is truthy and to `b` otherwise. The
    /// static type of the whole expression is therefore the union of the two
    /// operand types, not `boolean`.
    ///
    /// Modelling it as a boolean throws the operand away. es-toolkit's
    /// `expect(error instanceof Error && error.message).toBe('test')` lowered to
    /// a `bool`, so the comparison against a string was statically false and the
    /// assertion folded to `!(false)` — a test that could never fail and never
    /// checked anything.
    ///
    /// Returns `None` (so the caller keeps the boolean `BinOp`) when:
    ///
    /// * both operands are already `bool`, which is the overwhelmingly common
    ///   case and where the boolean lowering is exactly right — widening it
    ///   would push ordinary guards through a union for nothing; or
    /// * the two operand types have no common lowered shape, i.e.
    ///   [`Self::conditional_branch_type`] (the same unification a ternary uses,
    ///   so `a && b` and `a ? b : a` agree) cannot merge them. Degrading to the
    ///   previous boolean shape keeps lowering succeeding where it succeeds
    ///   today rather than erasing the value to `unknown` to force a merge.
    ///
    /// The left operand is bound to a temporary first: it is read twice (once
    /// for the truthiness test, once as the selected value) and re-emitting the
    /// expression would evaluate it twice.
    pub(in crate::lowering) fn logical_operand_value_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let lhs_ty = Self::expr_ty(body, lhs);
        let rhs_ty = Self::expr_ty(body, rhs);
        if lhs_ty == bool_ty && rhs_ty == bool_ty {
            return Ok(None);
        }
        let ty = match self.conditional_branch_type(
            rhs_ty,
            lhs_ty,
            None,
            logical.span.start,
            logical.span.end,
        ) {
            Ok(ty) => ty,
            // No closer common shape exists (a `string` operand beside a
            // `number` one, say). TypeScript's answer for `a && b` is the
            // literal union of the operand types, and a generated union is a
            // concrete Rust enum — not erasure — so build it rather than
            // falling back to a boolean that discards the value.
            Err(_) => self
                .ctx
                .krate
                .types
                .intern(Type::Union(vec![lhs_ty, rhs_ty])),
        };
        // `conditional_branch_type` is allowed to answer `unknown` for a merge
        // it cannot name, which is right when an operand is ALREADY erased (the
        // `error instanceof Error && error.message` case, where `error` is
        // source `unknown`) and wrong when both operands are concrete: erasing
        // two known shapes to reconcile them is the avoidable erasure the ABI
        // rules forbid. A generated union names that merge exactly.
        let ty = if self.ctx.krate.types.get(ty) == Some(&Type::Unknown)
            && !self.type_contains_unknown(lhs_ty)
            && !self.type_contains_unknown(rhs_ty)
        {
            self.ctx
                .krate
                .types
                .intern(Type::Union(vec![lhs_ty, rhs_ty]))
        } else {
            ty
        };
        let span = self.span(logical.span.start, logical.span.end);
        // Bind the left operand so the truthiness test and the selected value
        // read one evaluation.
        let selector = body.push_local(LocalDecl {
            name: Some(self.intern_source_name("smelt_logical")),
            ty: lhs_ty,
            mutable: false,
            span,
        });
        let selector_pat = body.push_pattern(Pattern::Binding(selector));
        let let_stmt = Stmt::Let {
            pat: selector_pat,
            ty: lhs_ty,
            value: Some(lhs),
        };
        if let Some(block) = self.current_statement_block {
            body.push_stmt_to_block(block, let_stmt);
        } else {
            body.push_stmt(let_stmt);
        }
        let selector_read = body.push_expr(Expr {
            kind: ExprKind::Local(selector),
            ty: lhs_ty,
            span,
        });
        let Ok(cond) = self.lowered_condition_expression(selector_read, span, body) else {
            return Ok(None);
        };
        let selector_value = body.push_expr(Expr {
            kind: ExprKind::Local(selector),
            ty: lhs_ty,
            span,
        });
        let (then_expr, else_expr) = if logical.operator == LogicalOperator::And {
            (rhs, selector_value)
        } else {
            (selector_value, rhs)
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            },
            ty,
            span,
        })))
    }

    /// Lower `a && b` / `a || b` that is consumed only for its truthiness.
    ///
    /// A condition observes nothing but the truthiness of the operand
    /// [`Self::logical_operand_value_expression`] selects, and
    /// `truthy(a && b) == truthy(a) && truthy(b)` (likewise for `||`). Lowering
    /// the condition form straight to a boolean `BinOp` therefore keeps
    /// `if (a && b)` as a plain Rust `&&` instead of materializing a union value
    /// and then testing it — the value-yielding rule is only needed where the
    /// value escapes.
    ///
    /// Returns `None` when this is not a plain `&&`/`||`, or when either operand
    /// has no truthiness lowering, so the caller falls back to lowering the
    /// logical expression as a value and testing that.
    pub(in crate::lowering) fn logical_condition_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if logical.operator == LogicalOperator::Coalesce {
            return Ok(None);
        }
        // `(typeof X === 'object' && X) || ...` folds to the detected global
        // before either operand is lowered; its absent-alias operands (`&&
        // window` in a non-browser build) cannot be lowered at all, so the fold
        // has to run first here too.
        if let Some(expr) = self.global_detection_chain_expression(logical, body) {
            return self
                .lowered_condition_expression(
                    expr,
                    self.span(logical.span.start, logical.span.end),
                    body,
                )
                .map(Some);
        }
        if let Some(expr) = self.short_circuited_static_guard(logical, body) {
            return Ok(Some(expr));
        }
        let Ok(cond) = self.condition_expression(&logical.left, body) else {
            return Ok(None);
        };
        if let Some(expr) = Self::short_circuited_logical_operand(logical, cond, body) {
            return Ok(Some(expr));
        }
        // `x instanceof T && x.field` narrows `x` for the right operand exactly
        // as it does in the value form.
        let rhs_narrowing = if logical.operator == LogicalOperator::And {
            self.guard_narrowing(&logical.left, body)
        } else {
            None
        };
        if let Some(narrowing) = rhs_narrowing.clone() {
            self.scope.push_narrowing_scope(narrowing);
        }
        let rhs = self.expression(&logical.right, body);
        if rhs_narrowing.is_some() {
            self.scope.pop_narrowing_scope();
        }
        let Ok(rhs) = rhs else {
            return Ok(None);
        };
        // A `Conditional`, not a boolean `BinOp`: MIR lowers the arms into
        // branches, so the right operand's own statements only run when the
        // left operand did not already decide the answer. A `BinOp` would hoist
        // them and evaluate both sides unconditionally, losing short-circuiting.
        let ty = self.ctx.krate.types.intern(Type::Bool);
        let identity = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(logical.operator == LogicalOperator::Or)),
            ty,
            span: self.expression_span(&logical.left),
        });
        let (then_expr, else_expr) = if logical.operator == LogicalOperator::And {
            (rhs, identity)
        } else {
            (identity, rhs)
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            },
            ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left && numeric` expressions in numeric value contexts.
    ///
    /// JavaScript returns either the falsy left value or the right value. When
    /// the right side is numeric, generated Rust needs a numeric result instead
    /// of the boolean shape used for conditions, so falsy left values are
    /// represented by numeric zero.
    pub(in crate::lowering) fn logical_and_numeric_value_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if logical.operator != LogicalOperator::And {
            return Ok(None);
        }
        let rhs = self.expression(&logical.right, body)?;
        let rhs_ty = Self::expr_ty(body, rhs);
        if !self.is_numeric_like_type(rhs_ty) {
            return Ok(None);
        }
        let cond = self.condition_expression(&logical.left, body)?;
        let zero = body.push_expr(Expr {
            kind: match self.ctx.krate.types.get(rhs_ty) {
                Some(Type::Int) => ExprKind::Literal(Literal::Int(0)),
                _ => ExprKind::Literal(Literal::Float(0.0)),
            },
            ty: rhs_ty,
            span: self.span(logical.left.span().start, logical.left.span().end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: rhs,
                else_expr: zero,
            },
            ty: rhs_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || right` fallback expressions for optional values.
    ///
    /// Date-fns uses this for locale-width defaults. For optional left operands
    /// Smelt preserves the runtime value fallback with the same optional
    /// coalescing HIR shape used by `??`.
    pub(in crate::lowering) fn logical_or_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if logical.operator != LogicalOperator::Or {
            return Ok(None);
        }
        if let Expression::LogicalExpression(left_logical) =
            Self::unparenthesized_expression(&logical.left)
            && let Some(value) =
                self.logical_and_value_fallback_expression(logical, left_logical, body)?
        {
            return Ok(Some(value));
        }
        if let Expression::LogicalExpression(left_logical) =
            Self::unparenthesized_expression(&logical.left)
            && let Some(value) = self.logical_and_numeric_value_expression(left_logical, body)?
        {
            let value_ty = Self::expr_ty(body, value);
            if let Some(expr) =
                self.logical_or_numeric_fallback_expression(logical, body, value, value_ty)?
            {
                return Ok(Some(expr));
            }
        }
        let optional = self.expression(&logical.left, body)?;
        let optional_ty = Self::expr_ty(body, optional);
        if self.ctx.krate.types.get(optional_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(optional_ty)
        {
            let optional_receiver = self.optionalize_index_receiver(optional, body);
            let optional_receiver_ty = Self::expr_ty(body, optional_receiver);
            if self.is_nullishable_type(optional_receiver_ty) {
                let fallback =
                    self.expression_with_hint(&logical.right, body, Some(optional_receiver_ty))?;
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::OptionalCoalesce {
                        optional: optional_receiver,
                        fallback,
                    },
                    ty: self.ctx.krate.types.intern(Type::Unknown),
                    span: self.span(logical.span.start, logical.span.end),
                })));
            }
        }
        if !self.is_nullishable_type(optional_ty) {
            if let Some(expr) =
                self.logical_or_unknown_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_numeric_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_object_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_string_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_list_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            return Ok(None);
        }
        let Some(ty) = self.non_nullish_type(optional_ty) else {
            if matches!(logical.left, Expression::ChainExpression(_)) {
                return self.expression(&logical.right, body).map(Some);
            }
            return Ok(None);
        };
        if matches!(self.ctx.krate.types.get(ty), Some(Type::Function(_))) {
            let fallback = self.expression(&logical.right, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::OptionalCoalesce { optional, fallback },
                ty: self.ctx.krate.types.intern(Type::Unknown),
                span: self.span(logical.span.start, logical.span.end),
            })));
        }
        let fallback = self.expression_with_hint(&logical.right, body, Some(ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        let ty = if fallback_ty == ty {
            ty
        } else if self.ctx.krate.types.get(ty) == Some(&Type::Unknown)
            || self.ctx.krate.types.get(fallback_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(ty)
            || self.type_contains_unknown(fallback_ty)
        {
            self.ctx.krate.types.intern(Type::Unknown)
        } else if self.is_structural_object_surface(ty) {
            // Object values are always truthy in JavaScript; keep the selected
            // runtime value when their fallback widens the expression surface.
            self.ctx.krate.types.intern(Type::Unknown)
        } else if self.is_string_compatible_type(ty) && self.is_string_compatible_type(fallback_ty)
        {
            self.ctx.krate.types.intern(Type::String)
        } else {
            return Ok(None);
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce { optional, fallback },
            ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || right` for object-like values without string coercion.
    ///
    /// Some type aliases that include `null` are represented as object surfaces
    /// after TypeScript lowering. JavaScript still returns the selected operand
    /// for `||`, so object-like operands must branch on runtime truthiness before
    /// the string fallback path can treat classes as string-compatible values.
    pub(in crate::lowering) fn logical_or_object_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_structural_object_surface(value_ty) {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&logical.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        if !self.is_structural_object_surface(fallback_ty) {
            return Ok(None);
        }
        let cond = body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToBool,
                operand: value,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.left.span().start, logical.left.span().end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: self.ctx.krate.types.intern(Type::Unknown),
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || right` for erased values without losing the selected operand.
    ///
    /// Dynamic interop and imported structural callbacks can surface as
    /// `unknown` even when the source expression returns an object-like value.
    /// JavaScript `||` returns one of the original operands, so erased values
    /// must branch on runtime truthiness instead of being coerced through a
    /// string or boolean fallback representation.
    pub(in crate::lowering) fn logical_or_unknown_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.type_contains_unknown(value_ty) {
            return Ok(None);
        }
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let fallback = self.expression_with_hint(&logical.right, body, Some(unknown_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        if !self.type_contains_unknown(fallback_ty) {
            return Ok(None);
        }
        let cond = body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToBool,
                operand: value,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.left.span().start, logical.left.span().end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: unknown_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `(guard && value) || fallback` as a value fallback.
    ///
    /// The normal logical lowering produces booleans because `&&`/`||` are also
    /// used in conditions. In value positions, JavaScript preserves the selected
    /// operand. This shape appears in option-bag and locale lookup code where a
    /// guarded member access falls back to another member with the same value
    /// type.
    pub(in crate::lowering) fn logical_and_value_fallback_expression(
        &mut self,
        outer: &oxc::ast::ast::LogicalExpression<'_>,
        left: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if left.operator != LogicalOperator::And {
            return Ok(None);
        }
        let value = self.expression(&left.right, body)?;
        let value_ty = Self::expr_ty(body, value);
        if self.is_numeric_like_type(value_ty) {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&outer.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        let Some(result_ty) = self.logical_fallback_result_type(value_ty, fallback_ty) else {
            return Ok(None);
        };
        let cond = self.condition_expression(&outer.left, body)?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: result_ty,
            span: self.span(outer.span.start, outer.span.end),
        })))
    }

    /// Return the common value type for JavaScript logical fallback operands.
    pub(in crate::lowering) fn logical_fallback_result_type(
        &mut self,
        value_ty: smelt_hir::TypeId,
        fallback_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        if value_ty == fallback_ty {
            return Some(value_ty);
        }
        if self.is_string_compatible_type(value_ty) && self.is_string_compatible_type(fallback_ty) {
            return Some(self.ctx.krate.types.intern(Type::String));
        }
        if self.type_contains_unknown(value_ty) || self.type_contains_unknown(fallback_ty) {
            return Some(self.ctx.krate.types.intern(Type::Unknown));
        }
        match (
            self.ctx.krate.types.get(value_ty),
            self.ctx.krate.types.get(fallback_ty),
        ) {
            (Some(Type::Optional(value)), _) if *value == fallback_ty => Some(fallback_ty),
            (_, Some(Type::Optional(fallback))) if value_ty == *fallback => Some(value_ty),
            _ => None,
        }
    }

    /// Lower numeric JavaScript `left || right` value fallback expressions.
    ///
    /// Date-fns uses `numeric % 7 || 7` to replace zero with a default value.
    /// Lowering this as boolean `||` loses the numeric result type, so Smelt
    /// models the expression as `left != 0 ? left : right` for numeric operands.
    pub(in crate::lowering) fn logical_or_numeric_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_numeric_like_type(value_ty) {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&logical.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        if !self.numeric_type_compatible(value_ty, fallback_ty) {
            return Ok(None);
        }
        let zero = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(0.0)),
            ty: self.ctx.krate.types.intern(Type::Float),
            span: self.span(logical.span.start, logical.span.end),
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: value,
                rhs: zero,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.span.start, logical.span.end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: value_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || right` fallback expressions selected by string truthiness.
    ///
    /// A numeric fallback remains an erased selected value because expressions
    /// such as `+(parts[index] || 0)` numerically coerce either branch after
    /// selection. Emitting a boolean result would discard the string value.
    pub(in crate::lowering) fn logical_or_string_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_string_compatible_type(value_ty) {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&logical.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        let result_ty = if self.is_string_compatible_type(fallback_ty) {
            self.ctx.krate.types.intern(Type::String)
        } else if self.is_numeric_like_type(fallback_ty) {
            self.ctx.krate.types.intern(Type::Unknown)
        } else {
            return Ok(None);
        };
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let empty = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(String::new())),
            ty: string_ty,
            span: self.span(logical.span.start, logical.span.end),
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: value,
                rhs: empty,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.span.start, logical.span.end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: result_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || []` fallback expressions for array values.
    pub(in crate::lowering) fn logical_or_list_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(item_ty) = self.list_fallback_item_ty(value_ty) else {
            return Ok(None);
        };
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let fallback = self.expression_with_hint(&logical.right, body, Some(list_ty))?;
        if !Self::is_empty_list_expr(body, fallback) && Self::expr_ty(body, fallback) != list_ty {
            return Ok(None);
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce {
                optional: value,
                fallback,
            },
            ty: list_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Return the item type for a value that can participate in an array fallback.
    pub(in crate::lowering) fn list_fallback_item_ty(
        &mut self,
        value_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(value_ty).cloned() {
            Some(Type::List(item_ty)) => Some(item_ty),
            Some(Type::Optional(inner_ty)) => self.list_fallback_item_ty(inner_ty),
            Some(Type::Union(items)) => items
                .into_iter()
                .find_map(|item| self.list_fallback_item_ty(item)),
            _ => None,
        }
    }

    /// Lower TypeScript nullish coalescing while preserving falsey values.
    pub(in crate::lowering) fn nullish_coalesce_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let optional = self.expression(&logical.left, body)?;
        let optional_ty = Self::expr_ty(body, optional);
        let Some(ty) = self.non_nullish_type(optional_ty) else {
            if self.ctx.krate.types.get(optional_ty) == Some(&Type::None) {
                let fallback = self.expression(&logical.right, body)?;
                return Ok(fallback);
            }
            return Ok(optional);
        };
        let right_hint = match &logical.right {
            Expression::LogicalExpression(right_logical)
                if right_logical.operator == LogicalOperator::Coalesce =>
            {
                type_hint
            }
            _ => Some(ty),
        };
        let mut fallback = self.expression_with_hint(&logical.right, body, right_hint)?;
        let fallback_ty = Self::expr_ty(body, fallback);
        let ty = if fallback_ty == ty || self.numeric_type_compatible(ty, fallback_ty)
        {
            ty
        } else if self.ctx.krate.types.get(ty) == Some(&Type::Unknown) {
            // This is a genuine dynamic boundary: the successful left operand
            // is source `unknown`, so a concrete fallback must enter its tagged
            // runtime surface through an explicit boundary adapter.
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty,
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            ty
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && self.numeric_type_compatible(ty, fallback_inner)
        {
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && fallback_inner == ty
        {
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if matches!(self.ctx.krate.types.get(optional_ty), Some(Type::Optional(inner)) if *inner == ty)
            && self.erased_or_union_surface(fallback_ty)
        {
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && self.nullish_fallback_types_are_structurally_compatible(ty, fallback_inner)
        {
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty: smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty),
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if matches!(self.ctx.krate.types.get(ty), Some(Type::TypeParam { .. }))
            && matches!(
                self.ctx.krate.types.get(fallback_ty),
                Some(Type::TypeParam { .. })
            )
        {
            type_hint.unwrap_or_else(|| {
                self.ctx
                    .krate
                    .types
                    .intern(Type::Union(vec![ty, fallback_ty]))
            })
        } else if matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Union(items)) if items.contains(&fallback_ty)
        ) {
            // The fallback is already one arm of the value surface. Selecting
            // it does not narrow every successful left-hand value to that one
            // arm: `value ?? null` must retain the other union members.
            ty
        } else if self.nullish_fallback_matches_union_member(ty, fallback_ty) {
            ty
        } else if self.nullish_fallback_types_are_structurally_compatible(ty, fallback_ty) {
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty,
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            ty
        } else if let Some(hint) = type_hint
            && !self.concrete_type_requires_never_value(hint)
        {
            hint
        } else if self.erased_or_union_surface(ty)
            || self.erased_or_union_surface(fallback_ty)
            || !self.concrete_type_requires_never_value(ty)
        {
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty,
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            ty
        } else {
            return Err(SmeltError::unsupported(
                self.span(logical.span.start, logical.span.end),
                "nullish coalescing fallback must match the non-nullish value type",
            ));
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce { optional, fallback },
            ty,
            span: self.span(logical.span.start, logical.span.end),
        }))
    }

    /// Return whether a `??` fallback is covered by one member of the non-null union.
    pub(in crate::lowering) fn nullish_fallback_matches_union_member(
        &self,
        non_nullish_ty: smelt_hir::TypeId,
        fallback_ty: smelt_hir::TypeId,
    ) -> bool {
        let Some(Type::Union(items)) = self.ctx.krate.types.get(non_nullish_ty) else {
            return false;
        };
        items
            .iter()
            .copied()
            .any(|item| item == fallback_ty || self.numeric_type_compatible(item, fallback_ty))
    }

    /// Return whether `??` may treat the fallback as the optional side's object surface.
    ///
    /// TypeScript uses structural object compatibility, so date-fns can coalesce
    /// an optional `Locale` interface with a concrete exported locale object.
    /// Smelt keeps the optional side's type and inserts a typed assertion around
    /// the fallback expression when both sides are object-like surfaces.
    pub(in crate::lowering) fn nullish_fallback_types_are_structurally_compatible(
        &mut self,
        optional_inner: smelt_hir::TypeId,
        fallback_ty: smelt_hir::TypeId,
    ) -> bool {
        let fallback_ty = self.non_nullish_type(fallback_ty).unwrap_or(fallback_ty);
        self.is_structural_object_surface(optional_inner)
            && self.is_structural_object_surface(fallback_ty)
    }

    /// Return whether a type behaves as a structural object surface.
    pub(in crate::lowering) fn is_structural_object_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Class { .. } | Type::Dict(_, _) | Type::TypeParam { .. } | Type::Unknown,
            ) => true,
            Some(Type::Optional(item)) => self.is_structural_object_surface(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.is_structural_object_surface(item)),
            _ => false,
        }
    }

    /// Return the type left after removing TypeScript nullish values.
    pub(in crate::lowering) fn non_nullish_type(
        &mut self,
        ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        smelt_hir::type_normalize::non_nullish_type(&mut self.ctx.krate.types, ty)
    }

    /// Return whether `ty` still admits a TypeScript nullish value.
    ///
    /// Used to decide whether a source-level `!` has anything to narrow at a
    /// sink: an `Optional`, the bare `null` type, and a union carrying a `null`
    /// arm all accept the nullish value, so the assertion is purely type-level
    /// there. Checked structurally rather than by comparing
    /// [`Self::non_nullish_type`] against the input, because that helper also
    /// normalizes and so can report a difference for wholly non-nullish types.
    pub(in crate::lowering) fn type_admits_nullish(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(_) | Type::None) => true,
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.type_admits_nullish(item)),
            _ => false,
        }
    }

    /// Lower a TypeScript non-null assertion while preserving the narrowed type.
    pub(in crate::lowering) fn non_null_assertion_expression(
        &mut self,
        expression: &Expression<'_>,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(expression, body)?;
        Ok(self.non_null_assertion_value(value, span, body))
    }

    /// Apply non-null assertion narrowing to an already-lowered expression.
    pub(in crate::lowering) fn non_null_assertion_value(
        &mut self,
        value: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let value_ty = Self::expr_ty(body, value);
        let Some(non_null_ty) = self.non_nullish_type(value_ty) else {
            return value;
        };
        if non_null_ty == value_ty {
            return value;
        }
        body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty: non_null_ty,
            span,
        })
    }

    /// Fold a `"<key>" in <global-alias>` feature probe to a literal.
    ///
    /// The receiver must be a recognized global alias (bare `globalThis` /
    /// `global` / `self`, or a local known to alias the global object) and the key
    /// must be a string literal — a dynamic key is on the erasure denylist and
    /// stays a runtime check. The presence answer is derived from the
    /// recognition registries via [`smelt_stdlib::global_member_presence`], so an
    /// unmodeled key (`Unknown`) is *not* folded: it returns `None` and falls
    /// through to ordinary lowering instead of guessing.
    pub(in crate::lowering) fn global_contains_key_probe(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if !self.expr_is_global_alias(&binary.right) {
            return None;
        }
        let Expression::StringLiteral(key_lit) = &binary.left else {
            return None;
        };
        let presence = smelt_stdlib::global_member_presence(key_lit.value.as_str());
        let value = match presence {
            smelt_stdlib::GlobalPresence::Present => true,
            smelt_stdlib::GlobalPresence::Absent => false,
            // `Unknown` (and any future undecided presence) must not fold.
            _ => return None,
        };
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(value)),
            ty: bool_ty,
            span: self.span(binary.span.start, binary.span.end),
        }))
    }

    /// Lower JavaScript `key in object` checks for dictionaries and static objects.
    ///
    /// Static object constants are often erased to reusable metadata before a
    /// function body is lowered. For those, membership is a pure key-set test,
    /// so emitting string equality checks keeps the generated Rust independent
    /// from a runtime object allocation.
    pub(in crate::lowering) fn in_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(binary.span.start, binary.span.end);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let string_ty = self.ctx.krate.types.intern(Type::String);

        if let Some(expr) = self.global_contains_key_probe(binary, body) {
            return Ok(expr);
        }

        // A `<key> in <global-alias>` membership test that the registry-derived
        // probe above could not fold (an unknown/undecided member, or a
        // non-literal key) must stay an honest blocker. The global object now
        // resolves to a marker host-object value (see
        // `global_object_value_expression`), so without this guard the test
        // would silently evaluate against the empty marker record and answer
        // `false` for members the real global actually has. Presence of the
        // global object as a value does not make its full key set known.
        if self.expr_is_global_alias(&binary.right) {
            return Err(SmeltError::unsupported(
                span,
                "`in` on the global object is only lowered for registry-decidable string-literal keys",
            ));
        }

        if let Expression::Identifier(receiver_ident) = &binary.right
            && let Some(object_const) = self.consts.object(receiver_ident.name.as_str())
                .cloned()
        {
            let mut key = self.expression(&binary.left, body)?;
            if Self::expr_ty(body, key) != string_ty {
                key = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: key },
                    ty: string_ty,
                    span: self.span(binary.left.span().start, binary.left.span().end),
                });
            }
            let mut condition = None;
            for entry in object_const.entries {
                let rhs = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(entry.key)),
                    ty: string_ty,
                    span,
                });
                let equals_key = body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Eq,
                        lhs: key,
                        rhs,
                    },
                    ty: bool_ty,
                    span,
                });
                condition = Some(condition.map_or(equals_key, |previous| {
                    body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Or,
                            lhs: previous,
                            rhs: equals_key,
                        },
                        ty: bool_ty,
                        span,
                    })
                }));
            }
            return Ok(condition.unwrap_or_else(|| {
                body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(false)),
                    ty: bool_ty,
                    span,
                })
            }));
        }

        let receiver = self.expression(&binary.right, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let mut key = self.expression(&binary.left, body)?;
        if matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Optional(_))
        ) && matches!(&binary.left, Expression::StringLiteral(value) if value.value == "done")
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty: bool_ty,
                span,
            }));
        }
        // `"field" in unionLocal` over a union of concrete member shapes can be
        // answered by a static discriminant test in codegen rather than erasing
        // the value into a runtime object map. Keeping the receiver at its union
        // type lets `dict_contains_key_text` emit `matches!(x, Union::Mi(_) ..)`
        // for the arms that carry `field`. This only fires for a string-literal
        // key; dynamic keys stay on the erased path below. Nullish/dynamic
        // boundaries are untouched because concrete-union eligibility (checked in
        // codegen) excludes `Optional`/`unknown` members.
        if matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::Union(_)))
            && matches!(&binary.left, Expression::StringLiteral(_))
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DictContainsKey {
                    dict: receiver,
                    key,
                },
                ty: bool_ty,
                span,
            }));
        }
        let Some(Type::Dict(receiver_key_ty, _)) = self.ctx.krate.types.get(receiver_ty) else {
            if self.ctx.krate.types.get(receiver_ty) == Some(&Type::Unknown)
                || self.erased_or_union_surface(receiver_ty)
                || matches!(
                    self.ctx.krate.types.get(receiver_ty),
                    // A union of concrete members (`string | string[]` after a
                    // typeof guard Smelt's erased locals do not re-type) is a
                    // dynamic surface for `in` just like an erased union.
                    Some(
                        Type::TypeParam { .. }
                            | Type::Class { .. }
                            | Type::List(_)
                            | Type::Tuple(_)
                            | Type::String
                            | Type::Union(_)
                    )
                )
            {
                let receiver = if self.ctx.krate.types.get(receiver_ty) == Some(&Type::Unknown) {
                    receiver
                } else {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: receiver },
                        ty,
                        span,
                    })
                };
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::DictContainsKey {
                        dict: receiver,
                        key,
                    },
                    ty: bool_ty,
                    span,
                }));
            }
            return Err(SmeltError::unsupported(
                span,
                "`in` checks require a static object, record, map, or unknown receiver",
            ));
        };
        let key_ty = *receiver_key_ty;
        if Self::expr_ty(body, key) != key_ty
            && self.is_string_compatible_type(Self::expr_ty(body, key))
        {
            key = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: key },
                ty: key_ty,
                span,
            });
        }
        if Self::expr_ty(body, key) != key_ty {
            return Err(SmeltError::unsupported(
                span,
                "`in` check key must match the record or map key type",
            ));
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictContainsKey {
                dict: receiver,
                key,
            },
            ty: bool_ty,
            span,
        }))
    }

    /// Lower a unary expression.
    pub(in crate::lowering) fn unary_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if unary.operator == UnaryOperator::Delete {
            return self.delete_unary_expression(unary, body);
        }
        if unary.operator == UnaryOperator::Typeof {
            return self.typeof_expression(unary, body);
        }
        if unary.operator == UnaryOperator::Void {
            let ty = self.ctx.krate.types.intern(Type::None);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Undefined),
                ty,
                span: self.span(unary.span.start, unary.span.end),
            }));
        }
        let op = match unary.operator {
            UnaryOperator::LogicalNot => UnaryOp::Not,
            UnaryOperator::UnaryNegation => UnaryOp::Neg,
            UnaryOperator::UnaryPlus => {
                let operand = self.expression(&unary.argument, body)?;
                let operand_ty = Self::expr_ty(body, operand);
                if self.is_numeric_like_type(operand_ty) {
                    return Ok(operand);
                }
                if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Bool))
                    || self.is_date_constructor_arg_type(operand_ty)
                {
                    let ty = self.ctx.krate.types.intern(Type::Float);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::PrimitiveCast {
                            op: PrimitiveCastOp::ToJsNumber,
                            operand,
                        },
                        ty,
                        span: self.span(unary.span.start, unary.span.end),
                    }));
                }
                return Err(SmeltError::unsupported(
                    self.span(unary.span.start, unary.span.end),
                    "unary plus requires a numeric or DateArg-compatible operand",
                ));
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(unary.span.start, unary.span.end),
                    format!("unary operator is not lowered yet: {:?}", unary.operator),
                ));
            }
        };
        let operand = self.expression(&unary.argument, body)?;
        let operand = if matches!(op, UnaryOp::Not) {
            self.optional_known_date_presence_condition(
                operand,
                self.span(unary.argument.span().start, unary.argument.span().end),
                body,
            )
            .unwrap_or(operand)
        } else {
            operand
        };
        let ty = match op {
            UnaryOp::Not => self.ctx.krate.types.intern(Type::Bool),
            UnaryOp::Neg => Self::expr_ty(body, operand),
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnaryOp { op, operand },
            ty,
            span: self.span(unary.span.start, unary.span.end),
        }))
    }

    /// Lower JavaScript `delete object[key]` to a dictionary key removal.
    pub(in crate::lowering) fn delete_unary_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let (object, key) = match &unary.argument {
            Expression::ComputedMemberExpression(member) => {
                let object = self.expression(&member.object, body)?;
                let key = self.expression(&member.expression, body)?;
                (object, key)
            }
            Expression::StaticMemberExpression(member) => {
                let object = self.expression(&member.object, body)?;
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let key = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(member.property.name.to_string())),
                    ty: string_ty,
                    span: self.span(member.property.span.start, member.property.span.end),
                });
                (object, key)
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(unary.argument.span().start, unary.argument.span().end),
                    "delete is only lowered for object keys",
                ));
            }
        };
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictRemoveKey { dict: object, key },
            ty,
            span: self.span(unary.span.start, unary.span.end),
        }))
    }

    /// Lower an array expression.
    pub(in crate::lowering) fn array_expression(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if array
            .elements
            .iter()
            .any(|element| matches!(element, ArrayExpressionElement::SpreadElement(_)))
        {
            return self.array_expression_with_spread(array, body, type_hint);
        }
        let mut items = Vec::new();
        let tuple_hints = type_hint.and_then(|hint| match self.ctx.krate.types.get(hint) {
            Some(Type::Tuple(tuple_items)) => Some(tuple_items.clone()),
            _ => None,
        });
        // A `List(item)` hint contextually types every element at `item` (e.g.
        // `[[1, 'a'], [2, 'b']]` hinted `SmeltList<(f64, String)>` lowers each
        // inner literal as the `(f64, String)` tuple). Only the homogeneous list
        // element type is propagated; the per-index tuple path above still wins
        // when the hint is itself a tuple.
        let list_element_hint = type_hint.and_then(|hint| match self.ctx.krate.types.get(hint) {
            Some(Type::List(item)) => Some(*item),
            _ => None,
        });
        for (index, element) in array.elements.iter().enumerate() {
            if matches!(element, ArrayExpressionElement::SpreadElement(_)) {
                return Err(SmeltError::unsupported(
                    self.span(element.span().start, element.span().end),
                    "array spread elements are not lowered",
                ));
            }
            let element_hint = tuple_hints
                .as_ref()
                .and_then(|hints| hints.get(index).copied())
                .or(list_element_hint);
            // Arity guard: a tuple hint applied to an inner array literal whose
            // element count differs (a ragged expected value such as
            // `cartesianProduct`'s rows) would force a wrong tuple shape, so drop
            // the hint and let the element infer its own type instead.
            let element_hint = self.array_element_hint_matches_arity(element, element_hint, body);
            let item = if let ArrayExpressionElement::Elision(elision) = element {
                // A HOLE in an array literal (`[1, , 2]`) reads as `undefined`,
                // never as `null`: the index is absent, and every absent
                // property read in JavaScript answers `undefined`. Lowering it
                // to `null` made `[1, , 2]` and `[1, null, 2]` indistinguishable
                // and `[...new Set([1, , 2])]` produce `null` where JavaScript
                // produces `undefined`.
                let ty = element_hint.unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Undefined),
                    ty,
                    span: self.span(elision.span.start, elision.span.end),
                })
            } else {
                self.array_element_with_hint(element, body, element_hint)?
            };
            items.push(item);
        }
        // Adopt the whole-literal hint unless it can never hold an array value.
        // A `Function` hint (e.g. a data-last overload's `Fn(...) -> ...` type
        // flowing in as a contextual hint from a deep-equality matcher) must not
        // be stamped onto the array literal, which would emit a
        // `let tmp: Rc<dyn Fn(...)> = vec![...]` mismatch (E0308). Erased
        // (`Unknown`), union, and `never`-assertion hints legitimately type array
        // literals, so only function-shaped hints are rejected; then fall back to
        // the inferred list type as if no hint was given.
        let array_compatible_hint = type_hint.filter(|hint| {
            !matches!(self.ctx.krate.types.get(*hint), Some(Type::Function(_)))
        });
        let ty = if let Some(hint) = array_compatible_hint {
            hint
        } else if !items.is_empty() {
            let item_ty = self.array_literal_item_type(&items, body);
            self.ctx.krate.types.intern(Type::List(item_ty))
        } else {
            let item_ty = self.ctx.krate.types.intern(Type::Unknown);
            self.ctx.krate.types.intern(Type::List(item_ty))
        };
        if self.array_literal_needs_never_value(ty, items.len()) {
            return Err(SmeltError::unsupported(
                self.span(array.span.start, array.span.end),
                "array or tuple literal cannot construct a never value",
            ));
        }
        Ok(body.push_expr(Expr {
            kind: if matches!(self.ctx.krate.types.get(ty), Some(Type::Tuple(_))) {
                ExprKind::TupleLit(items)
            } else {
                ExprKind::ListLit(items)
            },
            ty,
            span: self.span(array.span.start, array.span.end),
        }))
    }

    /// Infer one item type for an array literal, preserving nullability when needed.
    pub(in crate::lowering) fn array_literal_item_type(
        &mut self,
        items: &[smelt_hir::ExprId],
        body: &Body,
    ) -> smelt_hir::TypeId {
        let item_tys = items
            .iter()
            .map(|item| Self::expr_ty(body, *item))
            .collect::<Vec<_>>();
        let Some(first_ty) = item_tys.first().copied() else {
            return self.ctx.krate.types.intern(Type::Unknown);
        };
        if item_tys.iter().all(|item_ty| *item_ty == first_ty) {
            return first_ty;
        }
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let non_nullish = item_tys
            .iter()
            .copied()
            .filter(|item_ty| *item_ty != none_ty)
            .collect::<Vec<_>>();
        if let Some(first_non_nullish) = non_nullish.first().copied()
            && non_nullish
                .iter()
                .all(|item_ty| *item_ty == first_non_nullish)
            && item_tys.contains(&none_ty)
            && !Self::array_literal_mixes_nullish_spellings(items, body)
        {
            return self
                .ctx
                .krate
                .types
                .intern(Type::Optional(first_non_nullish));
        }
        self.ctx.krate.types.intern(Type::Unknown)
    }

    /// Report whether an array literal writes BOTH `null` and `undefined`.
    ///
    /// `null` and `undefined` share one HIR type (`Type::None`), so a literal
    /// mixing them looks uniformly nullish to the element-type join above and
    /// used to collapse into `Optional(T)`. `Option` has a single empty state,
    /// so both spellings then lower to the same `None` and the distinction is
    /// gone by the time the list erases to `SmeltUnknown` -- es-toolkit's
    /// `isEqualWith` primitives table generated byte-identical Rust for its
    /// `[null, null, true]` and `[undefined, undefined, true]` rows, and
    /// answered `true` for `[null, undefined, false]`.
    ///
    /// The literals themselves stay distinct (`Literal::None` vs
    /// `Literal::Undefined`), so the mix is recoverable here. When it is
    /// present the join falls through to its existing heterogeneous answer,
    /// `Unknown`, which keeps each element's own tag.
    ///
    /// `Unknown` is a genuine dynamic boundary here, not a convenience: the
    /// type system has no concrete spelling for `T | null | undefined`.
    /// `Optional(T)` holds one empty state. `Optional(Optional(T))` would hold
    /// two, but `smelt_hir::type_normalize` canonically flattens it back to
    /// `Optional(T)` (`normalize_optional_type`), and a whole-MIR pass applies
    /// that normalization to every type. `Union([T, None])` is flattened by the
    /// same pass (`flatten_union_none`), so a generated `SmeltUnion` enum
    /// cannot carry the two spellings either, and a scoped generic only defers
    /// the same choice to its instantiation. `NormalizeOptions` does expose
    /// `preserve_observable_absence`, which would suppress both flattenings,
    /// but nothing enables it -- making it meaningful is a global change to the
    /// canonical form and is tracked separately. `flattens_nested_optional_types`
    /// and `flattens_union_none_into_optional` in `smelt_hir::type_normalize`
    /// pin that limitation, and `mixed_nullish_array_literal_keeps_both_spellings`
    /// below pins the behaviour this boundary buys.
    fn array_literal_mixes_nullish_spellings(items: &[smelt_hir::ExprId], body: &Body) -> bool {
        let mut has_null = false;
        let mut has_undefined = false;
        for item in items {
            let index = usize::try_from(item.0).expect("expr id should fit into usize");
            let Some(expr) = body.exprs.get(index) else {
                continue;
            };
            match &expr.kind {
                ExprKind::Literal(Literal::None) => has_null = true,
                ExprKind::Literal(Literal::Undefined) => has_undefined = true,
                _ => {}
            }
        }
        has_null && has_undefined
    }

    /// Drop a tuple element hint whose arity does not match an inner array
    /// literal.
    ///
    /// A `List(Tuple(..))` hint propagates the tuple type to every element, but
    /// a deep-equality expected value can be ragged (e.g. `cartesianProduct`'s
    /// rows, or `zip`'s trailing `[3, undefined]`). Forcing a fixed-arity tuple
    /// onto a differently-sized literal would misshape it, so the hint is only
    /// kept when the literal's element count matches the tuple arity. Non-array
    /// elements and non-tuple hints are returned unchanged.
    fn array_element_hint_matches_arity(
        &self,
        element: &ArrayExpressionElement<'_>,
        element_hint: Option<smelt_hir::TypeId>,
        _body: &Body,
    ) -> Option<smelt_hir::TypeId> {
        let hint = element_hint?;
        let Some(Type::Tuple(tuple_items)) = self.ctx.krate.types.get(hint) else {
            return element_hint;
        };
        let ArrayExpressionElement::ArrayExpression(inner) = element else {
            return element_hint;
        };
        if inner
            .elements
            .iter()
            .any(|element| matches!(element, ArrayExpressionElement::SpreadElement(_)))
        {
            return None;
        }
        if inner.elements.len() == tuple_items.len() {
            element_hint
        } else {
            None
        }
    }

    /// Lower an array literal element with contextual type information.
    pub(in crate::lowering) fn array_element_with_hint(
        &mut self,
        element: &ArrayExpressionElement<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match element {
            ArrayExpressionElement::ArrayExpression(array) => {
                self.array_expression(array, body, type_hint)
            }
            ArrayExpressionElement::ObjectExpression(object) => {
                self.object_expression(object, body, type_hint)
            }
            _ => self.array_element(element, body),
        }
    }

    /// Lower an array literal that contains one or more spread elements.
    pub(in crate::lowering) fn array_expression_with_spread(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if type_hint.is_none()
            && let [ArrayExpressionElement::SpreadElement(spread)] = array.elements.as_slice()
        {
            let spread_value = self.expression(&spread.argument, body)?;
            let value_ty = self.type_param_constraint_or_self(Self::expr_ty(body, spread_value));
            let item_ty = match self.ctx.krate.types.get(value_ty) {
                Some(Type::List(item_ty) | Type::Set(item_ty)) => *item_ty,
                Some(Type::String) => self.ctx.krate.types.intern(Type::String),
                Some(
                    Type::Unknown
                    | Type::TypeParam { .. }
                    | Type::Class { .. }
                    | Type::Optional(_)
                    | Type::Union(_),
                )
                | None => self.ctx.krate.types.intern(Type::Unknown),
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(spread.span.start, spread.span.end),
                        "array spread operands must be arrays or sets",
                    ));
                }
            };
            let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
            return self.list_expr_from_spread_value(spread_value, list_ty, spread.span, body);
        }
        // Two-phase lowering: first lower every element/spread operand in
        // source order (preserving evaluation order and side effects), then
        // unify their item types, and finally assemble the `ListLit`/
        // `ListConcat` chain with the unified list type. Lowering before type
        // assembly lets homogeneous literals such as `[...typedArrays,
        // 'DataView']` keep their concrete item type instead of erasing to
        // `List<Unknown>` and mismatching against the typed spread operand.
        let mut pieces = Vec::new();
        for element in &array.elements {
            match element {
                ArrayExpressionElement::SpreadElement(spread) => {
                    let spread_value = self.expression(&spread.argument, body)?;
                    pieces.push(SpreadPiece::Spread(spread_value, spread.span));
                }
                ArrayExpressionElement::Elision(_) => {
                    return Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "array elisions are not lowered",
                    ));
                }
                _ => pieces.push(SpreadPiece::Item(self.array_element(element, body)?)),
            }
        }
        let piece_exprs: Vec<(smelt_hir::ExprId, bool)> = pieces
            .iter()
            .map(|piece| match piece {
                SpreadPiece::Spread(value, _) => (*value, true),
                SpreadPiece::Item(value) => (*value, false),
            })
            .collect();
        let item_ty = self.array_spread_item_type(&piece_exprs, body, type_hint);
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let mut packed = None;
        let mut current_items = Vec::new();
        let mut append = |target: &mut Body, right: smelt_hir::ExprId| {
            packed = Some(packed.map_or(right, |left| {
                target.push_expr(Expr {
                    kind: ExprKind::ListConcat { left, right },
                    ty: list_ty,
                    span: self.span(array.span.start, array.span.end),
                })
            }));
        };
        for piece in &pieces {
            match piece {
                SpreadPiece::Spread(spread_value, spread_span) => {
                    if !current_items.is_empty() {
                        let right = body.push_expr(Expr {
                            kind: ExprKind::ListLit(std::mem::take(&mut current_items)),
                            ty: list_ty,
                            span: self.span(array.span.start, array.span.end),
                        });
                        append(body, right);
                    }
                    let right = self.list_expr_from_spread_value(
                        *spread_value,
                        list_ty,
                        *spread_span,
                        body,
                    )?;
                    append(body, right);
                }
                SpreadPiece::Item(item) => current_items.push(*item),
            }
        }
        if !current_items.is_empty() {
            let right = body.push_expr(Expr {
                kind: ExprKind::ListLit(current_items),
                ty: list_ty,
                span: self.span(array.span.start, array.span.end),
            });
            append(body, right);
        }
        packed.ok_or_else(|| {
            SmeltError::unsupported(
                self.span(array.span.start, array.span.end),
                "array spread literal requires at least one element",
            )
        })
    }

    /// Infer the list item type for an array spread literal from its lowered
    /// pieces.
    ///
    /// A `List` item type from the contextual hint wins. Otherwise every
    /// lowered piece contributes one candidate item type — a spread operand
    /// contributes its unwrapped `List`/`Set` item type (or `String` for
    /// string spreads), a plain element contributes its own expression type —
    /// and the literal keeps the single unified candidate when they all
    /// agree. Mixed or erased candidates fall back to `Unknown`, matching the
    /// previous blanket behavior for genuinely heterogeneous literals.
    pub(in crate::lowering) fn array_spread_item_type(
        &mut self,
        pieces: &[(smelt_hir::ExprId, bool)],
        body: &Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        if let Some(hint) = type_hint
            && let Some(Type::List(item_ty)) = self.ctx.krate.types.get(hint)
        {
            return *item_ty;
        }
        let unknown = self.ctx.krate.types.intern(Type::Unknown);
        let mut unified: Option<smelt_hir::TypeId> = None;
        for &(value, is_spread) in pieces {
            let candidate = if is_spread {
                let value_ty = self.type_param_constraint_or_self(Self::expr_ty(body, value));
                match self.ctx.krate.types.get(value_ty) {
                    Some(Type::List(item_ty) | Type::Set(item_ty)) => *item_ty,
                    Some(Type::String) => self.ctx.krate.types.intern(Type::String),
                    _ => unknown,
                }
            } else {
                Self::expr_ty(body, value)
            };
            match unified {
                None => unified = Some(candidate),
                Some(existing) if existing == candidate => {}
                Some(_) => return unknown,
            }
        }
        unified.unwrap_or(unknown)
    }

    /// Convert an iterable spread operand into the list value required by list concatenation.
    pub(in crate::lowering) fn list_expr_from_spread_value(
        &self,
        value: smelt_hir::ExprId,
        list_ty: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // The operand's *own* declared type, BEFORE resolving a type parameter to
        // its constraint. An erased operand (`Type::TypeParam`/`Type::Unknown`)
        // whose constraint happens to be a list (e.g. Remeda's
        // `T extends IterableContainer = readonly unknown[]`) would otherwise hit
        // the `Type::List` arm below and be returned UNCHANGED — an alias that keeps
        // the erased type. That alias defeats typed list operations: a later
        // `[...items].sort(cmp)` stays dynamic and the sort result is discarded
        // (see blocker-logs/plan-sort-sortby-2026-06-23.md, Family 1, Option B).
        let raw_value_ty = Self::expr_ty(body, value);
        let value_ty = self.type_param_constraint_or_self(raw_value_ty);
        match self.ctx.krate.types.get(value_ty).cloned() {
            // A spread of an erased operand with a list constraint: construct a
            // FRESH `List`-typed value instead of returning the erased alias, so the
            // binding (`const ret = [...items]`) is a real `Vec` and downstream
            // typed list methods (e.g. in-place `sort`) fire. Reuse the verified
            // fresh-list idiom `ListConcat(value, [])`, which the multi-spread path
            // also uses; its emitter materializes a fresh `Vec` for erased operands.
            // A `[...list]` spread is a NEW array in JS, never an alias of its
            // source. Build it via the verified fresh-list idiom
            // `ListConcat(value, [])` (also used by the multi-spread path): it
            // coerces element types, materializes a fresh `Vec` for an erased
            // operand, and (via the empty-concat `fresh_copy`) mints a fresh
            // reference id for a concrete list — so the result never `===` source.
            Some(Type::List(_)) => {
                let empty = body.push_expr(Expr {
                    kind: ExprKind::ListLit(Vec::new()),
                    ty: list_ty,
                    span: self.span(span.start, span.end),
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::ListConcat {
                        left: value,
                        right: empty,
                    },
                    ty: list_ty,
                    span: self.span(span.start, span.end),
                }))
            }
            Some(Type::Set(_)) => Ok(body.push_expr(Expr {
                kind: ExprKind::SetProjection {
                    op: SetProjectionOp::Values,
                    set: value,
                },
                ty: list_ty,
                span: self.span(span.start, span.end),
            })),
            Some(Type::String) => Ok(body.push_expr(Expr {
                kind: ExprKind::StringChars { haystack: value },
                ty: list_ty,
                span: self.span(span.start, span.end),
            })),
            Some(Type::Unknown | Type::TypeParam { .. }) => Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: list_ty,
                span: self.span(span.start, span.end),
            })),
            Some(Type::Class { .. } | Type::Optional(_) | Type::Union(_)) | None => Ok(body
                .push_expr(Expr {
                    kind: ExprKind::TypeAssert { value },
                    ty: list_ty,
                    span: self.span(span.start, span.end),
                })),
            _ => Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "array spread operands must be arrays or sets",
            )),
        }
    }

    /// Lower an object expression.
    pub(in crate::lowering) fn object_expression(
        &mut self,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if object
            .properties
            .iter()
            .any(|property| matches!(property, ObjectPropertyKind::SpreadProperty(_)))
        {
            return self.object_expression_with_spread(object, body, type_hint);
        }

        let mut entries = Vec::new();
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(property.span().start, property.span().end),
                    "object spread properties are not lowered yet",
                ));
            };
            if object_property.kind == PropertyKind::Get {
                if Self::is_computed_symbol_key(object_property) {
                    continue;
                }
                let key = self.object_property_key_expr(object_property, body)?;
                let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                let getter =
                    if let Expression::FunctionExpression(function) = &object_property.value {
                        let getter_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                            params: Vec::new(),
                            rest: None,
                            required_params: None,
                            mutable_params: Vec::new(),
                            return_ty: unknown_ty,
                            is_async: false,
                            may_throw: false,
                        }));
                        self.function_expression_value(
                            function,
                            Some(getter_ty),
                            object_property.span,
                            body,
                        )?
                    } else {
                        self.object_property_value_expr(object_property, body, Some(unknown_ty))?
                    };
                let marker_key_ty = self.ctx.krate.types.intern(Type::String);
                let marker_key = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String("__smelt_get".to_owned())),
                    ty: marker_key_ty,
                    span: self.span(
                        object_property.key.span().start,
                        object_property.key.span().end,
                    ),
                });
                let marker_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Dict(marker_key_ty, unknown_ty));
                let value = body.push_expr(Expr {
                    kind: ExprKind::DictLit(vec![(marker_key, getter)]),
                    ty: marker_ty,
                    span: self.span(object_property.span.start, object_property.span.end),
                });
                entries.push((key, value));
                continue;
            }
            // A METHOD SHORTHAND is a function-valued property, and nothing
            // else: `{ f() { .. } }` and `{ f: function () { .. } }` build the
            // same object. Only the iterable-marker spelling was lowered as a
            // real function; every other method became `null`, so a descriptor
            // table's `get() { return 2 }`, an object of callbacks written in
            // shorthand, and `{ toString() { .. } }` all silently lost their
            // body with no diagnostic. Lowering every method through the same
            // function-expression path removes the special case rather than
            // adding one.
            if object_property.method {
                if let Expression::FunctionExpression(function) = &object_property.value {
                    let key = self.object_property_key_expr(object_property, body)?;
                    let value =
                        self.function_expression_value(function, None, object_property.span, body)?;
                    entries.push((key, value));
                    continue;
                }
                let key = self.object_property_key_expr(object_property, body)?;
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                let value = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(object_property.span.start, object_property.span.end),
                });
                entries.push((key, value));
                continue;
            }
            let key = self.object_property_key_expr(object_property, body)?;
            let value_hint = self.object_property_value_hint(object_property, type_hint);
            let value = self.object_property_value_expr(object_property, body, value_hint)?;
            entries.push((key, value));
        }
        let ty = self.object_literal_type(&entries, type_hint, body);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty,
            span: self.span(object.span.start, object.span.end),
        }))
    }

    /// Lower an object expression that uses JavaScript spread properties.
    ///
    /// The spread order is preserved by lowering each contiguous explicit
    /// property run into a dictionary literal and combining those chunks with
    /// spread sources through the ordered `DictAssign` operation.
    pub(in crate::lowering) fn object_expression_with_spread(
        &mut self,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut record_ty = self.dict_type_from_hint(type_hint);
        let mut sources = Vec::new();
        let mut pending_entries = Vec::new();
        let mut erased_spread_requires_unknown_record = false;

        for property in &object.properties {
            match property {
                ObjectPropertyKind::ObjectProperty(object_property) => {
                    // Same rule as the non-spread path above: a method
                    // shorthand is a function-valued property.
                    if object_property.method {
                        if let Expression::FunctionExpression(function) = &object_property.value {
                            let key = self.object_property_key_expr(object_property, body)?;
                            let value = self.function_expression_value(
                                function,
                                None,
                                object_property.span,
                                body,
                            )?;
                            pending_entries.push((key, value));
                            continue;
                        }
                        let key = self.object_property_key_expr(object_property, body)?;
                        let ty = self.ctx.krate.types.intern(Type::Unknown);
                        let value = body.push_expr(Expr {
                            kind: ExprKind::Literal(Literal::None),
                            ty,
                            span: self.span(object_property.span.start, object_property.span.end),
                        });
                        pending_entries.push((key, value));
                        continue;
                    }
                    let key = self.object_property_key_expr(object_property, body)?;
                    let value_hint = self.object_property_value_hint(object_property, record_ty);
                    let value =
                        self.object_property_value_expr(object_property, body, value_hint)?;
                    pending_entries.push((key, value));
                }
                ObjectPropertyKind::SpreadProperty(spread) => {
                    self.flush_object_spread_entries(
                        &mut pending_entries,
                        &mut sources,
                        &mut record_ty,
                        &mut erased_spread_requires_unknown_record,
                        body,
                        object.span,
                    );
                    if let Some(source) = self.conditional_object_spread_source(
                        &spread.argument,
                        record_ty,
                        body,
                        spread.span,
                    )? {
                        let source_ty = Self::expr_ty(body, source);
                        if record_ty.is_none()
                            && matches!(self.ctx.krate.types.get(source_ty), Some(Type::Dict(_, _)))
                        {
                            record_ty = Some(source_ty);
                        }
                        sources.push(source);
                        continue;
                    }
                    let mut source =
                        self.expression_with_hint(&spread.argument, body, record_ty)?;
                    let source_ty = Self::expr_ty(body, source);
                    if self.object_spread_source_erases_to_empty(source_ty) {
                        let ty = record_ty.unwrap_or_else(|| {
                            let key_ty = self.ctx.krate.types.intern(Type::String);
                            let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                            self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
                        });
                        source = body.push_expr(Expr {
                            kind: ExprKind::DictLit(Vec::new()),
                            ty,
                            span: self.span(spread.span.start, spread.span.end),
                        });
                    } else if self
                        .accept_object_spread_source(source_ty, record_ty, spread.span)
                        .is_err()
                    {
                        let ty = record_ty.unwrap_or_else(|| {
                            let key_ty = self.ctx.krate.types.intern(Type::String);
                            let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                            self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
                        });
                        source = body.push_expr(Expr {
                            kind: ExprKind::DictLit(Vec::new()),
                            ty,
                            span: self.span(spread.span.start, spread.span.end),
                        });
                    }
                    let final_source_ty = Self::expr_ty(body, source);
                    if record_ty.is_none()
                        && matches!(
                            self.ctx.krate.types.get(final_source_ty),
                            Some(Type::Dict(_, _))
                        )
                    {
                        record_ty = Some(final_source_ty);
                    } else if self.object_spread_source_needs_unknown_record(final_source_ty) {
                        erased_spread_requires_unknown_record = true;
                    }
                    sources.push(source);
                }
            }
        }
        self.flush_object_spread_entries(
            &mut pending_entries,
            &mut sources,
            &mut record_ty,
            &mut erased_spread_requires_unknown_record,
            body,
            object.span,
        );

        let key_ty = self.ctx.krate.types.intern(Type::String);
        let fallback_value_ty = self.ctx.krate.types.intern(Type::Unknown);
        let record_ty = record_ty.unwrap_or_else(|| {
            self.ctx
                .krate
                .types
                .intern(Type::Dict(key_ty, fallback_value_ty))
        });
        let target = body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty: record_ty,
            span: self.span(object.span.start, object.span.start),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictAssign { target, sources },
            ty: record_ty,
            span: self.span(object.span.start, object.span.end),
        }))
    }

    /// Lower `...(condition && { ... })` object spread sources to conditional records.
    ///
    /// JavaScript object spread treats falsey primitives as empty sources. The
    /// HIR spread operation expects object-like sources, so this helper keeps the
    /// object branch typed as a record and supplies an empty record for the
    /// false branch instead of exposing the boolean result of `&&`.
    pub(in crate::lowering) fn conditional_object_spread_source(
        &mut self,
        argument: &Expression<'_>,
        record_ty: Option<smelt_hir::TypeId>,
        body: &mut Body,
        span: oxc::span::Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let argument = Self::object_spread_condition_source(argument);
        let Expression::LogicalExpression(logical) = argument else {
            return Ok(None);
        };
        if logical.operator != LogicalOperator::And
            || !matches!(&logical.right, Expression::ObjectExpression(_))
        {
            return Ok(None);
        }
        let cond = self.expression(&logical.left, body)?;
        let rhs_narrowing = self.guard_narrowing(&logical.left, body);
        if let Some(narrowing) = rhs_narrowing.clone() {
            self.scope.push_narrowing_scope(narrowing);
        }
        let then_expr = self.expression_with_hint(&logical.right, body, record_ty)?;
        if rhs_narrowing.is_some() {
            self.scope.pop_narrowing_scope();
        }
        let source_ty = Self::expr_ty(body, then_expr);
        self.accept_object_spread_source(source_ty, record_ty, span)?;
        let else_expr = body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty: source_ty,
            span: self.span(span.start, span.start),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            },
            ty: source_ty,
            span: self.span(span.start, span.end),
        })))
    }

    /// Strip transparent wrappers around an object-spread source condition.
    pub(in crate::lowering) fn object_spread_condition_source<'a>(
        argument: &'a Expression<'a>,
    ) -> &'a Expression<'a> {
        match argument {
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::object_spread_condition_source(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::object_spread_condition_source(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::object_spread_condition_source(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::object_spread_condition_source(&non_null.expression)
            }
            _ => argument,
        }
    }

    /// Resolve a contextual field type for an object-literal property value.
    pub(in crate::lowering) fn object_property_value_hint(
        &mut self,
        property: &oxc::ast::ast::ObjectProperty<'_>,
        object_hint: Option<smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
        let hint = object_hint?;
        if let Some(Type::Dict(_, value_ty)) = self.ctx.krate.types.get(hint) {
            return Some(*value_ty);
        }
        let field_name = match &property.key {
            PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str(),
            PropertyKey::StringLiteral(literal) => literal.value.as_str(),
            _ => return None,
        };
        let field = self.intern_source_name(field_name);
        let field_ty = self.class_field_type(hint, field).ok()?;
        if matches!(&property.value, Expression::ObjectExpression(_))
            && matches!(
                self.ctx.krate.types.get(field_ty),
                Some(Type::Class { .. } | Type::Optional(_))
            )
        {
            return None;
        }
        Some(field_ty)
    }

    /// Report whether a function body references the implicit `arguments`
    /// binding.
    ///
    /// Scans the body's source slice for the `arguments` identifier with
    /// surrounding identifier-boundary checks, mirroring the source-text probes
    /// already used for forward-callable detection. Used to decide whether a
    /// zero-parameter object-method function expression must be lowered as a
    /// real function value (which establishes the array-like `arguments`
    /// object) rather than collapsed into a getter return expression.
    pub(in crate::lowering) fn function_body_references_arguments(
        &self,
        function_body: &oxc::ast::ast::FunctionBody<'_>,
    ) -> bool {
        let (Ok(start), Ok(end)) = (
            usize::try_from(function_body.span.start),
            usize::try_from(function_body.span.end),
        ) else {
            return false;
        };
        let Some(text) = self.source.get(start..end) else {
            return false;
        };
        Self::source_slice_mentions_identifier(text, "arguments")
    }

    /// Report whether `text` contains `identifier` as a standalone JavaScript
    /// identifier (not as a substring of a longer identifier such as a property
    /// name or a different variable).
    pub(in crate::lowering) fn source_slice_mentions_identifier(
        text: &str,
        identifier: &str,
    ) -> bool {
        let bytes = text.as_bytes();
        let mut search_from = 0;
        while let Some(offset) = text
            .get(search_from..)
            .and_then(|tail| tail.find(identifier))
        {
            let match_start = search_from + offset;
            let match_end = match_start + identifier.len();
            let before_ok = match_start
                .checked_sub(1)
                .and_then(|index| bytes.get(index))
                .is_none_or(|byte| !Self::is_identifier_byte(*byte));
            let after_ok = bytes
                .get(match_end)
                .is_none_or(|byte| !Self::is_identifier_byte(*byte));
            if before_ok && after_ok {
                return true;
            }
            search_from = match_start + 1;
        }
        false
    }

    /// Report whether `byte` can appear inside a JavaScript identifier.
    pub(in crate::lowering) fn is_identifier_byte(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
    }

    /// Lower an object property value, treating zero-argument getters as field values.
    pub(in crate::lowering) fn object_property_value_expr(
        &mut self,
        property: &oxc::ast::ast::ObjectProperty<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Expression::FunctionExpression(function) = &property.value {
            if !function.params.items.is_empty() || function.params.rest.is_some() {
                return self.function_expression_value(function, type_hint, property.span, body);
            }
            let Some(function_body) = &function.body else {
                return Err(SmeltError::unsupported(
                    self.span(function.span.start, function.span.end),
                    "object getter functions must have a body",
                ));
            };
            // A zero-parameter `function` value that references its own
            // `arguments` binding is a real function, not a collapsible getter:
            // collapsing it to the bare return expression would lower
            // `arguments` against the enclosing scope (where it is unavailable).
            // Lower it as a genuine function-expression value instead, which
            // establishes the array-like `arguments` object for the body.
            if self.function_body_references_arguments(function_body) {
                return self.function_expression_value(function, type_hint, property.span, body);
            }
            let [Statement::ReturnStatement(statement)] = function_body.statements.as_slice()
            else {
                let ty = type_hint.unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(function.span.start, function.span.end),
                }));
            };
            let Some(argument) = &statement.argument else {
                return Err(SmeltError::unsupported(
                    self.span(statement.span.start, statement.span.end),
                    "object getter functions must return a value",
                ));
            };
            return self.expression_with_hint(argument, body, type_hint);
        }
        if matches!(&property.value, Expression::Identifier(identifier) if identifier.name == "undefined")
            && type_hint.is_none()
        {
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Undefined),
                ty,
                span: self.span(property.value.span().start, property.value.span().end),
            }));
        }
        self.expression_with_hint(&property.value, body, type_hint)
    }

    /// Lower a function-valued object property into a closure expression.
    ///
    /// Object tables such as date-fns `formatters` use `key: function (...) {}`
    /// entries. Contextual object types provide the function parameter and
    /// return types when the function expression omits annotations.
    pub(in crate::lowering) fn function_expression_value(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        span: oxc::span::Span,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "function expressions must have a body",
            ));
        };
        // A NAMED function expression binds its own name inside its own body, and
        // that is how JavaScript spells a self-recursive callback:
        //
        //     mergeWith(cloneDeep(target), source, function mergeRecursively(a, b) {
        //       … return mergeWith(clone(a), b, mergeRecursively); …
        //     })
        //
        // The closure path never bound the name, so the self-reference fell through
        // identifier resolution to the forward-callable fallback and lowered to an
        // EMPTY OBJECT — and calling an empty object collapses to a null callback
        // instead of failing, so the recursion silently did nothing. All eight
        // es-toolkit `toMerged` specs are that one defect.
        //
        // An inline Rust closure cannot express it either: the closure would have
        // to capture the very binding it is being assigned to. So lift it to a
        // module-owned function item, which is what a hand port would write —
        // recursion becomes ordinary `fn` recursion and the value handed to the
        // caller is the same item-closure wrapper a named top-level function
        // reference already produces.
        if let Some(id) = &function.id
            && self.function_expression_is_self_recursive(function, id.name.as_str())
            && !self.function_expression_captures_enclosing_scope(function, id.name.as_str())
        {
            return self.lift_self_recursive_function_expression(
                id.name.as_str(),
                function,
                type_hint,
                span,
                outer_body,
            );
        }
        let hint_function = type_hint.and_then(|ty| {
            let ty = self
                .function_member_type(ty)
                .unwrap_or_else(|| self.type_param_constraint_or_self(ty));
            match self.ctx.krate.types.get(ty) {
                Some(Type::Function(function_ty)) => Some((ty, function_ty.clone())),
                _ => None,
            }
        });
        let return_ty = if let Some(return_type) = &function.return_type {
            self.ts_type_to_hir(&return_type.type_annotation)?
        } else if let Some((_, function_ty)) = &hint_function {
            function_ty.return_ty
        } else {
            self.ctx.krate.types.intern(Type::Unknown)
        };

        let saved_locals = self.scope.take_bindings();
        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        let saved_generator_yields = self.current_generator_yields;
        let saved_narrowed_locals = self.scope.take_narrowings();
        // A postfix update (`x++`) is emitted into the current body's block, but a
        // variable-declaration initializer defers its postfix updates into a
        // pending list so `const y = x++;` observes the old value. That deferral
        // must not leak across a nested function boundary: this function
        // expression may be the initializer being lowered (`const bound =
        // function () { … a[k++] … };`), and a postfix update inside its body
        // belongs to this body, not the outer declaration's deferred list — which
        // would otherwise flush a statement referencing this body's locals into
        // the enclosing body's block (a cross-body dangling reference). Reset the
        // deferral while lowering this body and restore it afterward.
        let saved_deferred_updates = self.deferred_postfix_updates.take();
        self.current_async = function.r#async;
        self.current_return_ty = Some(return_ty);
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        body.is_generator = function.generator;
        let mut params = Vec::new();
        let mut param_names = HashSet::new();
        let mut errors = Vec::new();
        // A `function` expression that reads its own `arguments` is variadic: the
        // object is the ACTUAL argument list, which a declared-arity signature
        // cannot carry (see `lowering::arguments_forwarding`). Replace the parameter
        // list with one rest list and re-bind each declared name from it.
        // es-toolkit's `partial`/`partialRight`/`flow` spec helpers are this shape.
        let arguments_forwarding = match self.arguments_forwarding_params(function, &mut body) {
            Ok(forwarding) => forwarding,
            Err(error) => {
                errors.push(error);
                None
            }
        };
        if let Some(forwarding) = &arguments_forwarding {
            params.extend(forwarding.params.iter().cloned());
            for (name, local) in forwarding.binding_pairs() {
                param_names.insert(name.clone());
                self.scope.bind(name, local);
            }
        }
        for (index, param) in function
            .params
            .items
            .iter()
            .enumerate()
            .take(if arguments_forwarding.is_some() { 0 } else { usize::MAX })
        {
            let result = (|| {
                let ty = if let Some(annotation) = &param.type_annotation {
                    self.ts_type_to_hir(&annotation.type_annotation)?
                } else if let Some((_, function_ty)) = &hint_function {
                    function_ty.params.get(index).copied().ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(param.span.start, param.span.end),
                            "function expression has more parameters than its type hint",
                        )
                    })?
                } else {
                    self.ctx.krate.types.intern(Type::Unknown)
                };
                let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                    return Err(SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "function expression parameters must be identifiers",
                    ));
                };
                let param_name = self.intern_source_name(binding.name.as_str());
                let local = body.push_local(LocalDecl {
                    name: Some(param_name),
                    ty,
                    mutable: false,
                    span: self.span(binding.span.start, binding.span.end),
                });
                body.params.push(local);
                self.scope.bind(binding.name.to_string(), local);
                param_names.insert(binding.name.to_string());
                params.push(Param {
                    name: param_name,
                    local,
                    ty,
                    span: self.span(binding.span.start, binding.span.end),
                });
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(error);
                break;
            }
        }
        // Lower an optional `...rest` parameter the same way top-level functions
        // and arrow expressions do: resolve its array element type, push a packed
        // list local/param, and record the rest index on the closure so codegen
        // collects the trailing source arguments into one list. Function
        // expressions appear as object property values, returned values, and call
        // arguments, so this keeps rest semantics for all of them.
        let mut rest = arguments_forwarding.as_ref().map(|forwarding| RestParam {
            index: forwarding.rest_index,
            item_ty: forwarding.item_ty,
        });
        if rest.is_none()
            && let Some(rest_param) = &function.params.rest
        {
            let result = (|| {
                let BindingPattern::BindingIdentifier(binding) = &rest_param.rest.argument else {
                    return Err(SmeltError::unsupported(
                        self.span(rest_param.span.start, rest_param.span.end),
                        "function expression destructured rest parameters need rest binding lowering",
                    ));
                };
                let annotated_ty = rest_param
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?;
                let rest_index = params.len();
                let hint_rest_ty = hint_function.as_ref().and_then(|(_, function_ty)| {
                    function_ty
                        .rest
                        .filter(|index| *index == rest_index)
                        .and_then(|index| function_ty.params.get(index).copied())
                });
                let source_ty = annotated_ty
                    .or(hint_rest_ty)
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                let Ok((ty, item_ty)) = self.rest_param_array_type(source_ty) else {
                    return Err(SmeltError::unsupported(
                        self.span(rest_param.span.start, rest_param.span.end),
                        "function expression rest parameter type must be an array type",
                    ));
                };
                let param_name = self.intern_source_name(binding.name.as_str());
                let local = body.push_local(LocalDecl {
                    name: Some(param_name),
                    ty,
                    mutable: false,
                    span: self.span(binding.span.start, binding.span.end),
                });
                body.params.push(local);
                self.scope.bind(binding.name.to_string(), local);
                param_names.insert(binding.name.to_string());
                params.push(Param {
                    name: param_name,
                    local,
                    ty,
                    span: self.span(binding.span.start, binding.span.end),
                });
                rest = Some(RestParam {
                    index: rest_index,
                    item_ty,
                });
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(error);
            }
        }
        let required_params = function
            .params
            .items
            .iter()
            .position(|param| param.optional || Self::formal_parameter_has_default(param))
            .unwrap_or(function.params.items.len());
        let mut captures = Vec::new();
        if errors.is_empty() {
            let mut capture_names = Vec::new();
            let function_locals = self.scope.snapshot_bindings();
            self.scope.restore_bindings(saved_locals.clone());
            for statement in &function_body.statements {
                self.collect_statement_capture_names(statement, &param_names, &mut capture_names);
            }
            self.scope.restore_bindings(function_locals);
            capture_names.sort();
            capture_names.dedup();
            for name in capture_names {
                let Some(source_local) = saved_locals.lookup(name.as_str()) else {
                    continue;
                };
                let Some(source_decl) = usize::try_from(source_local.0)
                    .ok()
                    .and_then(|index| outer_body.locals.get(index))
                else {
                    continue;
                };
                let symbol = source_decl
                    .name
                    .unwrap_or_else(|| self.ctx.krate.symbols.intern(name.as_str()));
                let body_local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty: source_decl.ty,
                    mutable: source_decl.mutable,
                    span: source_decl.span,
                });
                self.scope.bind(name, body_local);
                captures.push(ClosureCapture {
                    source_local,
                    body_local: Some(body_local),
                    symbol,
                    ty: source_decl.ty,
                    mode: CaptureMode::ByRef,
                });
            }
        }
        let generator_yields = function
            .generator
            .then(|| self.initialize_generator_yield_accumulator(function, &mut body));
        self.current_generator_yields = generator_yields;
        // A non-arrow `function` expression introduces its own `arguments`
        // binding, so make the array-like `arguments` object available while
        // lowering the body — mirroring the function-declaration and closure
        // lowering paths that also push the argument arity stack.
        // With the variadic rewrite the whole argument list is the single rest
        // parameter, so the FIXED arity is zero — that is what tells
        // `arguments_object_expression` to read the list rather than the declared
        // parameters.
        self.current_arguments_arities
            .push(if arguments_forwarding.is_some() {
                0
            } else {
                function.params.items.len()
            });
        for statement in &function_body.statements {
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }
        if let Some(accumulator) = generator_yields {
            Self::push_generator_return(accumulator, function, &mut body);
        }
        if function.r#async {
            body.build_async_state_machine();
        }
        self.current_arguments_arities.pop();
        self.scope.restore_bindings(saved_locals);
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        self.current_generator_yields = saved_generator_yields;
        self.scope.restore_narrowings(saved_narrowed_locals);
        self.deferred_postfix_updates = saved_deferred_updates;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let body_id = self.ctx.krate.push_body(body);
        let rest_index = rest.as_ref().map(|rest| rest.index);
        let function_ty = hint_function.map_or_else(
            || {
                self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params: params.iter().map(|param| param.ty).collect(),
                    rest: rest_index,
                    required_params: Some(required_params),
                    mutable_params: Vec::new(),
                    return_ty,
                    is_async: function.r#async,
                    may_throw: false,
                }))
            },
            |(ty, _)| ty,
        );
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params,
                rest: rest_index,
                required_params: Some(required_params),
                return_ty,
                captures,
                body: body_id,
                function_item: None,
                span: self.span(function.span.start, function.span.end),
            }),
            ty: function_ty,
            span: self.span(span.start, span.end),
        }))
    }

    /// Lower an object property key to a dictionary key expression.
    pub(in crate::lowering) fn object_property_key_expr(
        &mut self,
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if object_property.computed {
            if let Some(key_text) = self.computed_string_literal_key(object_property) {
                let ty = self.ctx.krate.types.intern(Type::String);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(key_text)),
                    ty,
                    span: self.span(
                        object_property.key.span().start,
                        object_property.key.span().end,
                    ),
                }));
            }
            // A computed key that names a STABLE SYMBOL (`[Symbol.toStringTag]`,
            // `[Symbol.for('k')]`, a const aliasing either) names one fixed
            // member, so the literal declares that member's storage key instead
            // of taking the dynamic-key path — the same resolution the interface
            // and class declaration sides use, so a declaration and a literal
            // cannot disagree about which member a symbol key names. The symbol's
            // VALUE spelling is a separate thing (see `symbol_static_member`); the
            // shared well-known table relates the two.
            if let Some((key_text, true)) =
                self.resolve_static_computed_key_name(&object_property.key)
            {
                let ty = self.ctx.krate.types.intern(Type::String);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(key_text)),
                    ty,
                    span: self.span(
                        object_property.key.span().start,
                        object_property.key.span().end,
                    ),
                }));
            }
            return self.property_key_index_expression(&object_property.key, body);
        }

        let key_text = match &object_property.key {
            PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
            PropertyKey::StringLiteral(lit) => lit.value.to_string(),
            PropertyKey::NumericLiteral(lit) => lit.raw.as_ref().map_or_else(
                || {
                    if lit.value.fract() == 0.0_f64 {
                        format!("{:.0}", lit.value)
                    } else {
                        lit.value.to_string()
                    }
                },
                ToString::to_string,
            ),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(
                        object_property.key.span().start,
                        object_property.key.span().end,
                    ),
                    "object literal keys must be static string keys or computed expressions",
                ));
            }
        };
        let key_ty = self.ctx.krate.types.intern(Type::String);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(key_text)),
            ty: key_ty,
            span: self.span(
                object_property.key.span().start,
                object_property.key.span().end,
            ),
        }))
    }

    /// Return true for computed symbol keys that getter/method enumeration ignores.
    pub(in crate::lowering) fn is_computed_symbol_key(
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
    ) -> bool {
        if !object_property.computed {
            return false;
        }
        Self::is_direct_computed_symbol_call_key(object_property)
            || matches!(
                &object_property.key,
                PropertyKey::Identifier(identifier) if identifier.name.contains("SYMBOL")
            )
    }

    /// Return true when a computed key is a direct `Symbol(...)` expression.
    pub(in crate::lowering) fn is_direct_computed_symbol_call_key(
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
    ) -> bool {
        object_property.computed
            && matches!(
                &object_property.key,
                PropertyKey::CallExpression(call)
                    if matches!(&call.callee, Expression::Identifier(callee) if callee.name == "Symbol")
            )
    }

    /// Extract the source string from a computed string literal key with erased assertions.
    pub(in crate::lowering) fn computed_string_literal_key(
        &self,
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
    ) -> Option<String> {
        let source = self
            .source
            .get(
                usize::try_from(object_property.key.span().start).ok()?
                    ..usize::try_from(object_property.key.span().end).ok()?,
            )?
            .trim();
        let quote = source.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let rest = &source[quote.len_utf8()..];
        let end = rest.find(quote)?;
        Some(rest[..end].to_owned())
    }

    /// Flush pending explicit properties into an ordered object-spread source.
    pub(in crate::lowering) fn flush_object_spread_entries(
        &mut self,
        pending_entries: &mut Vec<(smelt_hir::ExprId, smelt_hir::ExprId)>,
        sources: &mut Vec<smelt_hir::ExprId>,
        record_ty: &mut Option<smelt_hir::TypeId>,
        erased_spread_requires_unknown_record: &mut bool,
        body: &mut Body,
        span: oxc::span::Span,
    ) {
        if pending_entries.is_empty() {
            return;
        }
        let entries = std::mem::take(pending_entries);
        let force_unknown_record = record_ty.is_none()
            && *erased_spread_requires_unknown_record
            && !self.object_spread_entries_are_callable(&entries, body);
        let chunk_ty = if force_unknown_record {
            let key_ty = self.ctx.krate.types.intern(Type::String);
            let value_ty = self.ctx.krate.types.intern(Type::Unknown);
            self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
        } else {
            self.object_literal_type(&entries, *record_ty, body)
        };
        if record_ty.is_none() {
            *record_ty = Some(chunk_ty);
        }
        *erased_spread_requires_unknown_record = false;
        sources.push(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: record_ty.unwrap_or(chunk_ty),
            span: self.span(span.start, span.end),
        }));
    }

    /// Return whether every explicit property in a spread chunk is callable.
    pub(in crate::lowering) fn object_spread_entries_are_callable(
        &self,
        entries: &[(smelt_hir::ExprId, smelt_hir::ExprId)],
        body: &Body,
    ) -> bool {
        !entries.is_empty()
            && entries.iter().all(|(_, value)| {
                matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, *value)),
                    Some(Type::Function(_))
                )
            })
    }

    /// Validate a source expression used by an object spread property.
    pub(in crate::lowering) fn accept_object_spread_source(
        &mut self,
        source_ty: smelt_hir::TypeId,
        record_ty: Option<smelt_hir::TypeId>,
        span: oxc::span::Span,
    ) -> Result<(), SmeltError> {
        match self.ctx.krate.types.get(source_ty) {
            Some(Type::Dict(_, _)) if record_ty.is_none() || record_ty == Some(source_ty) => Ok(()),
            Some(Type::Dict(source_key, source_value)) => {
                let Some(record_ty) = record_ty else {
                    return Ok(());
                };
                let Some(Type::Dict(record_key, record_value)) =
                    self.ctx.krate.types.get(record_ty).cloned()
                else {
                    return Ok(());
                };
                if self.map_key_type_compatible(record_key, *source_key)
                    && (record_value == *source_value
                        || self.numeric_type_compatible(record_value, *source_value)
                        || self
                            .non_nullish_type(*source_value)
                            .is_some_and(|inner| self.numeric_type_compatible(record_value, inner)))
                {
                    Ok(())
                } else {
                    Err(SmeltError::unsupported(
                        self.span(span.start, span.end),
                        "object spread sources must be record, generic object, or unknown values",
                    ))
                }
            }
            Some(Type::Optional(inner)) => {
                self.accept_object_spread_source(*inner, record_ty, span)
            }
            Some(Type::Class { .. } | Type::TypeParam { .. } | Type::Unknown) => Ok(()),
            _ => Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "object spread sources must be record, generic object, or unknown values",
            )),
        }
    }

    /// Return whether JavaScript object spread treats a source as an empty object.
    pub(in crate::lowering) fn object_spread_source_erases_to_empty(
        &self,
        source_ty: smelt_hir::TypeId,
    ) -> bool {
        matches!(
            self.ctx.krate.types.get(source_ty),
            Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None)
        )
    }

    /// Return whether a spread source must keep later literal chunks erased.
    ///
    /// An unknown, generic, class, or optional object spread can carry
    /// heterogeneous property values. Without a contextual record type, later
    /// explicit properties must not force those copied fields into their own
    /// value type.
    pub(in crate::lowering) fn object_spread_source_needs_unknown_record(
        &self,
        source_ty: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(source_ty) {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => true,
            Some(Type::Optional(inner)) => self.object_spread_source_needs_unknown_record(*inner),
            _ => false,
        }
    }

    /// Extract a dictionary type from a contextual object-literal type hint.
    pub(in crate::lowering) fn dict_type_from_hint(
        &self,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
        let ty = type_hint?;
        match self.ctx.krate.types.get(ty) {
            Some(Type::Dict(_, _)) => Some(ty),
            Some(Type::Union(members)) => members
                .iter()
                .copied()
                .find(|member| matches!(self.ctx.krate.types.get(*member), Some(Type::Dict(_, _)))),
            _ => None,
        }
    }

    /// Infer the storage type used for a lowered object literal.
    ///
    /// A fully compatible contextual record keeps nested typed fields, such as
    /// a locale option bag, from first erasing through `Record<string, unknown>`.
    /// Incomplete or incompatible contextual records remain dictionaries so
    /// ordinary structural adaptation can still occur at their use site.
    pub(in crate::lowering) fn object_literal_type(
        &mut self,
        entries: &[(smelt_hir::ExprId, smelt_hir::ExprId)],
        type_hint: Option<smelt_hir::TypeId>,
        body: &Body,
    ) -> smelt_hir::TypeId {
        if let Some(ty) = type_hint
            && matches!(self.ctx.krate.types.get(ty), Some(Type::Dict(_, _)))
        {
            return ty;
        }
        if let Some(ty) =
            type_hint.and_then(|ty| self.contextual_record_literal_type(ty, entries, body))
        {
            return ty;
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let key_ty = if entries
            .iter()
            .all(|(key, _)| Self::expr_ty(body, *key) == string_ty)
        {
            string_ty
        } else {
            self.ctx.krate.types.intern(Type::Unknown)
        };
        let first_value_ty = entries
            .first()
            .map(|(_, value)| Self::expr_ty(body, *value));
        let value_ty = first_value_ty
            .filter(|first_ty| {
                entries
                    .iter()
                    .all(|(_, value)| Self::expr_ty(body, *value) == *first_ty)
            })
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
        self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
    }

    /// Preserve a contextual interface type only when the literal can be
    /// constructed directly without inventing values for required fields.
    pub(in crate::lowering) fn contextual_record_literal_type(
        &mut self,
        type_hint: smelt_hir::TypeId,
        entries: &[(smelt_hir::ExprId, smelt_hir::ExprId)],
        body: &Body,
    ) -> Option<smelt_hir::TypeId> {
        let candidate = match self.ctx.krate.types.get(type_hint) {
            Some(Type::Class { .. }) => type_hint,
            Some(Type::Optional(inner))
                if matches!(self.ctx.krate.types.get(*inner), Some(Type::Class { .. })) =>
            {
                *inner
            }
            _ => return None,
        };
        let fields = self.contextual_record_literal_fields(candidate)?;
        if fields.is_empty() || fields.iter().any(|field| !field.optional) {
            return None;
        }
        let mut needs_structural_adapter = false;
        for (key, value) in entries {
            let key_expr = body
                .exprs
                .get(usize::try_from(key.0).unwrap_or(usize::MAX))?;
            let ExprKind::Literal(Literal::String(field_key)) = &key_expr.kind else {
                return None;
            };
            let field = self.intern_source_name(field_key);
            let expected = self.class_field_type(candidate, field).ok()?;
            let actual = Self::expr_ty(body, *value);
            if !self.contextual_record_field_assignable(actual, expected) {
                return None;
            }
            needs_structural_adapter |=
                !self.contextual_record_field_directly_assignable(actual, expected);
        }
        needs_structural_adapter.then_some(candidate)
    }

    /// Return whether a contextual field can be assigned without record adaptation.
    pub(in crate::lowering) fn contextual_record_field_directly_assignable(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        if self.type_assignable_to(actual, expected) {
            return true;
        }
        matches!(self.ctx.krate.types.get(expected), Some(Type::Optional(inner)) if self.contextual_record_field_directly_assignable(actual, *inner))
    }

    /// Return whether direct record emission can initialize one contextual field.
    ///
    /// Typed interface values may require the backend's established structural
    /// record adapter even when their nominal HIR names differ.
    pub(in crate::lowering) fn contextual_record_field_assignable(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        if self.contextual_record_field_directly_assignable(actual, expected) {
            return true;
        }
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(expected) {
            return self.contextual_record_field_assignable(actual, *inner);
        }
        self.contextual_record_literal_fields(actual).is_some()
            && self.contextual_record_literal_fields(expected).is_some()
    }

    /// Collect fields that direct record-literal emission must initialize.
    ///
    /// Plain classes are deliberately excluded: constructor semantics are not
    /// equivalent to constructing a TypeScript options/interface literal.
    pub(in crate::lowering) fn contextual_record_literal_fields(
        &self,
        candidate: smelt_hir::TypeId,
    ) -> Option<Vec<Field>> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(candidate) else {
            return None;
        };
        if let Some(interface) = self.find_interface(*name) {
            return self.contextual_interface_fields(interface.name, &mut HashSet::new());
        }
        self.types.alias_fields(*name).cloned()
    }

    /// Collect inherited interface fields while rejecting recursive surfaces.
    pub(in crate::lowering) fn contextual_interface_fields(
        &self,
        name: smelt_hir::Symbol,
        visited: &mut HashSet<smelt_hir::Symbol>,
    ) -> Option<Vec<Field>> {
        if !visited.insert(name) {
            return None;
        }
        let interface = self.find_interface(name)?;
        let mut fields = interface.fields.clone();
        for parent in &interface.extends {
            for field in self.contextual_interface_fields(parent.parent, visited)? {
                if !fields.iter().any(|existing| existing.name == field.name) {
                    fields.push(field);
                }
            }
        }
        Some(fields)
    }

    /// Lower a static member access expression.
    pub(in crate::lowering) fn namespace_member_expression(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some((namespace, member_name)) = self.namespace_member_name(member) else {
            return Ok(None);
        };
        let span = self.span(member.span.start, member.span.end);
        if let Some(value) = self.consts.literal(member_name).cloned() {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(value.literal),
                ty: value.ty,
                span,
            })));
        }
        if let Some(value) = self.consts.object(member_name).cloned() {
            return Ok(Some(self.object_const_expression(
                &value,
                member.span.start,
                member.span.end,
                body,
            )));
        }
        let item = self
            .object_namespaces
            .get(namespace)
            .and_then(|members| members.get(member_name))
            .copied()
            .or_else(|| self.items.get(member_name).copied());
        let Some(item) = item else {
            if self.imports.is_namespace(namespace) {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span,
                })));
            }
            return Err(SmeltError::unsupported(
                span,
                format!("namespace import has no exported member `{member_name}`"),
            ));
        };
        match self.item_ref(item).clone() {
            Item::Function(_) => Ok(Some(self.item_function_closure_expression(
                item,
                member.span.start,
                member.span.end,
                body,
            )?)),
            Item::Const(const_item) => Ok(Some(self.const_item_expression(
                &const_item,
                member.span.start,
                member.span.end,
                body,
            )?)),
            _ => {
                let ty = self.item_expr_type(item, span)?;
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Item(item),
                    ty,
                    span,
                })))
            }
        }
    }

    // Continued in the next split builder file.
}
