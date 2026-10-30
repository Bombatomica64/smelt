//! Object spreads that build a typed record.
//!
//! `{ ...defaults, ...options, origin: "*" }` at a record type (`Options`, an
//! interface or object alias, so a generated struct) is that struct, built
//! field by field. Each target field takes its value from the LAST property
//! that writes it, in source order:
//!
//! - an explicit property `f: v` writes `v`;
//! - a spread source whose type declares `f` as REQUIRED writes `src.f`;
//! - a spread source whose type declares `f` as OPTIONAL writes `src.f` only
//!   when it is present: `src.f ?? previous` (with `Option<T>`, an absent
//!   property and an `undefined` one are the same case, see memory "strict
//!   types over JS nullish").
//!
//! The generic path builds an erased record (`dict_assign` over
//! `SmeltRecord<String, SmeltUnknown>`) and converts it back to the struct,
//! which is both slower and erased in between. This module is the typed rule;
//! everything it declines keeps the generic path.
//!
//! It declines BEFORE lowering anything that could have an effect: a spread
//! source must be an identifier or a plain field read (no call, no getter), and
//! every explicit property needs a static key and a non-method value. Lowering
//! a source to read its type therefore leaves at most an unreferenced read
//! behind. Explicit values and sources are then bound to locals in source
//! order, so the struct literal's field order cannot reorder their effects.

use oxc::ast::ast::{Expression, ObjectExpression, ObjectPropertyKind, PropertyKey};
use smelt_hir::{Body, Expr, ExprKind, Field, Literal, Type};

use super::ModuleBuilder;
use crate::SmeltError;

/// One property of the literal, in source order, once it is known to be
/// representable.
enum Piece<'a> {
    /// A spread source and the fields its type declares.
    Spread {
        source: smelt_hir::ExprId,
        source_ty: smelt_hir::TypeId,
        fields: Vec<Field>,
    },
    /// An explicit `key: value` property.
    Explicit {
        name: smelt_hir::Symbol,
        property: &'a oxc::ast::ast::ObjectProperty<'a>,
    },
}

