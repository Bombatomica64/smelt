//! TypeScript `Map` and `Set` collection-method lowering.
//!
//! Lowers standard-library `Map` and `Set` receiver methods into typed HIR
//! dict/set operations. The impl block below documents the concern in detail.

use crate::SmeltError;
use crate::lowering::erasure::NotNewErasure;
use crate::lowering::resolve::HostGlobal;
use crate::lowering::{ArrayExpressionElement, ModuleBuilder};
use oxc::ast::ast::{Argument, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, DictProjectionOp, ErasureReason, Expr, ExprKind, PropertyLookup, SetProjectionOp,
    SetRemoveOp, Type,
};
use smelt_stdlib::RuleId;

/// `Map` and `Set` collection-method lowering for the TypeScript frontend.
///
/// This concern owns the family that lowers standard-library `Map` and `Set`
/// receiver methods (`has`, `get`, `set`, `add`, `delete`, `clear`, `keys`,
/// `values`, `entries`) into typed HIR dict/set operations.
///
/// # Interface
///
/// Two entry points are registered in the call-lowering chain in `stdlib/call_dispatch.rs`:
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
    /// `stdlib/call_dispatch.rs`'s builtin handler chain.
    pub(in crate::lowering) fn dispatch_collection_method(
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
        let receiver = match self.dispatched_receiver(&member.object, body)? {
            Some(receiver) => receiver,
            None => self.expression(&member.object, body)?,
        };
        let receiver_ty = self.type_param_constraint_or_self(Self::expr_ty(body, receiver));
        // A `recv?.method(args)` call whose receiver is `T | undefined` for a
        // modeled receiver `T` (Map/Set) desugars to the same modeled operation
        // guarded by a presence test: the op runs on the narrowed receiver when
        // present and yields `undefined` otherwise. Detect the optional shape
        // here so the same per-method lowering serves both the plain and the
        // optional-chained receiver instead of the optional case falling through
        // to a generic (mis-typed) field access.
        let optional_inner =
            if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(receiver_ty) {
                Some(self.type_param_constraint_or_self(*inner))
            } else {
                None
            };
        let effective_ty = optional_inner.unwrap_or(receiver_ty);
        // A `Dict` is also how a string-keyed OBJECT type with uniform members
        // lowers (`{ has: (a: number) => string }`, `Record<string, Fn>`). A
        // call of a member whose value is callable (or `any`) on such a
        // receiver is a call of that PROPERTY, as in JavaScript; the checked
        // source cannot mean `Map.prototype.has` there, since a `Map` lowers
        // to `JsMap` and a non-callable record member cannot be called.
        if let Some(Type::Dict(_, value_ty)) = self.ctx.krate.types.get(effective_ty).cloned()
            && (self.ctx.krate.types.get(value_ty) == Some(&Type::Unknown)
                || self.function_member_type(value_ty).is_some())
        {
            return Ok(None);
        }
        let receiver_kind = match self.ctx.krate.types.get(effective_ty) {
            Some(Type::Dict(_, _) | Type::JsMap(_, _)) => smelt_stdlib::TypeScriptReceiverKind::Map,
            Some(Type::Set(_)) => smelt_stdlib::TypeScriptReceiverKind::Set,
            _ => return Ok(None),
        };
        let Some(rule) = smelt_stdlib::typescript_method_rule(receiver_kind, member_name) else {
            return Ok(None);
        };
        // The operation is built on the narrowed receiver (typed `T`, unwrapped
        // by codegen) when the source receiver was optional, and directly on the
        // receiver otherwise.
        let op_receiver = if optional_inner.is_some() {
            self.narrowed_optional_receiver(receiver, effective_ty, member.span, body)
        } else {
            receiver
        };
        let op = match rule {
            RuleId::TsMapHas => self.map_has_call(call, op_receiver, body)?,
            RuleId::TsMapGet => self.map_get_call(call, op_receiver, body)?,
            RuleId::TsMapMutation => self.map_mutation_call(call, op_receiver, body)?,
            RuleId::TsMapProjection => {
                self.map_projection_with_receiver(call, op_receiver, body)?
            }
            RuleId::TsSetHas => self.set_contains_call(call, op_receiver, body)?,
            RuleId::TsSetMutation => self.set_mutation_call(call, op_receiver, body)?,
            RuleId::TsSetProjection => self.set_projection_call(call, op_receiver, body)?,
            _ => return Ok(None),
        };
        let Some(op) = op else {
            return Ok(None);
        };
        if optional_inner.is_some() {
            Ok(Some(self.wrap_optional_receiver_method(
                receiver, op, call.span, body,
            )))
        } else {
            Ok(Some(op))
        }
    }

    /// Narrow an optional modeled receiver to its inner type for a guarded op.
    ///
    /// Builds an [`ExprKind::TypeAssert`] typed as the receiver's inner type. The
    /// Rust emitter recognizes an `Optional(inner)` operand assigned into an
    /// `inner`-typed slot and unwraps it (`.clone().expect(...)`), matching the
    /// narrowing the ordinary `if (recv) { recv.method() }` guard produces. The
    /// assertion is only ever evaluated inside the present branch of the
    /// conditional built by [`Self::wrap_optional_receiver_method`], so the
    /// unwrap cannot observe an absent receiver.
    fn narrowed_optional_receiver(
        &self,
        receiver: smelt_hir::ExprId,
        inner_ty: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value: receiver },
            ty: inner_ty,
            span: self.span(span.start, span.end),
        })
    }

    /// Wrap a modeled receiver operation as an optional-chained method result.
    ///
    /// Given the original optional receiver and the modeled operation `op` built
    /// on its narrowed form, produces `recv present ? Some(op) : undefined`. The
    /// result type flattens through [`Self::optional_chain_result_type`] so an op
    /// that already returns `Optional` (e.g. `Map.get`) does not double-wrap. The
    /// receiver expression is shared by the presence test and the narrowed
    /// access; MIR memoizes each HIR expression, so the receiver is evaluated
    /// exactly once and its temporary dominates both uses.
    pub(in crate::lowering) fn wrap_optional_receiver_method(
        &mut self,
        receiver: smelt_hir::ExprId,
        op: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let op_ty = Self::expr_ty(body, op);
        let result_ty = self.optional_chain_result_type(op_ty);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let is_absent = body.push_expr(Expr {
            kind: ExprKind::UnknownIs {
                value: receiver,
                kind: smelt_hir::UnknownKind::Null,
            },
            ty: bool_ty,
            span: self.span(span.start, span.end),
        });
        let present = self.unary_bool_expr(smelt_hir::UnaryOp::Not, is_absent, span, body);
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let absent = body.push_expr(Expr {
            kind: ExprKind::Literal(smelt_hir::Literal::None),
            ty: none_ty,
            span: self.span(span.start, span.end),
        });
        body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond: present,
                then_expr: op,
                else_expr: absent,
            },
            ty: result_ty,
            span: self.span(span.start, span.end),
        })
    }

    /// Return whether a member name belongs to a registry-backed collection method family.
    pub(in crate::lowering) fn is_collection_method_name(member: &str) -> bool {
        matches!(
            member,
            "add" | "clear" | "delete" | "entries" | "get" | "has" | "keys" | "set" | "values"
        )
    }

    /// Lower direct TypeScript `Set.prototype.has`.
    ///
    /// `receiver` is the pre-lowered set receiver supplied by
    /// [`Self::dispatch_collection_method`]; for an optional-chained receiver it
    /// is the narrowed (`T`-typed) form so the same op serves both spellings.
    pub(in crate::lowering) fn set_contains_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        receiver: smelt_hir::ExprId,
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
        let set = receiver;
        let set_ty = Self::expr_ty(body, set);
        let Some(Type::Set(set_element_ty)) = self.ctx.krate.types.get(set_ty) else {
            return Ok(None);
        };
        let element_ty = *set_element_ty;
        let item = self.collection_item_argument(item_argument, element_ty, body)?;
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

    /// Lower a `Set` item or `Map` key argument for a collection of `element_ty`.
    ///
    /// JavaScript has one number type, so an integer-carried value (a callback's
    /// array index) is the same key as the equal float: the collection stores
    /// `f64`, and the argument widens to it.
    fn collection_item_argument(
        &mut self,
        argument: &Argument<'_>,
        element_ty: smelt_hir::TypeId,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let item = self.argument(argument, body)?;
        let types = &self.ctx.krate.types;
        if types.get(element_ty) != Some(&Type::Float)
            || types.get(Self::expr_ty(body, item)) != Some(&Type::Int)
        {
            return Ok(item);
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: smelt_hir::PrimitiveCastOp::ToFloat,
                operand: item,
            },
            ty: element_ty,
            span: self.span(argument.span().start, argument.span().end),
        }))
    }

    /// Lower direct TypeScript `Set` mutation methods.
    ///
    /// `receiver` is the pre-lowered set receiver (narrowed for optional chains).
    pub(in crate::lowering) fn set_mutation_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        receiver: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "add" | "delete" | "clear") {
            return Ok(None);
        }
        let set = receiver;
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
                let item = self.collection_item_argument(item_argument, element_ty, body)?;
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
                let item = self.collection_item_argument(item_argument, element_ty, body)?;
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
    ///
    /// `receiver` is the pre-lowered map receiver (narrowed for optional chains).
    pub(in crate::lowering) fn map_has_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        receiver: smelt_hir::ExprId,
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
        let dict = receiver;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, _) | Type::JsMap(dict_key_ty, _)) =
            self.ctx.krate.types.get(dict_ty)
        else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let key = self.collection_item_argument(key_argument, key_ty, body)?;
        if !self.map_key_type_compatible(key_ty, Self::expr_ty(body, key)) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map.has key must match the map key type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictContainsKey {
                dict,
                key,
                lookup: PropertyLookup::Own,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Map.prototype.get`.
    ///
    /// `receiver` is the pre-lowered map receiver (narrowed for optional chains).
    pub(in crate::lowering) fn map_get_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        receiver: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "get" {
            return Ok(None);
        }
        let dict = receiver;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty) | Type::JsMap(dict_key_ty, dict_value_ty)) =
            self.ctx.krate.types.get(dict_ty)
        else {
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
        let key = self.collection_item_argument(key_argument, key_ty, body)?;
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
    ///
    /// `receiver` is the pre-lowered map receiver (narrowed for optional chains).
    pub(in crate::lowering) fn map_mutation_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        receiver: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "set" | "delete" | "clear") {
            return Ok(None);
        }
        let dict = receiver;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty) | Type::JsMap(dict_key_ty, dict_value_ty)) =
            self.ctx.krate.types.get(dict_ty)
        else {
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
    pub(in crate::lowering) fn map_value_type_compatible(
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
    /// Registered directly in `stdlib/call_dispatch.rs` in addition to being dispatched by
    /// [`Self::dispatch_collection_method`], because the single-argument utility
    /// form (`keys(value)` etc.) reaches this function with a non-`Map` receiver.
    pub(in crate::lowering) fn map_projection_call(
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
        let dict = self.expression(&member.object, body)?;
        self.map_projection_with_receiver(call, dict, body)
    }

    /// Lower a `Map` projection method on a pre-lowered receiver.
    ///
    /// Shared by [`Self::map_projection_call`] (receiver form) and
    /// [`Self::dispatch_collection_method`], which passes the narrowed receiver
    /// for optional-chained `recv?.keys()/values()/entries()` calls.
    pub(in crate::lowering) fn map_projection_with_receiver(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        receiver: smelt_hir::ExprId,
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
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map keys/values/entries require no arguments",
            ));
        }
        let dict = receiver;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty) | Type::JsMap(dict_key_ty, dict_value_ty)) =
            self.ctx.krate.types.get(dict_ty)
        else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let value_ty = *dict_value_ty;
        let ty = match op {
            // Neither `fromEntries` nor the own-key projection is reachable
            // through a collection METHOD call: `Reflect.ownKeys` is a static.
            DictProjectionOp::FromEntries | DictProjectionOp::OwnKeys => return Ok(None),
            DictProjectionOp::Keys | DictProjectionOp::ForInKeys => {
                self.ctx.krate.types.intern(Type::List(key_ty))
            }
            // A symbol-keyed property list holds symbol VALUES, not their
            // descriptions: the property key an erased record stores is
            // `__smelt_symbol:<description>`, so handing back the bare
            // description made `source[sym]` miss and `target[sym] = v` write a
            // plain string key. `Type::Unknown` is the representation a symbol
            // already has everywhere else (`Literal::Symbol` ->
            // `SmeltUnknown::Symbol`), and the dynamic index paths map that tag
            // back to the prefixed key. Interned only on this arm — interning
            // `Unknown`/`List<Unknown>` for every projection would change which
            // record backing the other arms pick.
            DictProjectionOp::Symbols => {
                let symbol_key_ty = self.erase_with(ErasureReason::UnmodeledType, "symbol");
                self.ctx.krate.types.intern(Type::List(symbol_key_ty))
            }
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
    pub(in crate::lowering) fn static_dict_projection_utility_call(
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
                let value_ty = self.erase_unless_unknown(dict_ty, ErasureReason::UntypedBuiltin);
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
                let value_ty = self.erase(ErasureReason::UntypedBuiltin);
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
        let ty = match op {
            // Neither `fromEntries` nor the own-key projection is reachable
            // through a collection METHOD call: `Reflect.ownKeys` is a static.
            DictProjectionOp::FromEntries | DictProjectionOp::OwnKeys => return Ok(None),
            DictProjectionOp::Keys | DictProjectionOp::ForInKeys => {
                self.ctx.krate.types.intern(Type::List(key_ty))
            }
            // A symbol-keyed property list holds symbol VALUES, not their
            // descriptions: the property key an erased record stores is
            // `__smelt_symbol:<description>`, so handing back the bare
            // description made `source[sym]` miss and `target[sym] = v` write a
            // plain string key. `Type::Unknown` is the representation a symbol
            // already has everywhere else (`Literal::Symbol` ->
            // `SmeltUnknown::Symbol`), and the dynamic index paths map that tag
            // back to the prefixed key. Interned only on this arm — interning
            // `Unknown`/`List<Unknown>` for every projection would change which
            // record backing the other arms pick.
            DictProjectionOp::Symbols => {
                let symbol_key_ty = self.erase_with(ErasureReason::UnmodeledType, "symbol");
                self.ctx.krate.types.intern(Type::List(symbol_key_ty))
            }
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
    ///
    /// `receiver` is the pre-lowered set receiver (narrowed for optional chains).
    pub(in crate::lowering) fn set_projection_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        receiver: smelt_hir::ExprId,
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
        let set = receiver;
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

    /// The arguments of a `Set`/`Map` construction, with a literal `undefined`
    /// or `null` iterable dropped: the constructors treat it as no iterable,
    /// so `new Set(undefined)` is `new Set()`. Neither literal has an effect
    /// to keep.
    fn constructor_iterable_arguments<'a, 'b>(
        new_expr: &'b oxc::ast::ast::NewExpression<'a>,
    ) -> &'b [Argument<'a>] {
        match new_expr.arguments.as_slice() {
            [Argument::NullLiteral(_)] => &[],
            [Argument::Identifier(identifier)] if identifier.name == "undefined" => &[],
            arguments => arguments,
        }
    }

    /// `iterable ?? <empty>` for an optional iterable argument of a collection
    /// constructor, or `None` when `iterable` is not optional.
    ///
    /// The `Set` and `Map` constructors treat an `undefined` or `null`
    /// iterable as no iterable at all (ECMA-262 `Set ( [ iterable ] )`, step
    /// 5), so the absent arm is the empty value of the present type: an empty
    /// array, map, set or string. The result has the present type, so the
    /// element type is never widened or erased. Asserting the optional to its
    /// present type instead panicked on the absent value.
    fn absent_iterable_as_empty(
        &self,
        iterable: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Some(Type::Optional(present_ty)) = self
            .ctx
            .krate
            .types
            .get(Self::expr_ty(body, iterable))
            .cloned()
        else {
            return None;
        };
        let viewed_ty = self.type_param_constraint_or_self(present_ty);
        let empty = match self.ctx.krate.types.get(viewed_ty)? {
            Type::List(_) => ExprKind::ListLit(Vec::new()),
            Type::Dict(..) | Type::JsMap(..) => ExprKind::DictLit(Vec::new()),
            Type::Set(_) => ExprKind::SetLit(Vec::new()),
            Type::String => ExprKind::Literal(smelt_hir::Literal::String(String::new())),
            _ => return None,
        };
        let hir_span = self.span(span.start, span.end);
        let fallback = body.push_expr(Expr {
            kind: empty,
            ty: viewed_ty,
            span: hir_span,
        });
        let fallback = if viewed_ty == present_ty {
            fallback
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty: present_ty,
                span: hir_span,
            })
        };
        Some(body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce {
                optional: iterable,
                fallback,
            },
            ty: present_ty,
            span: hir_span,
        }))
    }

    /// Lower `new Set(...)` from an array literal or annotated empty constructor.
    pub(in crate::lowering) fn set_constructor_expression(
        &mut self,
        host: HostGlobal<'_>,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !Self::is_ts_stdlib_class_name(host.name(), smelt_stdlib::StdlibClass::Set) {
            return Ok(None);
        }
        let (items, ty) = match Self::constructor_iterable_arguments(new_expr) {
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
                    let item_ty = self.erase_with(ErasureReason::MissingAnnotation, "new Set()");
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
                // An explicit `new Set<T>([..])` states the element type; the
                // first item's type is only the fallback inference. An `any`
                // argument opts out of checking rather than declaring a
                // boundary, so the items still refine it (an `unknown` one must
                // hold any value and is kept).
                let ty = if let Some(hint) = type_hint
                    && matches!(self.ctx.krate.types.get(hint), Some(Type::Set(_)))
                {
                    hint
                } else if let Some(type_args) = &new_expr.type_arguments
                    && let [item] = type_args.params.as_slice()
                    && !matches!(item, oxc::ast::ast::TSType::TSAnyKeyword(_))
                {
                    let item_ty = self.ts_type_to_hir(item)?;
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                } else if let Some(first_item) = items.first().copied() {
                    let item_ty = Self::expr_ty(body, first_item);
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                } else {
                    let item_ty = self.erase_with(ErasureReason::MissingAnnotation, "new Set([])");
                    self.ctx.krate.types.intern(Type::Set(item_ty))
                };
                (items, ty)
            }
            [argument] => {
                let argument_value = self.argument(argument, body)?;
                let argument_raw_ty = Self::expr_ty(body, argument_value);
                let argument_ty = self.type_param_constraint_or_self(argument_raw_ty);
                // `new Set(iterable)` accepts arrays directly. Optional iterables
                // default to their empty value, an existing Set is copied, and
                // erased/union surfaces (e.g. a generic helper return
                // typed `unknown`) are asserted to `List<Unknown>` so the
                // list-to-set conversion can proceed instead of being rejected.
                // An absent iterable (`undefined` or `null`) builds an empty
                // set, as `new Set(undefined)` does in JavaScript: the optional
                // argument coalesces to the empty value of its present type,
                // which then converts like any other argument.
                let (mut list, raw_ty, list_ty) = self
                    .absent_iterable_as_empty(argument_value, argument.span(), body)
                    .map_or((argument_value, argument_raw_ty, argument_ty), |present| {
                        let present_ty = Self::expr_ty(body, present);
                        (
                            present,
                            present_ty,
                            self.type_param_constraint_or_self(present_ty),
                        )
                    });
                let item_ty = match self.ctx.krate.types.get(list_ty).cloned() {
                    Some(Type::List(item_ty)) => item_ty,
                    // `new Set(otherSet)` builds a NEW set holding the other's
                    // members in its insertion order: a distinct identity that
                    // later `add`/`delete` on either set leave unchanged.
                    Some(Type::Set(_)) => {
                        // A type parameter bounded by a Set is viewed at its
                        // bound first: the copy reads a set.
                        let set = if raw_ty == list_ty {
                            list
                        } else {
                            body.push_expr(Expr {
                                kind: ExprKind::TypeAssert { value: list },
                                ty: list_ty,
                                span: self.span(argument.span().start, argument.span().end),
                            })
                        };
                        return Ok(Some(body.push_expr(Expr {
                            kind: ExprKind::SetCopy { set },
                            ty: list_ty,
                            span: self.span(new_expr.span.start, new_expr.span.end),
                        })));
                    }
                    // A string is an iterable of its characters, so
                    // `new Set('abc')` is the three-element set JavaScript
                    // builds — Hono's `new Set('.\\+*[^]$()')` (the regexp
                    // meta-character set in `reg-exp-router/node.ts`) is exactly
                    // this. The conversion is not written here: it goes through
                    // `list_expr_from_spread_value`, the same helper that lowers
                    // `[...iterable]`, so `new Set(x)` and `new Set([...x])`
                    // cannot disagree about what iterating `x` means.
                    Some(Type::String) => {
                        let string_ty = self.ctx.krate.types.intern(Type::String);
                        let chars_ty = self.ctx.krate.types.intern(Type::List(string_ty));
                        list = self.list_expr_from_spread_value(
                            list,
                            chars_ty,
                            argument.span(),
                            body,
                        )?;
                        string_ty
                    }
                    _ if self.erased_or_union_surface(list_ty) => {
                        let item_ty =
                            if matches!(self.ctx.krate.types.get(list_ty), Some(Type::Unknown)) {
                                // not a new erasure: the iterable is already erased
                                self.unknown_type(NotNewErasure::ErasedUpstream)
                            } else {
                                self.erase_with(ErasureReason::DynamicOperator, "new Set(union)")
                            };
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

    /// The arm of a union contextual type a `new Map(..)` builds: its only
    /// map or record arm (`{ input: new Map(..) }` where `input` is
    /// `string | Map<string, T> | ..`). Any other hint is returned unchanged.
    fn map_arm_of_hint(&self, hint: smelt_hir::TypeId) -> smelt_hir::TypeId {
        let Some(Type::Union(arms)) = self.ctx.krate.types.get(hint) else {
            return hint;
        };
        let mut maps = arms.iter().filter(|arm| {
            matches!(
                self.ctx.krate.types.get(**arm),
                Some(Type::Dict(..) | Type::JsMap(..))
            )
        });
        match (maps.next(), maps.next()) {
            (Some(arm), None) => *arm,
            _ => hint,
        }
    }

    /// Lower `new Map(...)` to a dictionary literal.
    pub(in crate::lowering) fn map_constructor_expression(
        &mut self,
        host: HostGlobal<'_>,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !Self::is_ts_stdlib_class_name(host.name(), smelt_stdlib::StdlibClass::Map) {
            return Ok(None);
        }
        let (entries, ty) = match Self::constructor_iterable_arguments(new_expr) {
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
                    let unknown = self.erase_with(ErasureReason::MissingAnnotation, "new Map()");
                    self.ctx.krate.types.intern(Type::JsMap(unknown, unknown))
                };
                (Vec::new(), ty)
            }
            [Argument::ArrayExpression(array)] => {
                let entries = self.map_constructor_entries(array, body)?;
                // Explicit `new Map<K, V>([..])` type arguments state the entry
                // types the way an annotation does; inference from the entries
                // is only the fallback.
                // `any` opts out rather than declares (see the `Set` arm).
                let explicit = match new_expr
                    .type_arguments
                    .as_ref()
                    .map(|args| args.params.as_slice())
                {
                    Some([key, value])
                        if ![key, value]
                            .iter()
                            .any(|arg| matches!(arg, oxc::ast::ast::TSType::TSAnyKeyword(_))) =>
                    {
                        let key_ty = self.ts_type_to_hir(key)?;
                        let value_ty = self.ts_type_to_hir(value)?;
                        Some(self.ctx.krate.types.intern(Type::JsMap(key_ty, value_ty)))
                    }
                    _ => None,
                };
                let type_hint = type_hint
                    .map(|hint| self.map_arm_of_hint(hint))
                    .or(explicit);
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
                    let unknown = self.erase_with(ErasureReason::MissingAnnotation, "new Map([])");
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
            [argument] if !matches!(argument, Argument::SpreadElement(_)) => {
                let value = self.argument(argument, body)?;
                return self.map_from_iterable(value, new_expr.span, body).map(Some);
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(new_expr.span.start, new_expr.span.end),
                    "new Map takes at most one iterable argument",
                ));
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// `new Map(iterable)`: a NEW map holding the iterable's `[key, value]`
    /// entries in iteration order (a later entry for the same key wins).
    ///
    /// The entries become a list of `[K, V]` tuples collected by
    /// [`ExprKind::ListPairsToDict`] into a `Map<K, V>`:
    ///
    /// * a `Map`/record argument contributes its `entries()` projection, so the
    ///   result is a copy with its own identity, never the argument itself;
    /// * a list of `[K, V]` tuples is collected as is;
    /// * an erased iterable (`unknown`, a type parameter, a union) is read as a
    ///   list of erased `[key, value]` pairs: its entries carry no static key
    ///   or value type, so the map's are the erased carrier.
    ///
    /// `new Map(otherMap)` and `new Map(pairs)` copy their argument's entries.
    fn map_from_iterable(
        &mut self,
        value: smelt_hir::ExprId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let hir_span = self.span(span.start, span.end);
        // `new Map(undefined)` is an empty map, like `new Set(undefined)`.
        let value = self
            .absent_iterable_as_empty(value, span, body)
            .unwrap_or(value);
        let value_ty = self.type_param_constraint_or_self(Self::expr_ty(body, value));
        let (list, key_ty, value_item_ty) = match self.ctx.krate.types.get(value_ty).cloned() {
            Some(Type::Dict(key_ty, item_ty) | Type::JsMap(key_ty, item_ty)) => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![key_ty, item_ty]));
                let entries_ty = self.ctx.krate.types.intern(Type::List(entry_ty));
                let entries = body.push_expr(Expr {
                    kind: ExprKind::DictProjection {
                        op: DictProjectionOp::Entries,
                        dict: value,
                    },
                    ty: entries_ty,
                    span: hir_span,
                });
                (entries, key_ty, item_ty)
            }
            Some(Type::List(entry_ty)) => match self.ctx.krate.types.get(entry_ty).cloned() {
                Some(Type::Tuple(items)) if items.len() == 2 => {
                    let (Some(key_ty), Some(item_ty)) = (items.first(), items.get(1)) else {
                        return Err(SmeltError::unsupported(
                            hir_span,
                            "new Map entries must be [key, value] pairs",
                        ));
                    };
                    (value, *key_ty, *item_ty)
                }
                _ => {
                    return Err(SmeltError::unsupported(
                        hir_span,
                        "new Map(iterable) needs an iterable of [key, value] tuples",
                    ));
                }
            },
            _ if self.erased_or_union_surface(value_ty) => {
                // The iterable's own type is erased, so its entries are too:
                // this is the dynamic boundary the argument already crossed,
                // not a new erasure.
                let unknown = self.unknown_type(NotNewErasure::ErasedUpstream);
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![unknown, unknown]));
                let entries_ty = self.ctx.krate.types.intern(Type::List(entry_ty));
                let entries = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value },
                    ty: entries_ty,
                    span: hir_span,
                });
                (entries, unknown, unknown)
            }
            _ => {
                return Err(SmeltError::unsupported(
                    hir_span,
                    "new Map(iterable) needs a Map, a record or an array of [key, value] tuples",
                ));
            }
        };
        let ty = self
            .ctx
            .krate
            .types
            .intern(Type::JsMap(key_ty, value_item_ty));
        // A `Map<K, V>` annotation whose components differ (a wider value
        // union) is reached the way any typed value reaches its declared type:
        // the binding coerces it.
        Ok(body.push_expr(Expr {
            kind: ExprKind::ListPairsToDict { list },
            ty,
            span: hir_span,
        }))
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
}
