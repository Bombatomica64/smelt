//! `Object.assign` onto records, classes and callable values.

use crate::lowering::{ModuleBuilder, SmeltError};
use crate::lowering::js_text::js_str_text;
use oxc::ast::ast::{Argument, CallExpression, Expression, ObjectPropertyKind, PropertyKey};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, ErasureReason, Expr, ExprKind, Literal, LocalDecl, Pattern, PropertyLookup, Stmt, Type,
};

/// Static properties and runtime-spread record sources produced when lowering
/// the source arguments of an `Object.assign` onto a callable value.
type CallableAssignSources = (
    Vec<(smelt_hir::Symbol, smelt_hir::ExprId)>,
    Vec<smelt_hir::ExprId>,
);

impl ModuleBuilder<'_> {
    /// Lower TypeScript `Object.assign(target, ...sources)` for homogeneous record values.
    pub(in crate::lowering) fn object_assign_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Object" || member.property.name != "assign" {
            return Ok(None);
        }
        let [target_arg, source_args @ ..] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.assign requires a target record and at least one source record",
            ));
        };
        if source_args.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.assign requires at least one source record",
            ));
        }

        // `Object.assign(globalThis, { .. })` writes INTO the one shared global
        // object (a dynamic boundary, see `ExprKind::GlobalObject`), so there is
        // no typed record to merge into: each source crosses the boundary and
        // is copied onto the global object in place.
        if let Some(target_expression) = target_arg.as_expression()
            && self.expr_is_global_alias(target_expression)
        {
            let target = self.argument(target_arg, body)?;
            let unknown_ty = self.erase_with(ErasureReason::ByName, "globalThis");
            let mut sources = Vec::with_capacity(source_args.len());
            for source_arg in source_args {
                sources.push(self.argument(source_arg, body)?);
            }
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DictAssign { target, sources },
                ty: unknown_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if let Some(expr) = self.object_assign_class_target(target_arg, source_args, call, body)? {
            return Ok(Some(expr));
        }
        if self.object_assign_callable_target_candidate(target_arg, body)
            && Self::object_assign_callable_sources_static(source_args)
        {
            let target = self.argument(target_arg, body)?;
            let target_ty = Self::expr_ty(body, target);
            let resolved = self.type_param_constraint_or_self(target_ty);
            if !matches!(
                self.ctx.krate.types.get(resolved),
                Some(Type::Function(_) | Type::Class { .. })
            ) {
                return Err(SmeltError::unsupported(
                    self.span(target_arg.span().start, target_arg.span().end),
                    "Object.assign callable target must lower to a function or class value",
                ));
            }
            let (props, spreads) = self.object_assign_callable_props(source_args, body)?;
            let unknown_ty = self.erase_with(ErasureReason::UnmodeledType, "callable object");
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::CallableObjectAssign {
                    callable: target,
                    props,
                    spreads,
                },
                ty: unknown_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }

        let mut sources = Vec::with_capacity(source_args.len());
        let mut record_ty = None;
        for source_arg in source_args {
            let source = self.argument_with_hint(source_arg, body, record_ty)?;
            let source_ty = Self::expr_ty(body, source);
            if let Some(source_record_ty) = self.object_assign_record_source_type(source_ty) {
                if let Some(expected_ty) = record_ty {
                    if source_record_ty != expected_ty {
                        return Err(SmeltError::unsupported(
                            self.span(source_arg.span().start, source_arg.span().end),
                            "Object.assign sources must share the target record type",
                        ));
                    }
                } else {
                    record_ty = Some(source_record_ty);
                }
            } else if self.object_assign_object_like_source(source_ty) {
                if record_ty.is_none() {
                    record_ty = Some(self.object_assign_generic_record_type());
                }
            } else {
                return Err(SmeltError::unsupported(
                    self.span(source_arg.span().start, source_arg.span().end),
                    "Object.assign sources must be record or object-like values",
                ));
            }
            sources.push(source);
        }

        let Some(record_ty) = record_ty else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.assign requires record-typed arguments",
            ));
        };
        let mut target = self.argument_with_hint(target_arg, body, Some(record_ty))?;
        if Self::expr_ty(body, target) != record_ty
            && self.object_assign_castable_target(Self::expr_ty(body, target))
        {
            target = body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: target,
                    target: record_ty,
                },
                ty: record_ty,
                span: self.span(target_arg.span().start, target_arg.span().end),
            });
        }
        if Self::expr_ty(body, target) != record_ty
            && matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, target)),
                Some(Type::Dict(_, _))
            )
        {
            target = body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: target,
                    target: record_ty,
                },
                ty: record_ty,
                span: self.span(target_arg.span().start, target_arg.span().end),
            });
        }
        if Self::expr_ty(body, target) != record_ty {
            target = body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: target,
                    target: record_ty,
                },
                ty: record_ty,
                span: self.span(target_arg.span().start, target_arg.span().end),
            });
        }

        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictAssign { target, sources },
            ty: record_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `Object.assign(instance, ...sources)` onto a program class instance.
    ///
    /// JavaScript copies each source's own enumerable properties ONTO the
    /// target object, so the instance itself changes. The record path below
    /// cannot express that for a class instance: it casts the target to a
    /// `Dict` — a copy — merges into the copy, and the instance never sees the
    /// writes. Hono's `HonoBase` constructor is the shape
    /// (`const { strict, ...rest } = options; Object.assign(this, rest)`): the
    /// `router` it copies never reached `this`, so every `basePath()`/`route()`
    /// clone kept a default router and mounted routes answered 404.
    ///
    /// A class instance's assignable keys are its declared own fields (a
    /// struct has nowhere to keep any other key), so the merge lowers to one
    /// guarded field write per field and source:
    /// `if (Object.hasOwn(source, 'f')) target.f = source.f`, the value
    /// converted to the field's declared type. Those are ordinary typed field
    /// writes, so reference-class classification and the field's own typed
    /// conversion apply exactly as for a hand-written `this.f = ...`. The
    /// sources are read through their erased view because their keys are only
    /// known at run time (a rest-destructured options bag); the target never
    /// is.
    ///
    /// Only a `this`/identifier target is taken: it is re-read for each write
    /// (a by-value instance must be written in place, not through a copy), and
    /// re-reading those has no side effect. Any other target, a target that is
    /// not a program class, or a spread source keeps the record path.
    fn object_assign_class_target(
        &mut self,
        target_arg: &Argument<'_>,
        source_args: &[Argument<'_>],
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(target_expression) = target_arg.as_expression() else {
            return Ok(None);
        };
        if !matches!(
            target_expression,
            Expression::ThisExpression(_) | Expression::Identifier(_)
        ) || source_args.iter().any(|arg| arg.as_expression().is_none())
        {
            return Ok(None);
        }
        let target = self.expression(target_expression, body)?;
        let target_ty = Self::expr_ty(body, target);
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(target_ty).cloned() else {
            return Ok(None);
        };
        let fields = self.object_assign_class_fields(name, target_ty);
        if fields.is_empty() {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        // Each source is read key by key at run time (`Object.assign` copies
        // whatever own properties the value has), so it is carried erased.
        let unknown_ty = self.erase_with(ErasureReason::ByName, "Object.assign source");
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        for source_arg in source_args {
            let mut source = self.argument(source_arg, body)?;
            if self.ctx.krate.types.get(Self::expr_ty(body, source)) != Some(&Type::Unknown) {
                source = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: source },
                    ty: unknown_ty,
                    span,
                });
            }
            let source_local = body.push_local(LocalDecl {
                name: Some(self.intern_source_name("smelt_assign_source")),
                ty: unknown_ty,
                mutable: false,
                span,
            });
            let source_pat = body.push_pattern(Pattern::Binding(source_local));
            self.push_current_stmt(
                body,
                Stmt::Let {
                    pat: source_pat,
                    ty: unknown_ty,
                    value: Some(source),
                },
            );
            for (field, field_ty) in &fields {
                let Some(key) = self.ctx.krate.symbols.get(*field).map(str::to_owned) else {
                    continue;
                };
                let probe_read = body.push_expr(Expr {
                    kind: ExprKind::Local(source_local),
                    ty: unknown_ty,
                    span,
                });
                let key_expr = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(key)),
                    ty: string_ty,
                    span,
                });
                let has_key = body.push_expr(Expr {
                    kind: ExprKind::DictContainsKey {
                        dict: probe_read,
                        key: key_expr,
                        lookup: PropertyLookup::Own,
                    },
                    ty: bool_ty,
                    span,
                });
                let source_read = body.push_expr(Expr {
                    kind: ExprKind::Local(source_local),
                    ty: unknown_ty,
                    span,
                });
                let mut value = body.push_expr(Expr {
                    kind: ExprKind::Field {
                        receiver: source_read,
                        field: *field,
                    },
                    ty: unknown_ty,
                    span,
                });
                if *field_ty != unknown_ty {
                    value = body.push_expr(Expr {
                        kind: ExprKind::UnknownCast {
                            value,
                            target: *field_ty,
                        },
                        ty: *field_ty,
                        span,
                    });
                }
                let receiver = self.expression(target_expression, body)?;
                let place = body.push_expr(Expr {
                    kind: ExprKind::Field {
                        receiver,
                        field: *field,
                    },
                    ty: *field_ty,
                    span,
                });
                let then_block = body.push_block(span);
                body.push_stmt_to_block(
                    then_block,
                    Stmt::Assign {
                        target: place,
                        value,
                    },
                );
                self.push_current_stmt(
                    body,
                    Stmt::If {
                        cond: has_key,
                        then_block,
                        else_block: None,
                    },
                );
            }
        }
        Ok(Some(self.expression(target_expression, body)?))
    }

    /// The declared own fields `Object.assign` can write on an instance of
    /// type `target_ty` (a `class`), base classes included, each with its type
    /// as seen through `target_ty`'s type arguments.
    ///
    /// The field list comes from the finished class item when there is one and
    /// otherwise from the in-progress class registry, because the commonest
    /// target, `this` in a constructor, is lowered before its class item
    /// exists. Private names (`#x`) and compiler storage slots are not own
    /// string-keyed properties, so `Object.assign` never writes them; a field
    /// redeclared by a subclass is listed once.
    fn object_assign_class_fields(
        &mut self,
        class: smelt_hir::Symbol,
        target_ty: smelt_hir::TypeId,
    ) -> Vec<(smelt_hir::Symbol, smelt_hir::TypeId)> {
        let mut names: Vec<smelt_hir::Symbol> = Vec::new();
        let mut current = Some(class);
        let mut visited = Vec::new();
        while let Some(symbol) = current {
            if visited.contains(&symbol) {
                break;
            }
            visited.push(symbol);
            let source_name = self
                .ctx
                .krate
                .names
                .get(symbol)
                .or_else(|| self.ctx.krate.symbols.get(symbol))
                .map(str::to_owned);
            let (fields, base) = if let Some(item) = self.class_by_symbol(symbol) {
                (item.fields.clone(), item.base)
            } else {
                let fields = source_name
                    .as_deref()
                    .and_then(|name| self.classes.fields(name).cloned())
                    .unwrap_or_default();
                let base = self
                    .classes
                    .base_of_symbol(symbol)
                    .map(|(base, _)| *base)
                    .or_else(|| {
                        source_name
                            .as_deref()
                            .and_then(|name| self.classes.base(name))
                            .map(|(base, _)| *base)
                    });
                (fields, base)
            };
            for field in fields {
                if field.visibility.is_own_property() && !names.contains(&field.name) {
                    names.push(field.name);
                }
            }
            current = base;
        }
        names
            .into_iter()
            .filter_map(|field| {
                self.class_field_type(target_ty, field)
                    .ok()
                    .map(|field_ty| (field, field_ty))
            })
            .collect()
    }

    /// Push `stmt` into the statement block the current expression lowers into.
    fn push_current_stmt(&self, body: &mut Body, stmt: Stmt) {
        self.push_stmt_to_sink(body, stmt);
    }

    /// Return the concrete record type represented by an `Object.assign` source.
    fn object_assign_record_source_type(
        &self,
        source_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(source_ty) {
            Some(Type::Dict(_, _)) => Some(source_ty),
            Some(Type::Optional(inner)) => self.object_assign_record_source_type(*inner),
            _ => None,
        }
    }

    /// Return whether a source is a TypeScript object surface assignable by `Object.assign`.
    fn object_assign_object_like_source(&self, source_ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(source_ty) {
            Some(Type::Class { .. } | Type::TypeParam { .. } | Type::Unknown) => true,
            Some(Type::Optional(inner)) => self.object_assign_object_like_source(*inner),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.object_assign_object_like_source(item)),
            _ => false,
        }
    }

    /// Return whether an `Object.assign` target can be coerced to the merged record type.
    fn object_assign_castable_target(&self, target_ty: smelt_hir::TypeId) -> bool {
        self.object_assign_object_like_source(target_ty)
            || matches!(self.ctx.krate.types.get(target_ty), Some(Type::None))
    }

    /// Build the fallback record type for object-like assign sources without a record source.
    fn object_assign_generic_record_type(&mut self) -> smelt_hir::TypeId {
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Object.assign");
        self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
    }

    /// Return whether an `Object.assign` target should use callable decoration lowering.
    fn object_assign_callable_target_candidate(
        &self,
        target_arg: &Argument<'_>,
        body: &Body,
    ) -> bool {
        match target_arg {
            Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_) => true,
            Argument::Identifier(ident) => {
                if let Some(local) = self.scope.lookup(ident.name.as_str()) {
                    let ty = Self::local_ty(body, local);
                    let resolved = self.type_param_constraint_or_self(ty);
                    return matches!(
                        self.ctx.krate.types.get(resolved),
                        Some(Type::Function(_) | Type::Class { .. })
                    );
                }
                false
            }
            _ => false,
        }
    }

    /// Lower the sources used to decorate a callable value.
    ///
    /// Object-literal sources contribute their static data properties directly
    /// (`props`). A record-typed local identifier source (e.g. remeda's
    /// `Object.assign(dataLast, lazyDefinition)`) has no statically-known keys
    /// once its shape is erased to a `Dict`, so it is emitted as a runtime
    /// `spread`: its own enumerable entries are copied onto the callable object
    /// at construction time. Returns `(props, spreads)`.
    fn object_assign_callable_props(
        &mut self,
        source_args: &[Argument<'_>],
        body: &mut Body,
    ) -> Result<CallableAssignSources, SmeltError> {
        let mut props = Vec::new();
        let mut spreads = Vec::new();
        for source_arg in source_args {
            if let Argument::Identifier(ident) = source_arg
                && self.scope.is_bound(ident.name.as_str())
            {
                let source = self.argument(source_arg, body)?;
                spreads.push(source);
                continue;
            }
            let Argument::ObjectExpression(object) = source_arg else {
                return Err(SmeltError::unsupported(
                    self.span(source_arg.span().start, source_arg.span().end),
                    "Object.assign callable sources must be static object literals",
                ));
            };
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                    return Err(SmeltError::unsupported(
                        self.span(property.span().start, property.span().end),
                        "Object.assign callable sources do not support spread properties yet",
                    ));
                };
                if object_property.computed || object_property.method {
                    return Err(SmeltError::unsupported(
                        self.span(object_property.span.start, object_property.span.end),
                        "Object.assign callable sources require static data properties",
                    ));
                }
                let key_text = match &object_property.key {
                    PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
                    PropertyKey::StringLiteral(lit) => js_str_text(lit.value).into_owned(),
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(
                                object_property.key.span().start,
                                object_property.key.span().end,
                            ),
                            "Object.assign callable property keys must be static string keys",
                        ));
                    }
                };
                let value = self.expression(&object_property.value, body)?;
                let name = self.intern_source_name(&key_text);
                props.push((name, value));
            }
        }
        Ok((props, spreads))
    }

    /// Return whether `Object.assign` sources are static enough for callable decoration.
    fn object_assign_callable_sources_static(source_args: &[Argument<'_>]) -> bool {
        source_args.iter().all(|source_arg| {
            matches!(
                source_arg,
                Argument::Identifier(_) | Argument::ObjectExpression(_)
            )
        })
    }
}