impl ModuleBuilder<'_> {
    /// Lower `object` (which contains a spread) as a struct of `type_hint`'s
    /// record type, or answer `None` to keep the erased-record path.
    pub(in crate::lowering) fn typed_record_spread(
        &mut self,
        object: &ObjectExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(target) = type_hint else {
            return Ok(None);
        };
        let Some(target_fields) = self.plain_record_fields(target)? else {
            return Ok(None);
        };
        if !object.properties.iter().all(|property| match property {
            ObjectPropertyKind::SpreadProperty(spread) => {
                Self::spread_source_is_side_effect_free(&spread.argument)
            }
            ObjectPropertyKind::ObjectProperty(property) => {
                !property.computed
                    && !property.method
                    && matches!(
                        property.key,
                        PropertyKey::StaticIdentifier(_) | PropertyKey::StringLiteral(_)
                    )
            }
        }) {
            return Ok(None);
        }
        let mut pieces = Vec::new();
        for property in &object.properties {
            match property {
                ObjectPropertyKind::SpreadProperty(spread) => {
                    let source = self.expression(&spread.argument, body)?;
                    let source_ty = Self::expr_ty(body, source);
                    let Some(fields) = self.plain_record_fields(source_ty)? else {
                        return Ok(None);
                    };
                    pieces.push(Piece::Spread {
                        source,
                        source_ty,
                        fields,
                    });
                }
                ObjectPropertyKind::ObjectProperty(property) => {
                    let key = match &property.key {
                        PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str(),
                        PropertyKey::StringLiteral(literal) => literal.value.as_str(),
                        _ => return Ok(None),
                    };
                    let name = self.intern_source_name(key);
                    if !target_fields.iter().any(|field| field.name == name) {
                        // A property the record does not declare (an
                        // intersection's extra member) needs the record path.
                        return Ok(None);
                    }
                    pieces.push(Piece::Explicit { name, property });
                }
            }
        }
        // Committed: from here on every expression is part of the result.
        let span = self.span(object.span.start, object.span.end);
        let mut current: Vec<Option<smelt_hir::ExprId>> = vec![None; target_fields.len()];
        for piece in pieces {
            match piece {
                Piece::Spread {
                    source,
                    source_ty,
                    fields,
                } => {
                    let spread_name = self.intern_source_name("__smelt_spread");
                    let local = self.capture_bind_value(source, source_ty, spread_name, span, body);
                    for (slot, target_field) in target_fields.iter().enumerate() {
                        let Some(source_field) =
                            fields.iter().find(|field| field.name == target_field.name)
                        else {
                            continue;
                        };
                        let receiver = body.push_expr(Expr {
                            kind: ExprKind::Local(local),
                            ty: source_ty,
                            span,
                        });
                        let read = body.push_expr(Expr {
                            kind: ExprKind::Field {
                                receiver,
                                field: source_field.name,
                            },
                            ty: source_field.ty,
                            span,
                        });
                        let previous = current.get(slot).copied().flatten();
                        let value = match previous {
                            Some(previous) if source_field.optional => {
                                // Present writes, absent keeps what an earlier
                                // property wrote.
                                let previous_ty = Self::expr_ty(body, previous);
                                body.push_expr(Expr {
                                    kind: ExprKind::OptionalCoalesce {
                                        optional: read,
                                        fallback: previous,
                                    },
                                    ty: previous_ty,
                                    span,
                                })
                            }
                            _ => read,
                        };
                        if let Some(entry) = current.get_mut(slot) {
                            *entry = Some(value);
                        }
                    }
                }
                Piece::Explicit { name, property } => {
                    let Some(slot) = target_fields.iter().position(|field| field.name == name)
                    else {
                        continue;
                    };
                    let field_ty = target_fields.get(slot).map(|field| field.ty);
                    let value = self.expression_with_hint(&property.value, body, field_ty)?;
                    let value_ty = Self::expr_ty(body, value);
                    let value_name = self.intern_source_name("__smelt_spread_value");
                    let local = self.capture_bind_value(value, value_ty, value_name, span, body);
                    let read = body.push_expr(Expr {
                        kind: ExprKind::Local(local),
                        ty: value_ty,
                        span,
                    });
                    if let Some(entry) = current.get_mut(slot) {
                        *entry = Some(read);
                    }
                }
            }
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let mut entries = Vec::new();
        for (field, value) in target_fields.iter().zip(current) {
            let value = match value {
                Some(value) => value,
                // Nothing wrote an optional field: it is absent.
                None if field.optional => body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: field.ty,
                    span,
                }),
                // A required field nothing wrote cannot happen in valid
                // TypeScript (tsc rejects the literal); the field's default
                // keeps the struct constructible.
                None => continue,
            };
            let key_text = self
                .ctx
                .krate
                .names
                .get(field.name)
                .or_else(|| self.ctx.krate.symbols.get(field.name))
                .unwrap_or_default()
                .to_owned();
            let key = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(key_text)),
                ty: string_ty,
                span,
            });
            entries.push((key, value));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: target,
            span,
        })))
    }

    /// Whether reading `expression` can have no effect: an identifier, or a
    /// chain of plain (non-computed, non-optional) member reads off one.
    ///
    /// A getter could still run behind a member read; the field check in
    /// [`Self::plain_record_fields`] only admits interface records, whose
    /// members are data.
    pub(in crate::lowering) fn spread_source_is_side_effect_free(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::Identifier(_) | Expression::ThisExpression(_) => true,
            Expression::StaticMemberExpression(member) => {
                !member.optional && Self::spread_source_is_side_effect_free(&member.object)
            }
            Expression::ParenthesizedExpression(inner) => {
                Self::spread_source_is_side_effect_free(&inner.expression)
            }
            _ => false,
        }
    }

    /// The fields of `ty` when it is a plain data interface record (no
    /// methods, call or construct signatures, heritage or index signature),
    /// instantiated at its type arguments; `None` otherwise.
    pub(in crate::lowering) fn plain_record_fields(
        &mut self,
        ty: smelt_hir::TypeId,
    ) -> Result<Option<Vec<Field>>, SmeltError> {
        let Some(Type::Class { name, args }) = self.ctx.krate.types.get(ty).cloned() else {
            return Ok(None);
        };
        let Some(interface) = self.find_interface(name).cloned() else {
            return Ok(None);
        };
        let plain = interface.methods.is_empty()
            && interface.extends.is_empty()
            && self.ctx.interface_call_signatures.get(&name).is_none_or(Vec::is_empty)
            && self.interfaces.index_value(name).is_none()
            && !interface.fields.iter().any(|field| {
                self.ctx
                    .krate
                    .symbols
                    .get(field.name)
                    .is_some_and(|text| text.starts_with("__smelt"))
            });
        if !plain {
            return Ok(None);
        }
        let span = interface.span;
        let substitutions = self.type_argument_substitution(&interface.type_params, &args, span)?;
        Ok(Some(self.substituted_fields(&interface.fields, &substitutions)))
    }
}
