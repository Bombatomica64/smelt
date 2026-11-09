//! Const collections: top-level `const` arrays and object-value lists whose
//! elements are known literals, recorded so reads can fold them.

use crate::lowering::erasure::NotNewErasure;
use crate::{ObjectConst, ObjectConstValue};
use crate::lowering::js_text::js_str_literal;
use oxc::ast::ast::{Argument, ArrayExpressionElement, Expression, ObjectPropertyKind};
use smelt_hir::{ErasureReason, ExprKind, Literal, Type};
use super::super::{ConstCollection, ConstCollectionItem, ConstCollectionValue, ModuleBuilder};

impl ModuleBuilder<'_> {
    /// Extract literal array and set constants that nested function bodies can inline.
    pub(in crate::lowering) fn const_collection_from_initializer(
        &mut self,
        init: &Expression<'_>,
        ty: smelt_hir::TypeId,
    ) -> Option<ConstCollection> {
        match init {
            Expression::ArrayExpression(array) => Some(ConstCollection {
                items: self.const_collection_items(array.elements.iter())?,
                ty,
                is_set: false,
            }),
            Expression::NewExpression(new_expr)
                if self.host_global(&new_expr.callee).is_some_and(|host| host.is("Set")) =>
            {
                let [Argument::ArrayExpression(array)] = new_expr.arguments.as_slice() else {
                    return None;
                };
                Some(ConstCollection {
                    items: self.const_collection_items(array.elements.iter())?,
                    ty,
                    is_set: true,
                })
            }
            Expression::CallExpression(call) => {
                let name = self.object_values_identifier_argument(call)?;
                self.consts.object_value_collection(name.as_str()).cloned()
            }
            // `export const ALIAS = otherConst;` shares the referenced
            // module-level collection so nested bodies inline the alias too.
            Expression::Identifier(identifier) => self.consts.collection(identifier.name.as_str())
                .cloned(),
            Expression::TSAsExpression(as_expr) => {
                self.const_collection_from_initializer(&as_expr.expression, ty)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.const_collection_from_initializer(&satisfies.expression, ty)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.const_collection_from_initializer(&non_null.expression, ty)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.const_collection_from_initializer(&parenthesized.expression, ty)
            }
            _ => None,
        }
    }

    /// Extract literal elements from a module-level constant array.
    pub(in crate::lowering) fn const_collection_items<'a>(
        &mut self,
        elements: impl Iterator<Item = &'a ArrayExpressionElement<'a>>,
    ) -> Option<Vec<ConstCollectionItem>> {
        let mut items = Vec::new();
        for element in elements {
            match element {
                ArrayExpressionElement::StringLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::String);
                    items.push(ConstCollectionItem {
                        value: ConstCollectionValue::Expr(ExprKind::Literal(js_str_literal(literal.value))),
                        ty,
                    });
                }
                ArrayExpressionElement::NumericLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::Float);
                    items.push(ConstCollectionItem {
                        value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::Float(
                            literal.value,
                        ))),
                        ty,
                    });
                }
                ArrayExpressionElement::BooleanLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::Bool);
                    items.push(ConstCollectionItem {
                        value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::Bool(
                            literal.value,
                        ))),
                        ty,
                    });
                }
                ArrayExpressionElement::NullLiteral(_) => {
                    let ty = self.ctx.krate.types.intern(Type::None);
                    items.push(ConstCollectionItem {
                        value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::None)),
                        ty,
                    });
                }
                ArrayExpressionElement::SpreadElement(spread) => {
                    let Expression::Identifier(identifier) = &spread.argument else {
                        return None;
                    };
                    let collection = self.consts.collection(identifier.name.as_str())?;
                    items.extend(collection.items.clone());
                }
                ArrayExpressionElement::Elision(_) => return None,
                _ => return None,
            }
        }
        Some(items)
    }

    /// Return the identifier passed to a direct `Object.values(identifier)` call.
    pub(in crate::lowering) fn object_values_identifier_argument(
        &self,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Option<String> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if !self.names_host(object.name.as_str(), "Object") || member.property.name != "values" {
            return None;
        }
        let [Argument::Identifier(identifier)] = call.arguments.as_slice() else {
            return None;
        };
        Some(identifier.name.as_str().to_owned())
    }

    /// Build an `Object.values` collection from a reusable static object const.
    pub(in crate::lowering) fn const_collection_from_object_const(
        &mut self,
        value: &ObjectConst,
    ) -> Option<ConstCollection> {
        let Type::Dict(_, value_ty) = self.ctx.krate.types.get(value.ty)? else {
            return None;
        };
        let items = value
            .entries
            .iter()
            .map(|entry| {
                let item_value = match &entry.value {
                    ObjectConstValue::Literal(literal) => {
                        ConstCollectionValue::Expr(ExprKind::Literal(literal.clone()))
                    }
                    ObjectConstValue::RegExp { .. } => ConstCollectionValue::UnknownObject,
                    ObjectConstValue::Expr(kind) => ConstCollectionValue::Expr(kind.clone()),
                    ObjectConstValue::List(_) => ConstCollectionValue::UnknownArray,
                    ObjectConstValue::Object(_) => ConstCollectionValue::UnknownObject,
                };
                ConstCollectionItem {
                    value: item_value,
                    ty: entry.value_ty,
                }
            })
            .collect();
        let ty = self.ctx.krate.types.intern(Type::List(*value_ty));
        Some(ConstCollection {
            items,
            ty,
            is_set: false,
        })
    }

    /// Build an approximate erased collection for `Object.values` over dynamic objects.
    ///
    /// Mixed JavaScript value-provider objects are frequently exported as module
    /// constants and projected in tests. The object itself still lowers as a
    /// runtime const; this side table only gives nested function/test bodies a
    /// stable list shape while preserving null versus undefined.
    pub(in crate::lowering) fn const_unknown_value_collection_from_object(
        &mut self,
        object: &oxc::ast::ast::ObjectExpression<'_>,
    ) -> Option<ConstCollection> {
        // Not recorded yet: the erasure is recorded once below, only when the
        // collection is actually built.
        let unknown_ty = self.unknown_type(NotNewErasure::ErasedUpstream);
        let optional_unknown_ty = self.ctx.krate.types.intern(Type::Optional(unknown_ty));
        let mut items = Vec::new();
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return None;
            };
            if object_property.computed || object_property.method {
                return None;
            }
            items.push(self.const_unknown_value_item(&object_property.value, unknown_ty));
        }
        let span = self.span(object.span.start, object.span.end);
        // Same interned `Type::Unknown` as `unknown_ty`; called for the record.
        let _ = self.erase_at(
            ErasureReason::DynamicOperator,
            span,
            "Object.values of a heterogeneous object",
        );
        let ty = self.ctx.krate.types.intern(Type::List(optional_unknown_ty));
        Some(ConstCollection {
            items,
            ty,
            is_set: false,
        })
    }

    /// Approximate one dynamic object value as an optional erased JS value.
    pub(in crate::lowering) fn const_unknown_value_item(
        &mut self,
        expression: &Expression<'_>,
        unknown_ty: smelt_hir::TypeId,
    ) -> ConstCollectionItem {
        match expression {
            Expression::StringLiteral(literal) => ConstCollectionItem {
                value: ConstCollectionValue::Expr(ExprKind::Literal(js_str_literal(literal.value))),
                ty: self.ctx.krate.types.intern(Type::String),
            },
            Expression::NumericLiteral(literal) => ConstCollectionItem {
                value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::Float(literal.value))),
                ty: self.ctx.krate.types.intern(Type::Float),
            },
            Expression::BooleanLiteral(literal) => ConstCollectionItem {
                value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::Bool(literal.value))),
                ty: self.ctx.krate.types.intern(Type::Bool),
            },
            Expression::NullLiteral(_) => ConstCollectionItem {
                value: ConstCollectionValue::UnknownNull,
                ty: unknown_ty,
            },
            Expression::Identifier(identifier) if identifier.name == "undefined" => {
                ConstCollectionItem {
                    value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::Undefined)),
                    ty: self.ctx.krate.types.intern(Type::None),
                }
            }
            Expression::ArrayExpression(_) | Expression::NewExpression(_) => ConstCollectionItem {
                value: ConstCollectionValue::UnknownArray,
                ty: unknown_ty,
            },
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
                ConstCollectionItem {
                    value: ConstCollectionValue::UnknownFunction,
                    ty: unknown_ty,
                }
            }
            Expression::TSAsExpression(as_expr) => {
                self.const_unknown_value_item(&as_expr.expression, unknown_ty)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.const_unknown_value_item(&satisfies.expression, unknown_ty)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.const_unknown_value_item(&non_null.expression, unknown_ty)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.const_unknown_value_item(&parenthesized.expression, unknown_ty)
            }
            _ => ConstCollectionItem {
                value: ConstCollectionValue::UnknownObject,
                ty: unknown_ty,
            },
        }
    }
}
