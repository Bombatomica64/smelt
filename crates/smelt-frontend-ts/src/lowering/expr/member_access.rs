//! Member access: namespace member calls, static and computed member reads, and the
//! well-known `Symbol`, `Number`, `Object` and `process` static surfaces.

use crate::lowering::body_arena::BodyArena;
use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::js_text::js_str_text;
use oxc::ast::ast::{Argument, ChainElement, Expression};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, DictProjectionOp, ErasureReason, Expr, ExprKind, FunctionType, Item, Literal, LocalDecl,
    NumericPredicateOp, Param, Span, Stmt, Type, UnknownKind,
};

impl ModuleBuilder<'_> {
    /// Resolve a direct-call argument hint, expanding a rest list slot to its item type.
    fn namespace_call_argument_hint(
        &self,
        params: &[smelt_hir::TypeId],
        rest: Option<usize>,
        index: usize,
    ) -> Option<smelt_hir::TypeId> {
        if let Some(rest_index) = rest
            && index >= rest_index
        {
            return params.get(rest_index).map(|ty| match self.ctx.krate.types.get(*ty) {
                Some(Type::List(item_ty)) => *item_ty,
                _ => *ty,
            });
        }
        params.get(index).copied()
    }

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
            if self.imports.is_namespace(namespace) {
                for arg in &call.arguments {
                    let _ = self.argument(arg, body)?;
                }
                let ty = self.erase_with(ErasureReason::UntypedPackage, member_name);
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
                    function
                        .params
                        .iter()
                        .map(|param| param.ty)
                        .collect::<Vec<_>>(),
                    function.rest,
                    function.return_ty,
                    function.is_async,
                )
            } else {
                let item_ty = self.item_expr_type(item, span)?;
                if let Some(Type::Function(function)) = self.ctx.krate.types.get(item_ty).cloned() {
                    let Item::Const(const_item) = self.item_ref(item).clone() else {
                        return Err(SmeltError::unsupported(
                            span,
                            format!("namespace member `{member_name}` is not a callable value"),
                        ));
                    };
                    let callee = self.const_item_expression(
                        &const_item,
                        member.span.start,
                        member.span.end,
                        body,
                    )?;
                    let args = call
                        .arguments
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            let hint = self.namespace_call_argument_hint(
                                &function.params,
                                function.rest,
                                index,
                            );
                            self.argument_with_hint(arg, body, hint)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let spread_positions =
                        call.arguments.iter().map(Argument::is_spread).collect::<Vec<_>>();
                    let call_span = self.span(call.span.start, call.span.end);
                    let args = self.pack_value_call_arguments(
                        &function,
                        args,
                        &spread_positions,
                        call_span,
                        body,
                    );
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
        let mut args = call
            .arguments
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                let hint = self.namespace_call_argument_hint(&params, rest, index);
                self.argument_with_hint(arg, body, hint)
            })
            .collect::<Result<Vec<_>, _>>()?;
        // A rest-parameter callee receives its trailing arguments as ONE list,
        // exactly as a direct identifier call packs them (`_.compose(f, g)` is
        // `compose(f, g)` through a namespace). Without the pack the emitter saw
        // two positional arguments for a one-parameter rest function and bound
        // an empty list. A spread argument is left to the emitter's spread path.
        if let Some(rest_index) = rest
            && let Some(&rest_ty) = params.get(rest_index)
            && matches!(self.ctx.krate.types.get(rest_ty), Some(Type::List(_)))
            && !call
                .arguments
                .iter()
                .any(|arg| matches!(arg, oxc::ast::ast::Argument::SpreadElement(_)))
            && args.len() >= rest_index
        {
            let rest_items = args.split_off(rest_index);
            args.push(body.push_expr(Expr {
                kind: ExprKind::ListLit(rest_items),
                ty: rest_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        // `_.iterate(..)` is `iterate(..)`: the same rule instantiates its generics.
        let type_params = match self.item_ref(item) {
            Item::Function(function) => function.type_params.clone(),
            _ => Vec::new(),
        };
        let call_ty =
            self.instantiated_call_return(&type_params, &params, &args, return_ty, call, body)?;
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
            ty: call_ty,
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
        (self.imports.is_namespace(namespace)
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
    ///   object, and is lowered as exactly that: an erased computed read whose
    ///   receiver is the global-object value. It used to fold to a constant
    ///   `undefined`, which made the two spellings of one JavaScript operation
    ///   disagree — `globalThis.Error` resolved to the modeled constructor while
    ///   `globalThis[name]` with `name === "Error"` answered `undefined`, so
    ///   `new (globalThis[type])(...)` fabricated a null-returning closure call
    ///   instead of constructing. The runtime read resolves a modeled global
    ///   constructor by name off the global-object marker record and still
    ///   answers `undefined` for a name this profile does not model, so an
    ///   unmodeled global stays absent.
    pub(in crate::lowering) fn global_alias_computed_read(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Expression::StringLiteral(key) = &member.expression {
            let name = js_str_text(key.value);
            if smelt_stdlib::is_javascript_global_builtin(&name) {
                return self.identifier_expression(&name, member.span.start, member.span.end, body);
            }
        }
        let receiver =
            self.global_object_value_expression(member.span.start, member.span.end, body);
        let index = self.expression(&member.expression, body)?;
        let unknown_ty = self.erase_with(ErasureReason::ByName, "globalThis[..]");
        Ok(body.push_expr(Expr {
            kind: ExprKind::Index { receiver, index },
            ty: unknown_ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Return whether a property name is a member every JavaScript function has.
    ///
    /// These come from `Function.prototype` rather than from any expando
    /// property the source assigned, so they are resolvable on *any* function
    /// value and must not be rejected as unmodeled. `length` never reaches the
    /// check (a dedicated arity path returns earlier); `prototype` and
    /// `constructor` resolve to the same dynamic property bag the class receiver
    /// already resolves them to, and `name` to the function's name string.
    pub(in crate::lowering) fn is_universal_function_member(property: &str) -> bool {
        matches!(property, "prototype" | "constructor" | "name" | "length")
    }

    /// Lower a static member access expression.
    pub(in crate::lowering) fn static_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.static_member_with_absent_fallback(member, body, true, true, false)
    }

    /// Lower a static member expression that is an assignment *target*.
    ///
    /// Identical to [`Self::static_member`] except that an unmodeled property on
    /// a function receiver is not rejected. A write is allowed to fall through
    /// to the (discarded) fieldless static-member assignment the callable-object
    /// design documents as its conditional-write punt
    /// (specs/plans/callable-object-construction.md §5); turning that into a hard
    /// error is exactly the regression that punt exists to prevent. The
    /// corresponding *read* still cannot invent a value, so the rejection stays
    /// on the read side where a wrong answer would otherwise be produced.
    pub(in crate::lowering) fn static_member_assignment_target(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.static_member_with_absent_fallback(member, body, true, false, true)
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
        self.static_member_with_absent_fallback(member, body, false, true, false)
    }

    /// Shared static-member lowering; `absent_list_field_is_undefined` gates
    /// the JS absent-property-read-yields-`undefined` rule for list receivers,
    /// `reject_unmodeled_function_field` gates the function-receiver
    /// rejection described on [`Self::static_member_assignment_target`], and
    /// `is_assignment_target` marks the one caller whose result is a write
    /// destination rather than a value (so the `__proto__` accessor read below
    /// must not claim it).
    fn static_member_with_absent_fallback(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
        absent_list_field_is_undefined: bool,
        reject_unmodeled_function_field: bool,
        is_assignment_target: bool,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(expr) = self.global_alias_member_read(member, body)? {
            return Ok(expr);
        }
        // A host-module data export read through its module (`path.sep`).
        if !is_assignment_target
            && let Some(resolved) = self.host_module_member_export_path(member)
            && let Some(expr) =
                self.host_module_value_expression(resolved, member.span.start, member.span.end, body)
        {
            return Ok(expr);
        }
        // A static property hung off a function declaration
        // (`partial.placeholder`). Checked before the receiver is lowered: the
        // receiver here is a *function*, which no property-read path can type, so
        // without this the read fell through to `undefined`. See
        // `lowering::function_statics`.
        if let Some(expr) = self.function_static_property_read(member, body)? {
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
        // A modeled SUB-NAMESPACE of a global namespace object (`crypto.subtle`).
        // Placed after the specific modeled reads (`Math.PI`, `process.version`)
        // so those keep their rules, and before the generic property paths,
        // which would answer `undefined` for a member the profile does model.
        if let Some(expr) = self.builtin_namespace_member_read(member, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.namespace_member_expression(member, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.materialized_static_member(member, body)? {
            return Ok(expr);
        }
        // A `URL` component or `searchParams` read, gated on the receiver's
        // lowered type like the `Response` reads below.
        if let Some(expr) = self.url_property_read(member, body)? {
            return Ok(expr);
        }
        // A `Response` data property. Placed with the other modeled-receiver
        // reads and gated on the receiver's lowered type, so `x.status` on
        // anything else falls through to the ordinary field paths.
        if let Some(expr) = self.response_property_read(member, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.request_property_read(member, body)? {
            return Ok(expr);
        }
        // `req.method`/`req.url`/`req.headers` and `res.statusCode`, gated the
        // same way on the receiver's lowered type.
        if let Some(expr) = self.http_property_read(member, body)? {
            return Ok(expr);
        }
        // A read through `(erased as { .. })` is a read off the erased value:
        // the record view would drop every inherited member. See
        // `lowering::asserted_member_read`.
        if !is_assignment_target
            && let Some(expr) = self.asserted_erased_member_read(member, body)?
        {
            return Ok(expr);
        }
        // The exemption an assignment target carries follows the whole target
        // chain: in `fn.prop.inner = value` the base `fn.prop` is read only to
        // locate the slot the (discarded) write targets, so rejecting it would
        // reject the write itself — the regression
        // [`Self::static_member_assignment_target`] exists to prevent. A
        // receiver in any other position keeps the rejection.
        let receiver = match &member.object {
            Expression::StaticMemberExpression(object) if !reject_unmodeled_function_field => {
                self.static_member_with_absent_fallback(object, body, true, false, false)?
            }
            object => self.expression(object, body)?,
        };
        if !is_assignment_target
            && member.property.name == "__proto__"
            && let Some(expr) = self.proto_accessor_read(member, receiver, body)
        {
            return Ok(expr);
        }
        let receiver_ty = Self::expr_ty(body, receiver);
        let optional_access = member.optional
            || matches!(
                self.ctx.krate.types.get(receiver_ty),
                Some(Type::Optional(_))
            );
        let access_receiver_ty = self.optional_receiver_inner_type(receiver_ty);
        let field = self.intern_source_name(member.property.name.as_str());
        // The text-codec and `Blob`/`File` data properties. Placed AFTER the
        // receiver is lowered, and reusing that receiver, because `length`,
        // `size`, `type` and `name` are members of almost every value: a
        // pre-receiver probe would lower the receiver a second time on every
        // miss, which duplicates its side effects.
        //
        // Skipped for an assignment TARGET. Every member both handlers model is
        // read-only in the spec, so a write to one is not a modeled operation
        // and must fall through to the ordinary (discarded) write path — a read
        // expression cannot be assigned to, and claiming one aborted MIR
        // lowering of es-toolkit's `isBlob` spec, whose `class File extends
        // Blob` writes `this.name`.
        if !is_assignment_target {
            if let Some(expr) =
                self.typed_array_member_read(member, receiver, access_receiver_ty, body)?
            {
                return Ok(expr);
            }
            if let Some(expr) =
                self.text_codec_member_read(member, receiver, access_receiver_ty, body)?
            {
                return Ok(expr);
            }
            if let Some(expr) = self.blob_member_read(member, receiver, access_receiver_ty, body)? {
                return Ok(expr);
            }
        }
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
                let len = body.push_expr(Expr {
                    kind: ExprKind::Len { operand },
                    ty,
                    span: self.span(member.span.start, member.span.end),
                });
                // `xs?.length` on an ABSENT `xs` is `undefined`, not a length:
                // the read is `Optional<number>`, evaluated only when present
                // (`!xs?.length` is then the presence-aware truthiness test).
                // A plain `xs.length` is the source asserting presence and
                // keeps its number.
                if member.optional
                    && matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::Optional(_)))
                {
                    return Ok(self.wrap_optional_receiver_method(receiver, len, member.span, body));
                }
                return Ok(len);
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
        if let Some(read) = self.iterator_result_member(
            receiver,
            receiver_ty,
            member.property.name.as_str(),
            self.span(member.span.start, member.span.end),
            body,
        ) {
            return Ok(read);
        }
        // A function value carries no field storage. JavaScript lets a function
        // own arbitrary static properties, but Smelt only models the ones it
        // recorded: a declared callable-interface member (`callable_fields`) or
        // a module-level function static, both of which resolve well before
        // here. Anything else would lower to `ExprKind::Field` on a receiver
        // with no fields, which MIR prints as a meaningless positional index
        // and codegen renders as a null — a wrong answer that looks like a
        // value. Refuse the read instead of inventing one.
        if reject_unmodeled_function_field
            && matches!(
                self.ctx.krate.types.get(access_receiver_ty),
                Some(Type::Function(_))
            )
            && !Self::is_universal_function_member(member.property.name.as_str())
            && !self
                .types
                .callable_fields(access_receiver_ty)
                .is_some_and(|fields| fields.iter().any(|item| item.name == field))
        {
            return Err(SmeltError::unsupported(
                self.span(member.span.start, member.span.end),
                format!(
                    "`{}` is not a modeled property of a function value; only a declared \
                     callable-interface member or a static property assigned onto a module-level \
                     function declaration resolves",
                    member.property.name
                ),
            ));
        }
        // A tagged-union receiver reads the member off its ARM instead of
        // erasing the union to look the property up at runtime. Asked here,
        // after every modeled-receiver handler has refused, because a union's
        // arms are read through those same rules one arm at a time — and it
        // answers `None` for any union it cannot fully dispatch, which keeps
        // the erased fallback below as the only other outcome. See
        // `lowering::union_member_read`.
        if !is_assignment_target
            && !member.optional
            && let Some(expr) = self.union_member_read(
                receiver,
                access_receiver_ty,
                field,
                self.span(member.span.start, member.span.end),
                body,
            )?
        {
            return Ok(expr);
        }
        let field_ty = match self.class_field_type(access_receiver_ty, field) {
            Ok(field_ty) => field_ty,
            // A JavaScript array is an exotic object: besides its elements it can
            // carry ordinary named properties, and a `RegExp.exec` result is
            // exactly that — an array with `index`, `input` and `groups`. So an
            // unmodeled name on a list receiver is not statically absent; it is a
            // question only the runtime can answer. Erase the receiver and read
            // the property off it: the erasure aliases the array's identity, which
            // is what the named-property table is keyed by, so the typed handle
            // and an erased view of the same array read the same properties.
            // Answering a constant `undefined` here dropped `.index`/`.input`
            // from every clone that narrowed with `Array.isArray` first.
            Err(error) => {
                if absent_list_field_is_undefined
                    && matches!(
                        self.ctx.krate.types.get(access_receiver_ty),
                        Some(Type::List(_))
                    )
                {
                    let span = self.span(member.span.start, member.span.end);
                    let ty = self.erase_at(
                        ErasureReason::ByName,
                        span,
                        member.property.name.as_str(),
                    );
                    let erased = body.push_expr(Expr {
                        kind: ExprKind::UnknownCast {
                            value: receiver,
                            target: ty,
                        },
                        ty,
                        span,
                    });
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Field {
                            receiver: erased,
                            field,
                        },
                        ty,
                        span,
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

    /// Read `done` or `value` off an iterator result, the two members of that
    /// protocol; `None` for any other receiver or member.
    ///
    /// A result whose iterator returns nothing carries `undefined` once done,
    /// so its `value` is `T | undefined`.
    pub(in crate::lowering) fn iterator_result_member(
        &mut self,
        receiver: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        member: &str,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Some(Type::GeneratorResult { yield_ty, return_ty }) = self.ctx.krate.types.get(receiver_ty).cloned() else {
            return None;
        };
        let (kind, ty) = match member {
            "done" => (
                ExprKind::GeneratorDone { result: receiver },
                self.ctx.krate.types.intern(Type::Bool),
            ),
            "value" => {
                let value_ty = if yield_ty == return_ty {
                    yield_ty
                } else if self.ctx.krate.types.get(return_ty) == Some(&Type::None) {
                    self.ctx.krate.types.intern(Type::Optional(yield_ty))
                } else {
                    self.ctx.krate.types.intern(Type::Union(vec![yield_ty, return_ty]))
                };
                (ExprKind::GeneratorValue { result: receiver }, value_ty)
            }
            _ => return None,
        };
        Some(body.push_expr(Expr { kind, ty, span }))
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
        let unknown_ty = self.erase_at(ErasureReason::RuntimeNarrowing, span, "Array.isArray");
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
        let value =
            self.enum_member_literal(object.name.as_str(), member.property.name.as_str())?;
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(value.literal),
            ty: value.ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower supported well-known `Symbol.<name>` member reads.
    ///
    /// `Symbol.iterator` in VALUE position is a symbol, exactly like
    /// `Symbol('d')` or `Symbol.for('d')`: `typeof Symbol.iterator` is
    /// `'symbol'`, it can be stored in a variable, handed to a predicate, and
    /// compared for identity. So it lowers to `Literal::Symbol` carrying the
    /// shared value spelling, not to the property-key string it indexes —
    /// lowering it as the key made every non-key use answer "string".
    ///
    /// The key half is a separate question, and one shared table
    /// ([`smelt_stdlib::well_known_symbols`]) relates the two: a static computed
    /// key (`[Symbol.asyncIterator]`) folds to the storage key through
    /// [`crate::lowering::ty::computed_key_symbols::well_known_symbol_key`], and
    /// a *runtime* symbol value used as a key resolves to the same storage key
    /// in the generated prelude's property-key coercion. Because both sides read
    /// one table, `obj[Symbol.asyncIterator]` still indexes the field declared
    /// by `[Symbol.asyncIterator]()` (issue #115).
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
        let spelling =
            crate::lowering::ty::computed_key_symbols::well_known_symbol_value_spelling(
                member.property.name.as_str(),
            )?;
        // Symbol values are erased at runtime (`SmeltUnknown::Symbol`), the same
        // type the `Symbol()` / `Symbol.for()` call paths intern.
        let ty = self.erase_with(ErasureReason::UnmodeledType, "symbol");
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Symbol(spelling)),
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
            name @ ("keys" | "values" | "entries" | "fromEntries" | "getPrototypeOf"
            | "create") => (1, self.erase_with(ErasureReason::UntypedBuiltin, name)),
            name @ ("assign" | "setPrototypeOf") => {
                (2, self.erase_with(ErasureReason::UntypedBuiltin, name))
            }
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
        // not a new erasure: names the parameter type of a synthesized runtime
        // helper closure (any JS value is accepted); the result's erasure is
        // recorded by the caller.
        let unknown = self.unknown_type();
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
                DictProjectionOp::Keys | DictProjectionOp::ForInKeys => {
                    self.ctx.krate.types.intern(Type::List(string_ty))
                }
                // `Reflect.ownKeys` answers `(string | symbol)[]`, so its
                // element type is the erased carrier -- see
                // `object_projection_call`.
                DictProjectionOp::OwnKeys => self.ctx.krate.types.intern(Type::List(unknown)),
                // Symbol keys come back as erased symbol VALUES, not descriptions,
                // so re-indexing the receiver with one still resolves to the
                // internal `__smelt_symbol:` key. See `object_projection_call`.
                DictProjectionOp::Symbols => self.ctx.krate.types.intern(Type::List(unknown)),
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

    /// Lower a `receiver.__proto__` *read* to the prototype accessor.
    ///
    /// `__proto__` is not an ordinary property: it is an accessor inherited from
    /// `Object.prototype` whose getter is `Object.getPrototypeOf(Object(this))`.
    /// Lowering it as a field read answered an own slot (usually absent, so
    /// `undefined`) instead of the prototype. It shares HIR with
    /// `Object.getPrototypeOf`, differing only in `own_slot_shadows`: a value
    /// with a `null` prototype does not inherit the accessor at all, so a
    /// `__proto__` write there really does create an own property that the read
    /// must answer first.
    ///
    /// Writes are excluded by the caller — `obj.__proto__ = v` keeps its existing
    /// store lowering, which is what makes the null-prototype own slot exist.
    fn proto_accessor_read(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        receiver: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let span = self.span(member.span.start, member.span.end);
        let unknown_ty = self.erase_at(ErasureReason::ByName, span, "__proto__");
        let value = if Self::expr_ty(body, receiver) == unknown_ty {
            receiver
        } else {
            body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: receiver,
                    target: unknown_ty,
                },
                ty: unknown_ty,
                span,
            })
        };
        Some(body.push_expr(Expr {
            kind: ExprKind::PrototypeSentinel {
                value,
                own_slot_shadows: true,
            },
            ty: unknown_ty,
            span,
        }))
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
        let ty = self.erase_with(ErasureReason::ByName, "Object.prototype");
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
        let value = smelt_stdlib::number_constant(member.property.name.as_str())?;
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
            "isSafeInteger" => NumericPredicateOp::IsSafeInteger,
            "isNaN" => NumericPredicateOp::IsNaN,
            _ => return None,
        };
        let span = self.span(member.span.start, member.span.end);
        // `Number.isSafeInteger` performs no ToNumber step, so its callable form
        // must accept an ERASED value and test the runtime tag. Typing the
        // parameter `number` would coerce at the call site and make
        // `['1'].every(Number.isSafeInteger)` answer `true`, the same defect the
        // direct-call path avoids.
        let number_ty = if matches!(op, NumericPredicateOp::IsSafeInteger) {
            self.erase_at(ErasureReason::RuntimeNarrowing, span, "Number.isSafeInteger")
        } else {
            self.ctx.krate.types.intern(Type::Float)
        };
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

    /// Lower the small Node `process` surface used by checked package probes.
    pub(in crate::lowering) fn node_process_static_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if Self::is_process_version_member(&member.object) {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(
                    smelt_stdlib::NODE_PROFILE_VERSION_STRING.to_owned(),
                )),
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        if Self::is_process_env_member(member) {
            // `process.env.X` is `string | undefined` in TypeScript, typed here
            // as `Optional(String)` so `process.env.PORT ?? 3000` keeps both
            // arms. A variable is read from the real environment: a program
            // that serves has to see its `PORT` (this used to fold every
            // variable to `undefined`). The timezone is the exception: the
            // profile fixes it, because the date runtime computes in that zone
            // whatever the host's `TZ` says, and a program must read the zone
            // its dates are actually in.
            let string_ty = self.ctx.krate.types.intern(Type::String);
            let span = self.span(member.span.start, member.span.end);
            let optional_ty =
                smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, string_ty);
            let kind = if Self::is_process_env_field(member, "TZ")
                || Self::is_process_env_field(member, "tz")
            {
                ExprKind::Literal(Literal::String(smelt_stdlib::NODE_PROFILE_TIMEZONE.to_owned()))
            } else {
                ExprKind::ProcessEnvRead {
                    name: member.property.name.as_str().to_owned(),
                }
            };
            return Some(body.push_expr(Expr {
                kind,
                ty: optional_ty,
                span,
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
    pub(in crate::lowering) fn is_process_env_member(
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
    ) -> bool {
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
        // A computed key that resolves to a STATIC member name is an ordinary
        // member read written with brackets, so it lowers to the same `Field` a
        // dot access would. This is what makes a symbol-keyed member usable:
        // `class C { get [KEY]() { .. } }` declares a member whose name is the
        // symbol's synthetic key, and `c[KEY]` has to find it. Without this the
        // declaration lowered and the read did not, so the program answered
        // `undefined` for a member that exists.
        //
        // Placed after the receiver and BEFORE the index so a decline costs no
        // stray lowered expression: the receiver is lowered once either way, and
        // the key expression is only lowered on the path that uses it.
        if let Some(expr) = self.static_computed_member_read(member, receiver, body)? {
            return Ok(expr);
        }
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
            return self.lower_negative_sequence_bracket_read(
                access_receiver_ty,
                member.span,
                body,
            );
        }
        // A TUPLE read through an `Optional` receiver. Its element is picked
        // statically, so the read is the ordinary tuple projection on the
        // present value: `x?.[i]` yields it when present and `undefined`
        // otherwise, while a plain `x[i]` is the source asserting presence
        // (`tsc` accepted it only because it believed `x` present; Smelt knows
        // more, e.g. `(m[a] || m[b]) as Tuple`), so it narrows and projects.
        let receiver = if optional_access
            && matches!(
                self.ctx.krate.types.get(receiver_ty),
                Some(Type::Optional(_))
            )
            && matches!(
                self.ctx.krate.types.get(access_receiver_ty),
                Some(Type::Tuple(_))
            ) {
            let present = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: receiver },
                ty: access_receiver_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
            if member.optional
                && let Some(Type::Tuple(items)) =
                    self.ctx.krate.types.get(access_receiver_ty).cloned()
            {
                let tuple_index = self.static_tuple_index(index, body, items.len(), member.span)?;
                let ty = items.get(tuple_index).copied().ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(member.span.start, member.span.end),
                        "tuple index is out of range",
                    )
                })?;
                let read = body.push_expr(Expr {
                    kind: ExprKind::TupleIndex {
                        tuple: present,
                        index: tuple_index,
                    },
                    ty,
                    span: self.span(member.span.start, member.span.end),
                });
                return Ok(self.wrap_optional_receiver_method(receiver, read, member.span, body));
            }
            present
        } else {
            receiver
        };
        let optional_access = optional_access && Self::expr_ty(body, receiver) == receiver_ty;
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
            let tuple_len = if self.allows_unknown_index_access() {
                usize::MAX
            } else {
                items.len()
            };
            let tuple_index = self.static_tuple_index(index, body, tuple_len, member.span)?;
            let Some(ty) = items.get(tuple_index).copied() else {
                if self.allows_unknown_index_access() {
                    let ty = self.erase_with(ErasureReason::DynamicOperator, "tuple index");
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
            let tuple_len = if self.allows_unknown_index_access() {
                usize::MAX
            } else {
                max_tuple_len
            };
            let tuple_index = self.static_tuple_index(index, body, tuple_len, member.span)?;
            let mut indexed_tys = Vec::new();
            // A tuple arm too short for the index reads `undefined` in
            // JavaScript (`[[h], stash] | [[h]]` read at `[1]` on the second
            // arm). Dropping that arm typed the read as the other arms' element
            // alone (`string[]`, never undefined), so `result[1] ? a : b` folded
            // to `a` and the one-element arm took the stash branch. The read is
            // that type OR `undefined`, which is `Optional`.
            let mut absent_in_an_arm = false;
            for item in items {
                if let Some(Type::Tuple(tuple_items)) = self.ctx.krate.types.get(item) {
                    match tuple_items.get(tuple_index).copied() {
                        Some(ty) if !indexed_tys.contains(&ty) => indexed_tys.push(ty),
                        Some(_) => {}
                        None => absent_in_an_arm = true,
                    }
                }
            }
            if !indexed_tys.is_empty() {
                let ty = match indexed_tys.as_slice() {
                    [single] => *single,
                    _ => self.ctx.krate.types.intern(Type::Union(indexed_tys)),
                };
                let ty = if absent_in_an_arm
                    && !matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_)))
                {
                    self.ctx.krate.types.intern(Type::Optional(ty))
                } else {
                    ty
                };
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Index { receiver, index },
                    ty,
                    span: self.span(member.span.start, member.span.end),
                }));
            }
        }
        if self.can_lower_acknowledged_unknown_index(access_receiver_ty, member.span.start) {
            let ty = self.callback_erase_unless_unknown(
                access_receiver_ty,
                ErasureReason::DynamicOperator,
                "computed index",
            );
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
        ) && matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Class { .. } | Type::Tuple(_))
        ) {
            // A missing key is `undefined`. For an OBJECT value (an instance, a
            // tuple) there is no falsy stand-in: a fabricated default would be
            // truthy and make `!record[k]` constantly false, so the read keeps
            // its absence as `Optional`. A plain `record[k][i]` on it is then a
            // presence assertion (see the tuple arm above).
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

    /// Lower `receiver[KEY]` as a member read when `KEY` names a static member.
    ///
    /// `KEY` is resolved by the same folding the class-member DECLARATION side
    /// uses (`resolve_static_computed_key_name_expr`), so a const-bound unique
    /// symbol, a well-known `Symbol.<name>` and a `Symbol.for(..)` registry
    /// symbol all name the member they declared. Answering `None` leaves the
    /// ordinary index lowering in charge, which is what every other key gets.
    ///
    /// The member must actually exist on the receiver's type: a folded key that
    /// names nothing is not a member read, and turning it into one would replace
    /// a runtime lookup with a static error.
    fn static_computed_member_read(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        receiver: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        // A type-only assertion is erased at run time: `(c.req as unknown)[KEY]`
        // reads `KEY` off the request itself. When the asserted value's class
        // declares the symbol member, the read is that member on the typed
        // value; the `unknown` was only there to get past the checker (Hono's
        // `matchedRoutes` behind a `@ts-expect-error`). Any other assertion
        // keeps the erased read.
        let receiver = self.asserted_class_value(receiver, body).unwrap_or(receiver);
        let receiver_ty = Self::expr_ty(body, receiver);
        if !matches!(
            self.ctx.krate.types.get(self.optional_receiver_inner_type(receiver_ty)),
            Some(Type::Class { .. })
        ) {
            return Ok(None);
        }
        // SYMBOL keys only. A string or numeric key resolves through the
        // ordinary index lowering, and it has to: a class with an index
        // signature (`[key: string]: T`) answers `class_field_type` for every
        // name, so folding `bag["a"]` into a named field read would take a
        // keyed-store read and turn it into a member that does not exist. A
        // symbol is never an index-signature key, so it is unambiguous -- and it
        // is the case that had no other path.
        let Some((name, true)) = self.resolve_static_computed_key_name_expr(&member.expression)
        else {
            return Ok(None);
        };
        let field = self.intern_source_name(&name);
        let access_receiver_ty = self.optional_receiver_inner_type(receiver_ty);
        let Ok(field_ty) = self.class_field_type(access_receiver_ty, field) else {
            return Ok(None);
        };
        let span = self.span(member.span.start, member.span.end);
        let optional_access = member.optional
            || matches!(
                self.ctx.krate.types.get(receiver_ty),
                Some(Type::Optional(_))
            );
        if optional_access {
            let ty = self.optional_chain_result_type(field_ty);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::OptionalField { receiver, field },
                ty,
                span,
            })));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Field { receiver, field },
            ty: field_ty,
            span,
        })))
    }

    /// The class-typed value under a type assertion to an erased type, or
    /// `None` when `expr` is not such an assertion.
    fn asserted_class_value(&self, expr: smelt_hir::ExprId, body: &Body) -> Option<smelt_hir::ExprId> {
        let node = body.expr_at(expr)?;
        let (ExprKind::TypeAssert { value } | ExprKind::UnknownCast { value, .. }) = node.kind else {
            return None;
        };
        if !matches!(self.ctx.krate.types.get(node.ty), Some(Type::Unknown)) {
            return None;
        }
        matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, value)),
            Some(Type::Class { .. })
        )
        .then_some(value)
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
        let receiver_ty = Self::expr_ty(body, receiver);
        if !matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. })
        ) {
            return Ok(None);
        }
        let index = self.expression(&member.expression, body)?;
        let ty = self.callback_erase_unless_unknown(
            receiver_ty,
            ErasureReason::DynamicOperator,
            "computed index",
        );
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
}
