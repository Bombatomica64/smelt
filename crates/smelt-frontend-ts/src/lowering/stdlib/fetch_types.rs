//! WHATWG fetch-type lowering for the TypeScript frontend.
//!
//! This concern owns the lowering of the fetch types that Smelt models as
//! *concrete* Rust values rather than tagged records. `Headers` is the first:
//! `new Headers(init)` becomes [`ExprKind::HeadersNew`] typed
//! `Type::Class { name: Headers }`, and every modeled method becomes an
//! [`ExprKind::HeadersOp`] whose HIR type is the method's exact source type —
//! `get` is `Optional(String)` because the source says `string | null`, `has` is
//! `Bool`, the projections are `List(String)` / `List((String, String))`.
//!
//! # Interface
//!
//! * [`Self::headers_constructor_expression`] is called from `new_expr.rs` when
//!   the constructor names `Headers` and no user class shadows it.
//! * [`Self::dispatch_headers_method`] is registered in the `call.rs` builtin
//!   handler chain; it recognizes receiver/member pairs through the shared
//!   `smelt-stdlib` method metadata (`TypeScriptReceiverKind::Headers`), never
//!   by matching a member name here.
//!
//! # Why the receiver type decides
//!
//! `get`/`set`/`has`/`entries` are also `Map` methods and ordinary user method
//! names, so recognition cannot key on the member alone. It keys on the
//! receiver's lowered type being the modeled `Headers` class, which is exactly
//! how the `Map`/`Set` collection dispatch works.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::Expression;
use oxc::span::GetSpan;
use smelt_hir::{Body, Expr, ExprKind, HeadersOp, ResponseOp, Type};
use smelt_stdlib::RuleId;

