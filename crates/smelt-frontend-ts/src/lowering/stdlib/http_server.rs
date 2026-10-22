//! TypeScript lowering for `node:http`'s server surface.
//!
//! Five entry points, registered where the other modeled receivers are:
//!
//! * [`ModuleBuilder::http_create_server_call`] in the builtin call-handler
//!   chain, for `createServer(handler)`;
//! * [`ModuleBuilder::dispatch_http_server_method`] and
//!   [`ModuleBuilder::dispatch_server_response_method`] in the same chain, for
//!   the two receivers with methods;
//! * [`ModuleBuilder::http_property_read`] in the static-member read path, for
//!   `req.method`/`req.url`/`req.headers` and `res.statusCode`;
//! * [`ModuleBuilder::try_server_response_assignment_expression`] for plain
//!   assignment to `res.statusCode`.
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
    Body, Expr, ExprKind, HttpServerOp, IncomingMessageOp, Literal, ServerResponseOp, Type,
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

    /// Lower `serve(options, onListen?)` from `@hono/node-server`.
    ///
    /// The adapter's whole job, done natively: a `node:http` server whose
    /// handler converts each request to a fetch `Request`, calls
    /// `options.fetch`, and writes back the `Response` it answers
    /// (`HttpCreateFetchServer`), then `listen(port, hostname?, onListen?)` —
    /// the existing `Server` operation, which answers the server as `serve`
    /// does. `port` defaults to 3000, as in the adapter.
    ///
    /// The handler keeps its own type: a fetch entry point usually declares
    /// more than the request (Hono's `app.fetch(request, env?, executionCtx?)`)
    /// and answers a `Response` or a promise of one, and codegen calls it the
    /// way the adapter does (see `http_create_fetch_server_text`).
    pub(in crate::lowering) fn hono_node_serve_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.host_module_export_path(&call.callee)
            != Some((smelt_stdlib::HostModuleId::HonoNodeServer, "serve".to_owned()))
        {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        let (options, on_listen) = match call.arguments.as_slice() {
            [options] => (options, None),
            [options, on_listen] => (options, Some(on_listen)),
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "`serve` from @hono/node-server is modeled as `serve(options)` or `serve(options, onListen)`",
                ));
            }
        };
        let Some(Expression::ObjectExpression(options)) = options.as_expression() else {
            return Err(SmeltError::unsupported(
                span,
                "`serve` from @hono/node-server is modeled with its options written as an object literal",
            ));
        };
        let mut fetch = None;
        let mut port = None;
        let mut hostname = None;
        for property in &options.properties {
            let oxc::ast::ast::ObjectPropertyKind::ObjectProperty(property) = property else {
                return Err(SmeltError::unsupported(
                    span,
                    "`serve` options are modeled without spread properties",
                ));
            };
            let Some(key) = property.key.static_name() else {
                return Err(SmeltError::unsupported(span, "`serve` options are modeled with static keys"));
            };
            let value = self.expression(&property.value, body)?;
            match key.as_ref() {
                "fetch" => fetch = Some(value),
                "port" => port = Some(value),
                "hostname" => hostname = Some(value),
                other => {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("the `{other}` option of `serve` from @hono/node-server is not modeled; `fetch`, `port` and `hostname` are"),
                    ));
                }
            }
        }
        let Some(fetch) = fetch else {
            return Err(SmeltError::unsupported(span, "`serve` from @hono/node-server needs a `fetch` handler"));
        };
        if !matches!(self.ctx.krate.types.get(Self::expr_ty(body, fetch)), Some(Type::Function(_))) {
            return Err(SmeltError::unsupported(span, "the `fetch` option of `serve` must be a function"));
        }
        let port = if let Some(port) = port {
            port
        } else {
            let float_ty = self.ctx.krate.types.intern(Type::Float);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(3000.0)),
                ty: float_ty,
                span,
            })
        };
        let mut listen_args = vec![port];
        // Positional, as `listen` reads them: a callback needs the host slot
        // filled, with the host `listen(port, cb)` binds (see
        // `http_listen_arguments`).
        let hostname = match (hostname, on_listen) {
            (None, Some(_)) => Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("127.0.0.1".to_owned())),
                ty: self.ctx.krate.types.intern(Type::String),
                span,
            })),
            (hostname, _) => hostname,
        };
        listen_args.extend(hostname);
        if let Some(on_listen) = on_listen {
            // The adapter passes the bound address to its callback; the
            // `listen` operation calls it without one, so only a callback that
            // declares no parameter is accepted rather than one silently fed
            // nothing.
            if let Some(
                Expression::ArrowFunctionExpression(function),
            ) = on_listen.as_expression()
                && !function.params.items.is_empty()
            {
                return Err(SmeltError::unsupported(
                    span,
                    "the address argument of `serve`'s listening callback is not modeled; declare the callback without parameters",
                ));
            }
            let hint = self.http_listening_callback_type();
            listen_args.push(self.argument_with_hint(on_listen, body, Some(hint))?);
        }
        let server_ty = self.http_server_type();
        let server = body.push_expr(Expr {
            kind: ExprKind::HttpCreateFetchServer { fetch },
            ty: server_ty,
            span,
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::HttpServerOp {
                op: HttpServerOp::Listen,
                server,
                args: listen_args,
            },
            ty: server_ty,
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
        let ty = self.http_server_op_result_type(op, span)?;
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
    /// `address` answers the bound socket's `AddressInfo`, or null once the
    /// server is closed (or before it listens).
    fn http_server_op_result_type(
        &mut self,
        op: HttpServerOp,
        span: smelt_hir::Span,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match op {
            HttpServerOp::Listen | HttpServerOp::Close => Ok(self.http_server_type()),
            HttpServerOp::Address => {
                let info = self.http_address_info_type(span)?;
                Ok(self.ctx.krate.types.intern(Type::Optional(info)))
            }
        }
    }

    /// `node:net`'s `AddressInfo`, materialized as a real interface.
    ///
    /// Node's `server.address()` answers `{ address, family, port }` for a TCP
    /// server — `{ address: '127.0.0.1', family: 'IPv4', port: 41234 }` — and
    /// `null` when the server is not listening. (Its third arm, a `string`,
    /// is a pipe or socket path, which `listen` here cannot bind.) Answering
    /// the bare port number instead was a modeling error: a program written
    /// for Node reads `.port` off the result, and the bare-number model made
    /// the program that is CORRECT for Node fail to lower while the one Smelt
    /// accepted printed `[object Object]` under Node.
    ///
    /// The three keys are REQUIRED and concretely typed, so the result is a
    /// generated struct (`Option<AddressInfo>` in Rust), never an erased
    /// record. Declared once per crate, like the ambient fetch dictionaries
    /// (`ambient_init_interface_type`). A source that declares its own
    /// `AddressInfo` is refused rather than silently given a struct whose
    /// shape it did not write.
    fn http_address_info_type(
        &mut self,
        span: smelt_hir::Span,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        const NAME: &str = "AddressInfo";
        let name = self.intern_type_name(NAME);
        let ty = self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        });
        if self.find_interface(name).is_some() {
            return Ok(ty);
        }
        if self.source_declares_type(NAME) {
            return Err(SmeltError::unsupported(
                span,
                "`server.address()` answers node:net's `AddressInfo`, which this module also declares; rename the local declaration",
            ));
        }
        let declaration_span = self.span(0, 0);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let fields = [("address", string_ty), ("family", string_ty), ("port", float_ty)]
            .into_iter()
            .map(|(key, field_ty)| smelt_hir::Field {
                name: self.intern_source_name(key),
                ty: field_ty,
                visibility: smelt_hir::Visibility::Public,
                optional: false,
                span: declaration_span,
            })
            .collect::<Vec<_>>();
        let item = self.ctx.krate.push_item(smelt_hir::Item::Interface(smelt_hir::Interface {
            name,
            span: declaration_span,
            type_params: Vec::new(),
            extends: Vec::new(),
            fields,
            methods: Vec::new(),
            decode_facts: Vec::new(),
        }));
        self.interfaces.register_lowered(
            crate::lowering::state::interface_registry::LoweredInterface {
                name,
                name_text: NAME.to_owned(),
                item,
                extends: Vec::new(),
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_value_ty: None,
            },
        );
        Ok(ty)
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
