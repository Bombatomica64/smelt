//! TypeScript `Map` and `Set` collection-method lowering.
//!
//! Lowers standard-library `Map` and `Set` receiver methods into typed HIR
//! dict/set operations. The impl block below documents the concern in detail.

use crate::lowering::ModuleBuilder;
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
        let receiver = self.expression(&member.object, body)?;
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
        let receiver_kind = match self.ctx.krate.types.get(effective_ty) {
            Some(Type::Dict(_, _) | Type::JsMap(_, _)) => {
                smelt_stdlib::TypeScriptReceiverKind::Map
            }
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
            RuleId::TsMapProjection => self.map_projection_with_receiver(call, op_receiver, body)?,
            RuleId::TsSetHas => self.set_contains_call(call, op_receiver, body)?,
            RuleId::TsSetMutation => self.set_mutation_call(call, op_receiver, body)?,
            RuleId::TsSetProjection => self.set_projection_call(call, op_receiver, body)?,
            _ => return Ok(None),
        };
        let Some(op) = op else {
            return Ok(None);
        };
        if optional_inner.is_some() {
            Ok(Some(
                self.wrap_optional_receiver_method(receiver, op, call.span, body),
            ))
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
    fn wrap_optional_receiver_method(
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
    /// Registered directly in `call.rs` in addition to being dispatched by
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
                let symbol_key_ty = self.ctx.krate.types.intern(Type::Unknown);
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
                let symbol_key_ty = self.ctx.krate.types.intern(Type::Unknown);
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
}
