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
//! * [`Self::dispatch_headers_method`] is registered in the `stdlib/call_dispatch.rs` builtin
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

use crate::lowering::erasure::NotNewErasure;
use crate::lowering::support::source_spelling;
use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::js_text::static_name_text;
use oxc::ast::ast::Expression;
use oxc::span::GetSpan;
use smelt_hir::{
    AsyncOp, Body, ErasureReason, Expr, ExprKind, HeadersOp, RequestInitMember, RequestOp,
    ResponseOp, Type,
};
use smelt_stdlib::{FetchInitKey, REQUEST_INIT_KEYS, RESPONSE_INIT_KEYS, RuleId};

/// The spellings of a fetch init key table, in table order.
fn init_key_names(table: &[FetchInitKey]) -> Vec<&'static str> {
    table.iter().map(|(name, _)| *name).collect()
}

/// Collect the stored `RequestInit` members an init supplied, in table order.
fn taken_request_init_members(fields: &InitFields) -> Vec<(RequestInitMember, smelt_hir::ExprId)> {
    RequestInitMember::ALL
        .into_iter()
        .filter_map(|member| {
            fields
                .take(member.property_name())
                .map(|value| (member, value))
        })
        .collect()
}

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
    pub(in crate::lowering) fn take(&self, key: &str) -> Option<smelt_hir::ExprId> {
        self.entries
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| *value)
    }
}

