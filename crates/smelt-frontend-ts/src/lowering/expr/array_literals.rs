//! Array literals: element types, contextual hints and spreads.

use crate::lowering::body_arena::BodyArena;
use crate::lowering::erasure::NotNewErasure;
use crate::lowering::js_text::js_str_literal;
use crate::lowering::{
    ArrayExpressionElement, Body, Expr, ExprKind, Literal, ModuleBuilder, SetProjectionOp,
    SmeltError, Type,
};
use oxc::span::GetSpan;
use smelt_hir::ErasureReason;

/// One lowered element of a spread-containing array literal.
///
/// `array_expression_with_spread` lowers all elements before assembling the
/// `ListLit`/`ListConcat` chain so the literal's item type can be unified from
/// the lowered piece types.
pub(in crate::lowering) enum SpreadPiece {
    /// A `...operand` spread with its lowered operand and source span.
    Spread(smelt_hir::ExprId, oxc::span::Span),
    /// A plain element expression.
    Item(smelt_hir::ExprId),
}

impl ModuleBuilder<'_> {
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
                    kind: ExprKind::Literal(js_str_literal(lit.value)),
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
                            let ty = self.erase_with(ErasureReason::DynamicOperator, "array hole");
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

    /// Project a UNION hint to the arm an array literal can be lowered at.
    ///
    /// Answers the hint unchanged when it is not a union, and `None` -> `None`.
    /// A union contributes an arm only when exactly ONE of its members is a
    /// list or a tuple: with two candidate arms (`string[] | number[]`) the
    /// literal's own elements are the better evidence, which is what the
    /// no-hint path already uses.
    fn array_literal_union_arm_hint(
        &self,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
        let hint = type_hint?;
        let Some(Type::Union(items)) = self.ctx.krate.types.get(hint) else {
            return type_hint;
        };
        // A union with an ERASED arm (`unknown[] | unknown`) renders as the
        // erased carrier, so its arms are not distinguishable in Rust and
        // contextual typing at one of them buys nothing: the literal is erased
        // either way, and the erased spelling is the one that round-trips.
        if items.iter().any(|item| {
            matches!(
                self.ctx.krate.types.get(*item),
                Some(Type::Unknown | Type::TypeParam { .. })
            )
        }) {
            return type_hint;
        }
        let mut arms = items.iter().copied().filter(|item| {
            matches!(
                self.ctx.krate.types.get(*item),
                Some(Type::List(_) | Type::Tuple(_))
            )
        });
        let arm = arms.next()?;
        if arms.next().is_some() {
            return type_hint;
        }
        Some(arm)
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
        // A UNION hint contextually types the literal at the arm that can hold
        // it. `firstPair(slot: number | [string, number][])` called with
        // `[['a', 1]]` used to lower the literal with no hint at all — the
        // union is neither a list nor a tuple — so its elements erased and the
        // argument became `SmeltList<SmeltUnknown>`, while the SAME literal
        // bound to a typed local first was injected as the union's arm. The arm
        // is the honest hint, and the existing union injection at the call
        // boundary then wraps the concretely typed value.
        let type_hint = self.array_literal_union_arm_hint(type_hint);
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
                let ty = element_hint.unwrap_or_else(|| {
                    self.erase_with(ErasureReason::DynamicOperator, "array hole")
                });
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
        let array_compatible_hint = type_hint
            .filter(|hint| !matches!(self.ctx.krate.types.get(*hint), Some(Type::Function(_))));
        let ty = if let Some(hint) = array_compatible_hint {
            // A hint that still mentions a type parameter (a generic callee's
            // `T[]`) takes its open positions from the elements; see
            // `lowering::open_array_hint`.
            self.fill_open_array_hint(hint, &items, body)
        } else if !items.is_empty() {
            // Elements with no common type take the union TypeScript proved,
            // except when they mix `null` and `undefined` (see
            // `array_literal_mixes_nullish_spellings`).
            let item_ty = if Self::array_literal_mixes_nullish_spellings(&items, body) {
                None
            } else {
                self.checker_mixed_array_item_type(array.span, &items, body)
            };
            let item_ty = item_ty.unwrap_or_else(|| self.array_literal_item_type(&items, body));
            self.ctx.krate.types.intern(Type::List(item_ty))
        } else {
            let item_ty = self.erase_with(ErasureReason::MissingAnnotation, "empty array literal");
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
            return self.erase_with(ErasureReason::MissingAnnotation, "empty array literal");
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
        self.erase_with(ErasureReason::DynamicOperator, "mixed array literal")
    }

    /// Report whether an array literal writes BOTH `null` and `undefined`.
    ///
    /// `null` and `undefined` share one HIR type (`Type::None`), so a literal
    /// mixing them looks uniformly nullish to the element-type join above and
    /// would collapse into `Optional(T)`. `Option` has a single empty state, so
    /// both spellings would lower to the same `None` and the distinction would be
    /// gone by the time the list erases to `SmeltUnknown` -- es-toolkit's
    /// `isEqualWith` primitives table depends on `[null, undefined, false]`
    /// differing from `[null, null, true]`.
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
            let Some(expr) = body.expr_at(*item) else {
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
            // A CONCRETE typed-array view decodes its own elements. Asked here,
            // where the operand has just been lowered, because the erased
            // iterable path below would read them back out of a marker record
            // as `unknown` — an erasure with the answer already in hand.
            let element_span = self.span(spread.span.start, spread.span.end);
            if let Some(elements) = self.typed_array_spread_list(spread_value, element_span, body) {
                return Ok(elements);
            }
            // A regex match spreads its numbered groups; the view is already
            // the list `[...m]` builds, typed rather than erased.
            let spread_value = self
                .match_array_view(spread_value, element_span, body)
                .unwrap_or(spread_value);
            let value_ty = self.type_param_constraint_or_self(Self::expr_ty(body, spread_value));
            let item_ty = match self.ctx.krate.types.get(value_ty) {
                Some(Type::List(item_ty) | Type::Set(item_ty)) => *item_ty,
                Some(Type::String) => self.ctx.krate.types.intern(Type::String),
                Some(Type::Generator {
                    is_async: false,
                    yield_ty,
                    ..
                }) => *yield_ty,
                // not a new erasure: the spread operand is already erased
                Some(Type::Unknown) | None => self.unknown_type(NotNewErasure::ErasedUpstream),
                Some(
                    Type::TypeParam { .. }
                    | Type::Class { .. }
                    | Type::Optional(_)
                    | Type::Union(_),
                ) => self.erase_with(ErasureReason::DynamicOperator, "array spread"),
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
                    // Same rule for a view spread among other elements
                    // (`[...view, 0]`): convert it to its elements here, before
                    // the pieces are unified, so the list stays numeric.
                    let element_span = self.span(spread.span.start, spread.span.end);
                    let spread_value = self
                        .typed_array_spread_list(spread_value, element_span, body)
                        .or_else(|| self.match_array_view(spread_value, element_span, body))
                        .unwrap_or(spread_value);
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
    ///
    /// A contextual hint that is a callee's *own* uninstantiated type parameter
    /// is not a hint at all — `T` in `total<T>(items: readonly T[], …)` names
    /// nothing at the call site, and adopting it types the literal `List<T>`
    /// where `T` is out of scope. Nothing downstream can relate that to the
    /// pieces' real types, so the pieces win instead.
    pub(in crate::lowering) fn array_spread_item_type(
        &mut self,
        pieces: &[(smelt_hir::ExprId, bool)],
        body: &Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        if let Some(hint) = type_hint
            && let Some(Type::List(item_ty)) = self.ctx.krate.types.get(hint)
            && !self.type_param_is_out_of_scope(*item_ty)
        {
            return *item_ty;
        }
        // Used only to compare candidates against the erased type.
        let unknown = self.unknown_type(NotNewErasure::Comparison);
        let mut unified: Option<smelt_hir::TypeId> = None;
        for &(value, is_spread) in pieces {
            let candidate = if is_spread {
                let value_ty = self.type_param_constraint_or_self(Self::expr_ty(body, value));
                match self.ctx.krate.types.get(value_ty) {
                    Some(Type::List(item_ty) | Type::Set(item_ty)) => *item_ty,
                    Some(Type::String) => self.ctx.krate.types.intern(Type::String),
                    Some(Type::Generator {
                        is_async: false,
                        yield_ty,
                        ..
                    }) => *yield_ty,
                    // not a new erasure: the spread operand is already erased
                    Some(Type::Unknown) => unknown,
                    _ => self.erase_with(ErasureReason::DynamicOperator, "array spread"),
                }
            } else {
                Self::expr_ty(body, value)
            };
            match unified {
                None => unified = Some(candidate),
                Some(existing) if existing == candidate => {}
                // not a new erasure: one piece is already erased
                Some(existing) if existing == unknown || candidate == unknown => return unknown,
                Some(_) => {
                    return self
                        .erase_with(ErasureReason::DynamicOperator, "mixed array spread items");
                }
            }
        }
        unified.unwrap_or_else(|| self.erase_with(ErasureReason::MissingAnnotation, "empty array"))
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
        // `[...items].sort(cmp)` stays dynamic and the sort result is discarded.
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
            Some(Type::Generator {
                is_async: false, ..
            }) => Ok(Self::generator_items_expr(
                value,
                list_ty,
                self.span(span.start, span.end),
                body,
            )),
            Some(Type::String) => Ok(body.push_expr(Expr {
                kind: ExprKind::StringChars { haystack: value },
                ty: list_ty,
                span: self.span(span.start, span.end),
            })),
            // An erased (or not statically list-shaped) operand is read as a
            // list, then copied by the same fresh-list idiom: `[...erased]` is
            // a NEW array, never the operand's own. The assertion alone was an
            // alias, so `[...C.items].push(x)` wrote into `C.items`.
            Some(
                Type::Unknown
                | Type::TypeParam { .. }
                | Type::Class { .. }
                | Type::Optional(_)
                | Type::Union(_),
            )
            | None => Ok(body.push_expr(Expr {
                kind: ExprKind::ListCopy { list: value },
                ty: list_ty,
                span: self.span(span.start, span.end),
            })),
            _ => Err(SmeltError::unsupported(
                self.span(span.start, span.end),
                "array spread operands must be arrays or sets",
            )),
        }
    }
}
