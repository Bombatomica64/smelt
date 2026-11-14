//! Destructuring patterns: `let { a, b: [c] } = value` and parameter patterns,
//! lowered to one local declaration per bound name.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::js_text::{has_lone_surrogate, js_str_literal};
use oxc::ast::ast::{BindingPattern, PropertyKey};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, ErasureReason, Expr, ExprKind, FileId, Literal, LocalDecl, Pattern, Span, Stmt, Type,
};

impl ModuleBuilder<'_> {
    /// Lower a binding pattern in a variable declaration.
    pub(in crate::lowering) fn binding_declaration(
        &mut self,
        pattern: &BindingPattern<'_>,
        value: Option<smelt_hir::ExprId>,
        annotated_ty: Option<smelt_hir::TypeId>,
        mutable: bool,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                // An erased binding (no type, or an erased derived type from a
                // pattern element) takes the checker's type for it. A source
                // `any` annotation stays erased: the checker answers `any`,
                // which is never adopted.
                let erased_annotation = annotated_ty
                    .is_none_or(|annotated| self.type_is_erased_binding_type(annotated));
                let typed_value = if erased_annotation {
                    value.map(|value| self.checker_typed_erased_binding(binding, value, body))
                } else {
                    value
                };
                let annotated_ty = match (annotated_ty, typed_value) {
                    (Some(annotated), Some(typed)) if typed_value != value => {
                        let typed_ty = Self::expr_ty(body, typed);
                        if typed_ty == annotated {
                            Some(annotated)
                        } else {
                            Some(typed_ty)
                        }
                    }
                    (annotated, _) => annotated,
                };
                let value = typed_value;
                let ty = annotated_ty
                    .or_else(|| value.map(|expr_id| Self::expr_ty(body, expr_id)))
                    .unwrap_or_else(|| {
                        self.erase_with(ErasureReason::MissingAnnotation, binding.name.as_str())
                    });
                if annotated_ty.is_some()
                    && value.is_some()
                    && matches!(self.ctx.krate.types.get(ty), Some(Type::Never))
                {
                    return Err(SmeltError::unsupported(
                        self.span(binding.span.start, binding.span.end),
                        "variable annotation `never` requires a diverging initializer",
                    ));
                }
                let name = binding.name.as_str();
                let symbol = self.intern_source_name(name);
                let local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty,
                    mutable,
                    span: self.span(binding.span.start, binding.span.end),
                });
                if value.is_some_and(|value| self.expression_is_known_date_value(value, body))
                    || self.type_is_known_date_value(ty)
                {
                    self.scope.mark_date_value(local);
                }
                // An explicit `any` annotation pins the local to the erased
                // `Unknown` boundary: record it so later concrete assignments do
                // not flow-narrow its storage type away from `Unknown`.
                if annotated_ty.is_some_and(|annotated| {
                    matches!(self.ctx.krate.types.get(annotated), Some(Type::Unknown))
                }) {
                    self.scope.mark_explicit_any(local);
                }
                self.scope.bind(name.to_owned(), local);
                let pat = body.push_pattern(Pattern::Binding(local));
                body.push_stmt_to_block(block, Stmt::Let { pat, ty, value });
                Ok(())
            }
            BindingPattern::ObjectPattern(object) => {
                let Some(receiver) = value else {
                    return Err(SmeltError::unsupported(
                        self.span(object.span.start, object.span.end),
                        "object destructuring requires an initializer",
                    ));
                };
                let mut omitted_keys = Vec::new();
                for property in &object.properties {
                    // A key holding a lone surrogate has no field spelling, so it
                    // reads by its exact runtime key, as a computed key does.
                    let keyed_at_runtime = property.computed
                        || matches!(&property.key, PropertyKey::StringLiteral(literal) if has_lone_surrogate(literal.value));
                    let (ty, extracted, omitted_key) =
                        if keyed_at_runtime {
                            let index = self.property_key_index_expression(&property.key, body)?;
                            let ty = self.dynamic_field_type(Self::expr_ty(body, receiver));
                            (
                                ty,
                                body.push_expr(Expr {
                                    kind: ExprKind::Index { receiver, index },
                                    ty,
                                    span: self.span(property.span.start, property.span.end),
                                }),
                                index,
                            )
                        } else {
                            let field = self.property_key_symbol(&property.key)?;
                            let omitted_key =
                                self.object_destructuring_static_key_expr(&property.key, body)?;
                            let receiver_ty = Self::expr_ty(body, receiver);
                            let field_span = self.span(property.span.start, property.span.end);
                            let field_name = self
                                .ctx
                                .krate
                                .symbols
                                .get(field)
                                .unwrap_or_default()
                                .to_owned();
                            if let Some(extracted) = self.iterator_result_member(
                                receiver,
                                receiver_ty,
                                &field_name,
                                field_span,
                                body,
                            ) {
                                omitted_keys.push(omitted_key);
                                let ty = Self::expr_ty(body, extracted);
                                self.binding_declaration(
                                    &property.value,
                                    Some(extracted),
                                    Some(ty),
                                    mutable,
                                    body,
                                    block,
                                )?;
                                continue;
                            }
                            let is_element_count =
                                self.ctx.krate.symbols.get(field).is_some_and(|name| {
                                    self.element_count_member(receiver_ty, name)
                                });
                            let ty = if is_element_count {
                                self.ctx.krate.types.intern(Type::Float)
                            } else {
                                self.class_field_type(receiver_ty, field)?
                            };
                            let extracted = if is_element_count {
                                body.push_expr(Expr {
                                    kind: ExprKind::Len { operand: receiver },
                                    ty,
                                    span: self.span(property.span.start, property.span.end),
                                })
                            } else {
                                body.push_expr(Expr {
                                    kind: ExprKind::Field { receiver, field },
                                    ty,
                                    span: self.span(property.span.start, property.span.end),
                                })
                            };
                            (ty, extracted, omitted_key)
                        };
                    omitted_keys.push(omitted_key);
                    self.binding_declaration(
                        &property.value,
                        Some(extracted),
                        Some(ty),
                        mutable,
                        body,
                        block,
                    )?;
                }
                if let Some(rest) = &object.rest {
                    self.object_rest_binding_declaration(
                        &rest.argument,
                        receiver,
                        &omitted_keys,
                        body,
                        block,
                    )?;
                }
                Ok(())
            }
            BindingPattern::ArrayPattern(array) => {
                let Some(initial_receiver) = value else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "array destructuring requires an initializer",
                    ));
                };
                let mut receiver = initial_receiver;
                let mut receiver_ty = Self::expr_ty(body, receiver);
                if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(receiver_ty).cloned()
                {
                    receiver = body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: receiver },
                        ty: inner,
                        span: self.span(array.span.start, array.span.end),
                    });
                    receiver_ty = inner;
                }
                let tuple_items = match self.ctx.krate.types.get(receiver_ty).cloned() {
                    Some(Type::Tuple(items)) => Some(items),
                    _ => None,
                };
                let fallback_item_ty = if tuple_items.is_none() {
                    Some(self.index_type(receiver_ty)?)
                } else {
                    None
                };
                for (idx, element) in array.elements.iter().enumerate() {
                    let Some(element) = element else {
                        continue;
                    };
                    let source_item_ty = tuple_items
                        .as_ref()
                        .and_then(|items| items.get(idx).copied())
                        .or(fallback_item_ty)
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(element.span().start, element.span().end),
                                "array destructuring index is outside tuple type",
                            )
                        })?;
                    let idx = u32::try_from(idx).map_err(|err| {
                        SmeltError::unsupported(
                            self.span(array.span.start, array.span.end),
                            format!("array destructuring index is too large: {err}"),
                        )
                    })?;
                    let index_ty = self.ctx.krate.types.intern(Type::Float);
                    let index = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::Float(f64::from(idx))),
                        ty: index_ty,
                        span: self.span(array.span.start, array.span.end),
                    });
                    let (extracted_kind, item_ty) = if tuple_items.is_some() {
                        (
                            ExprKind::TupleIndex {
                                tuple: receiver,
                                index: usize::try_from(idx).map_err(|err| {
                                    SmeltError::unsupported(
                                        self.span(array.span.start, array.span.end),
                                        format!(
                                            "array destructuring tuple index is too large: {err}"
                                        ),
                                    )
                                })?,
                            },
                            source_item_ty,
                        )
                    } else {
                        let optional_item_ty =
                            self.ctx.krate.types.intern(Type::Optional(source_item_ty));
                        (ExprKind::Index { receiver, index }, optional_item_ty)
                    };
                    let extracted = body.push_expr(Expr {
                        kind: extracted_kind,
                        ty: item_ty,
                        span: self.span(element.span().start, element.span().end),
                    });
                    self.binding_declaration(
                        element,
                        Some(extracted),
                        Some(item_ty),
                        mutable,
                        body,
                        block,
                    )?;
                }
                if let Some(rest) = &array.rest {
                    let start = array.elements.len();
                    let (rest_ty, extracted) = if let Some(items) = &tuple_items {
                        let end = items.len();
                        let selected = if start <= end {
                            items.get(start..).unwrap_or_default().to_vec()
                        } else {
                            Vec::new()
                        };
                        let item_ty = match selected.as_slice() {
                            [] => self
                                .erase_with(ErasureReason::MissingAnnotation, "empty tuple rest"),
                            [single] => *single,
                            _ => {
                                let mut unique = Vec::new();
                                for item in selected {
                                    if !unique.contains(&item) {
                                        unique.push(item);
                                    }
                                }
                                match unique.as_slice() {
                                    [single] => *single,
                                    _ => self.ctx.krate.types.intern(Type::Union(unique)),
                                }
                            }
                        };
                        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                        let mut rest_items = Vec::new();
                        for index in start..end {
                            let item_ty = items.get(index).copied().unwrap_or(item_ty);
                            rest_items.push(body.push_expr(Expr {
                                kind: ExprKind::TupleIndex {
                                    tuple: receiver,
                                    index,
                                },
                                ty: item_ty,
                                span: self.span(rest.span.start, rest.span.end),
                            }));
                        }
                        let extracted = body.push_expr(Expr {
                            kind: ExprKind::ListLit(rest_items),
                            ty,
                            span: self.span(rest.span.start, rest.span.end),
                        });
                        (ty, extracted)
                    } else {
                        let item_ty = self.index_type(receiver_ty)?;
                        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                        let start_index = u32::try_from(start).map_err(|err| {
                            SmeltError::unsupported(
                                self.span(array.span.start, array.span.end),
                                format!("array destructuring rest index is too large: {err}"),
                            )
                        })?;
                        let index_ty = self.ctx.krate.types.intern(Type::Float);
                        let start_expr = body.push_expr(Expr {
                            kind: ExprKind::Literal(Literal::Float(f64::from(start_index))),
                            ty: index_ty,
                            span: self.span(rest.span.start, rest.span.end),
                        });
                        (
                            ty,
                            body.push_expr(Expr {
                                kind: ExprKind::ListSlice {
                                    list: receiver,
                                    start: Some(start_expr),
                                    end: None,
                                },
                                ty,
                                span: self.span(rest.span.start, rest.span.end),
                            }),
                        )
                    };
                    self.binding_declaration(
                        &rest.argument,
                        Some(extracted),
                        Some(rest_ty),
                        mutable,
                        body,
                        block,
                    )?;
                }
                Ok(())
            }
            BindingPattern::AssignmentPattern(assign) => {
                let fallback_hint = annotated_ty
                    .and_then(|ty| self.non_nullish_type(ty))
                    .or(annotated_ty);
                let fallback = self.expression_with_hint(&assign.right, body, fallback_hint)?;
                let fallback_ty = Self::expr_ty(body, fallback);
                let value = if let Some(value) = value {
                    let value_ty = Self::expr_ty(body, value);
                    if self.ctx.krate.types.get(value_ty) == Some(&Type::None) {
                        fallback
                    } else if self.ctx.krate.types.get(value_ty) == Some(&Type::Unknown)
                        && !self.checker_types_binding_concretely(&assign.left)
                    {
                        // `const { x = true } = rec` with `rec: Record<string,
                        // unknown>` binds `x: unknown`, as TypeScript types it:
                        // a present value of ANY type wins over the default.
                        // Typing the binding from the default asserted `"t"`
                        // to `true`. The binding stays at the source's own
                        // `unknown` (the declared dynamic boundary), and the
                        // default joins it.
                        let fallback =
                            Self::fallback_at_type(fallback, fallback_ty, value_ty, body);
                        let span = self.span(assign.span.start, assign.span.end);
                        self.default_when_undefined(value, fallback, value_ty, span, body, block)
                    } else if self.ctx.krate.types.get(value_ty) == Some(&Type::Unknown) {
                        // The checker types the binding concretely: the value
                        // was erased upstream (an unmodeled declaration), and
                        // the default's type is the recovered one.
                        body.push_expr(Expr {
                            kind: ExprKind::OptionalCoalesce {
                                optional: value,
                                fallback,
                            },
                            ty: fallback_ty,
                            span: self.span(assign.span.start, assign.span.end),
                        })
                    } else if let Some(non_null_ty) = self.non_nullish_type(value_ty)
                        && self.ctx.krate.types.get(non_null_ty) == Some(&Type::Unknown)
                        && self.checker_types_binding_concretely(&assign.left)
                    {
                        // As above, for an optional erased value.
                        let optional = body.push_expr(Expr {
                            kind: ExprKind::TypeAssert { value },
                            ty: smelt_hir::type_normalize::optional_of(
                                &mut self.ctx.krate.types,
                                fallback_ty,
                            ),
                            span: self.span(assign.left.span().start, assign.left.span().end),
                        });
                        let coalesced_value = body.push_expr(Expr {
                            kind: ExprKind::OptionalCoalesce { optional, fallback },
                            ty: fallback_ty,
                            span: self.span(assign.span.start, assign.span.end),
                        });
                        let ty = Self::expr_ty(body, coalesced_value);
                        return self.binding_declaration(
                            &assign.left,
                            Some(coalesced_value),
                            Some(ty),
                            mutable,
                            body,
                            block,
                        );
                    } else if let Some(non_null_ty) = self.non_nullish_type(value_ty) {
                        if self.ctx.krate.types.get(non_null_ty) == Some(&Type::Unknown) {
                            // `unknown | undefined`: the binding is `unknown`
                            // (see the arm above), not the default's type.
                            let fallback =
                                Self::fallback_at_type(fallback, fallback_ty, non_null_ty, body);
                            let span = self.span(assign.span.start, assign.span.end);
                            // An absent `Option` is `undefined`; a present one
                            // still holds a tagged value that may be `undefined`.
                            let undefined = body.push_expr(Expr {
                                kind: ExprKind::Literal(Literal::Undefined),
                                ty: non_null_ty,
                                span,
                            });
                            let present = body.push_expr(Expr {
                                kind: ExprKind::OptionalCoalesce {
                                    optional: value,
                                    fallback: undefined,
                                },
                                ty: non_null_ty,
                                span,
                            });
                            let coalesced_value = self.default_when_undefined(
                                present,
                                fallback,
                                non_null_ty,
                                span,
                                body,
                                block,
                            );
                            let ty = Self::expr_ty(body, coalesced_value);
                            return self.binding_declaration(
                                &assign.left,
                                Some(coalesced_value),
                                Some(ty),
                                mutable,
                                body,
                                block,
                            );
                        }
                        // The default's type either already fits the element type
                        // (equal, numerically compatible, or a member of an
                        // element union that can inject it) or it does not. In the
                        // latter case the JavaScript binding is the *union* of the
                        // element and default types — e.g. `const [s, n = 0] =
                        // str.split('e')` types `n` as `string | number`, the
                        // string element defaulting to the numeric `0`. Force-
                        // asserting a numeric default to a `String` element would
                        // leave a runtime `f64` statically typed as `String`; unify
                        // into the union instead and let the coercion seam inject
                        // each arm.
                        let element_can_hold_fallback = fallback_ty == non_null_ty
                            || self.numeric_type_compatible(non_null_ty, fallback_ty)
                            || matches!(
                                self.ctx.krate.types.get(non_null_ty),
                                Some(Type::Union(_) | Type::Unknown)
                            );
                        if element_can_hold_fallback {
                            let fallback = if fallback_ty == non_null_ty
                                || self.numeric_type_compatible(non_null_ty, fallback_ty)
                            {
                                fallback
                            } else {
                                body.push_expr(Expr {
                                    kind: ExprKind::TypeAssert { value: fallback },
                                    ty: non_null_ty,
                                    span: self
                                        .span(assign.right.span().start, assign.right.span().end),
                                })
                            };
                            body.push_expr(Expr {
                                kind: ExprKind::OptionalCoalesce {
                                    optional: value,
                                    fallback,
                                },
                                ty: non_null_ty,
                                span: self.span(assign.span.start, assign.span.end),
                            })
                        } else {
                            let unified = self
                                .ctx
                                .krate
                                .types
                                .intern(Type::Union(vec![non_null_ty, fallback_ty]));
                            body.push_expr(Expr {
                                kind: ExprKind::OptionalCoalesce {
                                    optional: value,
                                    fallback,
                                },
                                ty: unified,
                                span: self.span(assign.span.start, assign.span.end),
                            })
                        }
                    } else {
                        value
                    }
                } else {
                    fallback
                };
                let ty = Self::expr_ty(body, value);
                self.binding_declaration(&assign.left, Some(value), Some(ty), mutable, body, block)
            }
        }
    }

    /// Whether the TS7 checker gives the binding `pattern` a type other than
    /// `unknown`/`any`. A binding whose value Smelt sees as erased but the
    /// checker types concretely was erased upstream, not declared dynamic.
    /// `false` without facts, where the declared `unknown` is taken at its word.
    fn checker_types_binding_concretely(&self, pattern: &BindingPattern<'_>) -> bool {
        let Some(facts) = self.ts_facts else {
            return false;
        };
        let span = pattern.span();
        facts
            .type_at(&self.path, span.start, span.end)
            .is_some_and(|ty| {
                !matches!(
                    ty.kind,
                    smelt_ts_model::TsTypeKind::Unknown | smelt_ts_model::TsTypeKind::Any
                )
            })
    }

    /// `value === undefined ? fallback : value` for a tagged `unknown` value:
    /// a destructuring default applies to `undefined` only, so a present
    /// `null` (or `0`, `""`) is kept, and the default is evaluated only when
    /// it applies. `value` is read once, through a temporary.
    fn default_when_undefined(
        &mut self,
        value: smelt_hir::ExprId,
        fallback: smelt_hir::ExprId,
        unknown_ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> smelt_hir::ExprId {
        let temp = body.push_local(LocalDecl {
            name: None,
            ty: unknown_ty,
            mutable: false,
            span,
        });
        let pat = body.push_pattern(Pattern::Binding(temp));
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat,
                ty: unknown_ty,
                value: Some(value),
            },
        );
        let read = |arena: &mut Body| {
            arena.push_expr(Expr {
                kind: ExprKind::Local(temp),
                ty: unknown_ty,
                span,
            })
        };
        let tested = read(body);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let cond_value = body.push_expr(Expr {
            kind: ExprKind::UnknownIs {
                value: tested,
                kind: smelt_hir::UnknownKind::Undefined,
            },
            ty: bool_ty,
            span,
        });
        let kept = read(body);
        body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond: cond_value,
                then_expr: fallback,
                else_expr: kept,
            },
            ty: unknown_ty,
            span,
        })
    }

    /// `fallback` (of type `fallback_ty`) viewed at `target_ty`, the binding's
    /// type: unchanged when they agree, else asserted to it.
    fn fallback_at_type(
        fallback: smelt_hir::ExprId,
        fallback_ty: smelt_hir::TypeId,
        target_ty: smelt_hir::TypeId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        if fallback_ty == target_ty {
            return fallback;
        }
        let span = Self::body_expr_span(body, fallback);
        body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value: fallback },
            ty: target_ty,
            span,
        })
    }

    /// Create a string key expression for a static object destructuring property.
    /// Whether a value of `ty` is erased at run time: the carrier, an optional
    /// of it, or a list or tuple of it.
    fn type_is_erased_container(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Unknown) => true,
            Some(Type::Optional(inner) | Type::List(inner)) => {
                self.type_is_erased_container(*inner)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .all(|item| self.type_is_erased_container(*item)),
            _ => false,
        }
    }

    /// Whether `ty` is a shape the generated Rust represents as itself:
    /// primitives, containers and tuples of such, and classes or interfaces the
    /// program declares. A host class Smelt does not model (`AbortSignal`)
    /// renders erased anyway, so adopting it would only add a cast that is
    /// erased again.
    fn checker_type_is_represented(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::BigInt) => true,
            Some(Type::Optional(inner) | Type::List(inner) | Type::Set(inner)) => {
                self.checker_type_is_represented(*inner)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .all(|item| self.checker_type_is_represented(*item)),
            Some(Type::Class { name, args }) => {
                (self.find_interface(*name).is_some() || self.class_by_symbol(*name).is_some())
                    && args
                        .iter()
                        .all(|arg| self.checker_type_is_represented(*arg))
            }
            _ => false,
        }
    }

    /// Whether a binding's type is the erased carrier or an optional of it.
    fn type_is_erased_binding_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Unknown) => true,
            Some(Type::Optional(inner)) => {
                matches!(self.ctx.krate.types.get(*inner), Some(Type::Unknown))
            }
            _ => false,
        }
    }

    /// A binding whose value is erased (`unknown`, or an optional `unknown`
    /// element read out of an erased array) takes the type TypeScript gave the
    /// binding, projected out of the erased value.
    ///
    /// `([x, y]) => x + y` as a ts-pattern handler: the checker types the
    /// parameter as a tuple it cannot always re-spell here, but `x` and `y`
    /// are each `number`. Without this both were `Option<SmeltUnknown>` and
    /// `x + y` did not compile (E0369). A binding the checker types as
    /// `unknown`, `never` or nothing keeps the erased value, and so does a part
    /// read out of a TYPED value (`const { start } = interval` with `interval:
    /// Interval<D, D>`), whose erased HIR type the backend renders as its real
    /// type: only a part of an erased value is erased at run time.
    fn checker_typed_erased_binding(
        &mut self,
        binding: &oxc::ast::ast::BindingIdentifier<'_>,
        value: smelt_hir::ExprId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let value_ty = Self::expr_ty(body, value);
        // Only a PART read out of an erased container: a whole erased value
        // bound to a name (`const start = performance.now()`) keeps the name
        // erased, since typing it moves the erased value into a temporary and
        // re-erases the name wherever it flows back into an erased position.
        let Some(ExprKind::Field { receiver, .. } | ExprKind::Index { receiver, .. }) =
            body.expr_at(value).map(|expr| &expr.kind)
        else {
            return value;
        };
        let receiver_ty = Self::expr_ty(body, *receiver);
        if !self.type_is_erased_container(receiver_ty) {
            return value;
        }
        let erased_inner = match self.ctx.krate.types.get(value_ty) {
            Some(Type::Unknown) => false,
            Some(Type::Optional(inner))
                if matches!(self.ctx.krate.types.get(*inner), Some(Type::Unknown)) =>
            {
                true
            }
            _ => return value,
        };
        let Some(checked) = self
            .checker_binding_type(binding.span.start, binding.span.end)
            .filter(|checked| self.checker_type_is_represented(*checked))
        else {
            return value;
        };
        let span = self.span(binding.span.start, binding.span.end);
        let erased = if erased_inner {
            let unknown =
                self.unknown_type(crate::lowering::erasure::NotNewErasure::ErasedUpstream);
            let undefined = body.push_expr(Expr {
                kind: ExprKind::Literal(smelt_hir::Literal::None),
                ty: unknown,
                span,
            });
            body.push_expr(Expr {
                kind: ExprKind::OptionalCoalesce {
                    optional: value,
                    fallback: undefined,
                },
                ty: unknown,
                span,
            })
        } else {
            value
        };
        body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: erased,
                target: checked,
            },
            ty: checked,
            span,
        })
    }

    pub(in crate::lowering) fn object_destructuring_static_key_expr(
        &mut self,
        key: &PropertyKey<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let (text, start, end) = match key {
            PropertyKey::StaticIdentifier(identifier) => (
                identifier.name.as_str().to_owned(),
                identifier.span.start,
                identifier.span.end,
            ),
            PropertyKey::PrivateIdentifier(identifier) => (
                identifier.name.as_str().to_owned(),
                identifier.span.start,
                identifier.span.end,
            ),
            PropertyKey::StringLiteral(literal) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(js_str_literal(literal.value)),
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                }));
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(key.span().start, key.span().end),
                    "object destructuring rest keys must be static identifiers or string literals",
                ));
            }
        };
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(text)),
            ty,
            span: self.span(start, end),
        }))
    }

    /// Bind an object rest pattern by copying the receiver and deleting extracted keys.
    pub(in crate::lowering) fn object_rest_binding_declaration(
        &mut self,
        pattern: &BindingPattern<'_>,
        receiver: smelt_hir::ExprId,
        omitted_keys: &[smelt_hir::ExprId],
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let rest_ty = if let Some(Type::Dict(_, _)) =
            self.ctx.krate.types.get(Self::expr_ty(body, receiver))
        {
            Self::expr_ty(body, receiver)
        } else {
            let key_ty = self.ctx.krate.types.intern(Type::String);
            let receiver_ty = Self::expr_ty(body, receiver);
            let value_ty = self.callback_erase_unless_unknown(
                receiver_ty,
                ErasureReason::DynamicOperator,
                "object rest",
            );
            self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
        };
        let receiver_span = Self::body_expr_span(body, receiver);
        let rest_value = body.push_expr(Expr {
            kind: ExprKind::DictCopy { dict: receiver },
            ty: rest_ty,
            span: receiver_span,
        });
        for key in omitted_keys {
            let key_span = Self::body_expr_span(body, *key);
            let removed = body.push_expr(Expr {
                kind: ExprKind::DictRemoveKey {
                    dict: rest_value,
                    key: *key,
                },
                ty: self.ctx.krate.types.intern(Type::Bool),
                span: key_span,
            });
            body.push_stmt_to_block(block, Stmt::Expr(removed));
        }
        self.binding_declaration(pattern, Some(rest_value), Some(rest_ty), false, body, block)
    }

    /// Return the span for an expression already inserted into a body.
    pub(in crate::lowering) fn body_expr_span(body: &Body, expr: smelt_hir::ExprId) -> Span {
        let root_span = body
            .block_at(body.root)
            .or_else(|| body.blocks.first())
            .map_or(Span::new(FileId(0), 0, 0), |block| block.span);
        body.expr_at(expr).map_or(root_span, |expr| expr.span)
    }

    /// Collect source names introduced by a binding pattern.
    pub(in crate::lowering) fn binding_pattern_names(
        pattern: &BindingPattern<'_>,
        names: &mut Vec<String>,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                names.push(binding.name.as_str().to_owned());
            }
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    Self::binding_pattern_names(&property.value, names);
                }
                if let Some(rest) = &object.rest {
                    Self::binding_pattern_names(&rest.argument, names);
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for element in array.elements.iter().flatten() {
                    Self::binding_pattern_names(element, names);
                }
                if let Some(rest) = &array.rest {
                    Self::binding_pattern_names(&rest.argument, names);
                }
            }
            BindingPattern::AssignmentPattern(assign) => {
                Self::binding_pattern_names(&assign.left, names);
            }
        }
    }
}