/// The global name that rebinding in source hides modeled class `class`, or `None`
/// when no source declaration can stand for the class.
const fn modeled_class_global(class: smelt_stdlib::StdlibClass) -> Option<&'static str> {
    use smelt_stdlib::StdlibClass;
    match class {
        StdlibClass::ArrayBuffer => Some("ArrayBuffer"),
        StdlibClass::DataView => Some("DataView"),
        StdlibClass::FormData => Some("FormData"),
        StdlibClass::Headers => Some("Headers"),
        StdlibClass::HttpServer => Some("Server"),
        StdlibClass::IncomingMessage => Some("IncomingMessage"),
        StdlibClass::NodeHash => Some("Hash"),
        StdlibClass::Request => Some("Request"),
        StdlibClass::Response => Some("Response"),
        StdlibClass::ServerResponse => Some("ServerResponse"),
        StdlibClass::TextDecoder => Some("TextDecoder"),
        StdlibClass::TextEncoder => Some("TextEncoder"),
        StdlibClass::Url => Some("URL"),
        StdlibClass::UrlSearchParams => Some("URLSearchParams"),
        _ => None,
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
        let Some(receiver) = self.dispatched_receiver(&member.object, body)? else {
            return Ok(None);
        };
        // A modeled method call on an OPTIONAL receiver is a call on the inner
        // value: `tsc` only accepts `maybe.set(k, v)` where it has already
        // narrowed `maybe` to non-nullish, so the optional surface here is one
        // Smelt's own flow typing did not drop. Asserting presence is what the
        // modeled PROPERTY reads already do (`present_receiver`); without it
        // the call fell through to a generic optional member read and emitted
        // `receiver.as_ref().map(|value| value.set.clone())`, which takes a
        // METHOD as a field (E0615).
        let (receiver, receiver_ty) =
            self.present_receiver(receiver, self.span(call.span.start, call.span.end), body);
        if !self.is_modeled_class(receiver_ty, smelt_stdlib::StdlibClass::Headers) {
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
        let Some(receiver) = self.dispatched_receiver(&member.object, body)? else {
            return Ok(None);
        };
        // Same presence assertion as the `Headers` dispatch above, for the same
        // reason: the two are one rule about modeled receivers.
        let (receiver, receiver_ty) =
            self.present_receiver(receiver, self.span(call.span.start, call.span.end), body);
        if !self.is_modeled_class(receiver_ty, smelt_stdlib::StdlibClass::UrlSearchParams) {
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

    /// Whether this module's own source binds a global type or class `name`.
    ///
    /// The one definition of shadowing: a declaration of this module, at any
    /// stage of lowering, or an import of the name, type-only or value, from
    /// anything but a modeled host module: `import type {
    /// Map } from './game'` is the game's `Map`). A class another module
    /// declares is visible crate-wide by symbol, and must not capture this
    /// file's `Response`; an import of a host module's class is that host
    /// class ([`Self::import_shadows_lib`]).
    pub(in crate::lowering) fn source_shadows_global(&self, name: &str) -> bool {
        self.types.module_declares_type_name(name)
            || self.classes.declared_in_module(name)
            || self.import_shadows_lib(name)
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
        self.is_modeled_class(ty, smelt_stdlib::StdlibClass::UrlSearchParams)
    }

    /// Resolve a call receiver's modeled class WITHOUT lowering it.
    ///
    /// The builtin call-handler chain is a flat list of independent handlers,
    /// each of which lowers the receiver to decide whether it owns the call. That
    /// is harmless while every handler's member names are distinct, and wrong as
    /// soon as two modeled receivers share one: `text()` is a member of `Blob`,
    /// `Response` AND `Request`, so whichever handler runs first lowers the
    /// receiver, answers `None`, and leaves a duplicate expression behind — which
    /// double-evaluates a receiver with side effects, and which the
    /// `44_node_http_echo` HIR golden caught the moment `Blob` gained a `text`.
    ///
    /// This is the cheap way out: for a receiver whose class is knowable from the
    /// binding alone, a handler can DECLINE before lowering anything. `None`
    /// means "not knowable here" — a call, an index, a property chain — and the
    /// caller falls back to lowering and checking, which is still correct, just
    /// not free.
    pub(in crate::lowering) fn receiver_class_hint(
        &self,
        object: &Expression<'_>,
        body: &Body,
    ) -> Option<smelt_stdlib::StdlibClass> {
        let Expression::Identifier(identifier) = object else {
            return None;
        };
        let name = identifier.name.as_str();
        let local = self.scope.lookup(name)?;
        let ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        self.stdlib_class_of_type(ty)
    }

    /// Resolve a lowered type to its shared stdlib class identity, if any.
    pub(in crate::lowering) fn stdlib_class_of_type(
        &self,
        ty: smelt_hir::TypeId,
    ) -> Option<smelt_stdlib::StdlibClass> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty) else {
            return None;
        };
        self.stdlib_class_of_symbol(*name)
    }

    /// Whether a lowered type is modeled class `class`, and this module's source
    /// has not rebound that class's global name.
    pub(in crate::lowering) fn is_modeled_class(
        &self,
        ty: smelt_hir::TypeId,
        class: smelt_stdlib::StdlibClass,
    ) -> bool {
        self.stdlib_class_of_type(ty) == Some(class)
            && modeled_class_global(class).is_none_or(|name| !self.source_shadows_global(name))
    }

    /// The type of a modeled host class's primitive data property read off a
    /// receiver of type `receiver_ty` (`smelt_stdlib::host_data_property`).
    ///
    /// The direct reads (`res.status`) already lower to modeled operations;
    /// this answers the generic member-type question for the other readers of
    /// the same property — an optional chain (`res?.status`, `res.body?.locked`)
    /// in particular — which otherwise typed it `unknown` while the emitter
    /// read the runtime's concrete value. A user class of the same name keeps
    /// its own members.
    pub(in crate::lowering) fn host_data_property_type(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        field: smelt_hir::Symbol,
    ) -> Option<smelt_hir::TypeId> {
        let class = self.stdlib_class_of_type(receiver_ty)?;
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(receiver_ty) else {
            return None;
        };
        let class_name = source_spelling(&self.ctx.krate, *name)?
            .to_owned();
        if self.source_shadows_global(&class_name) {
            return None;
        }
        let member = self.ctx.krate.symbols.get(field)?;
        let property = smelt_stdlib::host_data_property(class, member)?;
        let ty = match property.ty {
            smelt_stdlib::HostPrimitive::Number => Type::Float,
            smelt_stdlib::HostPrimitive::Bool => Type::Bool,
            smelt_stdlib::HostPrimitive::String => Type::String,
            // A primitive this frontend does not know keeps the generic path.
            _ => return None,
        };
        Some(self.ctx.krate.types.intern(ty))
    }

    /// Resolve a source class SPELLING to its stdlib class identity.
    ///
    /// The name-keyed counterpart of [`Self::stdlib_class_of_type`], for the
    /// sites that hold a source identifier rather than a lowered type — an
    /// `instanceof` right-hand side, for instance.
    pub(in crate::lowering) fn stdlib_class_for_name(
        name: &str,
    ) -> Option<smelt_stdlib::StdlibClass> {
        smelt_stdlib::typescript_stdlib_class(name)
    }

    /// Intern the class type for a modeled class spelling.
    ///
    /// Keeps the SPELLING the source used, which matters where two spellings
    /// share one runtime type: `Blob` and `File` are both `SmeltBlob`, but only
    /// the `File` type carries the two `File`-only data properties, and the
    /// spelling is the only thing that still distinguishes them at this point.
    pub(in crate::lowering) fn stdlib_class_type(&mut self, name: &str) -> smelt_hir::TypeId {
        let interned = self.intern_type_name(name);
        self.ctx.krate.types.intern(Type::Class {
            name: interned,
            args: Vec::new(),
        })
    }

    /// Return the modeled `Headers` class type.
    pub(in crate::lowering) fn headers_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("Headers");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
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
    pub(in crate::lowering) fn headers_op_result_type(&mut self, op: HeadersOp) -> smelt_hir::TypeId {
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
            self.lower_fetch_init(init_argument, &init_key_names(RESPONSE_INIT_KEYS), "Response", &mut fields, body)?;
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
        let Some(receiver) = self.dispatched_receiver(&member.object, body)? else {
            return Ok(None);
        };
        let span = self.span(member.span.start, member.span.end);
        let (receiver, receiver_ty) = if member.optional {
            (receiver, Self::expr_ty(body, receiver))
        } else {
            self.present_receiver(receiver, span, body)
        };
        if !self.is_modeled_class(receiver_ty, smelt_stdlib::StdlibClass::Response) {
            return Ok(None);
        }
        // One rule covers both body readers, because they ARE one reader in
        // the spec's terms: async, single-use, and a second call throws. The
        // member spelling picks which value it answers.
        let op = match (rule, member_name) {
            (RuleId::TsResponseBodyRead, "formData") => ResponseOp::FormData,
            (RuleId::TsResponseBodyRead, "json") => ResponseOp::Json,
            (RuleId::TsResponseBodyRead, "arrayBuffer") => ResponseOp::ArrayBuffer,
            (RuleId::TsResponseBodyRead, "bytes") => ResponseOp::Bytes,
            (RuleId::TsResponseBodyRead, _) => ResponseOp::Text,
            (RuleId::TsResponseClone, _) => ResponseOp::Clone,
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

    /// Unwrap a receiver whose DECLARED type is still optional at a member read.
    ///
    /// `if (this.res) { this.res.headers }` — Hono's `HTTPException.getResponse`
    /// — reads a member off a field typed `Response | undefined` that the source
    /// has just narrowed. Narrowing tracks LOCALS, so the field's declared type
    /// is what arrives here, and every modeled-host member read is gated on the
    /// receiver being exactly that class: the guarded read fell through to the
    /// erased member path and produced a `SmeltUnknown`, which is what refused
    /// `new Response(.., { headers: this.res.headers })` at the constructor.
    ///
    /// `tsc` proved the receiver present, so the read asserts it — the same
    /// `TypeAssert` the `length` path and the union-arm dispatch use. A source
    /// `?.` never reaches here: that spelling asks for `undefined` when the
    /// receiver is absent, which is the optional-chain lowering's job.
    pub(in crate::lowering) fn present_receiver(
        &self,
        receiver: smelt_hir::ExprId,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> (smelt_hir::ExprId, smelt_hir::TypeId) {
        let receiver_ty = Self::expr_ty(body, receiver);
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(receiver_ty).cloned() {
            let asserted = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: receiver },
                ty: inner,
                span,
            });
            return (asserted, inner);
        }
        (receiver, receiver_ty)
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
        // `None` marks a property that IS a `Response` member but has no
        // modeled operation, so the receiver still has to be checked before it
        // can be reported: `this.body = data` on an `any` receiver
        // (`declare module 'koa'` shapes) is not this class's `body` at all.
        let op = match member.property.name.as_str() {
            "status" => Some(ResponseOp::Status),
            "ok" => Some(ResponseOp::Ok),
            "statusText" => Some(ResponseOp::StatusText),
            "headers" => Some(ResponseOp::Headers),
            "bodyUsed" => Some(ResponseOp::BodyUsed),
            // `body` is a `ReadableStream | null` that Smelt models as a body
            // handle: `ResponseOp::Body` reads it. A body is moved through
            // `await res.text()`, or by passing the response itself at the body
            // position, which takes its handle.
            "body" => Some(ResponseOp::Body),
            _ => return Ok(None),
        };
        let Some(receiver) = self.dispatched_receiver(&member.object, body)? else {
            return Ok(None);
        };
        // A receiver the source narrowed to present keeps its modeled member
        // read; see `present_receiver`.
        let span = self.span(member.span.start, member.span.end);
        let (receiver, receiver_ty) = if member.optional {
            (receiver, Self::expr_ty(body, receiver))
        } else {
            self.present_receiver(receiver, span, body)
        };
        if !self.is_modeled_class(receiver_ty, smelt_stdlib::StdlibClass::Response) {
            return Ok(None);
        }
        // Every remaining name in the match above resolves to an operation;
        // the `Option` exists only so the receiver's type is checked first.
        let Some(op) = op else { return Ok(None) };
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

    /// The modeled `ReadableStream` class: a body HANDLE.
    ///
    /// `Response.body` and `Request.body` are the only sites that produce one,
    /// and passing it back to a constructor is the only thing that consumes
    /// one — see `StdlibClass::ReadableStream` for why the stream's own surface
    /// is deliberately absent.
    pub(in crate::lowering) fn readable_stream_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("ReadableStream");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// The `Optional<ReadableStream>` a `body` read answers.
    ///
    /// Optional because the spec's `body` is `ReadableStream | null`: a body
    /// built from nothing (`new Response()`, `new Response(null)`, a GET
    /// request) has none, while a body built from the empty string has one.
    /// The payload distinguishes those, so `if (res.body)` is presence.
    fn body_handle_type(&mut self) -> smelt_hir::TypeId {
        let stream_ty = self.readable_stream_type();
        self.ctx.krate.types.intern(Type::Optional(stream_ty))
    }

    /// The HIR type a `Response` operation answers.
    ///
    /// Each is the member's exact source type, so no caller has to re-narrow:
    /// `status` is `number`, `ok`/`bodyUsed` are `boolean`, `statusText` is
    /// `string`, `headers` is a `Headers`, `clone()` is a `Response`, and
    /// `text()` is a `Promise<string>` — a future, because it is `async`.
    pub(in crate::lowering) fn response_op_result_type(&mut self, op: ResponseOp) -> smelt_hir::TypeId {
        match op {
            ResponseOp::Status => self.ctx.krate.types.intern(Type::Float),
            ResponseOp::Ok | ResponseOp::BodyUsed => self.ctx.krate.types.intern(Type::Bool),
            ResponseOp::StatusText => self.ctx.krate.types.intern(Type::String),
            ResponseOp::Headers => self.headers_type(),
            ResponseOp::Body => self.body_handle_type(),
            ResponseOp::Clone => self.response_type(),
            ResponseOp::Text => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                self.ctx.krate.types.intern(Type::Future(string_ty))
            }
            // `json()` is `Promise<any>`: parsed JSON has no static shape.
            ResponseOp::Json => {
                let unknown_ty = self.erase_with(ErasureReason::ExternalType, "Response.json()");
                self.ctx.krate.types.intern(Type::Future(unknown_ty))
            }
            // `formData()` is `Promise<FormData>`: a future over the modeled
            // concrete form, so the awaited value needs no narrowing.
            ResponseOp::FormData => {
                let form_ty = self.form_data_type();
                self.ctx.krate.types.intern(Type::Future(form_ty))
            }
            // The two BYTE readers answer the two halves of the concrete byte
            // family, which is the spec's own distinction: `arrayBuffer()` is
            // storage and `bytes()` an element view over it, so
            // `ArrayBuffer.isView` separates them. `Blob` answers the same pair
            // for the same reason.
            ResponseOp::ArrayBuffer => {
                let buffer_ty = self.array_buffer_type();
                self.ctx.krate.types.intern(Type::Future(buffer_ty))
            }
            ResponseOp::Bytes => {
                let bytes_ty = self.byte_array_type();
                self.ctx.krate.types.intern(Type::Future(bytes_ty))
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
        // `RequestInfo` is `Request | string`, and a `URL` input is
        // stringified to its `href` by the constructor's string conversion.
        let input_span = self.span(input_argument.span().start, input_argument.span().end);
        let input = self.url_href_of(input, input_span, body).unwrap_or(input);
        let mut fields = InitFields::default();
        if let Some(init_argument) = new_expr.arguments.get(1) {
            self.lower_fetch_init(init_argument, &init_key_names(REQUEST_INIT_KEYS), "Request", &mut fields, body)?;
        }
        let (method, headers, body_expr, signal) = (
            fields.take("method"),
            fields.take("headers"),
            fields.take("body"),
            fields.take("signal"),
        );
        let init_members = taken_request_init_members(&fields);
        let ty = self.request_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::RequestNew {
                input,
                method,
                headers,
                body: body_expr,
                signal,
                init_members,
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
        self.lower_fetch_init(init_argument, &init_key_names(REQUEST_INIT_KEYS), "fetch", &mut fields, body)?;
        let (method, headers, body_expr, signal) = (
            fields.take("method"),
            fields.take("headers"),
            fields.take("body"),
            fields.take("signal"),
        );
        let init_members = taken_request_init_members(&fields);
        let ty = self.request_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::RequestNew {
                input,
                method,
                headers,
                body: body_expr,
                signal,
                init_members,
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
        let Some(receiver) = self.dispatched_receiver(&member.object, body)? else {
            return Ok(None);
        };
        let span = self.span(member.span.start, member.span.end);
        let (receiver, receiver_ty) = if member.optional {
            (receiver, Self::expr_ty(body, receiver))
        } else {
            self.present_receiver(receiver, span, body)
        };
        if !self.is_modeled_class(receiver_ty, smelt_stdlib::StdlibClass::Request) {
            return Ok(None);
        }
        let op = match rule {
            RuleId::TsRequestBodyRead if member_name == "formData" => RequestOp::FormData,
            RuleId::TsRequestBodyRead if member_name == "json" => RequestOp::Json,
            RuleId::TsRequestBodyRead if member_name == "arrayBuffer" => RequestOp::ArrayBuffer,
            RuleId::TsRequestBodyRead if member_name == "bytes" => RequestOp::Bytes,
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
            "url" => Some(RequestOp::Url),
            "method" => Some(RequestOp::Method),
            "headers" => Some(RequestOp::Headers),
            "bodyUsed" => Some(RequestOp::BodyUsed),
            "signal" => Some(RequestOp::Signal),
            // Same as `Response.body` above: a `ReadableStream` Smelt does not
            // model, named rather than erased into a field read that does not
            // compile.
            "body" => Some(RequestOp::Body),
            // The stored `RequestInit` members: read-only getters over values
            // the constructor kept, so one arm answers the whole table.
            other => match RequestInitMember::from_property_name(other) {
                Some(member) => Some(RequestOp::Init(member)),
                None => return Ok(None),
            },
        };
        let Some(receiver) = self.dispatched_receiver(&member.object, body)? else {
            return Ok(None);
        };
        let span = self.span(member.span.start, member.span.end);
        let (receiver, receiver_ty) = if member.optional {
            (receiver, Self::expr_ty(body, receiver))
        } else {
            self.present_receiver(receiver, span, body)
        };
        if !self.is_modeled_class(receiver_ty, smelt_stdlib::StdlibClass::Request) {
            return Ok(None);
        }
        // Every remaining name in the match above resolves to an operation;
        // the `Option` exists only so the receiver's type is checked first.
        let Some(op) = op else { return Ok(None) };
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

    /// The HIR type a `Request` operation answers.
    fn request_op_result_type(&mut self, op: RequestOp) -> smelt_hir::TypeId {
        match op {
            RequestOp::Url | RequestOp::Method => self.ctx.krate.types.intern(Type::String),
            RequestOp::BodyUsed => self.ctx.krate.types.intern(Type::Bool),
            RequestOp::Headers => self.headers_type(),
            RequestOp::Body => self.body_handle_type(),
            RequestOp::Clone => self.request_type(),
            // The signal is the erased abort record, the same value
            // `new AbortController().signal` is — so `request.signal.aborted`
            // and `request.signal.addEventListener(..)` resolve through the
            // one abort member path rather than a second one. It is never
            // `null` in the spec, so the type is not optional.
            RequestOp::Signal => self.erase_with(ErasureReason::UntypedBuiltin, "Request.signal"),
            // Every stored init member is a WebIDL enumeration surfaced as a
            // `string`, except `keepalive`, which is a `boolean`.
            RequestOp::Init(member) => {
                if member.is_boolean() {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    self.ctx.krate.types.intern(Type::String)
                }
            }
            RequestOp::Text => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                self.ctx.krate.types.intern(Type::Future(string_ty))
            }
            // `json()` is `Promise<any>`, as on `Response`.
            RequestOp::Json => {
                let unknown_ty = self.erase_with(ErasureReason::ExternalType, "Request.json()");
                self.ctx.krate.types.intern(Type::Future(unknown_ty))
            }
            // `formData()` is `Promise<FormData>`, the same reader as `text()`
            // answering the modeled concrete form instead of a string.
            RequestOp::FormData => {
                let form_ty = self.form_data_type();
                self.ctx.krate.types.intern(Type::Future(form_ty))
            }
            // The same pair as `Response`; see the comment there.
            RequestOp::ArrayBuffer => {
                let buffer_ty = self.array_buffer_type();
                self.ctx.krate.types.intern(Type::Future(buffer_ty))
            }
            RequestOp::Bytes => {
                let bytes_ty = self.byte_array_type();
                self.ctx.krate.types.intern(Type::Future(bytes_ty))
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
    /// For a literal init, `keys` is the modeled key set, so an unmodeled key is
    /// named rather than silently dropped — `redirect` and `signal` change what a request does.
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
                        let key = static_name_text(key);
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
        if self.is_modeled_class(source_ty, smelt_stdlib::StdlibClass::Request) {
            return self.request_init_fields(source, keys, span, fields, body);
        }
        // Symmetrically, a `Response` is allowed at the `ResponseInit` position:
        // WebIDL converts the object to the dictionary by reading its own
        // `status`, `statusText` and `headers` properties.
        if self.is_modeled_class(source_ty, smelt_stdlib::StdlibClass::Response) {
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
                // not a new erasure: the ambient receiver is already an erased record
                let unknown_ty = self.unknown_type(NotNewErasure::ErasedUpstream);
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
                // A `Request` at the init position is converted to the
                // dictionary by READING its properties, prototype getters
                // included, so the stored members copy across with the rest.
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

    /// Lower TypeScript `fetch(url[, options])` into an async HTTP fetch.
    pub(in crate::lowering) fn fetch_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.recognized_call(call) != Some(RuleId::TsFetch) {
            return Ok(None);
        }
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch lowering supports fetch(url[, options])",
            ));
        }
        let Some(url_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch lowering supports fetch(url[, options])",
            ));
        };
        let mut url = self.argument(url_argument, body)?;
        // A `URL` input is fetched by its `href`, as `new Request(url)` reads it.
        let url_span = self.span(url_argument.span().start, url_argument.span().end);
        if let Some(href) = self.url_href_of(url, url_span, body) {
            url = href;
        }
        // `fetch(url, init)` IS `fetch(new Request(url, init))` — that is the
        // spec's own definition, not a convenience — so the init goes through
        // the very same reader the `Request` constructor uses and the operand
        // becomes a request.
        //
        // The init must reach the transport: discarding it would make
        // `fetch(url, { method: "POST", body })` silently issue a GET with no
        // body, a false green that survives every compile step.
        if let Some(init_argument) = call.arguments.get(1) {
            let request = self.fetch_request_from_init(url, init_argument, body)?;
            let response_ty = self.response_type();
            let ty = self.ctx.krate.types.intern(Type::Future(response_ty));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::AsyncOp {
                    op: AsyncOp::HttpFetch,
                    args: vec![request],
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let url_ty = Self::expr_ty(body, url);
        // `fetch(request)`: the spec's other input form, the same operand the
        // two-argument form builds.
        if self.is_modeled_class(url_ty, smelt_stdlib::StdlibClass::Request) {
            let response_ty = self.response_type();
            let ty = self.ctx.krate.types.intern(Type::Future(response_ty));
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::AsyncOp {
                    op: AsyncOp::HttpFetch,
                    args: vec![url],
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if self.ctx.krate.types.get(url_ty) != Some(&Type::String) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            if self.is_string_compatible_type(url_ty) || self.type_contains_unknown(url_ty) {
                url = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: url,
                        target: string_ty,
                    },
                    ty: string_ty,
                    span: self.span(url_argument.span().start, url_argument.span().end),
                });
            } else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "fetch requires a string-compatible URL argument",
                ));
            }
        }
        // `fetch` resolves to a `Response`, not to the body text: the caller
        // reads `status`, `ok`, `headers` and the body separately, and a
        // `Promise<string>` threw all but one of those away.
        let response_ty = self.response_type();
        let ty = self.ctx.krate.types.intern(Type::Future(response_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op: AsyncOp::HttpFetch,
                args: vec![url],
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }
}
