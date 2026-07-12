//! Member-access and assignment-target lowering helpers.
//!
//! Covers namespace member calls, static/computed member reads (including the
//! well-known `Symbol`, `Math`, `Number`, and `Object` static surfaces),
//! optional chains, and the lowering of assignment/update targets.

use crate::lowering::ModuleBuilder;
use crate::SmeltError;
use oxc::span::GetSpan;
use oxc::syntax::operator::{AssignmentOperator, UpdateOperator};
use oxc::ast::ast::{
    AssignmentTarget, ChainElement, Expression, SimpleAssignmentTarget,
};
use smelt_hir::{
    BinOp, Body, CaptureMode, ClosureCapture, DictProjectionOp, Expr, ExprKind, FunctionType, Item,
    Literal, LocalDecl, NumericPredicateOp, NumericRoundOp, NumericUnaryFuncOp, Param, Pattern,
    PrimitiveCastOp, Span, Stmt, Type, UnknownKind,
};

impl ModuleBuilder<'_> {
    /// Lower supported namespace member calls into the matching HIR operation.
    pub(in crate::lowering) fn namespace_member_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Some((namespace, member_name)) = self.namespace_member_name(member) else {
            return Ok(None);
        };
        let span = self.span(member.span.start, member.span.end);
        let item = self
            .object_namespaces
            .get(namespace)
            .and_then(|members| members.get(member_name))
            .copied()
            .or_else(|| self.items.get(member_name).copied());
        let Some(item) = item else {
            if self.namespace_imports.contains(namespace) {
                for arg in &call.arguments {
                    let _ = self.argument(arg, body)?;
                }
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            return Err(SmeltError::unsupported(
                span,
                format!("namespace import has no exported member `{member_name}`"),
            ));
        };
        let (params, rest, return_ty, is_async) =
            if let Item::Function(function) = self.item_ref(item) {
                (
                    function.params.iter().map(|param| param.ty).collect(),
                    function.rest,
                    function.return_ty,
                    function.is_async,
                )
            } else {
                let item_ty = self.item_expr_type(item, span)?;
                if let Some(Type::Function(function)) = self.ctx.krate.types.get(item_ty).cloned() {
                    let callee = body.push_expr(Expr {
                        kind: ExprKind::Item(item),
                        ty: item_ty,
                        span,
                    });
                    let args = call
                        .arguments
                        .iter()
                        .map(|arg| self.argument(arg, body))
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::ClosureCall { callee, args },
                        ty: function.return_ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                }
                return Err(SmeltError::unsupported(
                    span,
                    format!("namespace member `{member_name}` is not callable"),
                ));
            };
        let args = call
            .arguments
            .iter()
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        let callee = body.push_expr(Expr {
            kind: ExprKind::Item(item),
            ty: self.ctx.krate.types.intern(Type::Function(FunctionType {
                params,
                rest,
                required_params: None,
                mutable_params: Vec::new(),
                return_ty,
                is_async,
                may_throw: false,
            })),
            span,
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Call { callee, args },
            ty: return_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return the namespace binding and exported member name for namespace member access.
    pub(in crate::lowering) fn namespace_member_name<'a>(
        &self,
        member: &'a oxc::ast::ast::StaticMemberExpression<'_>,
    ) -> Option<(&'a str, &'a str)> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        let namespace = object.name.as_str();
        (self.namespace_imports.contains(namespace)
            || self.object_namespaces.contains_key(namespace))
        .then_some((namespace, member.property.name.as_str()))
    }

    /// Compute the HIR expression type used when an item appears as an expression.
    pub(in crate::lowering) fn item_expr_type(
        &mut self,
        item: smelt_hir::ItemId,
        span: Span,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match self.item_ref(item) {
            Item::Function(function) => {
                Ok(self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params: function.params.iter().map(|param| param.ty).collect(),
                    rest: function.rest,
                    required_params: function.required_params,
                    mutable_params: Vec::new(),
                    return_ty: function.return_ty,
                    is_async: function.is_async,
                    may_throw: false,
                })))
            }
            Item::Class(class) => Ok(self.ctx.krate.types.intern(Type::Class {
                name: class.name,
                args: Vec::new(),
            })),
            Item::Const(const_item) => Ok(const_item.ty),
            _ => Err(SmeltError::unsupported(
                span,
                "namespace member item is not usable as an expression yet",
            )),
        }
    }

    /// Lower a TypeScript optional chain wrapper by delegating to its chain element.
    pub(in crate::lowering) fn chain_expression(
        &mut self,
        chain: &oxc::ast::ast::ChainExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match &chain.expression {
            ChainElement::CallExpression(call) => self.call_expression(call, body),
            ChainElement::StaticMemberExpression(member) => self.static_member(member, body),
            ChainElement::ComputedMemberExpression(member) => self.computed_member(member, body),
            ChainElement::TSNonNullExpression(non_null) => self.non_null_assertion_expression(
                &non_null.expression,
                self.span(non_null.span.start, non_null.span.end),
                body,
            ),
            ChainElement::PrivateFieldExpression(private_field) => Err(SmeltError::unsupported(
                self.span(private_field.span.start, private_field.span.end),
                "private field optional chains are not lowered yet",
            )),
        }
    }

    /// Normalize a `<global-alias>.<Member>` read to the bare `<Member>` value.
    ///
    /// Implements the read side of the plan §5 path normalization: a non-optional
    /// static member whose receiver is a recognized global alias (`globalThis` /
    /// `global` / `self`, or a tracked local alias) and whose member is a
    /// recognized JavaScript global is lowered exactly like the bare identifier,
    /// so `globalThis.Object` and `Object` produce the same concrete value. The
    /// rewrite is gated on the member being a recognized builtin: an unmodeled
    /// member such as `globalThis.Buffer` (whose bare form is itself unsupported)
    /// returns `None` and falls through to ordinary member lowering, which keeps
    /// the honest blocker rather than silently degrading to a dynamic read.
    pub(in crate::lowering) fn global_alias_member_read(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if member.optional || !self.expr_is_global_alias(&member.object) {
            return Ok(None);
        }
        let name = member.property.name.as_str();
        // A reassigned modeled host constructor reads through its override slot
        // (`globalThis.File` yields the native handle / stored ctor / undefined),
        // not the folded native identifier value.
        if self.is_written_host_global(name) {
            return Ok(Some(self.host_global_read_expr(
                name,
                member.span.start,
                member.span.end,
                body,
            )));
        }
        if !smelt_stdlib::is_javascript_global_builtin(name) {
            return Ok(None);
        }
        self.identifier_expression(name, member.span.start, member.span.end, body)
            .map(Some)
    }

    /// Lower a computed read off the global object (`globalThis[key]`).
    ///
    /// Two shapes are handled by one general rule:
    ///
    /// * A static string-literal key that names a modeled JavaScript global
    ///   (`globalThis['Object']`) is normalized to the bare identifier value,
    ///   exactly like the static-member spelling `globalThis.Object`, so the
    ///   concrete modeled global keeps its shape.
    /// * Any other key — a runtime variable (`globalThis[type]` in the
    ///   lodash-style typed-array/error spec loops) or a literal that names no
    ///   modeled global — is a genuine dynamic property lookup on the global
    ///   object. Smelt's deterministic profile has no runtime global-object
    ///   property store keyed by an arbitrary string, so the lookup resolves to
    ///   the JavaScript-correct `undefined`, tagged `SmeltUnknown` because the
    ///   value's static shape is genuinely erased at this dynamic boundary (see
    ///   the `SmeltUnknown` regression test `dynamic_global_computed_read_*`).
    ///   Downstream truthiness guards, `|| fallback`, and `new Ctor(...)`
    ///   dispatch through the existing erased-value machinery, so a present
    ///   guard folds to the absent branch and an unguarded construction becomes
    ///   a dynamic closure call — matching how the profile already treats
    ///   unmodeled host globals as absent.
    pub(in crate::lowering) fn global_alias_computed_read(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Expression::StringLiteral(key) = &member.expression {
            let name = key.value.as_str();
            if smelt_stdlib::is_javascript_global_builtin(name) {
                return self.identifier_expression(
                    name,
                    member.span.start,
                    member.span.end,
                    body,
                );
            }
        }
        Ok(self.dynamic_global_object_read(member.span.start, member.span.end, body))
    }

    /// Build the value of a dynamic global-object property lookup.
    ///
    /// The deterministic profile models no runtime global-object property store,
    /// so a read keyed by an arbitrary runtime string resolves to the
    /// JavaScript-correct `undefined`. It is tagged `SmeltUnknown` (not
    /// `Type::None`) because the result feeds erased-value code paths — dynamic
    /// `new Ctor(...)` construction, `|| fallback`, truthiness guards — that
    /// expect a dynamic boundary value, and the property's static shape is
    /// genuinely unknown here.
    pub(in crate::lowering) fn dynamic_global_object_read(
        &mut self,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let span = self.span(start, end);
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let undefined = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Undefined),
            ty: none_ty,
            span,
        });
        body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: undefined,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        })
    }

    /// Lower a static member access expression.
    pub(in crate::lowering) fn static_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.static_member_with_absent_fallback(member, body, true)
    }

    /// Lower a static member read without the absent-list-field `undefined`
    /// fallback.
    ///
    /// Call dispatch probes member expressions to decide whether a property is
    /// a callable field; there an unmodeled list member (e.g. `iterator.next`)
    /// must stay an error so the later modeled method paths can claim the
    /// call, instead of being folded into an `undefined` "callable".
    pub(in crate::lowering) fn static_member_no_absent_fallback(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.static_member_with_absent_fallback(member, body, false)
    }

    /// Shared static-member lowering; `absent_list_field_is_undefined` gates
    /// the JS absent-property-read-yields-`undefined` rule for list receivers.
    fn static_member_with_absent_fallback(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
        absent_list_field_is_undefined: bool,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(expr) = self.global_alias_member_read(member, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.enum_member_read(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.symbol_static_member(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.math_member_expression(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.number_static_constant(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.number_predicate_member_expression(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.array_is_array_member_expression(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.object_static_function_member(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.object_static_member(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.node_process_static_member(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.namespace_member_expression(member, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.materialized_static_member(member, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.url_field_expression(member, body)? {
            return Ok(expr);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let optional_access = member.optional
            || matches!(
                self.ctx.krate.types.get(receiver_ty),
                Some(Type::Optional(_))
            );
        let access_receiver_ty = self.optional_receiver_inner_type(receiver_ty);
        let field = self.intern_source_name(member.property.name.as_str());
        if member.property.name == "length" && self.supports_stdlib_length(access_receiver_ty)
            || member.property.name == "size" && self.supports_stdlib_size(access_receiver_ty)
        {
            let ty = self.ctx.krate.types.intern(Type::Float);
            let operand = if optional_access {
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: receiver },
                    ty: access_receiver_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                })
            } else {
                receiver
            };
            if optional_access {
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Len { operand },
                    ty,
                    span: self.span(member.span.start, member.span.end),
                }));
            }
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Len { operand },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        if member.property.name == "length"
            && matches!(
                self.ctx.krate.types.get(access_receiver_ty),
                Some(Type::Function(_))
            )
        {
            if optional_access {
                return Err(SmeltError::unsupported(
                    self.span(member.span.start, member.span.end),
                    "optional function length access is not lowered yet",
                ));
            }
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Len { operand: receiver },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(receiver_ty).cloned()
            && matches!(member.property.name.as_str(), "done" | "value")
        {
            let (kind, ty) = match member.property.name.as_str() {
                "done" => (
                    ExprKind::IteratorDone { result: receiver },
                    self.ctx.krate.types.intern(Type::Bool),
                ),
                "value" => (
                    ExprKind::IteratorValue { result: receiver },
                    self.ctx.krate.types.intern(Type::Optional(inner)),
                ),
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(member.span.start, member.span.end),
                        "iterator results only expose done and value",
                    ));
                }
            };
            return Ok(body.push_expr(Expr {
                kind,
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        let field_ty = match self.class_field_type(access_receiver_ty, field) {
            Ok(field_ty) => field_ty,
            // Reading an absent property yields `undefined` in JavaScript. A
            // Smelt list is a plain vector with no expando-property storage
            // (e.g. the RegExp match-array `index`/`input` fields), so an
            // unmodeled field read on a list receiver is truthfully
            // `undefined` rather than a lowering error.
            Err(error) => {
                if absent_list_field_is_undefined
                    && matches!(
                        self.ctx.krate.types.get(access_receiver_ty),
                        Some(Type::List(_))
                    )
                {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(member.span.start, member.span.end),
                    }));
                }
                return Err(error);
            }
        };
        if optional_access {
            let ty = self.optional_chain_result_type(field_ty);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::OptionalField { receiver, field },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        let ty = field_ty;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Field { receiver, field },
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower `Array.isArray` in value position to a callable runtime probe.
    ///
    /// The parameter is intentionally `Unknown`: JavaScript defines this
    /// predicate for every runtime value, so inspecting the tagged dynamic
    /// representation is the genuine boundary rather than erased type-level
    /// plumbing. Concrete callers are adapted into that boundary normally.
    pub(in crate::lowering) fn array_is_array_member_expression(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Array"
            || member.property.name != "isArray"
            || self.builtin_call_identifier_is_shadowed("Array")
        {
            return None;
        }
        let span = self.span(member.span.start, member.span.end);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let value_name = self.intern_source_name("value");
        let mut closure_body = Body::new(None, span);
        let value_local = closure_body.push_local(LocalDecl {
            name: Some(value_name),
            ty: unknown_ty,
            mutable: false,
            span,
        });
        closure_body.params.push(value_local);
        let value = closure_body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty: unknown_ty,
            span,
        });
        let result = closure_body.push_expr(Expr {
            kind: ExprKind::UnknownIs {
                value,
                kind: UnknownKind::Array,
            },
            ty: bool_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![unknown_ty],
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: bool_ty,
            is_async: false,
            may_throw: false,
        }));
        Some(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: vec![Param {
                    name: value_name,
                    local: value_local,
                    ty: unknown_ty,
                    span,
                }],
                rest: None,
                required_params: None,
                return_ty: bool_ty,
                captures: Vec::new(),
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Lower an `EnumName.Member` read to the member's const-folded literal.
    ///
    /// TypeScript enums have no distinct Smelt runtime representation; each
    /// member is a compile-time constant collected by `collect_module_enums`.
    /// A read of a known member therefore inlines the same numeric or string
    /// literal a manual `const` would, so ordinary enum usage compiles without a
    /// dedicated enum type. Returns `None` for a non-enum receiver or an
    /// unknown member so the caller continues with the general member paths.
    pub(in crate::lowering) fn enum_member_read(
        &self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if member.optional {
            return None;
        }
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        let value = self.enum_member_literal(object.name.as_str(), member.property.name.as_str())?;
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(value.literal),
            ty: value.ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower supported well-known `Symbol.<name>` member reads.
    ///
    /// Each modeled well-known symbol resolves to the same stable synthetic
    /// member spelling that computed property-key declaration uses (see
    /// [`crate::lowering::ty::computed_key_symbols::well_known_symbol_key`]), so a
    /// read such as `obj[Symbol.asyncIterator]` indexes the field declared by
    /// `[Symbol.asyncIterator]()` (issue #115). `Symbol.iterator` keeps its
    /// established `__smelt_symbol_iterator` spelling.
    pub(in crate::lowering) fn symbol_static_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Symbol" {
            return None;
        }
        let key = crate::lowering::ty::computed_key_symbols::well_known_symbol_key(
            member.property.name.as_str(),
        )?;
        let ty = self.ctx.krate.types.intern(Type::String);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(key)),
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower supported `Object.<fn>` member references to first-class callables.
    ///
    /// Remeda commonly passes static object helpers into `purry`, e.g.
    /// `purry(Object.fromEntries, args)`. Direct-call lowering handles
    /// `Object.fromEntries(value)`, while this path gives bare member
    /// references a callable shape and the correct `.length` arity.
    pub(in crate::lowering) fn object_static_function_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Object" {
            return None;
        }
        let (arity, return_ty) = match member.property.name.as_str() {
            "keys" | "values" | "entries" | "fromEntries" | "getPrototypeOf" | "create" => {
                (1, self.ctx.krate.types.intern(Type::Unknown))
            }
            "assign" | "setPrototypeOf" => (2, self.ctx.krate.types.intern(Type::Unknown)),
            "is" | "hasOwn" => (2, self.ctx.krate.types.intern(Type::Bool)),
            _ => return None,
        };
        Some(self.object_static_closure(member, arity, return_ty, outer_body))
    }

    /// Build an opaque closure for a supported static `Object` member reference.
    pub(in crate::lowering) fn object_static_closure(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        arity: usize,
        return_ty: smelt_hir::TypeId,
        outer_body: &mut Body,
    ) -> smelt_hir::ExprId {
        let span = self.span(member.span.start, member.span.end);
        let unknown = self.ctx.krate.types.intern(Type::Unknown);
        let mut closure_body = Body::new(None, span);
        let mut params = Vec::new();
        let mut param_tys = Vec::new();
        for index in 0..arity {
            let name = self.intern_source_name(&format!("arg{index}"));
            let local = closure_body.push_local(LocalDecl {
                name: Some(name),
                ty: unknown,
                mutable: false,
                span,
            });
            closure_body.params.push(local);
            params.push(Param {
                name,
                local,
                ty: unknown,
                span,
            });
            param_tys.push(unknown);
        }
        let projection = match member.property.name.as_str() {
            "fromEntries" if arity == 1 => Some(DictProjectionOp::FromEntries),
            "keys" if arity == 1 => Some(DictProjectionOp::Keys),
            "values" if arity == 1 => Some(DictProjectionOp::Values),
            "entries" if arity == 1 => Some(DictProjectionOp::Entries),
            _ => None,
        };
        let result = if let Some(op) = projection {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown));
            let argument_local = params.first().map_or_else(
                || {
                    closure_body.push_local(LocalDecl {
                        name: Some(self.intern_source_name("arg0")),
                        ty: unknown,
                        mutable: false,
                        span,
                    })
                },
                |param| param.local,
            );
            let argument = closure_body.push_expr(Expr {
                kind: ExprKind::Local(argument_local),
                ty: unknown,
                span,
            });
            let dict = if op == DictProjectionOp::FromEntries {
                argument
            } else {
                closure_body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: argument,
                        target: dict_ty,
                    },
                    ty: dict_ty,
                    span,
                })
            };
            let ty = match op {
                DictProjectionOp::FromEntries => dict_ty,
                DictProjectionOp::Keys
                | DictProjectionOp::ForInKeys
                | DictProjectionOp::Symbols => self.ctx.krate.types.intern(Type::List(string_ty)),
                DictProjectionOp::Values => self.ctx.krate.types.intern(Type::List(unknown)),
                DictProjectionOp::Entries => {
                    let entry_ty = self
                        .ctx
                        .krate
                        .types
                        .intern(Type::Tuple(vec![string_ty, unknown]));
                    self.ctx.krate.types.intern(Type::List(entry_ty))
                }
            };
            closure_body.push_expr(Expr {
                kind: ExprKind::DictProjection { op, dict },
                ty,
                span,
            })
        } else {
            closure_body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: return_ty,
                span,
            })
        };
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body = self.ctx.krate.push_body(closure_body);
        let ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: param_tys,
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        }));
        outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params,
                rest: None,
                required_params: None,
                return_ty,
                captures: Vec::new(),
                body,
                function_item: None,
                span,
            }),
            ty,
            span,
        })
    }

    /// Lower opaque static Object metadata reads such as `Object.prototype`.
    pub(in crate::lowering) fn object_static_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Object" || member.property.name != "prototype" {
            return None;
        }
        let ty = self.ctx.krate.types.intern(Type::Unknown);
        // `Object.prototype` is a sentinel string so prototype comparisons
        // (`proto === Object.prototype`) survive type erasure and distinguish
        // plain objects from arrays/null. Pairs with `object_get_prototype_of_call`.
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_proto:object".to_owned())),
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower supported TypeScript `Number.<constant>` reads.
    ///
    /// Test tables commonly use these global numeric constants as literal
    /// values, so they can be represented directly in HIR without resolving
    /// `Number` as a user-defined namespace.
    pub(in crate::lowering) fn number_static_constant(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Number" {
            return None;
        }
        let value = match member.property.name.as_str() {
            "NaN" => f64::NAN,
            "POSITIVE_INFINITY" => f64::INFINITY,
            "NEGATIVE_INFINITY" => f64::NEG_INFINITY,
            "MAX_VALUE" => f64::MAX,
            "MIN_VALUE" => f64::MIN_POSITIVE,
            "MAX_SAFE_INTEGER" => 9_007_199_254_740_991.0_f64,
            "MIN_SAFE_INTEGER" => -9_007_199_254_740_991.0_f64,
            "EPSILON" => f64::EPSILON,
            _ => return None,
        };
        let ty = self.ctx.krate.types.intern(Type::Float);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(value)),
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower a `Number.isNaN` / `Number.isFinite` / `Number.isInteger` member
    /// reference (used as a value, not called) to a first-class predicate closure.
    ///
    /// Utility libraries pass these predicates as callbacks (e.g. Remeda's
    /// `when(Number.isNaN, …)`). The direct-call form `Number.isNaN(x)` is
    /// handled by `number_predicate_call`; a bare member reference must become a
    /// callable `(value) => <NumericPredicate>(value)` value instead of resolving
    /// `Number` as an ordinary (unresolved) identifier and reading a field on it.
    pub(in crate::lowering) fn number_predicate_member_expression(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Number" {
            return None;
        }
        let op = match member.property.name.as_str() {
            "isFinite" => NumericPredicateOp::IsFinite,
            "isInteger" => NumericPredicateOp::IsInteger,
            "isNaN" => NumericPredicateOp::IsNaN,
            _ => return None,
        };
        let span = self.span(member.span.start, member.span.end);
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let value_name = self.intern_source_name("value");
        let mut closure_body = Body::new(None, span);
        let value_local = closure_body.push_local(LocalDecl {
            name: Some(value_name),
            ty: number_ty,
            mutable: false,
            span,
        });
        closure_body.params.push(value_local);
        let value_expr = closure_body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty: number_ty,
            span,
        });
        let result = closure_body.push_expr(Expr {
            kind: ExprKind::NumericPredicate {
                op,
                operand: value_expr,
            },
            ty: bool_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![number_ty],
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: bool_ty,
            is_async: false,
            may_throw: false,
        }));
        Some(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: vec![Param {
                    name: value_name,
                    local: value_local,
                    ty: number_ty,
                    span,
                }],
                rest: None,
                required_params: None,
                return_ty: bool_ty,
                captures: Vec::new(),
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Lower a supported `Math.<fn>` member reference to a first-class closure.
    ///
    /// Utility libraries such as Remeda pass `Math.ceil` or `Math.floor` as
    /// callbacks. The direct-call lowering handles `Math.ceil(value)`, but a
    /// bare member reference must become a callable value instead of resolving
    /// `Math` as a normal identifier.
    pub(in crate::lowering) fn math_member_expression(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Math" {
            return None;
        }
        let span = self.span(member.span.start, member.span.end);
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        // `Math.PI` and the other `Math.*` numeric constants are values, not
        // callables: fold them to their IEEE-754 double literal so a bare
        // `Math.PI` reference (e.g. `chunk(xs, Math.PI)` in the chunk spec)
        // resolves to a concrete number instead of an unresolved `Math`
        // identifier. These match the ECMAScript spec constant values.
        if let Some(constant) = match member.property.name.as_str() {
            "PI" => Some(std::f64::consts::PI),
            "E" => Some(std::f64::consts::E),
            "LN2" => Some(std::f64::consts::LN_2),
            "LN10" => Some(std::f64::consts::LN_10),
            "LOG2E" => Some(std::f64::consts::LOG2_E),
            "LOG10E" => Some(std::f64::consts::LOG10_E),
            "SQRT2" => Some(std::f64::consts::SQRT_2),
            "SQRT1_2" => Some(std::f64::consts::FRAC_1_SQRT_2),
            _ => None,
        } {
            return Some(outer_body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(constant)),
                ty: number_ty,
                span,
            }));
        }
        let value_name = self.intern_source_name("value");
        let mut closure_body = Body::new(None, span);
        let value_local = closure_body.push_local(LocalDecl {
            name: Some(value_name),
            ty: number_ty,
            mutable: false,
            span,
        });
        closure_body.params.push(value_local);
        let value_expr = closure_body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty: number_ty,
            span,
        });
        let result_kind = match member.property.name.as_str() {
            "abs" => ExprKind::NumericAbs {
                operand: value_expr,
            },
            "floor" => ExprKind::NumericRound {
                op: NumericRoundOp::Floor,
                operand: value_expr,
            },
            "ceil" => ExprKind::NumericRound {
                op: NumericRoundOp::Ceil,
                operand: value_expr,
            },
            "round" => ExprKind::NumericRound {
                op: NumericRoundOp::Round,
                operand: value_expr,
            },
            "trunc" => ExprKind::NumericRound {
                op: NumericRoundOp::Trunc,
                operand: value_expr,
            },
            "sqrt" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Sqrt,
                operand: value_expr,
            },
            "cbrt" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Cbrt,
                operand: value_expr,
            },
            "sign" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Sign,
                operand: value_expr,
            },
            "sin" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Sin,
                operand: value_expr,
            },
            "cos" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Cos,
                operand: value_expr,
            },
            "tan" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Tan,
                operand: value_expr,
            },
            "asin" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Asin,
                operand: value_expr,
            },
            "acos" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Acos,
                operand: value_expr,
            },
            "atan" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Atan,
                operand: value_expr,
            },
            "log" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Log,
                operand: value_expr,
            },
            "log10" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Log10,
                operand: value_expr,
            },
            "log2" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Log2,
                operand: value_expr,
            },
            "exp" => ExprKind::NumericUnaryFunc {
                op: NumericUnaryFuncOp::Exp,
                operand: value_expr,
            },
            _ => return None,
        };
        let result = closure_body.push_expr(Expr {
            kind: result_kind,
            ty: number_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![number_ty],
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: number_ty,
            is_async: false,
            may_throw: false,
        }));
        Some(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: vec![Param {
                    name: value_name,
                    local: value_local,
                    ty: number_ty,
                    span,
                }],
                rest: None,
                required_params: None,
                return_ty: number_ty,
                captures: Vec::new(),
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Lower the small Node `process` surface used by checked package probes.
    pub(in crate::lowering) fn node_process_static_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if Self::is_process_version_member(&member.object) {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("v20.0.0".to_owned())),
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        if Self::is_process_env_member(member) {
            let ty = self.ctx.krate.types.intern(Type::String);
            let value = if Self::is_process_env_field(member, "TZ")
                || Self::is_process_env_field(member, "tz")
            {
                "America/Santiago".to_owned()
            } else {
                String::new()
            };
            return Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(value)),
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        None
    }

    /// Return true for the specific `process.version` member expression.
    pub(in crate::lowering) fn is_process_version_member(object: &Expression<'_>) -> bool {
        let Expression::StaticMemberExpression(member) = object else {
            return false;
        };
        matches!(&member.object, Expression::Identifier(identifier) if identifier.name == "process")
            && member.property.name == "version"
    }

    /// Return true for any static `process.env.<field>` member expression.
    pub(in crate::lowering) fn is_process_env_member(member: &oxc::ast::ast::StaticMemberExpression<'_>) -> bool {
        let Expression::StaticMemberExpression(env_member) = &member.object else {
            return false;
        };
        matches!(&env_member.object, Expression::Identifier(identifier) if identifier.name == "process")
            && env_member.property.name == "env"
    }

    /// Return true for a specific static `process.env.<field>` member expression.
    pub(in crate::lowering) fn is_process_env_field(
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        field: &str,
    ) -> bool {
        member.property.name == field && Self::is_process_env_member(member)
    }

    /// Lower a computed member access expression.
    pub(in crate::lowering) fn computed_member(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(expr) = self.dynamic_math_member_expression(member, body) {
            return Ok(expr);
        }
        // A computed read off the global object (`globalThis[key]`). A statically
        // known string-literal key that names a modeled JavaScript global is
        // normalized to the concrete builtin value; any other key is a genuine
        // dynamic global-object lookup handled by `global_alias_computed_read`.
        if self.expr_is_global_alias(&member.object) {
            return self.global_alias_computed_read(member, body);
        }
        let receiver = self.expression(&member.object, body)?;
        let index = self.expression(&member.expression, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let optional_access = member.optional
            || matches!(
                self.ctx.krate.types.get(receiver_ty),
                Some(Type::Optional(_))
            );
        let access_receiver_ty = self.optional_receiver_inner_type(receiver_ty);
        // A statically-negative bracket index on an array, string, or tuple is a
        // JavaScript property lookup that never names an element, so the read is
        // `undefined` regardless of whether the receiver is optional. Lower it to
        // an honest optional `None` instead of rejecting or wrapping like `.at`.
        // (Write targets are intercepted earlier as property-store no-ops.)
        if self.is_negative_sequence_bracket_index(access_receiver_ty, index, body) {
            return self.lower_negative_sequence_bracket_read(access_receiver_ty, member.span, body);
        }
        if optional_access {
            let value_ty = self.index_type(access_receiver_ty)?;
            let ty = self.optional_chain_result_type(value_ty);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::OptionalIndex { receiver, index },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        if let Some(Type::Tuple(items)) = self.ctx.krate.types.get(access_receiver_ty).cloned() {
            let tuple_len = if self.allow_unknown_index_access {
                usize::MAX
            } else {
                items.len()
            };
            let tuple_index = self.static_tuple_index(index, body, tuple_len, member.span)?;
            let Some(ty) = items.get(tuple_index).copied() else {
                if self.allow_unknown_index_access {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Index { receiver, index },
                        ty,
                        span: self.span(member.span.start, member.span.end),
                    }));
                }
                return Err(SmeltError::unsupported(
                    self.span(member.span.start, member.span.end),
                    "tuple index is out of bounds",
                ));
            };
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TupleIndex {
                    tuple: receiver,
                    index: tuple_index,
                },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        if let Some(Type::Union(items)) = self.ctx.krate.types.get(access_receiver_ty).cloned()
            && let Some(max_tuple_len) = items
                .iter()
                .filter_map(|item| match self.ctx.krate.types.get(*item) {
                    Some(Type::Tuple(tuple_items)) => Some(tuple_items.len()),
                    _ => None,
                })
                .max()
        {
            let tuple_len = if self.allow_unknown_index_access {
                usize::MAX
            } else {
                max_tuple_len
            };
            let tuple_index = self.static_tuple_index(index, body, tuple_len, member.span)?;
            let mut indexed_tys = Vec::new();
            for item in items {
                if let Some(Type::Tuple(tuple_items)) = self.ctx.krate.types.get(item)
                    && let Some(ty) = tuple_items.get(tuple_index).copied()
                    && !indexed_tys.contains(&ty)
                {
                    indexed_tys.push(ty);
                }
            }
            if !indexed_tys.is_empty() {
                let ty = match indexed_tys.as_slice() {
                    [single] => *single,
                    _ => self.ctx.krate.types.intern(Type::Union(indexed_tys)),
                };
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Index { receiver, index },
                    ty,
                    span: self.span(member.span.start, member.span.end),
                }));
            }
        }
        if self.can_lower_acknowledged_unknown_index(access_receiver_ty, member.span.start) {
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Index { receiver, index },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        let ty = self.index_type(access_receiver_ty)?;
        if matches!(
            self.ctx.krate.types.get(access_receiver_ty),
            Some(Type::Dict(_, _))
        ) && matches!(self.ctx.krate.types.get(ty), Some(Type::Class { .. }))
        {
            let ty = self.optional_chain_result_type(ty);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Index { receiver, index },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::Index { receiver, index },
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower `Math[method]` for supported numeric method-key unions to a closure.
    ///
    /// Date-fns stores a selected rounding method as a string literal union and
    /// then calls the selected function. Smelt keeps that value dynamic by
    /// emitting a captured closure that dispatches over supported rounding names.
    pub(in crate::lowering) fn dynamic_math_member_expression(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Math" {
            return None;
        }
        let Expression::Identifier(method_ident) = &member.expression else {
            return None;
        };
        let source_local = self.locals.get(method_ident.name.as_str()).copied()?;
        let source_decl = usize::try_from(source_local.0)
            .ok()
            .and_then(|index| outer_body.locals.get(index))?;

        let span = self.span(member.span.start, member.span.end);
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        let value_name = self.intern_source_name("value");
        let method_name = source_decl
            .name
            .unwrap_or_else(|| self.intern_source_name(method_ident.name.as_str()));
        let method_ty = source_decl.ty;
        let mut closure_body = Body::new(None, span);
        let method_local = closure_body.push_local(LocalDecl {
            name: Some(method_name),
            ty: method_ty,
            mutable: source_decl.mutable,
            span: source_decl.span,
        });
        let value_local = closure_body.push_local(LocalDecl {
            name: Some(value_name),
            ty: number_ty,
            mutable: false,
            span,
        });
        closure_body.params.push(value_local);
        let value_expr = closure_body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty: number_ty,
            span,
        });

        let mut result = Self::dynamic_math_round_expr(
            &mut closure_body,
            NumericRoundOp::Trunc,
            value_expr,
            number_ty,
            span,
        );
        for (method, op) in [
            ("round", NumericRoundOp::Round),
            ("floor", NumericRoundOp::Floor),
            ("ceil", NumericRoundOp::Ceil),
        ] {
            let method_expr = closure_body.push_expr(Expr {
                kind: ExprKind::Local(method_local),
                ty: method_ty,
                span,
            });
            let method_text = closure_body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(method.to_owned())),
                ty: self.ctx.krate.types.intern(Type::String),
                span,
            });
            let cond = closure_body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Eq,
                    lhs: method_expr,
                    rhs: method_text,
                },
                ty: self.ctx.krate.types.intern(Type::Bool),
                span,
            });
            let then_expr =
                Self::dynamic_math_round_expr(&mut closure_body, op, value_expr, number_ty, span);
            result = closure_body.push_expr(Expr {
                kind: ExprKind::Conditional {
                    cond,
                    then_expr,
                    else_expr: result,
                },
                ty: number_ty,
                span,
            });
        }
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![number_ty],
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: number_ty,
            is_async: false,
            may_throw: false,
        }));
        Some(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: vec![Param {
                    name: value_name,
                    local: value_local,
                    ty: number_ty,
                    span,
                }],
                rest: None,
                required_params: None,
                return_ty: number_ty,
                captures: vec![ClosureCapture {
                    source_local,
                    body_local: Some(method_local),
                    symbol: method_name,
                    ty: method_ty,
                    mode: CaptureMode::ByRef,
                }],
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Push one numeric rounding operation inside a generated dynamic Math closure.
    pub(in crate::lowering) fn dynamic_math_round_expr(
        body: &mut Body,
        op: NumericRoundOp,
        operand: smelt_hir::ExprId,
        number_ty: smelt_hir::TypeId,
        span: Span,
    ) -> smelt_hir::ExprId {
        body.push_expr(Expr {
            kind: ExprKind::NumericRound { op, operand },
            ty: number_ty,
            span,
        })
    }

    /// Lower explicitly acknowledged `unknown[key]` reads in unknown contexts.
    ///
    /// Runtime indexing into `unknown` is intentionally rejected by the normal
    /// index path. This fallback only covers source that already carries a
    /// nearby `@ts-expect-error [ts7053]`, which Remeda uses for dynamic
    /// accumulator reads that TypeScript itself cannot prove.
    pub(in crate::lowering) fn unknown_computed_member_with_hint(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.has_ts_expect_error_before(member.span.start, "ts7053") {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, receiver)),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. })
        ) {
            return Ok(None);
        }
        let index = self.expression(&member.expression, body)?;
        let ty = self.ctx.krate.types.intern(Type::Unknown);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Index { receiver, index },
            ty,
            span: self.span(member.span.start, member.span.end),
        })))
    }

    /// Return whether source explicitly acknowledges a dynamic unknown index read.
    pub(in crate::lowering) fn can_lower_acknowledged_unknown_index(
        &self,
        receiver_ty: smelt_hir::TypeId,
        start: u32,
    ) -> bool {
        matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. })
        ) && self.has_ts_expect_error_before(start, "ts7053")
    }

    /// Return whether a TypeScript error-suppression pragma covers this position.
    ///
    /// `tsc` suppresses every diagnostic on the line that follows a
    /// `// @ts-expect-error` or `// @ts-ignore` comment, so a call marked this
    /// way is *intentionally* invalid source: the author is probing runtime
    /// behavior that the static signature rejects (for example calling an
    /// overloaded function with too few arguments). Lowering uses this to
    /// distinguish such deliberate probes from genuine signature mismatches.
    /// Only the contiguous comment-only lines directly above the line
    /// containing `start` are inspected, so a pragma cannot leak past the
    /// statement it annotates onto later code.
    pub(in crate::lowering) fn has_ts_error_suppression_before(&self, start: u32) -> bool {
        let Ok(start) = usize::try_from(start) else {
            return false;
        };
        let Some(prefix) = self.source.get(..start) else {
            return false;
        };
        let mut lines = prefix.lines().rev();
        // Drop the (partial) line that contains `start` itself; the pragma
        // must sit on a preceding line to apply to this one.
        let _current_line = lines.next();
        for line in lines {
            let trimmed = line.trim_start();
            let is_comment_line =
                trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*');
            if !is_comment_line {
                return false;
            }
            if trimmed.contains("@ts-expect-error") || trimmed.contains("@ts-ignore") {
                return true;
            }
        }
        false
    }

    /// Return whether a nearby preceding comment expects the given TS error code.
    pub(in crate::lowering) fn has_ts_expect_error_before(&self, start: u32, code: &str) -> bool {
        let Ok(start) = usize::try_from(start) else {
            return false;
        };
        let prefix_start = start.saturating_sub(256);
        let Some(prefix) = self.source.get(prefix_start..start) else {
            return false;
        };
        prefix.contains("@ts-expect-error") && prefix.contains(code)
    }

    /// Lower TypeScript global primitive conversion and numeric parse calls.
    pub(in crate::lowering) fn primitive_cast_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        // These names (`parseInt`, `String`, `Number`, `parseFloat`, `BigInt`,
        // `Boolean`) are recognized as JavaScript globals by identifier name. A
        // value import, module item, or local binding of the same name shadows
        // the global — e.g. es-toolkit's `import { parseInt } from './parseInt'`
        // accepts a value and an optional `undefined` radix. Defer to the
        // ordinary call path so the shadowing binding is called instead of the
        // global primitive-conversion op.
        if self.builtin_call_identifier_is_shadowed(callee.name.as_str()) {
            return Ok(None);
        }
        if callee.name == "parseInt" {
            let (operand, radix) = self.parse_int_operand("parseInt", call, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            let kind = radix.map_or(
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToInt,
                    operand,
                },
                |radix| ExprKind::ParseIntRadix { operand, radix },
            );
            return Ok(Some(body.push_expr(Expr {
                kind,
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if callee.name == "parseFloat" {
            let operand = self.parse_float_operand("parseFloat", call, body)?;
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ParseFloat,
                    operand,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let (op, result_ty) = match callee.name.as_str() {
            "String" => (PrimitiveCastOp::ToString, Type::String),
            "Number" => (PrimitiveCastOp::ToJsNumber, Type::Float),
            "BigInt" => (PrimitiveCastOp::ToFloat, Type::Float),
            "Boolean" => (PrimitiveCastOp::ToBool, Type::Bool),
            _ => return Ok(None),
        };
        if call.arguments.is_empty() {
            // Zero-argument primitive conversions are legal JavaScript and return
            // the type's default primitive: `Boolean()` -> `false`, `Number()` ->
            // `0`, `String()` -> `""`. (`parseFloat`/`BigInt` are not
            // default-value coercions and keep the arity error.)
            let default_literal = match callee.name.as_str() {
                "Boolean" => Some(Literal::Bool(false)),
                "Number" => Some(Literal::Float(0.0)),
                "String" => Some(Literal::String(String::new())),
                _ => None,
            };
            if let Some(literal) = default_literal {
                let ty = self.ctx.krate.types.intern(result_ty);
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(literal),
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
        }
        let [arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{} requires exactly one argument", callee.name),
            ));
        };
        let operand = self.argument(arg, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        let operand_type = self.ctx.krate.types.get(operand_ty);
        if !self.primitive_cast_accepts_operand(op, operand_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{} requires a primitive argument", callee.name),
            ));
        }
        if callee.name == "String" && matches!(operand_type, Some(Type::Bool)) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "String currently supports number and string arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(result_ty);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast { op, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether a global primitive conversion accepts a lowered operand type.
    pub(in crate::lowering) fn primitive_cast_accepts_operand(
        &self,
        op: PrimitiveCastOp,
        operand_ty: smelt_hir::TypeId,
    ) -> bool {
        if op == PrimitiveCastOp::ToString && self.erased_or_union_surface(operand_ty) {
            return true;
        }
        if matches!(op, PrimitiveCastOp::ToFloat | PrimitiveCastOp::ToJsNumber) {
            return !matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Never));
        }
        match self.ctx.krate.types.get(operand_ty) {
            Some(Type::Bool | Type::String | Type::Int | Type::Float | Type::Unknown) => true,
            Some(Type::TypeParam { .. } | Type::Class { .. }) => {
                matches!(
                    op,
                    PrimitiveCastOp::ToBool
                        | PrimitiveCastOp::ToFloat
                        | PrimitiveCastOp::ToJsNumber
                )
            }
            Some(Type::Union(items)) => items.iter().copied().all(|item| {
                matches!(self.ctx.krate.types.get(item), Some(Type::None))
                    || self.primitive_cast_accepts_operand(op, item)
            }),
            Some(Type::Optional(item)) => self.primitive_cast_accepts_operand(op, *item),
            Some(_) if self.is_numeric_like_type(operand_ty) => true,
            _ => false,
        }
    }

    /// Return the mutable-global item id an assignment target names, if any.
    ///
    /// A function-local or test-body binding with the same name shadows the
    /// module global, so a registered local always wins over the global path.
    fn assignment_target_mutable_global(
        &self,
        target: &AssignmentTarget<'_>,
    ) -> Option<smelt_hir::ItemId> {
        let AssignmentTarget::AssignmentTargetIdentifier(identifier) = target else {
            return None;
        };
        if self.locals.contains_key(identifier.name.as_str()) {
            return None;
        }
        self.mutable_global_item(identifier.name.as_str())
    }

    /// Desugar an assignment to a lifted mutable global into a `GlobalSet`.
    ///
    /// Reuses [`Self::assignment_parts`] to compute the stored value (which
    /// reads the current value through `GlobalGet` for compound and logical
    /// operators), then wraps it in a `GlobalSet` that evaluates to the stored
    /// value so `x = e` / `x += e` compose as expressions. Returns `Ok(None)`
    /// when the target is not a lifted global so ordinary lowering proceeds.
    pub(in crate::lowering) fn try_global_assignment_expression(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        // `globalThis.<Name> = ...` for a reassigned modeled host constructor
        // desugars to a `HostGlobalWrite` slot store; it must intercept before
        // the lifted-mutable-global path and the ordinary member-assignment path.
        if let Some(write) = self.try_host_global_write_expression(assign, body)? {
            return Ok(Some(write));
        }
        let Some(item) = self.assignment_target_mutable_global(&assign.left) else {
            return Ok(None);
        };
        let (_target, value) = self.assignment_parts(assign, body)?;
        let ty = Self::expr_ty(body, value);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::GlobalSet { item, value },
            ty,
            span: self.span(assign.span.start, assign.span.end),
        })))
    }

    /// Desugar an increment/decrement of a lifted mutable global.
    ///
    /// Prefix `++x` evaluates to the stored (new) value, so the `GlobalSet`
    /// itself is returned. Postfix `x++` must evaluate to the old value while
    /// still storing the incremented one; when `keep_old_value` is set (an
    /// expression-value position) the old value is captured in a temporary
    /// before the store is emitted as a side statement. In statement/loop
    /// positions the result is discarded, so the `GlobalSet` is returned
    /// directly. Returns `Ok(None)` when the target is not a lifted global.
    pub(in crate::lowering) fn try_global_update_expression(
        &mut self,
        update: &oxc::ast::ast::UpdateExpression<'_>,
        body: &mut Body,
        keep_old_value: bool,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) = &update.argument else {
            return Ok(None);
        };
        // A same-named local (function body or re-lowered test setup) shadows
        // the module global; the ordinary local update path handles it.
        if self.locals.contains_key(identifier.name.as_str()) {
            return Ok(None);
        }
        let Some(item) = self.mutable_global_item(identifier.name.as_str()) else {
            return Ok(None);
        };
        let span = self.span(update.span.start, update.span.end);
        let ty = match self.item_ref(item) {
            Item::MutableGlobal(global_item) => global_item.ty,
            _ => self.ctx.krate.types.intern(Type::Unknown),
        };
        let op = match update.operator {
            UpdateOperator::Increment => BinOp::Add,
            UpdateOperator::Decrement => BinOp::Sub,
        };
        let build_set = |target_body: &mut Body| {
            let current = target_body.push_expr(Expr {
                kind: ExprKind::GlobalGet { item },
                ty,
                span,
            });
            let one = target_body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(1.0)),
                ty,
                span,
            });
            let next = target_body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op,
                    lhs: current,
                    rhs: one,
                },
                ty,
                span,
            });
            target_body.push_expr(Expr {
                kind: ExprKind::GlobalSet { item, value: next },
                ty,
                span,
            })
        };
        if update.prefix || !keep_old_value {
            let set = build_set(body);
            return Ok(Some(set));
        }
        // Postfix in an expression-value position: capture the old value in a
        // temporary, emit the store as a side statement, then evaluate to the
        // captured old value.
        let old = body.push_expr(Expr {
            kind: ExprKind::GlobalGet { item },
            ty,
            span,
        });
        let temp_local = body.push_local(LocalDecl {
            name: Some(self.ctx.krate.symbols.intern("__smelt_global_old")),
            ty,
            mutable: false,
            span,
        });
        let temp_pat = body.push_pattern(Pattern::Binding(temp_local));
        let set = build_set(body);
        if let Some(block) = self.current_statement_block {
            body.push_stmt_to_block(
                block,
                Stmt::Let {
                    pat: temp_pat,
                    ty,
                    value: Some(old),
                },
            );
            body.push_stmt_to_block(block, Stmt::Expr(set));
        } else {
            body.push_stmt(Stmt::Let {
                pat: temp_pat,
                ty,
                value: Some(old),
            });
            body.push_stmt(Stmt::Expr(set));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Local(temp_local),
            ty,
            span,
        })))
    }

    /// Extract target and value from assignment expression.
    pub(in crate::lowering) fn assignment_parts(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, smelt_hir::ExprId), SmeltError> {
        let target = self.assignment_target_expr(&assign.left, body)?;
        let target_ty = Self::expr_ty(body, target);
        let right_ty_hint = match assign.operator {
            AssignmentOperator::Assign => Some(target_ty),
            AssignmentOperator::LogicalNullish => self.non_nullish_type(target_ty),
            AssignmentOperator::LogicalOr | AssignmentOperator::LogicalAnd => Some(target_ty),
            _ => None,
        };
        let right = self.expression_with_hint(&assign.right, body, right_ty_hint)?;
        let value = match assign.operator {
            AssignmentOperator::Assign => right,
            AssignmentOperator::LogicalNullish => body.push_expr(Expr {
                kind: ExprKind::OptionalCoalesce {
                    optional: target,
                    fallback: right,
                },
                ty: self.non_nullish_type(target_ty).unwrap_or(target_ty),
                span: self.span(assign.span.start, assign.span.end),
            }),
            AssignmentOperator::LogicalOr | AssignmentOperator::LogicalAnd => {
                let span = self.span(assign.span.start, assign.span.end);
                let cond = self.lowered_condition_expression(target, span, body)?;
                let (then_expr, else_expr) = if assign.operator == AssignmentOperator::LogicalOr {
                    (target, right)
                } else {
                    (right, target)
                };
                body.push_expr(Expr {
                    kind: ExprKind::Conditional {
                        cond,
                        then_expr,
                        else_expr,
                    },
                    ty: target_ty,
                    span,
                })
            }
            AssignmentOperator::Addition
            | AssignmentOperator::Subtraction
            | AssignmentOperator::Multiplication
            | AssignmentOperator::Division => {
                let op = match assign.operator {
                    AssignmentOperator::Addition => BinOp::Add,
                    AssignmentOperator::Subtraction => BinOp::Sub,
                    AssignmentOperator::Multiplication => BinOp::Mul,
                    AssignmentOperator::Division => BinOp::Div,
                    other => {
                        return Err(SmeltError::unsupported(
                            self.span(assign.span.start, assign.span.end),
                            format!("assignment operator is not lowered yet: {other:?}"),
                        ));
                    }
                };
                let ty = Self::expr_ty(body, target);
                body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op,
                        lhs: target,
                        rhs: right,
                    },
                    ty,
                    span: self.span(assign.span.start, assign.span.end),
                })
            }
            other => {
                return Err(SmeltError::unsupported(
                    self.span(assign.span.start, assign.span.end),
                    format!("assignment operator is not lowered yet: {other:?}"),
                ));
            }
        };
        self.apply_assignment_observed_type(&assign.left, Self::expr_ty(body, value), body);
        Ok((target, value))
    }

    /// Try to claim a straight-line `fn.prop = value` write onto a function-typed
    /// local for later callable-object construction.
    ///
    /// A function local carries no data fields, so a static-member write onto it
    /// only appears in the callable-object construction pattern: a local function
    /// (`debounced`) that receives `debounced.schedule = …`, `debounced.cancel =
    /// …` writes and is then returned at a callable-interface type. Instead of
    /// lowering the (fieldless) write, this collects `(property, value)` pairs in
    /// [`ModuleBuilder::callable_local_props`] so the eventual callable-interface
    /// coercion can synthesize a typed `CallableObjectAssign` struct. The RHS is
    /// lowered into a fresh compiler local via `Stmt::Let` so its evaluation order
    /// and side effects are preserved exactly where the write appears, even though
    /// the write itself is deferred into the struct construction.
    ///
    /// Two shapes are documented punts the collection cannot claim:
    /// * a write outside the straight-line function body block (inside an `if`,
    ///   loop, or other nested block), because the field would need to become
    ///   `Optional` to model the maybe-unwritten case — this returns `Ok(false)`
    ///   and falls through to normal (pre-feature) assignment lowering, which
    ///   lowers the fieldless static-member write as a discarded statement; and
    /// * a write after the local has escaped (been read for anything other than
    ///   the consuming coercion), because the escaped view never saw the props —
    ///   this stays an `unsupported` diagnostic, guarding the construction path
    ///   from a silently mis-constructed value.
    ///
    /// Returns `Ok(true)` when the write was claimed (no statement is emitted),
    /// `Ok(false)` when normal assignment lowering should proceed.
    pub(in crate::lowering) fn try_collect_callable_local_prop(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        if assign.operator != AssignmentOperator::Assign {
            return Ok(false);
        }
        let AssignmentTarget::StaticMemberExpression(member) = &assign.left else {
            return Ok(false);
        };
        if member.optional {
            return Ok(false);
        }
        let Expression::Identifier(object) = &member.object else {
            return Ok(false);
        };
        let Some(local) = self.locals.get(object.name.as_str()).copied() else {
            return Ok(false);
        };
        let Some(local_ty) = Self::local_ty_checked(body, local) else {
            return Ok(false);
        };
        if !matches!(self.ctx.krate.types.get(local_ty), Some(Type::Function(_))) {
            return Ok(false);
        }
        let span = self.span(assign.span.start, assign.span.end);
        if block != body.root {
            // Conditional / nested-block write: the feature only claims
            // straight-line writes in the body root block, because modeling a
            // maybe-unwritten field would require weakening it to `Optional`.
            // Rather than blocking the whole crate, fall through to normal
            // assignment lowering — the pre-feature behavior where a fieldless
            // static-member write is lowered as a discarded statement. This
            // matches the design's "conditionals fall through + diagnostic"
            // punt (see specs/plans/callable-object-construction.md §5) and is
            // safe: the hard construction guard below only fires once props are
            // actually consumed into a typed callable-interface struct.
            return Ok(false);
        }
        if self
            .callable_local_props
            .get(&local)
            .is_some_and(|state| state.escaped)
        {
            return Err(SmeltError::unsupported(
                span,
                "property writes onto a callable local after it escapes are not lowered yet",
            ));
        }
        let value = self.expression(&assign.right, body)?;
        let value_ty = Self::expr_ty(body, value);
        let value_local = body.push_local(LocalDecl {
            name: Some(self.ctx.krate.symbols.intern("__smelt_callable_prop")),
            ty: value_ty,
            mutable: false,
            span,
        });
        let value_pat = body.push_pattern(Pattern::Binding(value_local));
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat: value_pat,
                ty: value_ty,
                value: Some(value),
            },
        );
        let value_read = body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty: value_ty,
            span,
        });
        let prop = self.intern_source_name(member.property.name.as_str());
        let entry = self.callable_local_props.entry(local).or_default();
        // Last write wins: a repeated write to the same property replaces the
        // earlier value while keeping source order for the surviving props.
        entry.props.retain(|(name, _)| *name != prop);
        entry.props.push((prop, value_read));
        Ok(true)
    }

    /// Synthesize a typed `CallableObjectAssign` when a callable local coerces to
    /// a callable-interface class.
    ///
    /// Consumes the property writes collected by
    /// [`Self::try_collect_callable_local_prop`] for the named function-typed
    /// local and bundles them with the base callable into an expression typed at
    /// the interface class `type_hint`. Returns `Ok(None)` (leaving the local's
    /// collected props intact) when there is no hint, the hint is not a callable
    /// interface, or the local collected no writes — so an ordinary identifier
    /// read proceeds normally.
    pub(in crate::lowering) fn try_consume_callable_local(
        &mut self,
        ident_name: &str,
        start: u32,
        end: u32,
        type_hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(hint) = type_hint else {
            return Ok(None);
        };
        if !self.type_is_callable_interface(hint) {
            return Ok(None);
        }
        let Some(local) = self.locals.get(ident_name).copied() else {
            return Ok(None);
        };
        let Some(state) = self.callable_local_props.remove(&local) else {
            return Ok(None);
        };
        let base_ty = Self::local_ty_checked(body, local)
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
        let callable = body.push_expr(Expr {
            kind: ExprKind::Local(local),
            ty: base_ty,
            span: self.span(start, end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::CallableObjectAssign {
                callable,
                props: state.props,
            },
            ty: hint,
            span: self.span(start, end),
        })))
    }

    /// Return whether a type is a callable-interface class (a class whose
    /// interface declaration carries the synthetic `__smelt_call` field).
    pub(in crate::lowering) fn type_is_callable_interface(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty) else {
            return false;
        };
        self.find_interface(*name).is_some_and(|interface| {
            interface
                .fields
                .iter()
                .any(|field| self.ctx.krate.symbols.get(field.name) == Some("__smelt_call"))
        })
    }

    /// Try to lower a statement-level `list.length = value` assignment.
    ///
    /// Assigning to a JavaScript array's `length` resizes it: when the new length
    /// is smaller than the current one the array is truncated to that many
    /// elements; when it is larger the array grows with empty (`undefined`) slots.
    /// The truncation case is modeled here as an in-place splice that removes
    /// every element from the new length onward (`arr.splice(new_len)`), reusing
    /// the existing list-splice machinery rather than inventing a new op.
    ///
    /// Growing a list past its current length is intentionally *not* modeled: a
    /// Smelt list is a homogeneous `Vec<T>`, so padding it with `undefined` holes
    /// has no representation for a non-optional element type. That case (rare in
    /// practice — `length` writes overwhelmingly shrink) lowers to a no-op growth,
    /// which is an explicit deferral of JavaScript's hole-padding semantics.
    ///
    /// Returns `Ok(true)` when the target matched a list `.length` write and a
    /// statement was emitted, `Ok(false)` when normal assignment lowering should
    /// proceed.
    pub(in crate::lowering) fn try_lower_list_length_assignment_statement(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        // Only a plain `=` resize is modeled; a compound `arr.length += n` would
        // first read the current length and is left to normal lowering.
        if assign.operator != AssignmentOperator::Assign {
            return Ok(false);
        }
        let AssignmentTarget::StaticMemberExpression(member) = &assign.left else {
            return Ok(false);
        };
        if member.property.name != "length" {
            return Ok(false);
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(list_ty), Some(Type::List(_))) {
            return Ok(false);
        }
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let start = self.expression_with_hint(&assign.right, body, Some(float_ty))?;
        let span = self.span(assign.span.start, assign.span.end);
        // `splice(new_len)` with no delete count removes everything from the new
        // length to the end, truncating the list in place. The splice yields the
        // removed elements (typed as the list); discarded as an expression stmt.
        let splice = body.push_expr(Expr {
            kind: ExprKind::ListSplice {
                list,
                start,
                delete_count: None,
                items: Vec::new(),
                mutate: true,
            },
            ty: list_ty,
            span,
        });
        body.push_stmt_to_block(block, Stmt::Expr(splice));
        Ok(true)
    }

    /// Lower a plain array destructuring assignment statement.
    ///
    /// JavaScript evaluates the right-hand side before writing any targets, so
    /// Smelt stores that value in a compiler local and then emits one ordinary
    /// assignment per destructured element. This keeps swaps like
    /// `[data[i], data[j]] = [data[j], data[i]]` from observing their own writes.
    pub(in crate::lowering) fn array_destructuring_assignment_statement(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        if assign.operator != AssignmentOperator::Assign {
            return Ok(false);
        }
        let AssignmentTarget::ArrayAssignmentTarget(array) = &assign.left else {
            return Ok(false);
        };
        let value = self.expression(&assign.right, body)?;
        let value_ty = Self::expr_ty(body, value);
        let value_local = body.push_local(LocalDecl {
            name: Some(self.ctx.krate.symbols.intern("__smelt_destructure")),
            ty: value_ty,
            mutable: false,
            span: self.span(assign.right.span().start, assign.right.span().end),
        });
        let value_pat = body.push_pattern(Pattern::Binding(value_local));
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat: value_pat,
                ty: value_ty,
                value: Some(value),
            },
        );

        for (index, target) in array.elements.iter().enumerate() {
            let Some(target) = target else {
                continue;
            };
            let target = self.assignment_maybe_default_target_expr(target, body)?;
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(value_local),
                ty: value_ty,
                span: self.span(assign.right.span().start, assign.right.span().end),
            });
            let index_ty = self.ctx.krate.types.intern(Type::Float);
            let index_value = u32::try_from(index).map_or(f64::INFINITY, f64::from);
            let index_expr = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(index_value)),
                ty: index_ty,
                span: self.span(assign.left.span().start, assign.left.span().end),
            });
            let (kind, ty) = match self.ctx.krate.types.get(value_ty) {
                Some(Type::Tuple(items)) => {
                    let Some(ty) = items.get(index).copied() else {
                        return Err(SmeltError::unsupported(
                            self.span(assign.left.span().start, assign.left.span().end),
                            "array assignment index is outside tuple type",
                        ));
                    };
                    (
                        ExprKind::TupleIndex {
                            tuple: receiver,
                            index,
                        },
                        ty,
                    )
                }
                _ => (
                    ExprKind::Index {
                        receiver,
                        index: index_expr,
                    },
                    self.index_type(value_ty)?,
                ),
            };
            let element_value = body.push_expr(Expr {
                kind,
                ty,
                span: self.span(assign.left.span().start, assign.left.span().end),
            });
            body.push_stmt_to_block(
                block,
                Stmt::Assign {
                    target,
                    value: element_value,
                },
            );
        }
        Ok(true)
    }

    /// Record the observed type produced by assigning into an unknown local.
    ///
    /// TypeScript flow analysis narrows a variable's observed type after direct
    /// assignment even when its declaration started as `unknown` in Smelt's
    /// no-`any` model. Keeping this as a narrowing fact preserves the local's
    /// declared storage type while allowing later reads in the same flow to be
    /// lowered with the assigned value type.
    pub(in crate::lowering) fn apply_assignment_observed_type(
        &mut self,
        target: &AssignmentTarget<'_>,
        observed_ty: smelt_hir::TypeId,
        body: &Body,
    ) {
        let AssignmentTarget::AssignmentTargetIdentifier(identifier) = target else {
            return;
        };
        let name = identifier.name.as_str();
        let Some(local) = self.locals.get(name).copied() else {
            return;
        };
        let base_ty = Self::local_ty(body, local);
        // Writing through a narrowed local invalidates the prior flow fact: a new
        // value may inhabit a different union arm (or leave the narrowed subset
        // entirely), so any stale narrowing must be reconciled before later reads
        // project it into a concrete arm. When the observed value's type is a
        // member of the narrowed union (or matches it exactly) the fact is
        // refined to the observed type; otherwise it is reset to the declared
        // storage type so codegen falls back to the erased/full-union shape.
        if let Some(current) = self.narrowed_type(name)
            && current != observed_ty
        {
            if self.type_is_narrowing_of(observed_ty, current) {
                self.apply_narrowing(name.to_owned(), observed_ty);
            } else {
                self.invalidate_narrowing(name, base_ty);
            }
            return;
        }
        // Assigning a provably non-null value into an optional local narrows it
        // to the non-optional inner type for later reads in this flow. This is
        // the `x = x ?? default` / `x = x!` defaulting idiom: the declared
        // storage stays `Optional`, but subsequent reads (`let i = x; i + 1`)
        // must see the concrete inner type instead of `Option<T>`. A later
        // assignment of a possibly-null value reconciles through the
        // narrowed-type branch above, so the fact cannot outlive its flow.
        if let Some(inner) = self.optional_inner_for_narrowing(base_ty, observed_ty) {
            self.apply_narrowing(name.to_owned(), inner);
            return;
        }
        if self.ctx.krate.types.get(base_ty) != Some(&Type::Unknown) {
            return;
        }
        if self.ctx.krate.types.get(observed_ty) == Some(&Type::Unknown) {
            return;
        }
        // A local declared with an explicit `any` annotation stays pinned to the
        // erased `Unknown` boundary by source spelling. Narrowing it to a concrete
        // assignment's type would let later writes through the boundary (e.g.
        // `obj.b = obj` where `obj` is a self-referential `any`) demand a concrete
        // record value type that the erased shape cannot supply.
        if self.explicit_any_locals.contains(&local) {
            return;
        }
        self.apply_narrowing(name.to_owned(), observed_ty);
    }

    /// Return the non-optional inner type when an assignment removes optionality.
    ///
    /// Applies when the declared storage is `Optional(inner)` and the assigned
    /// value's type is provably non-null and inhabits `inner` — either an exact
    /// match, or (when `inner` is itself a union) a member of it. Returns `None`
    /// when the value could still be null/undefined (the local keeps its
    /// optional storage type) so this never hides a real optional value.
    fn optional_inner_for_narrowing(
        &self,
        base_ty: smelt_hir::TypeId,
        observed_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let Some(Type::Optional(inner)) = self.ctx.krate.types.get(base_ty) else {
            return None;
        };
        let inner_ty = *inner;
        if matches!(
            self.ctx.krate.types.get(observed_ty),
            Some(Type::Optional(_) | Type::None | Type::Unknown)
        ) {
            return None;
        }
        if observed_ty == inner_ty {
            return Some(inner_ty);
        }
        if matches!(
            self.ctx.krate.types.get(inner_ty),
            Some(Type::Union(items)) if items.contains(&observed_ty)
        ) {
            return Some(observed_ty);
        }
        None
    }

    /// Return whether `candidate` is compatible with a `current` narrowed type.
    ///
    /// Assignment refinement only keeps a narrowing when the assigned value is
    /// provably still inside the narrowed set: an exact type match, or a union
    /// member of the current narrowing. Anything else is treated as escaping the
    /// narrowed subset and triggers invalidation.
    fn type_is_narrowing_of(
        &self,
        candidate: smelt_hir::TypeId,
        current: smelt_hir::TypeId,
    ) -> bool {
        if candidate == current {
            return true;
        }
        matches!(
            self.ctx.krate.types.get(current),
            Some(Type::Union(items)) if items.contains(&candidate)
        )
    }

    /// Drop the active narrowing for `name` by pinning `base_ty` in this scope.
    ///
    /// The narrowing stack is a stack of branch scopes and `narrowed_type` reads
    /// the innermost hit. Pinning the declared storage type in the *current*
    /// scope shadows any narrowing recorded here or in an enclosing scope, so
    /// later reads in this flow see the widened (erased/full-union) shape. The
    /// shadow is scoped: when the branch pops, an enclosing-scope narrowing that
    /// legitimately still holds on the other control-flow path is restored.
    fn invalidate_narrowing(&mut self, name: &str, base_ty: smelt_hir::TypeId) {
        self.apply_narrowing(name.to_owned(), base_ty);
    }

    /// Extract target and value from increment/decrement expression.
    pub(in crate::lowering) fn update_parts(
        &mut self,
        update: &oxc::ast::ast::UpdateExpression<'_>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, smelt_hir::ExprId), SmeltError> {
        let target = self.simple_assignment_target_expr(&update.argument, body)?;
        let one_ty = Self::expr_ty(body, target);
        let one = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(1.0)),
            ty: one_ty,
            span: self.span(update.span.start, update.span.end),
        });
        let op = match update.operator {
            UpdateOperator::Increment => BinOp::Add,
            UpdateOperator::Decrement => BinOp::Sub,
        };
        let value = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op,
                lhs: target,
                rhs: one,
            },
            ty: one_ty,
            span: self.span(update.span.start, update.span.end),
        });
        Ok((target, value))
    }

    /// Lower prefix and postfix increment/decrement used as expression values.
    ///
    /// JavaScript returns the old value for postfix updates and the updated
    /// value for prefix updates. Variable initializers defer postfix assignments
    /// until after their binding statement so the initializer observes the old
    /// value; other expression contexts emit in their owning statement block.
    pub(in crate::lowering) fn update_expression(
        &mut self,
        update: &oxc::ast::ast::UpdateExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // A `++`/`--` of a lifted mutable global desugars to a `GlobalSet` that
        // preserves JavaScript's prefix (new value) / postfix (old value)
        // result semantics.
        if let Some(expr) = self.try_global_update_expression(update, body, true)? {
            return Ok(expr);
        }
        let (target, value) = self.update_parts(update, body)?;
        if !update.prefix
            && let Some(deferred_updates) = self.deferred_postfix_updates.as_mut()
        {
            deferred_updates.push(Stmt::Assign { target, value });
        } else if let Some(block) = self.current_statement_block {
            body.push_stmt_to_block(block, Stmt::Assign { target, value });
        } else {
            body.push_stmt(Stmt::Assign { target, value });
        }
        if update.prefix { Ok(value) } else { Ok(target) }
    }

    /// Convert assignment target to expression.
    pub(in crate::lowering) fn assignment_target_expr(
        &mut self,
        target: &AssignmentTarget<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            AssignmentTarget::StaticMemberExpression(member) => self.static_member(member, body),
            AssignmentTarget::PrivateFieldExpression(member) => self.private_field_member(
                &member.object,
                member.field.name.as_str(),
                member.span,
                body,
            ),
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                let Some(Some(first)) = array.elements.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "array assignment targets require at least one element",
                    ));
                };
                self.assignment_maybe_default_target_expr(first, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(target.span().start, target.span().end),
                "assignment target must be a local, field, or index expression",
            )),
        }
    }

    /// Convert an array destructuring assignment element to its target expression.
    pub(in crate::lowering) fn assignment_maybe_default_target_expr(
        &mut self,
        target: &oxc::ast::ast::AssignmentTargetMaybeDefault<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match target {
            oxc::ast::ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => {
                self.assignment_target_expr(&default.binding, body)
            }
            _ => self.assignment_target_expr(target.to_assignment_target(), body),
        }
    }

    /// Convert simple assignment target to expression.
    pub(in crate::lowering) fn simple_assignment_target_expr(
        &mut self,
        target: &SimpleAssignmentTarget<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => self
                .identifier_expression(ident.name.as_str(), ident.span.start, ident.span.end, body),
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.static_member(member, body)
            }
            SimpleAssignmentTarget::PrivateFieldExpression(member) => self.private_field_member(
                &member.object,
                member.field.name.as_str(),
                member.span,
                body,
            ),
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(target.span().start, target.span().end),
                "update target must be a local, field, or index expression",
            )),
        }
    }

    /// Lower access to a private class field through the same field HIR as public fields.
    pub(in crate::lowering) fn private_field_member(
        &mut self,
        object: &Expression<'_>,
        field_name: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let receiver = self.expression(object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let field = self.intern_source_name(field_name);
        let ty = self.class_field_type(receiver_ty, field)?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Field { receiver, field },
            ty,
            span: self.span(span.start, span.end),
        }))
    }

    // Continued in the next split builder file.
}
