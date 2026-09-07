//! Rust emission for the `node:http` server operations.
//!
//! One module rather than four arms in `fetch_types.rs`, because `node:http`
//! shares nothing with the fetch types but the `SmeltHeaders` conversion it
//! borrows for `writeHead`. The runtime types these render against live in
//! [`crate::http_server_prelude`].
//!
//! # Callbacks here are not erased
//!
//! Both callbacks the module takes — the request handler and the listening
//! callback — have signatures fixed by `node:http`, so they are adapted into
//! their real Rust types rather than pushed through the erased callable ABI.
//! The adapter is what makes the source's own function type (which may or may
//! not be `may_throw`, and answers whatever the arrow body answers) meet the
//! runtime's `Result<(), _>`: it calls, discards the answer, and reports the
//! error the source could raise.

use super::*;

impl FunctionEmitter<'_> {
    /// Emit `createServer(handler)`.
    ///
    /// The handler operand carries the source's own function type. It is
    /// wrapped rather than passed through so the server always holds the one
    /// signature `SmeltHttpHandler` names, whatever the source's arrow was
    /// inferred as.
    pub(super) fn http_create_server_text(
        &self,
        handler: &Operand,
    ) -> Result<String, EmitError> {
        let adapted = self.http_handler_adapter_text(handler)?;
        Ok(format!("SmeltHttpServer::new({adapted})"))
    }

    /// Wrap a source request handler into a `SmeltHttpHandler`.
    ///
    /// A `copy` of a function-typed operand is left un-cloned by
    /// `operand_text` (a callee position only borrows it), but this adapter
    /// MOVES the value into a closure, so it takes its own handle — the same
    /// correction the emitter's listener erasure needs.
    fn http_handler_adapter_text(&self, handler: &Operand) -> Result<String, EmitError> {
        let ty = self.operand_ty(handler)?;
        let mut text = self.operand_text(handler)?;
        if matches!(handler, Operand::Copy(_))
            && matches!(self.mir.types.get(ty), Some(Type::Function(_)))
        {
            text = format!("{text}.clone()");
        }
        // A handler declared as throwing already answers a `Result`; one that
        // is not answers its body's value directly. Both are discarded — Node
        // ignores a handler's return value — and only the throwing one has an
        // error to propagate.
        let call = if self.function_type_may_throw(ty) {
            "smelt_handler(smelt_request, smelt_response)?;"
        } else {
            "let _ = smelt_handler(smelt_request, smelt_response);"
        };
        Ok(format!(
            "{{ let smelt_handler = {text}; ::std::rc::Rc::new(move |smelt_request: SmeltIncomingMessage, smelt_response: SmeltServerResponse| {{ {call} Ok(()) }}) as SmeltHttpHandler }}"
        ))
    }

    /// Emit a `node:http` `Server` member operation.
    pub(super) fn http_server_op_text(
        &self,
        op: smelt_hir::HttpServerOp,
        server: &Operand,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        let receiver = self.operand_text(server)?;
        match op {
            smelt_hir::HttpServerOp::Listen => {
                let float_ty = self.type_id(Type::Float)?;
                let Some(port) = args.first() else {
                    return Err(EmitError::new("`Server.listen` requires a port"));
                };
                let port_text = self.value_at_type(port, float_ty)?;
                // The frontend has already sorted the overloads
                // (`listen(port)`, `listen(port, cb)`, `listen(port, host)`,
                // `listen(port, host, cb)`) into a fixed operand order, so this
                // reads positionally: index 1 is the host when present, index 2
                // the callback.
                let host_text = match args.get(1) {
                    Some(host) => {
                        let string_ty = self.type_id(Type::String)?;
                        format!("Some({})", self.value_at_type(host, string_ty)?)
                    }
                    None => "None".to_owned(),
                };
                let callback_text = match args.get(2) {
                    Some(callback) => {
                        format!("Some({})", self.http_listen_callback_text(callback)?)
                    }
                    None => "None".to_owned(),
                };
                Ok(format!(
                    "{receiver}.listen({port_text}, {host_text}, {callback_text})?"
                ))
            }
            smelt_hir::HttpServerOp::Close => Ok(format!("{receiver}.close()?")),
            smelt_hir::HttpServerOp::Address => Ok(format!("{receiver}.address()")),
        }
    }

    /// Wrap a source listening callback into the runtime's `Rc<dyn Fn() -> ..>`.
    fn http_listen_callback_text(&self, callback: &Operand) -> Result<String, EmitError> {
        let ty = self.operand_ty(callback)?;
        let mut text = self.operand_text(callback)?;
        if matches!(callback, Operand::Copy(_))
            && matches!(self.mir.types.get(ty), Some(Type::Function(_)))
        {
            text = format!("{text}.clone()");
        }
        let call = if self.function_type_may_throw(ty) {
            "smelt_listening()?;"
        } else {
            "let _ = smelt_listening();"
        };
        Ok(format!(
            "{{ let smelt_listening = {text}; ::std::rc::Rc::new(move || {{ {call} Ok(()) }}) as ::std::rc::Rc<dyn Fn() -> Result<(), Box<dyn ::std::error::Error>>> }}"
        ))
    }

    /// Emit a `node:http` `IncomingMessage` property read.
    ///
    /// `headers` needs the DESTINATION type: `req.headers` is a
    /// `Record<string, string>`, and which Rust container that is depends on
    /// the crate — a plain `HashMap`, or the reference-semantics `SmeltRecord`
    /// when the program stores the value where JS object identity matters. The
    /// runtime answers name/value pairs and the container is named here, so
    /// neither choice is baked into the prelude.
    pub(super) fn incoming_message_op_text(
        &self,
        op: smelt_hir::IncomingMessageOp,
        message: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let receiver = self.operand_text(message)?;
        Ok(match op {
            smelt_hir::IncomingMessageOp::Method => format!("{receiver}.method()"),
            smelt_hir::IncomingMessageOp::Url => format!("{receiver}.url()"),
            smelt_hir::IncomingMessageOp::Headers => {
                let container = self.type_text(dest_ty)?;
                format!(
                    "<{container} as ::std::iter::FromIterator<(String, String)>>::from_iter({receiver}.headers())"
                )
            }
        })
    }

    /// Emit a `node:http` `ServerResponse` member operation.
    pub(super) fn server_response_op_text(
        &self,
        op: smelt_hir::ServerResponseOp,
        response: &Operand,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        let receiver = self.operand_text(response)?;
        let string_ty = self.type_id(Type::String)?;
        let float_ty = self.type_id(Type::Float)?;
        let arg_at = |index: usize| -> Result<&Operand, EmitError> {
            args.get(index).ok_or_else(|| {
                EmitError::new(format!("`ServerResponse` operation {op:?} is missing an argument"))
            })
        };
        Ok(match op {
            smelt_hir::ServerResponseOp::StatusCode => format!("{receiver}.status_code()"),
            smelt_hir::ServerResponseOp::SetStatusCode => {
                let value = self.value_at_type(arg_at(0)?, float_ty)?;
                format!("{receiver}.set_status_code({value})")
            }
            smelt_hir::ServerResponseOp::SetHeader => {
                let name = self.value_at_type(arg_at(0)?, string_ty)?;
                let value = self.value_at_type(arg_at(1)?, string_ty)?;
                // Answers the response so the statement has a value to discard
                // and `res.setHeader(..)` composes like the other members.
                format!("{{ {receiver}.set_header(&{name}, &{value}); {receiver}.clone() }}")
            }
            smelt_hir::ServerResponseOp::GetHeader => {
                let name = self.value_at_type(arg_at(0)?, string_ty)?;
                format!("{receiver}.get_header(&{name})")
            }
            smelt_hir::ServerResponseOp::WriteHead => {
                let status = self.value_at_type(arg_at(0)?, float_ty)?;
                // The headers argument goes through the SAME `HeadersInit`
                // conversion `new Headers(init)` uses, so an object literal, a
                // pair list and a `Headers` all mean here what they mean there
                // rather than through a second, drifting reader.
                let headers_text = match args.get(1) {
                    Some(headers) => {
                        let headers_ty = self.operand_ty(headers)?;
                        let headers_value = self.operand_text(headers)?;
                        let converted =
                            self.headers_conversion_text(&headers_value, headers_ty)?;
                        format!("Some({converted}.entries_in_insertion_order())")
                    }
                    None => "None".to_owned(),
                };
                format!("{receiver}.write_head({status}, {headers_text})")
            }
            smelt_hir::ServerResponseOp::Write => {
                let chunk = self.value_at_type(arg_at(0)?, string_ty)?;
                format!("{receiver}.write(&{chunk})")
            }
            smelt_hir::ServerResponseOp::End => {
                let chunk = match args.first() {
                    Some(chunk) => format!("Some({})", self.value_at_type(chunk, string_ty)?),
                    None => "None".to_owned(),
                };
                format!("{receiver}.end({chunk})")
            }
        })
    }

    /// Emit `fetch(request)`: the whole request, not just its URL.
    ///
    /// Lives beside the server emission rather than with the one-line
    /// `reqwest::get` because it is the same shape in the other direction — a
    /// `SmeltRequest` taken apart into a transport call, where the server takes
    /// a transport call apart into a `SmeltIncomingMessage`.
    ///
    /// The body is TAKEN (`take_bytes`), which is what makes `request.bodyUsed`
    /// true afterwards exactly as the spec says a fetched request is consumed;
    /// an empty body is sent as no body rather than as zero bytes, so a GET
    /// built this way is indistinguishable on the wire from a plain one.
    pub(super) fn http_fetch_request_text(
        &self,
        request: &Operand,
    ) -> Result<String, EmitError> {
        let request_text = self.operand_text(request)?;
        Ok(format!(
            "SmeltFuture::from_future(Box::pin(async move {{ \
             let smelt_request = {request_text}; \
             let smelt_method = reqwest::Method::from_bytes(smelt_request.method().as_bytes()).unwrap_or(reqwest::Method::GET); \
             let mut smelt_builder = reqwest::Client::new().request(smelt_method, smelt_request.url()); \
             for (smelt_name, smelt_value) in smelt_request.headers().entries_in_insertion_order() {{ smelt_builder = smelt_builder.header(smelt_name, smelt_value); }} \
             let smelt_sent = smelt_request.body().take_bytes()?; \
             if !smelt_sent.is_empty() {{ smelt_builder = smelt_builder.body(smelt_sent); }} \
             let smelt_http = smelt_builder.send().await.expect(\"HTTP request failed\"); \
             let smelt_status = f64::from(smelt_http.status().as_u16()); \
             let smelt_reason = smelt_http.status().canonical_reason().unwrap_or_default().to_owned(); \
             let smelt_pairs: Vec<(String, String)> = smelt_http.headers().iter().map(|(smelt_name, smelt_value)| (smelt_name.as_str().to_owned(), smelt_value.to_str().unwrap_or_default().to_owned())).collect(); \
             let smelt_bytes = smelt_http.bytes().await.expect(\"HTTP response body read failed\").to_vec(); \
             Ok::<_, Box<dyn std::error::Error>>(SmeltResponse::from_parts(smelt_status, smelt_reason, SmeltHeaders::from_pairs(smelt_pairs), SmeltBody::from_bytes(smelt_bytes))) }}))"
        ))
    }

    /// Return whether a function-typed operand's type is declared throwing.
    ///
    /// A non-function type answers `false`: an erased handler is called through
    /// the adapter as an infallible value, which is what the erased callable
    /// ABI already guarantees.
    fn function_type_may_throw(&self, ty: TypeId) -> bool {
        matches!(self.mir.types.get(ty), Some(Type::Function(function)) if function.may_throw)
    }
}
