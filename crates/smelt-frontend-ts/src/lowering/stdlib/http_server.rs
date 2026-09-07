//! TypeScript lowering for `node:http`'s server surface.
//!
//! Four entry points, registered where the other modeled receivers are:
//!
//! * [`ModuleBuilder::http_create_server_call`] in the builtin call-handler
//!   chain, for `createServer(handler)`;
//! * [`ModuleBuilder::dispatch_http_server_method`] and
//!   [`ModuleBuilder::dispatch_server_response_method`] in the same chain, for
//!   the two receivers with methods;
//! * [`ModuleBuilder::http_property_read`] in the static-member read path, for
//!   `req.method`/`req.url`/`req.headers` and `res.statusCode`.
//!
//! `IncomingMessage` has no method dispatch of its own: everything it does
//! besides those three reads is an `EventEmitter` operation, and it reaches
//! that through [`ModuleBuilder::dispatch_event_emitter_method`], which tests
//! whether the receiver's class HAS an emitter rather than whether it IS one.
//!
//! # The handler's parameters are typed from the module, not inferred
//!
//! A source handler is written `(req, res) => { .. }` with no annotations, so
//! nothing in the arrow says what `req` is. The types come from `node:http`
//! itself: `createServer` lowers its argument under a function-type HINT of
//! `(IncomingMessage, ServerResponse) => void`, which is what makes `req.url` a
//! modeled read instead of an erased property lookup. A source that DOES
//! annotate its handler gets the same types, because the annotations resolve
//! through the same registry entries.

use super::super::ModuleBuilder;
use crate::error::SmeltError;
use oxc::ast::ast::Expression;
use smelt_hir::{
    Body, Expr, ExprKind, HttpServerOp, IncomingMessageOp, ServerResponseOp, Type,
};