impl ModuleBuilder<'_> {
    /// Lower `new Headers(init?)` into a concrete `Headers` value.
    ///
    /// At most one initializer argument, as the source surface has; the
    /// initializer's own lowered type is what selects the conversion in
    /// codegen, so nothing about its shape is decided here.
    pub(in crate::lowering) fn headers_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Headers constructor supports at most one initializer",
            ));
        }
        let init = match new_expr.arguments.first() {
            Some(argument) => Some(self.argument(argument, body)?),
            None => None,
        };
        let ty = self.headers_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::HeadersNew { init },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Dispatch a modeled `Headers` method on a concrete `Headers` receiver.
    ///
    /// Registered in the builtin call-handler chain. Returns `Ok(None)` when the
    /// receiver is not a modeled `Headers` value, so an unrelated `get`/`set`
    /// call falls through to the ordinary paths.
    pub(in crate::lowering) fn dispatch_headers_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        let Some(rule) = smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::Headers,
            member_name,
        ) else {
            return Ok(None);
        };
        // The receiver is lowered before its type can be inspected, so bail out
        // on a receiver that cannot lower rather than reporting a `Headers`
        // diagnostic for an unrelated expression.
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        if !self.is_headers_type(receiver_ty) {
            return Ok(None);
        }
        let Some(op) = Self::headers_method_op(rule, member_name) else {
            return Ok(None);
        };
        let expected = Self::headers_op_arity(op);
        if call.arguments.len() < expected {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("`Headers.{member_name}` requires {expected} argument(s)"),
            ));
        }
        let args = call
            .arguments
            .iter()
            .take(expected)
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        let ty = self.headers_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::HeadersOp {
                op,
                headers: receiver,
                args,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Dispatch a modeled `URLSearchParams` method on a concrete receiver.
    ///
    /// Registered in the builtin call-handler chain next to the `Headers`
    /// dispatch, and recognized the same way: the shared registry names the
    /// receiver/member pairs, and the receiver's lowered type decides.
    pub(in crate::lowering) fn dispatch_url_search_params_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        let Some(rule) = smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::UrlSearchParams,
            member_name,
        ) else {
            return Ok(None);
        };
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        if !self.is_url_search_params_type(receiver_ty) {
            return Ok(None);
        }
        let Some(op) = Self::url_search_params_method_op(rule, member_name) else {
            return Ok(None);
        };
        let expected = Self::url_search_params_op_arity(op);
        if call.arguments.len() < expected {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("`URLSearchParams.{member_name}` requires {expected} argument(s)"),
            ));
        }
        let args = call
            .arguments
            .iter()
            .take(expected)
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        let ty = self.url_search_params_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UrlSearchParamsOp {
                op,
                params: receiver,
                args,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return the modeled `URLSearchParams` class type.
    pub(in crate::lowering) fn url_search_params_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("URLSearchParams");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return whether a lowered type is the modeled `URLSearchParams` class.
    pub(in crate::lowering) fn is_url_search_params_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::UrlSearchParams)
            && !self.user_class_shadows("URLSearchParams")
    }

    /// Return whether a source class in this module shadows a host class name.
    ///
    /// `contains` alone is not enough. A class is only PENDING while its own
    /// members are being lowered, so a `this.status` read inside a user
    /// `class Response` saw no registered class and was claimed by the modeled
    /// fetch type — the receiver's type genuinely *is* `Class { Response }`
    /// there, and only the shadowing check separates the two meanings. Both
    /// states are the same answer to "does the source own this name", so both
    /// belong in one predicate that every modeled fetch type reads.
    fn user_class_shadows(&self, name: &str) -> bool {
        self.classes.contains(name) || self.classes.is_pending(name)
    }

    /// Map a recognized rule and member spelling to its parameter operation.
    fn url_search_params_method_op(
        rule: RuleId,
        member: &str,
    ) -> Option<smelt_hir::UrlSearchParamsOp> {
        use smelt_hir::UrlSearchParamsOp as Op;
        match rule {
            RuleId::TsUrlSearchParamsRead => match member {
                "get" => Some(Op::Get),
                "getAll" => Some(Op::GetAll),
                "has" => Some(Op::Has),
                _ => None,
            },
            RuleId::TsUrlSearchParamsMutation => match member {
                "set" => Some(Op::Set),
                "append" => Some(Op::Append),
                "delete" => Some(Op::Delete),
                "sort" => Some(Op::Sort),
                _ => None,
            },
            RuleId::TsUrlSearchParamsProjection => match member {
                "keys" => Some(Op::Keys),
                "values" => Some(Op::Values),
                "entries" => Some(Op::Entries),
                _ => None,
            },
            RuleId::TsUrlSearchParamsToString => Some(Op::ToText),
            _ => None,
        }
    }

    /// Return how many source arguments an operation consumes.
    const fn url_search_params_op_arity(op: smelt_hir::UrlSearchParamsOp) -> usize {
        use smelt_hir::UrlSearchParamsOp as Op;
        match op {
            Op::Get | Op::GetAll | Op::Has | Op::Delete => 1,
            Op::Set | Op::Append => 2,
            Op::Sort | Op::ToText | Op::Keys | Op::Values | Op::Entries => 0,
        }
    }

    /// Return the exact source result type of a parameter operation.
    fn url_search_params_op_result_type(
        &mut self,
        op: smelt_hir::UrlSearchParamsOp,
    ) -> smelt_hir::TypeId {
        use smelt_hir::UrlSearchParamsOp as Op;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        match op {
            Op::Get => self.ctx.krate.types.intern(Type::Optional(string_ty)),
            Op::Has => self.ctx.krate.types.intern(Type::Bool),
            Op::ToText => string_ty,
            Op::Set | Op::Append | Op::Delete | Op::Sort => {
                self.ctx.krate.types.intern(Type::None)
            }
            Op::GetAll | Op::Keys | Op::Values => {
                self.ctx.krate.types.intern(Type::List(string_ty))
            }
            Op::Entries => {
                let pair_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(Vec::from([string_ty, string_ty])));
                self.ctx.krate.types.intern(Type::List(pair_ty))
            }
        }
    }

    /// Return whether a modeled host class defines its own `toString`.
    ///
    /// Consulted by the generic `.toString()` handler so a modeled fetch type
    /// keeps its own serialization. `Headers` has no `toString` in the spec, so
    /// only the parameter list answers `true` here.
    pub(in crate::lowering) fn type_defines_its_own_to_string(
        &self,
        ty: smelt_hir::TypeId,
    ) -> bool {
        self.is_url_search_params_type(ty)
    }

    /// Resolve a lowered type to its shared stdlib class identity, if any.
    pub(in crate::lowering) fn stdlib_class_of_type(
        &self,
        ty: smelt_hir::TypeId,
    ) -> Option<smelt_stdlib::StdlibClass> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty) else {
            return None;
        };
        let class_name = self
            .ctx
            .krate
            .names
            .get(*name)
            .or_else(|| self.ctx.krate.symbols.get(*name))?;
        smelt_stdlib::typescript_stdlib_class(class_name)
    }

    /// Return the modeled `Headers` class type.
    pub(in crate::lowering) fn headers_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("Headers");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return whether a lowered type is the modeled `Headers` class.
    pub(in crate::lowering) fn is_headers_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::Headers)
            && !self.user_class_shadows("Headers")
    }

    /// Map a recognized rule and member spelling to its header operation.
    ///
    /// The rule groups the surface (read / mutation / projection); the member
    /// name selects the operation inside the group. Both come from the shared
    /// recognition registry, so this mapping is total for the entries it
    /// declares and `None` for anything else.
    fn headers_method_op(rule: RuleId, member: &str) -> Option<HeadersOp> {
        match rule {
            RuleId::TsHeadersGet => Some(HeadersOp::Get),
            RuleId::TsHeadersHas => Some(HeadersOp::Has),
            RuleId::TsHeadersMutation => match member {
                "set" => Some(HeadersOp::Set),
                "append" => Some(HeadersOp::Append),
                "delete" => Some(HeadersOp::Delete),
                _ => None,
            },
            RuleId::TsHeadersProjection => match member {
                "keys" => Some(HeadersOp::Keys),
                "values" => Some(HeadersOp::Values),
                "entries" => Some(HeadersOp::Entries),
                "getSetCookie" => Some(HeadersOp::GetSetCookie),
                _ => None,
            },
            _ => None,
        }
    }

    /// Return how many source arguments an operation consumes.
    const fn headers_op_arity(op: HeadersOp) -> usize {
        match op {
            HeadersOp::Get | HeadersOp::Has | HeadersOp::Delete => 1,
            HeadersOp::Set | HeadersOp::Append => 2,
            HeadersOp::Keys
            | HeadersOp::Values
            | HeadersOp::Entries
            | HeadersOp::GetSetCookie => 0,
        }
    }

    /// Return the exact source result type of a header operation.
    ///
    /// These are the spec's own types, not approximations: `get` is
    /// `string | null` (an `Optional(String)`, so the caller narrows a real
    /// `Option`), the mutations are `void`, and the projections are lists.
    fn headers_op_result_type(&mut self, op: HeadersOp) -> smelt_hir::TypeId {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        match op {
            HeadersOp::Get => self.ctx.krate.types.intern(Type::Optional(string_ty)),
            HeadersOp::Has => self.ctx.krate.types.intern(Type::Bool),
            HeadersOp::Set | HeadersOp::Append | HeadersOp::Delete => {
                self.ctx.krate.types.intern(Type::None)
            }
            HeadersOp::Keys | HeadersOp::Values | HeadersOp::GetSetCookie => {
                self.ctx.krate.types.intern(Type::List(string_ty))
            }
            HeadersOp::Entries => {
                let pair_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(Vec::from([string_ty, string_ty])));
                self.ctx.krate.types.intern(Type::List(pair_ty))
            }
        }
    }
    /// Lower `new Response(body?, init?)` into a concrete `Response` value.
    ///
    /// The init argument is read as an OBJECT LITERAL and its `status`,
    /// `statusText` and `headers` keys become their own typed fields (see
    /// [`ExprKind::ResponseNew`]). A non-literal init (a `ResponseInit`
    /// variable) is a named blocker rather than an erased record: its keys have
    /// exact source types, and recovering them from a tagged value at run time
    /// would throw that away.
    pub(in crate::lowering) fn response_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Response constructor takes at most a body and an init",
            ));
        }
        let body_expr = match new_expr.arguments.first() {
            Some(argument) => Some(self.argument(argument, body)?),
            None => None,
        };
        let mut status = None;
        let mut status_text = None;
        let mut headers = None;
        if let Some(init_argument) = new_expr.arguments.get(1) {
            let Some(Expression::ObjectExpression(init)) = init_argument.as_expression() else {
                return Err(SmeltError::unsupported(
                    self.span(init_argument.span().start, init_argument.span().end),
                    "Response init must be an object literal so its keys keep their types",
                ));
            };
            for property in &init.properties {
                let oxc::ast::ast::ObjectPropertyKind::ObjectProperty(property) = property else {
                    return Err(SmeltError::unsupported(
                        self.span(init.span.start, init.span.end),
                        "Response init does not support spread properties yet",
                    ));
                };
                let Some(key) = property.key.static_name() else {
                    return Err(SmeltError::unsupported(
                        self.span(init.span.start, init.span.end),
                        "Response init requires statically named keys",
                    ));
                };
                let value = self.expression(&property.value, body)?;
                match key.as_ref() {
                    "status" => status = Some(value),
                    "statusText" => status_text = Some(value),
                    "headers" => headers = Some(value),
                    other => {
                        return Err(SmeltError::unsupported(
                            self.span(init.span.start, init.span.end),
                            format!("Response init key `{other}` is not modeled yet"),
                        ));
                    }
                }
            }
        }
        let ty = self.response_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::ResponseNew {
                body: body_expr,
                status,
                status_text,
                headers,
            },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Dispatch a modeled `Response` method on a concrete `Response` receiver.
    ///
    /// Registered in the builtin call-handler chain beside the `Headers` and
    /// `URLSearchParams` dispatches, and recognized the same way: the shared
    /// registry names the receiver/member pairs and the receiver's lowered type
    /// decides, so an unrelated `text()` or `clone()` falls through.
    pub(in crate::lowering) fn dispatch_response_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        let Some(rule) = smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::Response,
            member_name,
        ) else {
            return Ok(None);
        };
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        if !self.is_response_type(receiver_ty) {
            return Ok(None);
        }
        let op = match rule {
            RuleId::TsResponseBodyRead => ResponseOp::Text,
            RuleId::TsResponseClone => ResponseOp::Clone,
            _ => return Ok(None),
        };
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("`Response.{member_name}` takes no arguments"),
            ));
        }
        let ty = self.response_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ResponseOp {
                op,
                response: receiver,
                args: Vec::new(),
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower a `Response` data-property read on a concrete receiver.
    ///
    /// `status`/`ok`/`statusText`/`headers`/`bodyUsed` are properties in the
    /// source but operations on a concrete receiver here, which is why they
    /// share [`ResponseOp`] with the methods rather than going through the
    /// generic field-read path — there is no struct field to read; the value is
    /// computed by the runtime type (`ok` is derived from `status`).
    pub(in crate::lowering) fn response_property_read(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let op = match member.property.name.as_str() {
            "status" => ResponseOp::Status,
            "ok" => ResponseOp::Ok,
            "statusText" => ResponseOp::StatusText,
            "headers" => ResponseOp::Headers,
            "bodyUsed" => ResponseOp::BodyUsed,
            _ => return Ok(None),
        };
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        if !self.is_response_type(receiver_ty) {
            return Ok(None);
        }
        let ty = self.response_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ResponseOp {
                op,
                response: receiver,
                args: Vec::new(),
            },
            ty,
            span: self.span(member.span.start, member.span.end),
        })))
    }

    /// Return the modeled `Response` class type.
    pub(in crate::lowering) fn response_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("Response");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return whether a lowered type is the modeled `Response` class.
    pub(in crate::lowering) fn is_response_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::Response)
            && !self.user_class_shadows("Response")
    }

    /// The HIR type a `Response` operation answers.
    ///
    /// Each is the member's exact source type, so no caller has to re-narrow:
    /// `status` is `number`, `ok`/`bodyUsed` are `boolean`, `statusText` is
    /// `string`, `headers` is a `Headers`, `clone()` is a `Response`, and
    /// `text()` is a `Promise<string>` — a future, because it is `async`.
    fn response_op_result_type(&mut self, op: ResponseOp) -> smelt_hir::TypeId {
        match op {
            ResponseOp::Status => self.ctx.krate.types.intern(Type::Float),
            ResponseOp::Ok | ResponseOp::BodyUsed => self.ctx.krate.types.intern(Type::Bool),
            ResponseOp::StatusText => self.ctx.krate.types.intern(Type::String),
            ResponseOp::Headers => self.headers_type(),
            ResponseOp::Clone => self.response_type(),
            ResponseOp::Text => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                self.ctx.krate.types.intern(Type::Future(string_ty))
            }
        }
    }

}
