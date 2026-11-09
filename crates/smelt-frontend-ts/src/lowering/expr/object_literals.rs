//! Object literals: keys, accessors, spreads and the record type a literal takes from
//! its context.

use crate::lowering::erasure::NotNewErasure;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::peel::{Wrappers, peel};
use crate::lowering::{
    Body, Expr, ExprKind, Expression, Field, FunctionType, HashSet, Literal, LogicalOperator,
    ModuleBuilder, ObjectPropertyKind, PropertyKey, PropertyKind, SmeltError, Statement, Type,
};
use crate::lowering::js_text::{js_str_literal, js_str_text};
use oxc::span::GetSpan;
use smelt_hir::ErasureReason;
use std::borrow::Cow;

/// Whether two literal values are the same JavaScript value (`1` and `1.0`
/// are one number).
pub(in crate::lowering) fn literal_values_equal(left: &Literal, right: &Literal) -> bool {
    #[expect(clippy::cast_precision_loss, reason = "a JavaScript number literal is an f64")]
    let number = |literal: &Literal| match literal {
        Literal::Int(value) => Some(*value as f64),
        Literal::Float(value) => Some(*value),
        _ => None,
    };
    match (number(left), number(right)) {
        // JavaScript `===` on numbers: exact IEEE equality is the semantics.
        (Some(left), Some(right)) => left.total_cmp(&right).is_eq() || (left == 0.0 && right == 0.0),
        _ => left == right,
    }
}