impl ModuleBuilder<'_> {
    /// Lower `createServer(handler)` from `node:http`.
    ///
    /// Refuses (returns `Ok(None)`) when `createServer` resolves to something
    /// the program defined or imported from elsewhere, so a source with its own
    /// `createServer` keeps it. A modeled host-module export is recognized at
    /// its use site rather than at its binding, which is why that test lives
    /// here rather than in the import classifier.
    pub(in crate::lowering) fn http_create_server_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "createServer" || self.http_create_server_is_shadowed() {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        let [handler] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                span,
                "`createServer` is modeled with exactly one argument, the request handler; the options form is not modeled yet",
            ));
        };
        // The hint is what gives an unannotated `(req, res) => ..` its
        // parameter types. Without it both parameters lower as unknown and
        // every read through them erases.
        let hint = self.http_handler_type();
        let handler = self.argument_with_hint(handler, body, Some(hint))?;
        let ty = self.http_server_type();
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::HttpCreateServer { handler },
            ty,
            span,
        })))
    }

    /// Return whether the program binds `createServer` to something of its own.
    ///
    /// A source function, class, or import that resolved to a real item wins
    /// over the modeled export, on the same rule that lets a user class named
    /// `Response` shadow the fetch type.
    fn http_create_server_is_shadowed(&self) -> bool {
        self.scope.is_bound("createServer")
            || self.classes.contains("createServer")
            || self.import_alias_resolved("createServer")
    }

    /// Dispatch a modeled `Server` method on a concrete server receiver.
    ///
    /// `listen`'s four Node overloads (`(port)`, `(port, cb)`, `(port, host)`,
    /// `(port, host, cb)`) are sorted HERE into the fixed operand order codegen
    /// reads positionally — port, host, callback — so the ambiguity of the
    /// second argument is resolved once, by its lowered type, rather than at
    /// every consumer.
    pub(in crate::lowering) fn dispatch_http_server_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        if smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::HttpServer,
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
        if !self.is_http_server_type(receiver_ty) {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        let op = match member_name {
            "listen" => HttpServerOp::Listen,
            "close" => HttpServerOp::Close,
            "address" => HttpServerOp::Address,
            _ => return Ok(None),
        };
        let args = match op {
            HttpServerOp::Listen => self.http_listen_arguments(call, body, span)?,
            HttpServerOp::Close | HttpServerOp::Address => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("`Server.{member_name}` is modeled without arguments"),
                    ));
                }
                Vec::new()
            }
        };
        let ty = self.http_server_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::HttpServerOp {
                op,
                server: receiver,
                args,
            },
            ty,
            span,
        })))
    }

    /// Sort `listen`'s overloads into `[port, host?, callback?]`.
    ///
    /// The second argument is a host when it lowers to a string and the
    /// listening callback when it lowers to a function; that is exactly how
    /// Node decides, and deciding it from the LOWERED TYPE rather than from the
    /// syntax means a host held in a variable works the same as a literal.
    fn http_listen_arguments(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        span: smelt_hir::Span,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let Some(port_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                span,
                "`Server.listen` is modeled with a port; the options and path forms are not",
            ));
        };
        let port = self.argument(port_argument, body)?;
        let mut host = None;
        let mut callback = None;
        for argument in call.arguments.iter().skip(1) {
            // A function written inline is the listening callback, and
            // `node:http` fixes its signature: no parameters, no result. The
            // hint is what stops the arrow from lowering as an erased callback
            // — which would put a `SmeltUnknown` return inside the program's
            // own closure for a signature the module already publishes. The
            // syntactic peek only chooses which hint to try; the slot is still
            // decided by the LOWERED type below.
            let lowered = if matches!(
                argument.as_expression(),
                Some(
                    Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
                )
            ) {
                let hint = self.http_listening_callback_type();
                self.argument_with_hint(argument, body, Some(hint))?
            } else {
                self.argument(argument, body)?
            };
            let lowered_ty = Self::expr_ty(body, lowered);
            match self.ctx.krate.types.get(lowered_ty) {
                Some(Type::Function(_)) if callback.is_none() => callback = Some(lowered),
                Some(Type::String) if host.is_none() => host = Some(lowered),
                _ => {
                    return Err(SmeltError::unsupported(
                        span,
                        "`Server.listen` accepts a port, an optional host string, and an optional listening callback",
                    ));
                }
            }
        }
        // The operand list is positional, so an absent host with a present
        // callback still needs the host slot filled. `listen(port, cb)` is the
        // common spelling, and it is exactly this case.
        let mut args = vec![port];
        if let Some(callback) = callback {
            args.push(host.unwrap_or_else(|| {
                body.push_expr(Expr {
                    kind: ExprKind::Literal(smelt_hir::Literal::String(
                        "127.0.0.1".to_owned(),
                    )),
                    ty: self.ctx.krate.types.intern(Type::String),
                    span,
                })
            }));
            args.push(callback);
        } else if let Some(host) = host {
            args.push(host);
        }
        Ok(args)
    }

    /// Dispatch a modeled `ServerResponse` method on a concrete receiver.
    pub(in crate::lowering) fn dispatch_server_response_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        if smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::ServerResponse,
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
        if !self.is_server_response_type(receiver_ty) {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        let (op, required, optional) = match member_name {
            "setHeader" => (ServerResponseOp::SetHeader, 2, 0),
            "getHeader" => (ServerResponseOp::GetHeader, 1, 0),
            "writeHead" => (ServerResponseOp::WriteHead, 1, 1),
            "write" => (ServerResponseOp::Write, 1, 0),
            "end" => (ServerResponseOp::End, 0, 1),
            _ => return Ok(None),
        };
        if call.arguments.len() < required || call.arguments.len() > required + optional {
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "`ServerResponse.{member_name}` is modeled with {required} required argument(s) and {optional} optional"
                ),
            ));
        }
        let args = call
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        let ty = self.server_response_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ServerResponseOp {
                op,
                response: receiver,
                args,
            },
            ty,
            span,
        })))
    }

    /// Lower a `node:http` data-property READ on a concrete receiver.
    ///
    /// `req.method`/`req.url`/`req.headers` and `res.statusCode` are properties
    /// in the source but operations here, for the same reason the `Response`
    /// reads are: there is no struct field behind them, the runtime type
    /// computes the value.
    pub(in crate::lowering) fn http_property_read(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let property = member.property.name.as_str();
        // The property name gates FIRST, before the receiver is lowered.
        // Lowering the receiver to decide is what a type-directed read wants to
        // do, but `expression` pushes into the body, so an unrelated `x.ada`
        // would leave a discarded expression behind and every record read in
        // the corpus would gain one. Which of the two shapes this is still
        // comes from the receiver's type below -- `headers` belongs to a
        // `Response` as well as to an `IncomingMessage` -- the name only
        // decides whether it is worth asking.
        if !matches!(property, "method" | "url" | "headers" | "statusCode") {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        let span = self.span(member.span.start, member.span.end);
        if self.is_incoming_message_type(receiver_ty) {
            let op = match property {
                "method" => IncomingMessageOp::Method,
                "url" => IncomingMessageOp::Url,
                "headers" => IncomingMessageOp::Headers,
                _ => return Ok(None),
            };
            let ty = self.incoming_message_op_result_type(op);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::IncomingMessageOp {
                    op,
                    message: receiver,
                },
                ty,
                span,
            })));
        }
        if self.is_server_response_type(receiver_ty) && property == "statusCode" {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ServerResponseOp {
                    op: ServerResponseOp::StatusCode,
                    response: receiver,
                    args: Vec::new(),
                },
                ty,
                span,
            })));
        }
        Ok(None)
    }

    /// Lower `res.statusCode = value` into a status-line WRITE.
    ///
    /// The only settable member on any modeled class. It is an operation rather
    /// than a stored field because that is what it is: the status of a response
    /// that has not been sent, which the runtime clamps to a status a wire
    /// format can carry. Returns `Ok(None)` for any other target so ordinary
    /// member assignment proceeds.
    pub(in crate::lowering) fn try_server_response_assignment_expression(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let oxc::ast::ast::AssignmentTarget::StaticMemberExpression(member) = &assign.left else {
            return Ok(None);
        };
        if member.property.name != "statusCode" {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        if !self.is_server_response_type(Self::expr_ty(body, receiver)) {
            return Ok(None);
        }
        let span = self.span(assign.span.start, assign.span.end);
        // Only a plain `=` is modeled. A compound spelling (`res.statusCode +=
        // 1`) would have to read the pending status back before storing, and
        // there is no source that wants that; blocking it names the gap rather
        // than silently dropping the read.
        if assign.operator != oxc::ast::ast::AssignmentOperator::Assign {
            return Err(SmeltError::unsupported(
                span,
                "`res.statusCode` is modeled for plain assignment only",
            ));
        }
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let value = self.expression_with_hint(&assign.right, body, Some(float_ty))?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ServerResponseOp {
                op: ServerResponseOp::SetStatusCode,
                response: receiver,
                args: vec![value],
            },
            // The assignment evaluates to the ASSIGNED value, as every
            // JavaScript assignment does, not to the clamped one the runtime
            // stored.
            ty: float_ty,
            span,
        })))
    }

    /// The modeled `node:http` `Server` class type.
    pub(in crate::lowering) fn http_server_type(&mut self) -> smelt_hir::TypeId {
        self.http_class_type("Server")
    }

    /// The modeled `node:http` `IncomingMessage` class type.
    pub(in crate::lowering) fn incoming_message_type(&mut self) -> smelt_hir::TypeId {
        self.http_class_type("IncomingMessage")
    }

    /// The modeled `node:http` `ServerResponse` class type.
    pub(in crate::lowering) fn server_response_type(&mut self) -> smelt_hir::TypeId {
        self.http_class_type("ServerResponse")
    }

    /// Intern one of the three modeled `node:http` class types.
    fn http_class_type(&mut self, name: &str) -> smelt_hir::TypeId {
        let name = self.intern_type_name(name);
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// The request handler's type: `(IncomingMessage, ServerResponse) => void`.
    ///
    /// Declared THROWING, because a handler that throws is a real program: Node
    /// answers 500 for it, and so does the generated server. Typing it
    /// otherwise would force a throwing arrow body into a panic.
    fn http_handler_type(&mut self) -> smelt_hir::TypeId {
        let message_ty = self.incoming_message_type();
        let response_ty = self.server_response_type();
        let none_ty = self.ctx.krate.types.intern(Type::None);
        self.ctx
            .krate
            .types
            .intern(Type::Function(smelt_hir::FunctionType {
                params: vec![message_ty, response_ty],
                rest: None,
                required_params: Some(2),
                mutable_params: Vec::new(),
                return_ty: none_ty,
                is_async: false,
                may_throw: true,
            }))
    }

    /// The listening callback's type: `() => void`.
    ///
    /// Throwing for the same reason the handler is: `listen` runs this callback
    /// in the same turn, so an exception it raises leaves through `listen`.
    fn http_listening_callback_type(&mut self) -> smelt_hir::TypeId {
        let none_ty = self.ctx.krate.types.intern(Type::None);
        self.ctx
            .krate
            .types
            .intern(Type::Function(smelt_hir::FunctionType {
                params: Vec::new(),
                rest: None,
                required_params: Some(0),
                mutable_params: Vec::new(),
                return_ty: none_ty,
                is_async: false,
                may_throw: true,
            }))
    }

    /// Return whether a lowered type is the modeled `Server` class.
    pub(in crate::lowering) fn is_http_server_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::HttpServer)
            && !self.user_class_shadows("Server")
    }

    /// Return whether a lowered type is the modeled `IncomingMessage` class.
    pub(in crate::lowering) fn is_incoming_message_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::IncomingMessage)
            && !self.user_class_shadows("IncomingMessage")
    }

    /// Return whether a lowered type is the modeled `ServerResponse` class.
    pub(in crate::lowering) fn is_server_response_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::ServerResponse)
            && !self.user_class_shadows("ServerResponse")
    }

    /// The HIR type a `Server` operation answers.
    ///
    /// `listen` and `close` answer the SERVER, which is what makes
    /// `createServer(h).listen(0)` a server-valued expression as it is in Node;
    /// `address` answers the bound port or null.
    fn http_server_op_result_type(&mut self, op: HttpServerOp) -> smelt_hir::TypeId {
        match op {
            HttpServerOp::Listen | HttpServerOp::Close => self.http_server_type(),
            HttpServerOp::Address => {
                let float_ty = self.ctx.krate.types.intern(Type::Float);
                self.ctx.krate.types.intern(Type::Optional(float_ty))
            }
        }
    }

    /// The HIR type an `IncomingMessage` read answers.
    fn incoming_message_op_result_type(&mut self, op: IncomingMessageOp) -> smelt_hir::TypeId {
        match op {
            IncomingMessageOp::Method | IncomingMessageOp::Url => {
                self.ctx.krate.types.intern(Type::String)
            }
            // A plain `Record<string, string>`, not a `Headers`: Node's
            // `req.headers` is an object with lower-cased keys and no methods.
            IncomingMessageOp::Headers => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                self.ctx.krate.types.intern(Type::Dict(string_ty, string_ty))
            }
        }
    }

    /// The HIR type a `ServerResponse` operation answers.
    fn server_response_op_result_type(&mut self, op: ServerResponseOp) -> smelt_hir::TypeId {
        match op {
            ServerResponseOp::StatusCode | ServerResponseOp::SetStatusCode => {
                self.ctx.krate.types.intern(Type::Float)
            }
            ServerResponseOp::GetHeader => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                self.ctx.krate.types.intern(Type::Optional(string_ty))
            }
            // `write` answers whether the chunk was flushed, as Node does.
            ServerResponseOp::Write => self.ctx.krate.types.intern(Type::Bool),
            // `setHeader`, `writeHead` and `end` all answer the response, which
            // is what makes `res.writeHead(200, ..).end(body)` chain.
            ServerResponseOp::SetHeader
            | ServerResponseOp::WriteHead
            | ServerResponseOp::End => self.server_response_type(),
        }
    }
}
