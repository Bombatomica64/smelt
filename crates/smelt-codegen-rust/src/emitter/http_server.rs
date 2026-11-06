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

use crate::rust::RustExpr;
use super::*;

impl FunctionEmitter<'_> {
    /// Emit `createServer(handler)`.
    ///
    /// The handler operand carries the source's own function type. It is
    /// wrapped rather than passed through so the server always holds the one
    /// signature `SmeltHttpHandler` names, whatever the source's arrow was
    /// inferred as.
    pub(super) fn http_create_server_expr(
        &self,
        handler: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let adapted = self.http_handler_adapter(handler)?;
        Ok(RustExpr::atom(format!("SmeltHttpServer::new({adapted})")))
    }

    /// Emit a server answering requests with a fetch handler.
    ///
    /// The handler is called the way the Node adapter calls it: with the
    /// request alone, every further parameter absent. Those must be optional
    /// (Hono's `app.fetch(request, env?, executionCtx?)`), so each takes its
    /// absent value. Whatever the handler answers — a `Response`, a promise of
    /// one, or a union of the two — is erased through the ordinary erasure and
    /// written back by `smelt_fetch_server_respond`, which awaits a promise;
    /// a throwing handler's error becomes the adapter's 500.
    pub(super) fn http_create_fetch_server_expr(
        &self,
        fetch: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let ty = self.operand_ty(fetch)?;
        let Some(Type::Function(function)) = self.mir.types.get(ty) else {
            return Err(EmitError::new("`serve` needs a function as its `fetch` handler"));
        };
        let Some(request_ty) = function.params.first().copied() else {
            return Err(EmitError::new("a `fetch` handler takes the request as its first parameter"));
        };
        let mut text = self.operand_expr(fetch)?;
        if matches!(fetch, Operand::Copy(_)) {
            text = RustExpr::postfix_on(&text, format!("{text}.clone()")).ending_in_clone();
        }
        let request_by_reference = function.mutable_params.contains(&0)
            || self.callback_param_is_shared_reference(function, 0, request_ty);
        let mut args = vec![if request_by_reference {
            "&smelt_fetch_request".to_owned()
        } else {
            "smelt_fetch_request".to_owned()
        }];
        let required = function.required_params.unwrap_or(function.params.len());
        for (index, param) in function.params.iter().enumerate().skip(1) {
            if index < required {
                return Err(EmitError::new(
                    "a `fetch` handler is called with the request alone, so its other parameters must be optional",
                ));
            }
            args.push(self.default_value(*param)?.into_string());
        }
        let call = format!("smelt_fetch({})", args.join(", "));
        let erased = self.erase_rendered(&RustExpr::path("smelt_answer"), function.return_ty)?;
        let answer = if function.may_throw {
            format!("{call}.map(|smelt_answer| {erased})")
        } else {
            self.carrier_format(format_args!("{{ let smelt_answer = {call}; Ok::<SmeltUnknown, Box<dyn ::std::error::Error>>({erased}) }}"))
        };
        Ok(RustExpr::atom(format!(
            "SmeltHttpServer::new({{ let smelt_fetch = {text}; ::std::rc::Rc::new(move |smelt_request: SmeltIncomingMessage, smelt_response: SmeltServerResponse| {{ let smelt_fetch_request = smelt_fetch_server_request(&smelt_request); smelt_fetch_server_respond(smelt_response, {answer}); Ok(()) }}) as SmeltHttpHandler }})"
        )))
    }

    /// Wrap a source request handler into a `SmeltHttpHandler`.
    ///
    /// A `copy` of a function-typed operand is left un-cloned by
    /// `operand_expr` (a callee position only borrows it), but this adapter
    /// MOVES the value into a closure, so it takes its own handle — the same
    /// correction the emitter's listener erasure needs.
    fn http_handler_adapter(&self, handler: &Operand) -> Result<RustExpr, EmitError> {
        let ty = self.operand_ty(handler)?;
        let mut text = self.operand_expr(handler)?;
        if matches!(handler, Operand::Copy(_))
            && matches!(self.mir.types.get(ty), Some(Type::Function(_)))
        {
            text = RustExpr::postfix_on(&text, format!("{text}.clone()")).ending_in_clone();
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
        Ok(RustExpr::atom(format!(
            "{{ let smelt_handler = {text}; ::std::rc::Rc::new(move |smelt_request: SmeltIncomingMessage, smelt_response: SmeltServerResponse| {{ {call} Ok(()) }}) as SmeltHttpHandler }}"
        )))
    }

    /// Emit a `node:http` `Server` member operation.
    pub(super) fn http_server_op_expr(
        &self,
        op: smelt_hir::HttpServerOp,
        server: &Operand,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let receiver = self.operand_expr(server)?;
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
                        format!("Some({})", self.http_listen_callback_expr(callback)?)
                    }
                    None => "None".to_owned(),
                };
                Ok(RustExpr::loose(format!(
                    "{receiver}.listen({port_text}, {host_text}, {callback_text})?"
                )))
            }
            smelt_hir::HttpServerOp::Close => Ok(RustExpr::loose(format!("{receiver}.close()?"))),
            smelt_hir::HttpServerOp::Address => {
                let record = self.address_info_record_expr("address", "family", "port")?;
                Ok(RustExpr::postfix_on(&receiver, format!(
                    "{receiver}.address().map(|(address, family, port)| {record})"
                )))
            }
        }
    }

    /// Build the generated `AddressInfo` record from three bound names.
    ///
    /// The frontend declares `AddressInfo` as an interface with the three
    /// required keys Node answers (`address`, `family`, `port`), so its Rust
    /// storage is the ordinary generated struct: a by-value `AddressInfo { .. }`,
    /// or the shared-cell handle when the program writes one of its fields.
    /// Nothing here is erased — `server.address()` is `Option<AddressInfo>`.
    fn address_info_record_expr(
        &self,
        address: &str,
        family: &str,
        port: &str,
    ) -> Result<RustExpr, EmitError> {
        let interface = self
            .mir
            .interfaces
            .iter()
            .find(|interface| self.mir.symbols.get(interface.name) == Some("AddressInfo"))
            .ok_or_else(|| {
                EmitError::new("`server.address()` needs the AddressInfo record the frontend declares")
            })?;
        let fields = format!("address: {address}, family: {family}, port: {port}");
        if self.context.is_reference_class(interface.name) {
            return Ok(RustExpr::chain(format!(
                "AddressInfo(::std::rc::Rc::new(::std::cell::RefCell::new(AddressInfoInner {{ {fields} }})))"
            )));
        }
        Ok(RustExpr::loose(format!("AddressInfo {{ {fields} }}")))
    }

    /// Wrap a source listening callback into the runtime's `Rc<dyn Fn() -> ..>`.
    fn http_listen_callback_expr(&self, callback: &Operand) -> Result<RustExpr, EmitError> {
        let ty = self.operand_ty(callback)?;
        let mut text = self.operand_expr(callback)?;
        if matches!(callback, Operand::Copy(_))
            && matches!(self.mir.types.get(ty), Some(Type::Function(_)))
        {
            text = RustExpr::postfix_on(&text, format!("{text}.clone()")).ending_in_clone();
        }
        let call = if self.function_type_may_throw(ty) {
            "smelt_listening()?;"
        } else {
            "let _ = smelt_listening();"
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_listening = {text}; ::std::rc::Rc::new(move || {{ {call} Ok(()) }}) as ::std::rc::Rc<dyn Fn() -> Result<(), Box<dyn ::std::error::Error>>> }}"
        )))
    }

    /// Emit a `node:http` `IncomingMessage` property read.
    ///
    /// `headers` needs the DESTINATION type: `req.headers` is a
    /// `Record<string, string>`, and which Rust container that is depends on
    /// the crate — a plain `HashMap`, or the reference-semantics `SmeltRecord`
    /// when the program stores the value where JS object identity matters. The
    /// runtime answers name/value pairs and the container is named here, so
    /// neither choice is baked into the prelude.
    pub(super) fn incoming_message_op_expr(
        &self,
        op: smelt_hir::IncomingMessageOp,
        message: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver = self.operand_expr(message)?;
        Ok(match op {
            smelt_hir::IncomingMessageOp::Method => RustExpr::postfix_on(&receiver, format!("{receiver}.method()")),
            smelt_hir::IncomingMessageOp::Url => RustExpr::postfix_on(&receiver, format!("{receiver}.url()")),
            smelt_hir::IncomingMessageOp::Headers => {
                let container = self.signature_type(dest_ty)?;
                RustExpr::loose(format!(
                    "<{container} as ::std::iter::FromIterator<(String, String)>>::from_iter({receiver}.headers())"
                ))
            }
        })
    }

    /// Emit a `node:http` `ServerResponse` member operation.
    pub(super) fn server_response_op_expr(
        &self,
        op: smelt_hir::ServerResponseOp,
        response: &Operand,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let receiver = self.operand_expr(response)?;
        let string_ty = self.type_id(Type::String)?;
        let float_ty = self.type_id(Type::Float)?;
        let arg_at = |index: usize| -> Result<&Operand, EmitError> {
            args.get(index).ok_or_else(|| {
                EmitError::new(format!("`ServerResponse` operation {op:?} is missing an argument"))
            })
        };
        Ok(match op {
            smelt_hir::ServerResponseOp::StatusCode => RustExpr::postfix_on(&receiver, format!("{receiver}.status_code()")),
            smelt_hir::ServerResponseOp::SetStatusCode => {
                let value = self.value_at_type(arg_at(0)?, float_ty)?;
                RustExpr::postfix_on(&receiver, format!("{receiver}.set_status_code({value})"))
            }
            smelt_hir::ServerResponseOp::SetHeader => {
                let name = self.value_at_type(arg_at(0)?, string_ty)?;
                let value = self.value_at_type(arg_at(1)?, string_ty)?;
                // Answers the response so the statement has a value to discard
                // and `res.setHeader(..)` composes like the other members.
                RustExpr::atom(format!("{{ {receiver}.set_header(&{name}, &{value}); {receiver}.clone() }}"))
            }
            smelt_hir::ServerResponseOp::GetHeader => {
                let name = self.value_at_type(arg_at(0)?, string_ty)?;
                RustExpr::postfix_on(&receiver, format!("{receiver}.get_header(&{name})"))
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
                        let headers_value = self.operand_expr(headers)?;
                        let converted =
                            self.headers_conversion_expr(&headers_value, headers_ty)?;
                        format!("Some({converted}.entries_in_insertion_order())")
                    }
                    None => "None".to_owned(),
                };
                RustExpr::postfix_on(&receiver, format!("{receiver}.write_head({status}, {headers_text})"))
            }
            smelt_hir::ServerResponseOp::Write => {
                let chunk = self.value_at_type(arg_at(0)?, string_ty)?;
                RustExpr::postfix_on(&receiver, format!("{receiver}.write(&{chunk})"))
            }
            smelt_hir::ServerResponseOp::End => {
                let chunk = match args.first() {
                    Some(chunk) => format!("Some({})", self.value_at_type(chunk, string_ty)?),
                    None => "None".to_owned(),
                };
                RustExpr::postfix_on(&receiver, format!("{receiver}.end({chunk})"))
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
    pub(super) fn http_fetch_request_expr(
        &self,
        request: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let request_text = self.operand_expr(request)?;
        Ok(RustExpr::atom(format!(
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
             Ok::<_, {ERROR_BOX}>(SmeltResponse::from_parts(smelt_status, smelt_reason, SmeltHeaders::from_pairs(smelt_pairs), SmeltBody::from_bytes(smelt_bytes))) }}))"
        )))
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