impl ModuleBuilder<'_> {
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
                if self.is_computed_symbol_key(object_property) {
                    continue;
                }
                let key = self.object_property_key_expr(object_property, body)?;
                // Under a contextual record type the getter returns the
                // field's type (`get cachedValue(): T` of `readonly
                // cachedValue: T`), which is what lets the record store it as
                // an accessor of that type; see `Crate::accessor_fields`.
                let unknown_ty = self
                    .object_property_value_hint(object_property, type_hint)
                    .unwrap_or_else(|| self.erase_with(ErasureReason::UnmodeledType, "object literal getter"));
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
                    // The property's contextual type types the method as it
                    // types any other property value: `swap() { .. }` under
                    // `Pair<T>` returns `Pair<T>`, not an inferred `unknown`.
                    let value_hint = self.object_property_value_hint(object_property, type_hint);
                    let value = self.function_expression_value(
                        function,
                        value_hint,
                        object_property.span,
                        body,
                    )?;
                    entries.push((key, value));
                    continue;
                }
                let key = self.object_property_key_expr(object_property, body)?;
                let ty = self.erase_with(ErasureReason::UnmodeledType, "object method");
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
        let entries = self.record_literal_accessor_entries(ty, entries, body);
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty,
            span: self.span(object.span.start, object.span.end),
        }))
    }

    /// Store each getter of an object literal built as record `ty` as the
    /// accessor of its field.
    ///
    /// The getter entry was lowered as the `{ __smelt_get: fn }` marker an
    /// erased object carries. A literal that became a record struct instead
    /// hands the getter closure itself to the field and records the field as
    /// an accessor (`Crate::accessor_fields`), so every read of it calls the
    /// getter, as JavaScript does. Literals that stayed erased objects keep
    /// the marker.
    fn record_literal_accessor_entries(
        &mut self,
        ty: smelt_hir::TypeId,
        entries: Vec<(smelt_hir::ExprId, smelt_hir::ExprId)>,
        body: &Body,
    ) -> Vec<(smelt_hir::ExprId, smelt_hir::ExprId)> {
        let Some(Type::Class { name: owner, .. }) = self.ctx.krate.types.get(ty).cloned() else {
            return entries;
        };
        entries
            .into_iter()
            .map(|(key, value)| {
                let Some(getter) = Self::accessor_marker_function(body, value, "__smelt_get") else {
                    return (key, value);
                };
                if let Some(ExprKind::Literal(Literal::String(field_key))) =
                    body.expr_at(key).map(|expr| &expr.kind)
                {
                    let field = self.intern_source_name(&field_key.clone());
                    if !self.ctx.krate.accessor_fields.contains(&(owner, field)) {
                        self.ctx.krate.accessor_fields.push((owner, field));
                    }
                }
                (key, getter)
            })
            .collect()
    }

    /// The function an accessor marker `{ <marker>: fn }` wraps.
    fn accessor_marker_function(body: &Body, value: smelt_hir::ExprId, marker: &str) -> Option<smelt_hir::ExprId> {
        let Some(ExprKind::DictLit(pairs)) =
            body.expr_at(value).map(|expr| &expr.kind)
        else {
            return None;
        };
        let [(key, function)] = pairs.as_slice() else {
            return None;
        };
        matches!(
            body.expr_at(*key).map(|expr| &expr.kind),
            Some(ExprKind::Literal(Literal::String(text))) if text == marker
        )
        .then_some(*function)
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
        // At a record type the spread builds the struct itself; see
        // `lowering::typed_spread`.
        if let Some(record) = self.typed_record_spread(object, body, type_hint)? {
            return Ok(record);
        }
        let mut record_ty = self.dict_type_from_hint(type_hint);
        // Without a contextual record type the result's type is the join of
        // ALL its pieces, not the first piece's: `{ path: '/', ...opt, secure:
        // true }` has string AND boolean values. Pieces that disagree make the
        // literal the erased record, exactly as a plain heterogeneous literal
        // is (`object_literal_type`). `chunks` are the explicit-property runs
        // that then need re-typing.
        let hinted = record_ty.is_some();
        let mut heterogeneous = false;
        let mut chunks: Vec<smelt_hir::ExprId> = Vec::new();
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
                            let value_hint =
                                self.object_property_value_hint(object_property, record_ty);
                            let value = self.function_expression_value(
                                function,
                                value_hint,
                                object_property.span,
                                body,
                            )?;
                            pending_entries.push((key, value));
                            continue;
                        }
                        let key = self.object_property_key_expr(object_property, body)?;
                        let ty = self.erase_with(ErasureReason::UnmodeledType, "object method");
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
                    if !hinted {
                        self.note_spread_chunk_join(&sources, &mut chunks, record_ty, &mut heterogeneous, body);
                    }
                    if let Some(source) = self.conditional_object_spread_source(
                        &spread.argument,
                        record_ty,
                        body,
                        spread.span,
                    )? {
                        let source_ty = Self::expr_ty(body, source);
                        if record_ty.is_none()
                            && !erased_spread_requires_unknown_record
                            && matches!(self.ctx.krate.types.get(source_ty), Some(Type::Dict(_, _)))
                        {
                            record_ty = Some(source_ty);
                        }
                        sources.push(source);
                        continue;
                    }
                    let mut source = self.expression_with_hint(
                        &spread.argument,
                        body,
                        if hinted { record_ty } else { None },
                    )?;
                    let source_ty = Self::expr_ty(body, source);
                    // Unhinted: a record source whose value type differs from
                    // the pieces so far joins them into the erased record
                    // rather than being judged against the first piece.
                    let source_record = match self.ctx.krate.types.get(source_ty) {
                        Some(Type::Optional(inner)) => *inner,
                        _ => source_ty,
                    };
                    let joins_erased = !hinted
                        && record_ty.is_some_and(|record| record != source_record)
                        && matches!(self.ctx.krate.types.get(source_record), Some(Type::Dict(_, _)));
                    if joins_erased {
                        heterogeneous = true;
                    }
                    // A source that erases to `{}`, or one the record cannot
                    // accept, contributes an empty record of the result type.
                    if self.object_spread_source_erases_to_empty(source_ty)
                        || (!joins_erased
                            && self
                                .accept_object_spread_source(source_ty, record_ty, spread.span)
                                .is_err())
                    {
                        let ty = record_ty.unwrap_or_else(|| {
                            let key_ty = self.ctx.krate.types.intern(Type::String);
                            let value_ty = self.erase_with(ErasureReason::DynamicOperator, "object spread");
                            self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
                        });
                        source = body.push_expr(Expr {
                            kind: ExprKind::DictLit(Vec::new()),
                            ty,
                            span: self.span(spread.span.start, spread.span.end),
                        });
                    }
                    let final_source_ty = Self::expr_ty(body, source);
                    // An EARLIER erased source (`...route` of an erased
                    // object) already carries heterogeneous values; adopting a
                    // later homogeneous source's `Dict<String, f64>` would
                    // coerce the earlier `path: string` to `NaN`.
                    if record_ty.is_none()
                        && !erased_spread_requires_unknown_record
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
        if !hinted {
            self.note_spread_chunk_join(&sources, &mut chunks, record_ty, &mut heterogeneous, body);
        }
        let key_ty = self.ctx.krate.types.intern(Type::String);
        if heterogeneous {
            let fallback_value_ty =
                self.erase_with(ErasureReason::DynamicOperator, "heterogeneous object spread");
            let erased = self.ctx.krate.types.intern(Type::Dict(key_ty, fallback_value_ty));
            for chunk in &chunks {
                if let Some(expr) = body.expr_at_mut(*chunk) {
                    expr.ty = erased;
                }
            }
            record_ty = Some(erased);
        }
        let record_ty = record_ty.unwrap_or_else(|| {
            let fallback_value_ty = self.erase_with(ErasureReason::DynamicOperator, "object spread");
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
        let argument = peel(argument, Wrappers::Transparent);
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
        let then_expr = self.with_narrowing(rhs_narrowing, |this| this.expression_with_hint(&logical.right, body, record_ty))?;
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

    /// Resolve a contextual field type for an object-literal property value.
    pub(in crate::lowering) fn object_property_value_hint(
        &mut self,
        property: &oxc::ast::ast::ObjectProperty<'_>,
        object_hint: Option<smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
        let hint = object_hint?;
        if let Some(Type::Dict(_, value_ty)) = self.ctx.krate.types.get(hint) {
            return self.unambiguous_property_hint(*value_ty);
        }
        let field_name = match &property.key {
            PropertyKey::StaticIdentifier(identifier) => Cow::Borrowed(identifier.name.as_str()),
            PropertyKey::StringLiteral(literal) => js_str_text(literal.value),
            _ => return None,
        };
        let field = self.intern_source_name(&field_name);
        let field_ty = self.class_field_type(hint, field).ok()?;
        // A nested object literal gets the field's type as its own hint, class
        // and optional-class fields included. This used to be refused, which
        // left `{ inner: { count: 7 } }` against `interface Outer { inner?:
        // Config }` lowering the inner literal with no hint: it became a
        // `Dict`, the outer literal could then not be built as a struct either
        // (its field was not assignable), and both ends went through the erased
        // `SmeltRecord` reconstruction. The struct path can build the nested
        // value directly -- `Outer { inner: Some(Config { count: Some(7.0) }) }`
        // -- so the refusal cost avoidable erasure at every nesting level for
        // nothing. `contextual_record_literal_type` already unwraps `Optional`
        // and still declines any literal it cannot construct, so the decision
        // stays where it belongs rather than being pre-empted here.
        self.unambiguous_property_hint(field_ty)
    }

    /// `hint` as a property value's contextual type, or `None` when it names
    /// more than one function signature.
    ///
    /// A record type lowered to a `Dict` joins every field's type into one
    /// value union, so `{ handler?: (c: Ctx) => unknown; rewrite?: (r: Req) =>
    /// Req }` hints each value with BOTH signatures. Such a union cannot say
    /// which one a function-valued property has: the first function arm used
    /// to win, typing `rewrite: (req) => req` as `(Ctx) => Ctx` (Hono's
    /// `mount(app, { replaceRequest: (req) => req })` then rewrote every
    /// request to `about:blank`). Without the hint the value types itself, its
    /// arrow parameters through the checker, as with no record hint at all.
    fn unambiguous_property_hint(&self, hint: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        (self.function_arm_count(hint) <= 1).then_some(hint)
    }

    /// How many function types `ty` admits, through nested unions and
    /// optionals (see `unambiguous_property_hint`).
    fn function_arm_count(&self, ty: smelt_hir::TypeId) -> usize {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Function(_)) => 1,
            Some(Type::Optional(inner)) => self.function_arm_count(*inner),
            Some(Type::Union(members)) => members
                .iter()
                .map(|member| self.function_arm_count(*member))
                .sum(),
            _ => 0,
        }
    }

    /// Report whether a function body references the implicit `arguments`
    /// binding.
    ///
    /// Used to decide whether a zero-parameter object-method function expression
    /// must be lowered as a real function value (which establishes the
    /// array-like `arguments` object) rather than collapsed into a getter return
    /// expression.
    pub(in crate::lowering) fn function_body_references_arguments(
        &self,
        function_body: &oxc::ast::ast::FunctionBody<'_>,
    ) -> bool {
        self.source_facts.mentions(function_body.span, "arguments")
    }

    /// Lower an object property value, treating zero-argument getters as field values.
    pub(in crate::lowering) fn object_property_value_expr(
        &mut self,
        property: &oxc::ast::ast::ObjectProperty<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Expression::FunctionExpression(function) = &property.value {
            // Only a plain zero-parameter `function` value can be read as a
            // getter-style field. A generator returns an iterator when *called*
            // and its body never runs on the read, so collapsing it would drop
            // the function entirely.
            if !function.params.items.is_empty()
                || function.params.rest.is_some()
                || function.generator
                || function.r#async
            {
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
            // The getter collapse only applies to `{ k: function () { return v; } }`,
            // whose single return expression IS the field's value. Any other body
            // is a function the property holds, so it lowers as a function value —
            // fabricating `null` here erased the function, and code that asks
            // `typeof value === 'function'` (es-toolkit's JSON validators) then
            // saw a null field instead of the function the source wrote.
            let [Statement::ReturnStatement(statement)] = function_body.statements.as_slice()
            else {
                return self.function_expression_value(function, type_hint, property.span, body);
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
            let ty = self.erase_with(ErasureReason::MissingAnnotation, "undefined property");
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Undefined),
                ty,
                span: self.span(property.value.span().start, property.value.span().end),
            }));
        }
        self.expression_with_hint(&property.value, body, type_hint)
    }

    /// Lower an object property key to a dictionary key expression.
    pub(in crate::lowering) fn object_property_key_expr(
        &mut self,
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if object_property.computed {
            if let PropertyKey::StringLiteral(literal) = &object_property.key {
                let ty = self.ctx.krate.types.intern(Type::String);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(js_str_literal(literal.value)),
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

        // A string key keeps a lone surrogate's code units, as a string literal does.
        if let PropertyKey::StringLiteral(lit) = &object_property.key {
            let key_ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(js_str_literal(lit.value)),
                ty: key_ty,
                span: self.span(object_property.key.span().start, object_property.key.span().end),
            }));
        }
        let key_text = match &object_property.key {
            PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
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

    /// Return true for a computed key that is a fresh `Symbol(...)` call, whose
    /// getter no code can name again, so the literal may leave it out. Any
    /// other computed key (a constant holding a string or a symbol) lowers
    /// like a data property's key.
    pub(in crate::lowering) fn is_computed_symbol_key(
        &self,
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
    ) -> bool {
        self.is_direct_computed_symbol_call_key(object_property)
    }

    /// Return true when a computed key is a direct `Symbol(...)` expression.
    pub(in crate::lowering) fn is_direct_computed_symbol_call_key(
        &self,
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
    ) -> bool {
        object_property.computed
            && matches!(
                &object_property.key,
                PropertyKey::CallExpression(call)
                    if matches!(&call.callee, Expression::Identifier(callee) if self.names_host(callee.name.as_str(), "Symbol"))
            )
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
            // not a new erasure: an earlier erased spread source forces the erased record
            let value_ty = self.unknown_type(NotNewErasure::ErasedUpstream);
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

    /// Record the explicit-property chunk `flush_object_spread_entries` just
    /// pushed (the last of `sources`, if it is a new `DictLit`) and whether its
    /// own type disagrees with the record type so far — the unhinted join rule
    /// of [`Self::object_expression_with_spread`].
    pub(in crate::lowering) fn note_spread_chunk_join(
        &mut self,
        sources: &[smelt_hir::ExprId],
        chunks: &mut Vec<smelt_hir::ExprId>,
        record_ty: Option<smelt_hir::TypeId>,
        heterogeneous: &mut bool,
        body: &Body,
    ) {
        let Some(&last) = sources.last() else { return };
        if chunks.contains(&last) {
            return;
        }
        let Some(ExprKind::DictLit(entries)) = body.expr_at(last)
            .map(|expr| &expr.kind)
        else {
            return;
        };
        let entries = entries.clone();
        chunks.push(last);
        let own = self.object_literal_type(&entries, None, body);
        if record_ty.is_some_and(|record| record != own) {
            *heterogeneous = true;
        }
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
                // A record whose values are the dynamic boundary holds any
                // source value: `{ ...params }` of a `Record<string, string>`
                // into an erased record copies the strings. Rejecting it made
                // the caller replace the source with an EMPTY record, which
                // silently dropped every copied property (Hono's trie-router
                // lost a parent node's path params this way).
                let record_holds_any = matches!(
                    self.ctx.krate.types.get(record_value),
                    Some(Type::Unknown)
                );
                if self.map_key_type_compatible(record_key, *source_key)
                    && (record_value == *source_value
                        || record_holds_any
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
        } else if entries.iter().all(|(key, _)| {
            matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, *key)),
                Some(Type::String | Type::Float | Type::Int | Type::FixedInt(_) | Type::Symbol | Type::PropKey)
            )
        }) {
            // Computed keys that are strings, numbers and symbols are all
            // property keys: the literal is a property-key record.
            self.ctx.krate.types.intern(Type::PropKey)
        } else {
            self.erase_with(ErasureReason::DynamicOperator, "object literal key")
        };
        let first_value_ty = entries
            .first()
            .map(|(_, value)| Self::expr_ty(body, *value));
        let value_ty = first_value_ty
            .filter(|first_ty| {
                entries
                    .iter()
                    .all(|(_, value)| self.alpha_equivalent_types(Self::expr_ty(body, *value), *first_ty))
            })
            .unwrap_or_else(|| {
                if entries.is_empty() {
                    self.erase_with(ErasureReason::MissingAnnotation, "empty object literal")
                } else {
                    self.erase_with(ErasureReason::DynamicOperator, "heterogeneous object literal")
                }
            });
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
        let hint = match self.ctx.krate.types.get(type_hint) {
            Some(Type::Optional(inner)) => *inner,
            _ => type_hint,
        };
        match self.ctx.krate.types.get(hint).cloned() {
            Some(Type::Class { .. }) => self.contextual_record_literal_member(hint, entries, body),
            Some(Type::Union(members)) => {
                self.contextual_union_record_literal_member(&members, entries, body)
            }
            _ => None,
        }
    }

    /// The one object-shaped arm of an expected union that an object literal
    /// is a value of, chosen as TypeScript's discriminated-union check
    /// chooses it.
    ///
    /// `return { done: false, hasNext: true, next: value }` against
    /// `LazyEmpty | LazyMany<T> | LazyNext<T>` is a `LazyNext<T>`: it supplies
    /// every required field of that arm, every key it writes is declared
    /// there, and its literal-typed field `hasNext: true` admits the literal
    /// written (`LazyEmpty` declares `hasNext: false`, `LazyMany` requires
    /// `hasMany`). HIR stores literal-typed fields as their primitive, so the
    /// literal values come from the interface's recorded
    /// [`smelt_hir::FieldDecodeFacts`]. A hand port builds the matching
    /// variant directly; anything other than exactly one fitting arm declines
    /// and keeps the record path.
    fn contextual_union_record_literal_member(
        &mut self,
        members: &[smelt_hir::TypeId],
        entries: &[(smelt_hir::ExprId, smelt_hir::ExprId)],
        body: &Body,
    ) -> Option<smelt_hir::TypeId> {
        let mut fitting = Vec::new();
        for member in members {
            if !matches!(self.ctx.krate.types.get(*member), Some(Type::Class { .. })) {
                continue;
            }
            if self.record_literal_discriminants_match(*member, entries, body)
                && let Some(fit) = self.contextual_record_literal_member(*member, entries, body)
            {
                fitting.push(fit);
            }
        }
        match fitting.as_slice() {
            [single] => Some(*single),
            _ => None,
        }
    }

    /// Whether every literal-valued entry of an object literal is admitted by
    /// the literal type of the field it writes on interface arm `member`.
    ///
    /// A field without recorded literals (or an arm that is not an interface)
    /// constrains nothing here; the field-type check does the rest.
    fn record_literal_discriminants_match(
        &self,
        member: smelt_hir::TypeId,
        entries: &[(smelt_hir::ExprId, smelt_hir::ExprId)],
        body: &Body,
    ) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(member) else {
            return true;
        };
        let Some(interface) = self.find_interface(*name) else {
            return true;
        };
        entries.iter().all(|(key, value)| {
            let expr = |id: smelt_hir::ExprId| body.expr_at(id);
            let Some(ExprKind::Literal(Literal::String(field_key))) = expr(*key).map(|key| &key.kind) else {
                return true;
            };
            let Some(ExprKind::Literal(written)) = expr(*value).map(|value| &value.kind) else {
                return true;
            };
            let Some(facts) = interface.decode_facts.iter().find(|facts| {
                self.ctx.krate.symbols.source(facts.name) == Some(field_key.as_str())
                    && !facts.literals.is_empty()
            }) else {
                return true;
            };
            facts.literals.iter().any(|admitted| literal_values_equal(admitted, written))
        })
    }

    /// Whether the literal object literal `entries` is exactly a value of the
    /// single record type `candidate`.
    fn contextual_record_literal_member(
        &mut self,
        candidate: smelt_hir::TypeId,
        entries: &[(smelt_hir::ExprId, smelt_hir::ExprId)],
        body: &Body,
    ) -> Option<smelt_hir::TypeId> {
        let fields = self.contextual_record_literal_fields(candidate)?;
        if fields.is_empty() {
            return None;
        }
        // A REQUIRED field is fine as long as the literal supplies it
        // (`const h: Entry = { handler: 'x', score: 1 }` is `Entry { .. }`);
        // only a missing required field would force inventing a value.
        let supplied_keys = entries
            .iter()
            .map(|(key, _)| {
                let key_expr = body.expr_at(*key)?;
                match &key_expr.kind {
                    ExprKind::Literal(Literal::String(field_key)) => Some(field_key.clone()),
                    _ => None,
                }
            })
            .collect::<Option<Vec<_>>>()?;
        // Compared by SOURCE spelling: a literal writes `hasNext`, the field
        // symbol renders `has_next`.
        if fields.iter().any(|field| {
            !field.optional
                && !self
                    .ctx
                    .krate
                    .symbols
                    .source(field.name)
                    .is_some_and(|name| supplied_keys.iter().any(|key| key == name))
        }) {
            return None;
        }
        for (key, value) in entries {
            let key_expr = body.expr_at(*key)?;
            let ExprKind::Literal(Literal::String(field_key)) = &key_expr.kind else {
                return None;
            };
            let field = self.intern_source_name(field_key);
            let expected = self.class_field_type(candidate, field).ok()?;
            // A getter is stored as the field's accessor, so it fits when it
            // returns exactly the field's type.
            if let Some(getter) = Self::accessor_marker_function(body, *value, "__smelt_get") {
                let returns_field = matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, getter)),
                    Some(Type::Function(function)) if function.params.is_empty() && function.return_ty == expected
                );
                if !returns_field {
                    return None;
                }
                continue;
            }
            let actual = Self::expr_ty(body, *value);
            if !self.contextual_record_field_assignable(actual, expected) {
                return None;
            }
        }
        // Every entry is assignable, so the literal IS the struct and takes the
        // struct's type.
        //
        // This used to be gated on at least one field needing the backend's
        // structural adapter (`needs_structural_adapter.then_some(candidate)`),
        // which had it backwards: a literal needing no adaptation is the easiest
        // one to build directly, and declining it sent the literal down the
        // erased path instead. `const c: Config = {}` for
        // `interface Config { label?: string }` built a
        // `SmeltRecord<String, SmeltUnknown>` and then reconstructed `Config`
        // out of it — reading three keys per field and funnelling every
        // `SmeltUnknown` tag through `to_string()`, which is harmless for
        // `label?: string` and turns a `count?: number` into its decimal text.
        // Pure avoidable erasure plus a latent wrong-value coercion, for the
        // shape a hand-writing Rust team would spell `Config { label: None }`.
        Some(candidate)
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
}
