//! TypeScript `Map` and `Set` collection-method lowering.
//!
//! Lowers standard-library `Map` and `Set` receiver methods into typed HIR
//! dict/set operations. The impl block below documents the concern in detail.

use super::ModuleBuilder;
use crate::SmeltError;
use oxc::ast::ast::{Argument, Expression};
use oxc::span::GetSpan;
use smelt_hir::{Body, DictProjectionOp, Expr, ExprKind, SetProjectionOp, SetRemoveOp, Type};
use smelt_stdlib::RuleId;

/// `Map` and `Set` collection-method lowering for the TypeScript frontend.
///
/// This concern owns the family that lowers standard-library `Map` and `Set`
/// receiver methods (`has`, `get`, `set`, `add`, `delete`, `clear`, `keys`,
/// `values`, `entries`) into typed HIR dict/set operations.
///
/// # Interface
///
/// Two entry points are registered in the call-lowering chain in `call.rs`:
///
/// * [`Self::dispatch_collection_method`] is the primary entry. It recognizes
///   receiver/member pairs through `smelt-stdlib` method metadata and routes
///   each recognized rule to the matching per-method lowering function.
/// * [`Self::map_projection_call`] is also registered directly because it
///   additionally handles utility-style `keys(value)` / `values(value)` /
///   `entries(value)` calls whose receiver is not statically a `Map`, which the
///   receiver-kind check in `dispatch_collection_method` would not route to it.
///
/// Every other function here is internal to this concern. Because the split
/// lowering files are `include!`-d into a single `impl ModuleBuilder`, Rust
/// cannot enforce file-private visibility; these helpers are documented as
/// internal to `collections.rs` and are not referenced from other parts.
impl ModuleBuilder<'_> {
    /// Dispatch typed `Map` and `Set` receiver methods through stdlib method metadata.
    ///
    /// The frontend still owns typed HIR construction, but recognition of which
    /// receiver/member pairs are standard-library collection methods lives in
    /// `smelt-stdlib`. This is the primary collection-method entry registered in
    /// `call.rs`'s builtin handler chain.
    pub(super) fn dispatch_collection_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        if !Self::is_collection_method_name(member_name) {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = self.type_param_constraint_or_self(Self::expr_ty(body, receiver));
        let receiver_kind = match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::Dict(_, _)) => smelt_stdlib::TypeScriptReceiverKind::Map,
            Some(Type::Set(_)) => smelt_stdlib::TypeScriptReceiverKind::Set,
            _ => return Ok(None),
        };
        let Some(rule) = smelt_stdlib::typescript_method_rule(receiver_kind, member_name) else {
            return Ok(None);
        };
        match rule {
            RuleId::TsMapHas => self.map_has_call(call, body),
            RuleId::TsMapGet => self.map_get_call(call, body),
            RuleId::TsMapMutation => self.map_mutation_call(call, body),
            RuleId::TsMapProjection => self.map_projection_call(call, body),
            RuleId::TsSetHas => self.set_contains_call(call, body),
            RuleId::TsSetMutation => self.set_mutation_call(call, body),
            RuleId::TsSetProjection => self.set_projection_call(call, body),
            _ => Ok(None),
        }
    }

    /// Return whether a member name belongs to a registry-backed collection method family.
    pub(super) fn is_collection_method_name(member: &str) -> bool {
        matches!(
            member,
            "add" | "clear" | "delete" | "entries" | "get" | "has" | "keys" | "set" | "values"
        )
    }

    /// Lower direct TypeScript `Set.prototype.has`.
    pub(super) fn set_contains_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "has" {
            return Ok(None);
        }
        let [item_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Set.has requires exactly one argument",
            ));
        };
        let set = self.expression(&member.object, body)?;
        let set_ty = Self::expr_ty(body, set);
        let Some(Type::Set(set_element_ty)) = self.ctx.krate.types.get(set_ty) else {
            return Ok(None);
        };
        let element_ty = *set_element_ty;
        let item = self.argument(item_argument, body)?;
        if !self.array_item_type_compatible(Self::expr_ty(body, item), element_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Set.has argument must match the set element type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::SetContains { set, item },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Set` mutation methods.
    pub(super) fn set_mutation_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "add" | "delete" | "clear") {
            return Ok(None);
        }
        let set = self.expression(&member.object, body)?;
        let set_ty = Self::expr_ty(body, set);
        let Some(Type::Set(set_element_ty)) = self.ctx.krate.types.get(set_ty) else {
            return Ok(None);
        };
        let element_ty = *set_element_ty;
        match method {
            "add" => {
                let [item_argument] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Set.add requires exactly one item argument",
                    ));
                };
                let item = self.argument(item_argument, body)?;
                if !self.array_item_type_compatible(Self::expr_ty(body, item), element_ty) {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Set.add item must match the set element type",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::SetAdd { set, item },
                    ty: set_ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "delete" => {
                let [item_argument] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Set.delete requires exactly one item argument",
                    ));
                };
                let item = self.argument(item_argument, body)?;
                if !self.array_item_type_compatible(Self::expr_ty(body, item), element_ty) {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Set.delete item must match the set element type",
                    ));
                }
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::SetRemove {
                        op: SetRemoveOp::Delete,
                        set,
                        item,
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "clear" => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Set.clear requires no arguments",
                    ));
                }
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::SetClear { set },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            _ => Ok(None),
        }
    }

    /// Lower direct TypeScript `Map.prototype.has`.
    pub(super) fn map_has_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "has" {
            return Ok(None);
        }
        let [key_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map.has requires exactly one key argument",
            ));
        };
        let dict = self.expression(&member.object, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, _)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let key = self.argument(key_argument, body)?;
        if !self.map_key_type_compatible(key_ty, Self::expr_ty(body, key)) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map.has key must match the map key type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictContainsKey { dict, key },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Map.prototype.get`.
    pub(super) fn map_get_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "get" {
            return Ok(None);
        }
        let dict = self.expression(&member.object, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let [key_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map.get requires exactly one key argument",
            ));
        };
        let key_ty = *dict_key_ty;
        let value_ty = *dict_value_ty;
        let key = self.argument(key_argument, body)?;
        if !self.map_key_type_compatible(key_ty, Self::expr_ty(body, key)) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map.get key must match the map key type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Optional(value_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictGet {
                dict,
                key,
                default: None,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Map` mutation methods.
    pub(super) fn map_mutation_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "set" | "delete" | "clear") {
            return Ok(None);
        }
        let dict = self.expression(&member.object, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let value_ty = *dict_value_ty;
        match method {
            "set" => {
                let [key_argument, value_argument] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Map.set requires key and value arguments",
                    ));
                };
                let mut key = self.argument(key_argument, body)?;
                let mut value = self.argument(value_argument, body)?;
                if !self.map_key_type_compatible(key_ty, Self::expr_ty(body, key)) {
                    key = body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: key },
                        ty: key_ty,
                        span: self.span(key_argument.span().start, key_argument.span().end),
                    });
                }
                if !self.map_value_type_compatible(value_ty, Self::expr_ty(body, value)) {
                    value = body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value },
                        ty: value_ty,
                        span: self.span(value_argument.span().start, value_argument.span().end),
                    });
                }
                if !self.map_key_type_compatible(key_ty, Self::expr_ty(body, key))
                    || !self.map_value_type_compatible(value_ty, Self::expr_ty(body, value))
                {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Map.set key and value must match the map type",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::DictSet { dict, key, value },
                    ty: dict_ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "delete" => {
                let [key_argument] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Map.delete requires exactly one key argument",
                    ));
                };
                let mut key = self.argument(key_argument, body)?;
                if !self.map_key_type_compatible(key_ty, Self::expr_ty(body, key)) {
                    key = body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: key },
                        ty: key_ty,
                        span: self.span(key_argument.span().start, key_argument.span().end),
                    });
                }
                if !self.map_key_type_compatible(key_ty, Self::expr_ty(body, key)) {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Map.delete key must match the map key type",
                    ));
                }
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::DictRemoveKey { dict, key },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "clear" => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Map.clear requires no arguments",
                    ));
                }
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::DictClear { dict },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            _ => Ok(None),
        }
    }

    /// Return whether a value argument can be stored in a lowered map value slot.
    ///
    /// Internal to `collections.rs`: only `map_mutation_call` consumes this.
    pub(super) fn map_value_type_compatible(
        &self,
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
    ) -> bool {
        let expected = self.type_param_constraint_or_self(expected);
        let actual = self.type_param_constraint_or_self(actual);
        self.numeric_type_compatible(expected, actual)
            || matches!(self.ctx.krate.types.get(expected), Some(Type::Unknown))
            || matches!(self.ctx.krate.types.get(actual), Some(Type::Unknown))
    }

    /// Lower supported `Map` projection calls into HIR collection operations.
    ///
    /// Registered directly in `call.rs` in addition to being dispatched by
    /// [`Self::dispatch_collection_method`], because the single-argument utility
    /// form (`keys(value)` etc.) reaches this function with a non-`Map` receiver.
    pub(super) fn map_projection_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "keys" => DictProjectionOp::Keys,
            "values" => DictProjectionOp::Values,
            "entries" => DictProjectionOp::Entries,
            _ => return Ok(None),
        };
        if let [dict_argument] = call.arguments.as_slice() {
            return self.static_dict_projection_utility_call(call, body, op, dict_argument);
        }
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map keys/values/entries require no arguments",
            ));
        }
        let dict = self.expression(&member.object, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let value_ty = *dict_value_ty;
        let symbol_key_ty = self.ctx.krate.types.intern(Type::String);
        let symbol_list_ty = self.ctx.krate.types.intern(Type::List(symbol_key_ty));
        let ty = match op {
            DictProjectionOp::FromEntries => return Ok(None),
            DictProjectionOp::Keys | DictProjectionOp::ForInKeys => {
                self.ctx.krate.types.intern(Type::List(key_ty))
            }
            DictProjectionOp::Symbols => symbol_list_ty,
            DictProjectionOp::Values => self.ctx.krate.types.intern(Type::List(value_ty)),
            DictProjectionOp::Entries => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![key_ty, value_ty]));
                self.ctx.krate.types.intern(Type::List(entry_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictProjection { op, dict },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower utility-style `keys(value)`, `values(value)`, and `entries(value)` calls.
    ///
    /// Libraries such as Lodash expose these as namespace functions instead of
    /// receiver methods. The callee namespace is intentionally ignored here:
    /// once TypeScript has accepted a static member call with a single value
    /// argument, the frontend can lower the projection through the same record
    /// operation used by `Object.keys`.
    ///
    /// Internal to `collections.rs`: only `map_projection_call` calls this.
    pub(super) fn static_dict_projection_utility_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        op: DictProjectionOp,
        dict_argument: &Argument<'_>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let mut dict = self.argument(dict_argument, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let (key_ty, value_ty) = match self.ctx.krate.types.get(dict_ty) {
            Some(Type::Dict(key_ty, value_ty)) => (*key_ty, *value_ty),
            Some(
                Type::Unknown
                | Type::TypeParam { .. }
                | Type::Class { .. }
                | Type::String
                | Type::Bool,
            ) => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let target = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                dict = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: dict,
                        target,
                    },
                    ty: target,
                    span: self.span(dict_argument.span().start, dict_argument.span().end),
                });
                (key_ty, value_ty)
            }
            Some(Type::Union(items))
                if items
                    .iter()
                    .all(|item| self.object_keys_compatible_type(*item)) =>
            {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let target = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                dict = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: dict,
                        target,
                    },
                    ty: target,
                    span: self.span(dict_argument.span().start, dict_argument.span().end),
                });
                (key_ty, value_ty)
            }
            _ => return Ok(None),
        };
        let symbol_key_ty = self.ctx.krate.types.intern(Type::String);
        let symbol_list_ty = self.ctx.krate.types.intern(Type::List(symbol_key_ty));
        let ty = match op {
            DictProjectionOp::FromEntries => return Ok(None),
            DictProjectionOp::Keys | DictProjectionOp::ForInKeys => {
                self.ctx.krate.types.intern(Type::List(key_ty))
            }
            DictProjectionOp::Symbols => symbol_list_ty,
            DictProjectionOp::Values => self.ctx.krate.types.intern(Type::List(value_ty)),
            DictProjectionOp::Entries => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![key_ty, value_ty]));
                self.ctx.krate.types.intern(Type::List(entry_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictProjection { op, dict },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Set` projection methods.
    pub(super) fn set_projection_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "keys" | "values" => SetProjectionOp::Values,
            "entries" => SetProjectionOp::Entries,
            _ => return Ok(None),
        };
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Set keys/values/entries require no arguments",
            ));
        }
        let set = self.expression(&member.object, body)?;
        let set_ty = Self::expr_ty(body, set);
        let Some(Type::Set(set_item_ty)) = self.ctx.krate.types.get(set_ty) else {
            return Ok(None);
        };
        let item_ty = *set_item_ty;
        let ty = match op {
            SetProjectionOp::Values => self.ctx.krate.types.intern(Type::List(item_ty)),
            SetProjectionOp::Entries => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![item_ty, item_ty]));
                self.ctx.krate.types.intern(Type::List(entry_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::SetProjection { op, set },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }
}
