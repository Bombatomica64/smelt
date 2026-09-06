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
use smelt_hir::{Body, EventEmitterOp, Expr, ExprKind, HeadersOp, RequestOp, ResponseOp, Type};
use smelt_stdlib::RuleId;


/// The `ResponseInit` keys Smelt models.
const RESPONSE_INIT_KEYS: &[&str] = &["status", "statusText", "headers"];

/// The `RequestInit` keys Smelt models.
const REQUEST_INIT_KEYS: &[&str] = &["method", "headers", "body"];

/// Per-key init operands collected from a literal, a spread, or a typed value.
///
/// A key set twice keeps the LAST value, which is what an object literal does
/// (`{ ...init, status: 201 }` takes 201 even when `init` has a status).
#[derive(Default)]
struct InitFields {
    entries: Vec<(String, smelt_hir::ExprId)>,
}

impl InitFields {
    /// Record `key`'s operand, replacing an earlier one.
    fn set(&mut self, key: &str, value: smelt_hir::ExprId) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|(existing, _)| existing == key)
        {
            entry.1 = value;
            return;
        }
        self.entries.push((key.to_owned(), value));
    }

    /// Take `key`'s operand, when the init supplied one.
    fn take(&self, key: &str) -> Option<smelt_hir::ExprId> {
        self.entries
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| *value)
    }
}

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
    ///
    /// Shared with the `node:http` classes, which are shadowed by the same
    /// rule: a program with its own `Server` keeps it.
    pub(in crate::lowering) fn user_class_shadows(&self, name: &str) -> bool {
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
        let mut fields = InitFields::default();
        if let Some(init_argument) = new_expr.arguments.get(1) {
            self.lower_fetch_init(init_argument, RESPONSE_INIT_KEYS, "Response", &mut fields, body)?;
        }
        let (status, status_text, headers) = (
            fields.take("status"),
            fields.take("statusText"),
            fields.take("headers"),
        );
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

    /// Lower `new Request(input, init?)` into a concrete `Request` value.
    ///
    /// Same init handling as [`Self::response_constructor_expression`]: the
    /// literal's keys become typed fields, and a non-literal init is a named
    /// blocker rather than an erased record.
    pub(in crate::lowering) fn request_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 2 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Request constructor takes at most an input and an init",
            ));
        }
        let Some(input_argument) = new_expr.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Request constructor requires a URL argument",
            ));
        };
        let input = self.argument(input_argument, body)?;
        let mut fields = InitFields::default();
        if let Some(init_argument) = new_expr.arguments.get(1) {
            self.lower_fetch_init(init_argument, REQUEST_INIT_KEYS, "Request", &mut fields, body)?;
        }
        let (method, headers, body_expr) = (
            fields.take("method"),
            fields.take("headers"),
            fields.take("body"),
        );
        let ty = self.request_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::RequestNew {
                input,
                method,
                headers,
                body: body_expr,
            },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Build the `Request` that `fetch(url, init)` is defined to fetch.
    ///
    /// The spec defines `fetch(input, init)` as fetching `new Request(input,
    /// init)`, so this is that constructor with the same init reader and the
    /// same key set rather than a second, parallel one — an init key that the
    /// `Request` constructor blocks is blocked here too, by construction.
    pub(in crate::lowering) fn fetch_request_from_init(
        &mut self,
        input: smelt_hir::ExprId,
        init_argument: &oxc::ast::ast::Argument<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(init_argument.span().start, init_argument.span().end);
        let mut fields = InitFields::default();
        self.lower_fetch_init(init_argument, REQUEST_INIT_KEYS, "fetch", &mut fields, body)?;
        let (method, headers, body_expr) = (
            fields.take("method"),
            fields.take("headers"),
            fields.take("body"),
        );
        let ty = self.request_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::RequestNew {
                input,
                method,
                headers,
                body: body_expr,
            },
            ty,
            span,
        }))
    }

    /// Dispatch a modeled `Request` method on a concrete `Request` receiver.
    pub(in crate::lowering) fn dispatch_request_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        let Some(rule) = smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::Request,
            member_name,
        ) else {
            return Ok(None);
        };
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        if !self.is_request_type(receiver_ty) {
            return Ok(None);
        }
        let op = match rule {
            RuleId::TsRequestBodyRead => RequestOp::Text,
            RuleId::TsRequestClone => RequestOp::Clone,
            _ => return Ok(None),
        };
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("`Request.{member_name}` takes no arguments"),
            ));
        }
        let ty = self.request_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RequestOp {
                op,
                request: receiver,
                args: Vec::new(),
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower a `Request` data-property read on a concrete receiver.
    ///
    /// `url` being typed `string` is what unblocks `request.url.indexOf(':')`
    /// — an untyped read made that a "string search methods require a string
    /// receiver" error (`blocker-logs/hono-fetch-demand.md` item 6).
    pub(in crate::lowering) fn request_property_read(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let op = match member.property.name.as_str() {
            "url" => RequestOp::Url,
            "method" => RequestOp::Method,
            "headers" => RequestOp::Headers,
            "bodyUsed" => RequestOp::BodyUsed,
            _ => return Ok(None),
        };
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        if !self.is_request_type(receiver_ty) {
            return Ok(None);
        }
        let ty = self.request_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RequestOp {
                op,
                request: receiver,
                args: Vec::new(),
            },
            ty,
            span: self.span(member.span.start, member.span.end),
        })))
    }

    /// Return the modeled `Request` class type.
    pub(in crate::lowering) fn request_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("Request");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return whether a lowered type is the modeled `Request` class.
    pub(in crate::lowering) fn is_request_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::Request)
            && !self.user_class_shadows("Request")
    }

    /// The HIR type a `Request` operation answers.
    fn request_op_result_type(&mut self, op: RequestOp) -> smelt_hir::TypeId {
        match op {
            RequestOp::Url | RequestOp::Method => self.ctx.krate.types.intern(Type::String),
            RequestOp::BodyUsed => self.ctx.krate.types.intern(Type::Bool),
            RequestOp::Headers => self.headers_type(),
            RequestOp::Clone => self.request_type(),
            RequestOp::Text => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                self.ctx.krate.types.intern(Type::Future(string_ty))
            }
        }
    }

    /// Lower a `Response`/`Request` init argument into per-key operands.
    ///
    /// Three sources, one result. What matters is whether the key's value can
    /// be reached with its type intact, not how the source spelled it:
    ///
    /// * an **object literal** — each key's value is lowered directly;
    /// * a **spread** inside a literal (`{ ...init, status: 201 }`) — the
    ///   spread source is read by field, then later keys overwrite, which is
    ///   the object-literal evaluation order the source has;
    /// * a **typed value** (an `init: ResponseInit` parameter, a variable, a
    ///   field) — each modeled key is an ordinary typed field read on it.
    ///
    /// Only a genuinely erased init is a blocker: an `unknown`/`any` value has
    /// no declared keys to read, and inventing them at run time is exactly the
    /// tagged-record path modeling these types exists to avoid.
    ///
    /// `keys` is the modeled key set, so an unmodeled key is named rather than
    /// silently dropped — `redirect` and `signal` change what a request does.
    fn lower_fetch_init(
        &mut self,
        init_argument: &oxc::ast::ast::Argument<'_>,
        keys: &[&str],
        type_name: &str,
        fields: &mut InitFields,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let span = self.span(init_argument.span().start, init_argument.span().end);
        let Some(init) = init_argument.as_expression() else {
            return Err(SmeltError::unsupported(
                span,
                format!("{type_name} init must be a value, not a spread argument"),
            ));
        };
        if let Expression::ObjectExpression(literal) = init {
            for property in &literal.properties {
                match property {
                    oxc::ast::ast::ObjectPropertyKind::ObjectProperty(property) => {
                        let Some(key) = property.key.static_name() else {
                            return Err(SmeltError::unsupported(
                                span,
                                format!("{type_name} init requires statically named keys"),
                            ));
                        };
                        if !keys.contains(&key.as_ref()) {
                            return Err(SmeltError::unsupported(
                                span,
                                format!("{type_name} init key `{key}` is not modeled yet"),
                            ));
                        }
                        let value = self.expression(&property.value, body)?;
                        fields.set(&key, value);
                    }
                    oxc::ast::ast::ObjectPropertyKind::SpreadProperty(spread) => {
                        // `{ ...init, status: 201 }`: read the spread source by
                        // field, in place, so a later key overwrites it exactly
                        // as the source's evaluation order says.
                        let source = self.expression(&spread.argument, body)?;
                        self.spread_init_fields(source, keys, type_name, span, fields, body)?;
                    }
                }
            }
            return Ok(());
        }
        let source = self.expression(init, body)?;
        self.spread_init_fields(source, keys, type_name, span, fields, body)
    }

    /// Read every modeled key off a typed init value as a typed field read.
    ///
    /// A key the type does not declare is simply absent — an init interface's
    /// keys are all optional, so a `RequestInit` without `body` is not an
    /// error. A receiver with no declared keys at all is the erased case and is
    /// a blocker, because then nothing could be read with its type intact.
    fn spread_init_fields(
        &mut self,
        source: smelt_hir::ExprId,
        keys: &[&str],
        type_name: &str,
        span: smelt_hir::Span,
        fields: &mut InitFields,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let source_ty = Self::expr_ty(body, source);
        // The spec allows a `Request` at the init position: its method, headers
        // and body are copied into the new request. Its members are modeled
        // operations rather than struct fields, so they are read through those
        // rather than through the field path below.
        if self.is_request_type(source_ty) {
            return self.request_init_fields(source, keys, span, fields, body);
        }
        // Symmetrically, a `Response` is allowed at the `ResponseInit` position:
        // WebIDL converts the object to the dictionary by reading its own
        // `status`, `statusText` and `headers` properties.
        if self.is_response_type(source_ty) {
            return self.response_init_fields(source, keys, span, fields, body);
        }
        if matches!(self.ctx.krate.types.get(source_ty), Some(Type::Unknown)) {
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "{type_name} init is an erased value, so its keys cannot be read with their types"
                ),
            ));
        }
        // An ambient init interface (`ResponseInit`/`RequestInit`) has no
        // runtime representation, so the value arrives as an erased record and
        // each key is read through the checked cast below. A source-declared
        // interface is a real struct, so its keys are read directly.
        // Whether the init's keys are read directly or through the checked cast
        // depends on ONE thing: does the crate emit a struct for this type? A
        // source class or interface does, so its fields are real Rust fields. An
        // ambient interface (`ResponseInit`) and a type ALIAS
        // (`RequiredRequestInit = Required<Omit<RequestInit, ..>>`) do not, so a
        // value of either arrives as an erased record and a typed read of it
        // would claim a shape the runtime value does not have.
        let ambient = matches!(self.ctx.krate.types.get(source_ty), Some(Type::Class { name, .. })
            if self.erased_init_receiver(*name));
        let mut read_any = false;
        for key in keys {
            let field = self.intern_source_name(key);
            let Ok(field_ty) = self.class_field_type(source_ty, field) else {
                continue;
            };
            // An init interface may declare a key generically -- Hono's own
            // `interface ResponseInit<T extends StatusCode>` has `status?: T`,
            // where `StatusCode` is a numeric literal union. The key's type is
            // then a type PARAMETER, which carries no runtime representation,
            // so it resolves through its constraint: the bound is what the
            // source guarantees about every instantiation, and it is what the
            // construction site needs (a number, here).
            let field_ty = self.resolve_init_key_constraint(field_ty);
            // A key the type does not declare resolves to an erased type
            // rather than failing — bare `Unknown`, or `Optional<Unknown>` when
            // the read went through the optional-field path. Reading it would
            // put an erased value where a typed one belongs, so it is skipped
            // and the key falls back to the spec's default, which is what an
            // init that does not declare the key means.
            let erased = match self.ctx.krate.types.get(field_ty) {
                Some(Type::Unknown) => true,
                Some(&Type::Optional(inner)) => {
                    matches!(self.ctx.krate.types.get(inner), Some(Type::Unknown))
                }
                _ => false,
            };
            if erased {
                continue;
            }
            read_any = true;
            let read = if ambient {
                // **Dynamic boundary.** The receiver is an erased record, so
                // the key read yields an erased value and the cast is what
                // recovers the key's declared type. A concrete type cannot
                // stand in for the receiver: the interface is not in the crate,
                // so there is no struct to read a field from — the shape is
                // only present in the record the caller's literal became.
                let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                let erased = body.push_expr(Expr {
                    kind: ExprKind::Field {
                        receiver: source,
                        field,
                    },
                    ty: unknown_ty,
                    span,
                });
                body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: erased,
                        target: field_ty,
                    },
                    ty: field_ty,
                    span,
                })
            } else {
                body.push_expr(Expr {
                    kind: ExprKind::Field {
                        receiver: source,
                        field,
                    },
                    ty: field_ty,
                    span,
                })
            };
            fields.set(key, read);
        }
        if !read_any {
            return Err(SmeltError::unsupported(
                span,
                format!("{type_name} init type declares none of its modeled keys"),
            ));
        }
        Ok(())
    }

    /// Read the modeled init keys off a `Response` used as an init.
    ///
    /// `new Response(body, source)` where `source` is a `Response` copies the
    /// source's status, statusText and headers. The dictionary conversion reads
    /// exactly those three properties, so this is the same read the caller could
    /// have written by hand as `{ status: src.status, statusText:
    /// src.statusText, headers: src.headers }` — and it goes through the same
    /// `ResponseOp` reads, so nothing about it is a special case.
    ///
    /// Nothing is SHARED with the source, which is the part worth stating
    /// because the `Request` case above deliberately shares its body handle.
    /// Two differences, both diffed against Node 22:
    ///
    /// * the init's BODY is not taken. `new Response(null, src)` reads as the
    ///   empty string and leaves `src.bodyUsed` false, because `ResponseInit`
    ///   has no body member at all — the body comes only from the first
    ///   argument.
    /// * the headers are COPIED, not aliased: after
    ///   `const dst = new Response(null, src)`, a later `src.headers.set(..)`
    ///   is invisible on `dst`. That falls out of passing the source's header
    ///   list through the ordinary `headers` init key, which the constructor
    ///   already fills by copying entries.
    ///
    /// `keys` is still consulted, so this cannot supply a key the caller's type
    /// does not model.
    fn response_init_fields(
        &mut self,
        source: smelt_hir::ExprId,
        keys: &[&str],
        span: smelt_hir::Span,
        fields: &mut InitFields,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        for key in keys {
            let op = match *key {
                "status" => ResponseOp::Status,
                "statusText" => ResponseOp::StatusText,
                "headers" => ResponseOp::Headers,
                _ => continue,
            };
            let ty = self.response_op_result_type(op);
            let read = body.push_expr(Expr {
                kind: ExprKind::ResponseOp {
                    op,
                    response: source,
                    args: Vec::new(),
                },
                ty,
                span,
            });
            fields.set(key, read);
        }
        Ok(())
    }

    /// Read the modeled init keys off a `Request` used as an init.
    ///
    /// `new Request(url, source)` copies the source's method, headers and body
    /// (WHATWG "new request" steps). The body is passed as the source request
    /// itself so the emitter's body conversion can take its handle: sharing the
    /// handle is what makes reading the new request's body mark the SOURCE used,
    /// which is what Node does — `src.bodyUsed` becomes `true` and a later
    /// `src.text()` throws `Body is unusable`.
    ///
    /// `keys` is still consulted, so this cannot supply a key the caller's type
    /// does not model.
    fn request_init_fields(
        &mut self,
        source: smelt_hir::ExprId,
        keys: &[&str],
        span: smelt_hir::Span,
        fields: &mut InitFields,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        for key in keys {
            let op = match *key {
                "method" => RequestOp::Method,
                "headers" => RequestOp::Headers,
                // The body is the source value itself; the emitter selects the
                // handle conversion from its `Request` type.
                "body" => {
                    fields.set(key, source);
                    continue;
                }
                _ => continue,
            };
            let ty = self.request_op_result_type(op);
            let read = body.push_expr(Expr {
                kind: ExprKind::RequestOp {
                    op,
                    request: source,
                    args: Vec::new(),
                },
                ty,
                span,
            });
            fields.set(key, read);
        }
        Ok(())
    }

    /// Resolve an init key's type through a type parameter's constraint.
    ///
    /// A key declared `status?: T` where `T extends StatusCode` arrives as
    /// `Optional<T>`. The parameter itself has no runtime shape; its constraint
    /// does, and the constraint is exactly what the source promises about every
    /// instantiation. An unconstrained parameter is left alone, so it stays
    /// erased and the caller's erasure check rejects it rather than a wrong
    /// concrete type being invented.
    fn resolve_init_key_constraint(&mut self, ty: smelt_hir::TypeId) -> smelt_hir::TypeId {
        match self.ctx.krate.types.get(ty) {
            Some(Type::TypeParam { .. }) => self.type_param_constraint_or_self(ty),
            Some(&Type::Optional(inner))
                if matches!(
                    self.ctx.krate.types.get(inner),
                    Some(Type::TypeParam { .. })
                ) =>
            {
                let resolved = self.type_param_constraint_or_self(inner);
                if resolved == inner {
                    return ty;
                }
                self.ctx.krate.types.intern(Type::Optional(resolved))
            }
            _ => ty,
        }
    }

    /// Return whether an init receiver's type emits no struct.
    ///
    /// Such a value crosses as an erased record, so its keys are read through
    /// the checked cast rather than as struct fields. The test is structural --
    /// "is there a class or interface item for this name" -- rather than a list
    /// of names, so an alias over a utility type is covered by the same rule
    /// that covers the ambient interfaces.
    fn erased_init_receiver(&self, name: smelt_hir::Symbol) -> bool {
        self.class_by_symbol(name).is_none() && self.find_interface(name).is_none()
    }

    /// Lower `new EventEmitter()` into a concrete emitter.
    ///
    /// The constructor's only option is `{ captureRejections }`, which changes
    /// how a rejected promise returned by a listener is reported. Nothing here
    /// can observe that yet, so passing it is a named blocker rather than an
    /// argument that is accepted and ignored.
    pub(in crate::lowering) fn event_emitter_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if !new_expr.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "EventEmitter options are not modeled yet",
            ));
        }
        let ty = self.event_emitter_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::EventEmitterNew,
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Dispatch a modeled `EventEmitter` method on a concrete receiver.
    pub(in crate::lowering) fn dispatch_event_emitter_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        if smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::EventEmitter,
            member_name,
        )
        .is_none()
        {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        if !self.is_event_emitter_type(receiver_ty) {
            return Ok(None);
        }
        // The member spelling picks the operation: `on` and `addListener` are
        // the same operation under two names, as are `off` and
        // `removeListener`.
        let op = match member_name {
            "on" | "addListener" => EventEmitterOp::On,
            "once" => EventEmitterOp::Once,
            "off" | "removeListener" => EventEmitterOp::Off,
            "removeAllListeners" => EventEmitterOp::RemoveAll,
            "emit" => EventEmitterOp::Emit,
            "listenerCount" => EventEmitterOp::ListenerCount,
            _ => return Ok(None),
        };
        let required = match op {
            EventEmitterOp::On | EventEmitterOp::Once | EventEmitterOp::Off => 2,
            _ => 1,
        };
        if call.arguments.len() < required {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("`EventEmitter.{member_name}` requires {required} argument(s)"),
            ));
        }
        // `emit` forwards its whole tail to every listener, so every argument is
        // lowered; the others take exactly the arguments they model.
        let taken = if matches!(op, EventEmitterOp::Emit) {
            call.arguments.len()
        } else {
            required
        };
        // A modeled receiver publishes a schema for its own events, so the
        // listener can be lowered at a REAL signature instead of an erased one.
        // `req.on('data', (chunk) => ..)` types `chunk` as a string, which is
        // what `node:http` says it carries; only the registration adapter still
        // erases, and that adapter is the emitter's documented boundary either
        // way. Without this the source's own closure took a `SmeltUnknown` and
        // coerced it by hand at every use — avoidable erasure inside program
        // code, which is precisely what the metric exists to remove.
        let listener_hint = self.modeled_listener_hint(op, receiver_ty, call);
        let args = call
            .arguments
            .iter()
            .take(taken)
            .enumerate()
            .map(|(index, argument)| match listener_hint {
                Some(hint) if index == 1 => self.argument_with_hint(argument, body, Some(hint)),
                _ => self.argument(argument, body),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ty = self.event_emitter_op_result_type(op, receiver_ty);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::EventEmitterOp {
                op,
                emitter: receiver,
                args,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// The listener signature a modeled receiver's event schema publishes.
    ///
    /// Answers `None` — leaving the listener erased, as it has always been —
    /// unless all three of these hold: the operation registers or removes a
    /// listener, the receiver is a modeled class rather than a plain
    /// `EventEmitter`, and the event name is a literal the class publishes a
    /// schema for. Anything else is the open case the emitter's erased store
    /// exists for: a name computed at run time cannot select a signature.
    fn modeled_listener_hint(
        &mut self,
        op: EventEmitterOp,
        receiver_ty: smelt_hir::TypeId,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Option<smelt_hir::TypeId> {
        if !matches!(
            op,
            EventEmitterOp::On | EventEmitterOp::Once | EventEmitterOp::Off
        ) {
            return None;
        }
        let class = self.stdlib_class_of_type(receiver_ty)?;
        let Some(oxc::ast::ast::Argument::StringLiteral(event)) = call.arguments.first() else {
            return None;
        };
        let params = class.event_listener_string_params(event.value.as_str())?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let none_ty = self.ctx.krate.types.intern(Type::None);
        Some(
            self.ctx
                .krate
                .types
                .intern(Type::Function(smelt_hir::FunctionType {
                    params: vec![string_ty; params],
                    rest: None,
                    required_params: Some(params),
                    mutable_params: Vec::new(),
                    return_ty: none_ty,
                    is_async: false,
                    // A listener that throws leaves through the emitting call,
                    // which is what `statement_can_throw` already reports for
                    // `emit`; the signature has to agree with that.
                    may_throw: true,
                })),
        )
    }

    /// Return the modeled `EventEmitter` class type.
    pub(in crate::lowering) fn event_emitter_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("EventEmitter");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return whether a lowered type HAS a `node:events` listener list.
    ///
    /// Not "is the `EventEmitter` class". Node's `IncomingMessage` extends
    /// `EventEmitter`, and `req.on('data', ..)` is how a request body is read,
    /// so the test that gates the emitter operations has to admit every class
    /// that carries a listener list. The registry answers which those are
    /// (`StdlibClass::has_event_emitter`), and the generated types make the
    /// answer true by COMPOSITION — a `SmeltIncomingMessage` holds a
    /// `SmeltEventEmitter` and forwards the same five methods to it — so one
    /// listener-list implementation serves both receivers and they cannot drift
    /// apart on ordering or on `once` removal.
    ///
    /// The shadow test follows the class the type actually names, so a user
    /// class called `EventEmitter` still shadows the emitter without affecting
    /// `IncomingMessage`, and vice versa.
    pub(in crate::lowering) fn is_event_emitter_type(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(class) = self.stdlib_class_of_type(ty) else {
            return false;
        };
        if !class.has_event_emitter() {
            return false;
        }
        match class {
            smelt_stdlib::StdlibClass::EventEmitter => !self.user_class_shadows("EventEmitter"),
            smelt_stdlib::StdlibClass::IncomingMessage => {
                !self.user_class_shadows("IncomingMessage")
            }
            _ => false,
        }
    }

    /// The HIR type an `EventEmitter` operation answers.
    ///
    /// Every registration and removal answers the RECEIVER, which is what makes
    /// `e.on(..).on(..)` chain as it does in Node. The receiver, not the
    /// emitter class: `req.on('data', ..)` answers the request in Node, so a
    /// chained `req.on(..).on(..)` must keep the `IncomingMessage` type or the
    /// second call would look for `on` on an emitter that has no `url`.
    /// `emit` answers whether a listener ran, and `listenerCount` a number.
    fn event_emitter_op_result_type(
        &mut self,
        op: EventEmitterOp,
        receiver_ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        match op {
            EventEmitterOp::On
            | EventEmitterOp::Once
            | EventEmitterOp::Off
            | EventEmitterOp::RemoveAll => receiver_ty,
            EventEmitterOp::Emit => self.ctx.krate.types.intern(Type::Bool),
            EventEmitterOp::ListenerCount => self.ctx.krate.types.intern(Type::Float),
        }
    }

}
