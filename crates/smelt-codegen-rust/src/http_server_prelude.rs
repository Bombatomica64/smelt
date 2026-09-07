//! Runtime prelude for `node:http`'s server surface, backed by hyper 1.
//!
//! Three generated types, one per object the module hands a program:
//!
//! * [`SmeltHttpServer`] — a handler, a bound port, and a shutdown signal;
//! * [`SmeltIncomingMessage`] — one request, plus the listener list that makes
//!   `req.on('data', ..)` work;
//! * [`SmeltServerResponse`] — one response being assembled, and the only
//!   modeled object with settable members.
//!
//! # Nothing here is erased
//!
//! Unlike the `EventEmitter` prelude this module leans on, `node:http` has no
//! dynamic boundary of its own. Every signature the module defines is fixed by
//! the module: a request handler is always `(IncomingMessage, ServerResponse)`,
//! a listening callback always takes nothing, and a header name and value are
//! always strings. So the handler and the listening callback are stored at
//! their real Rust types (`Rc<dyn Fn(SmeltIncomingMessage, SmeltServerResponse)
//! -> Result<(), _>>` and `Rc<dyn Fn() -> Result<(), _>>`) rather than through
//! the erased callable ABI. The one `SmeltUnknown` that reaches this file
//! arrives through the composed [`SmeltEventEmitter`], whose listener store is
//! erased for reasons documented there and nowhere else.
//!
//! # Why the emitter is composed rather than inherited
//!
//! Node's `IncomingMessage` extends `EventEmitter`, and a body is read through
//! that inheritance:
//!
//! ```text
//! req.on('data', (chunk) => { body += chunk; });
//! req.on('end',  () => { res.end(body); });
//! ```
//!
//! Rather than teach the frontend a notion of "a modeled class extends another
//! modeled class", `SmeltIncomingMessage` HOLDS a `SmeltEventEmitter` and
//! forwards the five emitter methods to it under the same names. Codegen's
//! `EventEmitterOp` rendering is then unchanged — `recv.add("data", cb, false)`
//! is valid on both receivers — and there is exactly one listener list
//! implementation, so the two receivers cannot drift apart on ordering,
//! `once` removal, or the emit snapshot.
//!
//! # The one place this differs from Node, and why
//!
//! `res.write(chunk)` BUFFERS instead of streaming: the response is sent to the
//! socket in one piece when `end` is called. A client sees identical bytes and
//! an identical status line; what it does not see is the chunked framing Node
//! would produce for a response written in several calls before `end`. Doing
//! better needs a streaming body channel, which is a separate feature from
//! serving a request at all, so this is stated here rather than hidden.
//!
//! # Request lifetime, in the order it happens
//!
//! 1. hyper accepts a connection and hands over the head and a body stream;
//! 2. the body is collected in full, then the handler is called;
//! 3. the handler registers `data`/`end` listeners and returns;
//! 4. the collected body is DELIVERED — `data` once (when non-empty), then
//!    `end`. Delivering after the handler returns is what Node does too: a
//!    `data` event never fires synchronously inside the handler, so a program
//!    that registers its listeners at the end of the handler still sees the
//!    whole body;
//! 5. the response is awaited until `end` has been called, then sent.

use crate::rust::CodeWriter;

/// Emit the whole `node:http` server runtime.
///
/// Requires the `SmeltEventEmitter` prelude (composed into
/// [`SmeltIncomingMessage`]) and the timer prelude's live-handle counter, both
/// of which `crate::stdlib` gates on this prelude's own use.
pub(crate) fn emit(writer: &mut CodeWriter) {
    emit_incoming_message(writer);
    emit_server_response(writer);
    emit_server(writer);
    emit_connection_glue(writer);
}

/// Emit `SmeltIncomingMessage`: one request, and its listener list.
fn emit_incoming_message(writer: &mut CodeWriter) {
    writer.line("/// A `node:http` `IncomingMessage`: the request half of one exchange.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltIncomingMessage", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("method: String,");
        struct_writer.line("url: String,");
        struct_writer.line("/// Header names lower-cased, duplicates already joined.");
        struct_writer.line("headers: ::std::rc::Rc<Vec<(String, String)>>,");
        struct_writer.line("/// The emitter this message IS, in Node's type hierarchy.");
        struct_writer.line("emitter: SmeltEventEmitter,");
        struct_writer.line("/// The collected body, until `deliver_body` hands it to the listeners.");
        struct_writer.line("pending_body: ::std::rc::Rc<::std::cell::RefCell<Option<Vec<u8>>>>,");
    });
    writer.blank_line();
    writer.line(
        "impl PartialEq for SmeltIncomingMessage { fn eq(&self, other: &Self) -> bool { self.id == other.id } }",
    );
    writer.line(
        "impl ::std::fmt::Debug for SmeltIncomingMessage { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct(\"SmeltIncomingMessage\").field(\"method\", &self.method).field(\"url\", &self.url).finish() } }",
    );
    writer.blank_line();
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltIncomingMessage", |impl_writer| {
        impl_writer.line("/// Build a request from the parts hyper delivered.");
        impl_writer.block(
            "pub fn from_parts(method: String, url: String, headers: Vec<(String, String)>, body: Vec<u8>) -> Self",
            |fn_writer| {
                fn_writer.line("Self { id: smelt_next_object_id(), method, url, headers: ::std::rc::Rc::new(headers), emitter: SmeltEventEmitter::new(), pending_body: ::std::rc::Rc::new(::std::cell::RefCell::new(Some(body))) }");
            },
        );
        impl_writer.line("/// JS reference identity of this message.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// `req.method`.");
        impl_writer.line("pub fn method(&self) -> String { self.method.clone() }");
        impl_writer.line("/// `req.url`: the request target, path and query only.");
        impl_writer.line("pub fn url(&self) -> String { self.url.clone() }");
        impl_writer.line("/// `req.headers`: the lower-cased name/value pairs, in arrival order.");
        impl_writer.line("///");
        impl_writer.line("/// PAIRS rather than a finished map, because the container a");
        impl_writer.line("/// `Record<string, string>` uses is decided per crate -- a plain");
        impl_writer.line("/// `HashMap`, or the reference-semantics `SmeltRecord` when the");
        impl_writer.line("/// program stores the value somewhere that needs JS object identity.");
        impl_writer.line("/// Both build from these pairs through `FromIterator`, so the emit");
        impl_writer.line("/// site names the one it needs and this method commits to neither.");
        impl_writer.line(
            "pub fn headers(&self) -> Vec<(String, String)> { (*self.headers).clone() }",
        );
        // The five emitter methods, forwarded under the SAME names codegen
        // renders for an `EventEmitterOp`. That is the whole of the
        // composition: no dispatch site needs to know which of the two
        // receivers it holds.
        impl_writer.line("/// `on`/`addListener`/`once`, forwarded to the composed emitter.");
        impl_writer.block(
            "pub fn add(&self, event: &str, callback: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>>>, once: bool) -> Self",
            |fn_writer| {
                fn_writer.line("self.emitter.add(event, callback, once);");
                // Node's `req.on(..)` answers the REQUEST, not the emitter, so
                // `req.on(..).on(..)` chains on the request's own type.
                fn_writer.line("self.clone()");
            },
        );
        impl_writer.line("/// `off`/`removeListener`, forwarded to the composed emitter.");
        impl_writer.block(
            "pub fn remove(&self, event: &str, callback: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>>>) -> Self",
            |fn_writer| {
                fn_writer.line("self.emitter.remove(event, callback);");
                fn_writer.line("self.clone()");
            },
        );
        impl_writer.line("/// `removeAllListeners`, forwarded to the composed emitter.");
        impl_writer.block("pub fn remove_all(&self, event: &str) -> Self", |fn_writer| {
            fn_writer.line("self.emitter.remove_all(event);");
            fn_writer.line("self.clone()");
        });
        impl_writer.line("/// `listenerCount`, forwarded to the composed emitter.");
        impl_writer
            .line("pub fn listener_count(&self, event: &str) -> f64 { self.emitter.listener_count(event) }");
        impl_writer.line("/// `emit`, forwarded to the composed emitter.");
        impl_writer.line(
            "pub fn emit(&self, event: &str, args: Vec<SmeltUnknown>) -> Result<bool, Box<dyn ::std::error::Error>> { self.emitter.emit(event, args) }",
        );
        impl_writer.line("/// Hand the collected body to the listeners the handler registered.");
        impl_writer.line("///");
        impl_writer.line("/// One `data` for a non-empty body then one `end`, which is the");
        impl_writer.line("/// event sequence Node produces for a fully buffered request. The");
        impl_writer.line("/// chunk is a STRING rather than a byte view because `body += chunk`");
        impl_writer.line("/// -- the shape every such handler is written in -- stringifies it");
        impl_writer.line("/// in JavaScript anyway.");
        impl_writer.block(
            "pub fn deliver_body(&self) -> Result<(), Box<dyn ::std::error::Error>>",
            |fn_writer| {
                fn_writer.line("let Some(bytes) = self.pending_body.borrow_mut().take() else { return Ok(()); };");
                fn_writer.block("if !bytes.is_empty()", |arm_writer| {
                    arm_writer.line("let chunk = String::from_utf8_lossy(&bytes).into_owned();");
                    arm_writer.line("self.emitter.emit(\"data\", vec![SmeltUnknown::String(chunk.into())])?;");
                });
                fn_writer.line("self.emitter.emit(\"end\", Vec::new())?;");
                fn_writer.line("Ok(())");
            },
        );
    });
    writer.blank_line();
}

/// Emit `SmeltServerResponse`: one response being assembled.
fn emit_server_response(writer: &mut CodeWriter) {
    writer.line("/// A `node:http` `ServerResponse`: the response half of one exchange.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltServerResponse", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("status: ::std::rc::Rc<::std::cell::Cell<u16>>,");
        struct_writer.line("/// Name/value pairs in the order they were set, names lower-cased.");
        struct_writer
            .line("headers: ::std::rc::Rc<::std::cell::RefCell<Vec<(String, String)>>>,");
        struct_writer.line("body: ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>>,");
        struct_writer.line("ended: ::std::rc::Rc<::std::cell::Cell<bool>>,");
    });
    writer.blank_line();
    writer.line(
        "impl PartialEq for SmeltServerResponse { fn eq(&self, other: &Self) -> bool { self.id == other.id } }",
    );
    writer.line(
        "impl ::std::fmt::Debug for SmeltServerResponse { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct(\"SmeltServerResponse\").field(\"status\", &self.status.get()).field(\"ended\", &self.ended.get()).finish() } }",
    );
    writer.line("impl Default for SmeltServerResponse { fn default() -> Self { Self::new() } }");
    writer.blank_line();
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltServerResponse", |impl_writer| {
        impl_writer.line("/// A fresh response: 200, no headers, empty body, not yet sent.");
        impl_writer.line(
            "pub fn new() -> Self { Self { id: smelt_next_object_id(), status: ::std::rc::Rc::new(::std::cell::Cell::new(200)), headers: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), body: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), ended: ::std::rc::Rc::new(::std::cell::Cell::new(false)) } }",
        );
        impl_writer.line("/// JS reference identity of this response.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// `res.statusCode`.");
        impl_writer.line("pub fn status_code(&self) -> f64 { f64::from(self.status.get()) }");
        impl_writer.line("/// `res.statusCode = n`.");
        impl_writer.line("///");
        impl_writer.line("/// A status outside the representable range is clamped rather than");
        impl_writer.line("/// wrapped: `res.statusCode = 70000` must not become a `4464` on the");
        impl_writer.line("/// wire, and there is no status line that could carry it.");
        impl_writer.block("pub fn set_status_code(&self, status: f64) -> f64", |fn_writer| {
            fn_writer.line("let clamped = if status.is_finite() { status.clamp(100.0, 599.0) as u16 } else { 200 };");
            fn_writer.line("self.status.set(clamped);");
            fn_writer.line("status");
        });
        impl_writer.line("/// `res.setHeader(name, value)`: set or REPLACE, case-insensitively.");
        impl_writer.block("pub fn set_header(&self, name: &str, value: &str)", |fn_writer| {
            fn_writer.line("let key = name.to_ascii_lowercase();");
            fn_writer.line("let mut headers = self.headers.borrow_mut();");
            fn_writer.block("if let Some(existing) = headers.iter_mut().find(|(entry, _)| *entry == key)", |arm_writer| {
                arm_writer.line("existing.1 = value.to_owned();");
                arm_writer.line("return;");
            });
            fn_writer.line("headers.push((key, value.to_owned()));");
        });
        impl_writer.line("/// `res.getHeader(name)`: the pending value, or null.");
        impl_writer.line(
            "pub fn get_header(&self, name: &str) -> Option<String> { let key = name.to_ascii_lowercase(); self.headers.borrow().iter().find(|(entry, _)| *entry == key).map(|(_, value)| value.clone()) }",
        );
        impl_writer.line("/// `res.writeHead(status, headers?)`: set the status, merge headers.");
        impl_writer.line("///");
        impl_writer.line("/// Merge rather than replace, as Node does: `writeHead` applies its");
        impl_writer.line("/// object on top of whatever `setHeader` already put there.");
        impl_writer.block(
            "pub fn write_head(&self, status: f64, headers: Option<Vec<(String, String)>>) -> Self",
            |fn_writer| {
                fn_writer.line("self.set_status_code(status);");
                fn_writer.block("if let Some(pairs) = headers", |arm_writer| {
                    arm_writer.block("for (name, value) in pairs", |loop_writer| {
                        loop_writer.line("self.set_header(&name, &value);");
                    });
                });
                // Node answers the response so `writeHead(..).end(..)` chains.
                fn_writer.line("self.clone()");
            },
        );
        impl_writer.line("/// `res.write(chunk)`: append to the buffered body.");
        impl_writer.line(
            "pub fn write(&self, chunk: &str) -> bool { self.body.borrow_mut().extend_from_slice(chunk.as_bytes()); true }",
        );
        impl_writer.line("/// `res.end(chunk?)`: append the last chunk and send.");
        impl_writer.line("///");
        impl_writer.line("/// Ending twice is a no-op rather than an error, which is how Node");
        impl_writer.line("/// treats a second `end` on an already-finished response.");
        impl_writer.block("pub fn end(&self, chunk: Option<String>) -> Self", |fn_writer| {
            fn_writer.block("if !self.ended.get()", |arm_writer| {
                arm_writer.block("if let Some(chunk) = chunk", |chunk_writer| {
                    chunk_writer.line("self.body.borrow_mut().extend_from_slice(chunk.as_bytes());");
                });
                arm_writer.line("self.ended.set(true);");
            });
            fn_writer.line("self.clone()");
        });
        impl_writer.line("/// Whether `end` has been called.");
        impl_writer.line("pub fn is_ended(&self) -> bool { self.ended.get() }");
        impl_writer.line("/// Wait until the handler (or one of its listeners) calls `end`.");
        impl_writer.line("///");
        impl_writer.line("/// A response that is never ended leaves the request hanging, exactly");
        impl_writer.line("/// as it does in Node, so there is deliberately no timeout here. The");
        impl_writer.line("/// poll interval only costs anything while a handler really has not");
        impl_writer.line("/// answered yet.");
        impl_writer.block("pub async fn wait_until_ended(&self)", |fn_writer| {
            fn_writer.block("while !self.ended.get()", |loop_writer| {
                loop_writer
                    .line("tokio::time::sleep(::std::time::Duration::from_millis(1)).await;");
            });
        });
        impl_writer.line("/// The status, headers and body, ready for the wire.");
        impl_writer.line(
            "pub fn into_wire_parts(&self) -> (u16, Vec<(String, String)>, Vec<u8>) { (self.status.get(), self.headers.borrow().clone(), self.body.borrow().clone()) }",
        );
    });
    writer.blank_line();
}

/// Emit `SmeltHttpServer` and its `listen`/`close`/`address` surface.
fn emit_server(writer: &mut CodeWriter) {
    writer.line("/// The handler `createServer` takes: one call per accepted request.");
    writer.line("///");
    writer.line("/// Stored at its real signature rather than through the erased callable");
    writer.line("/// ABI: `node:http` fixes both parameter types, so there is nothing here a");
    writer.line("/// concrete type cannot say.");
    writer.line("pub type SmeltHttpHandler = ::std::rc::Rc<dyn Fn(SmeltIncomingMessage, SmeltServerResponse) -> Result<(), Box<dyn ::std::error::Error>>>;");
    writer.blank_line();
    writer.line("/// A `node:http` `Server`.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltHttpServer", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("handler: SmeltHttpHandler,");
        struct_writer.line("/// The bound port once `listen` has run, for `address()`.");
        struct_writer.line("port: ::std::rc::Rc<::std::cell::Cell<Option<u16>>>,");
        struct_writer.line("/// Sends the accept loop its stop signal; taken by `close`.");
        struct_writer.line("shutdown: ::std::rc::Rc<::std::cell::RefCell<Option<tokio::sync::oneshot::Sender<()>>>>,");
    });
    writer.blank_line();
    writer.line(
        "impl PartialEq for SmeltHttpServer { fn eq(&self, other: &Self) -> bool { self.id == other.id } }",
    );
    writer.line(
        "impl ::std::fmt::Debug for SmeltHttpServer { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct(\"SmeltHttpServer\").field(\"port\", &self.port.get()).finish() } }",
    );
    writer.blank_line();
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltHttpServer", |impl_writer| {
        impl_writer.line("/// `createServer(handler)`.");
        impl_writer.line(
            "pub fn new(handler: SmeltHttpHandler) -> Self { Self { id: smelt_next_object_id(), handler, port: ::std::rc::Rc::new(::std::cell::Cell::new(None)), shutdown: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }",
        );
        impl_writer.line("/// JS reference identity of this server.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// `server.listen(port[, host][, callback])`.");
        impl_writer.line("///");
        impl_writer.line("/// SYNCHRONOUS through the bind, which is what makes");
        impl_writer.line("/// `server.listen(0); server.address()` a legal pair: the socket is");
        impl_writer.line("/// bound with `std::net` before this returns, and only the accept");
        impl_writer.line("/// loop is deferred to a spawned task. Port 0 therefore has a real");
        impl_writer.line("/// port to report the moment `listen` answers.");
        impl_writer.line("///");
        impl_writer.line("/// Listening also registers a LIVE HANDLE, which is what keeps the");
        impl_writer.line("/// program alive at exit -- Node's ref'd-handle rule (see");
        impl_writer.line("/// `smelt_run_until_exit`).");
        impl_writer.block(
            "pub fn listen(&self, port: f64, host: Option<String>, callback: Option<::std::rc::Rc<dyn Fn() -> Result<(), Box<dyn ::std::error::Error>>>>) -> Result<Self, Box<dyn ::std::error::Error>>",
            |fn_writer| {
                fn_writer.line("let host = host.unwrap_or_else(|| \"127.0.0.1\".to_owned());");
                fn_writer.line("let port = if port.is_finite() && port >= 0.0 && port <= 65535.0 { port as u16 } else { 0 };");
                fn_writer.line("let listener = ::std::net::TcpListener::bind((host.as_str(), port))?;");
                fn_writer.line("listener.set_nonblocking(true)?;");
                fn_writer.line("self.port.set(Some(listener.local_addr()?.port()));");
                fn_writer.line("let listener = tokio::net::TcpListener::from_std(listener)?;");
                fn_writer.line("let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();");
                fn_writer.line("*self.shutdown.borrow_mut() = Some(shutdown_tx);");
                fn_writer.line("smelt_retain_handle();");
                fn_writer.line("let handler = ::std::rc::Rc::clone(&self.handler);");
                // `spawn_local`, not `spawn`: every generated value is `Rc`-based,
                // so the handler and everything it captured is not `Send`.
                fn_writer.line("tokio::task::spawn_local(smelt_http_accept_loop(listener, handler, shutdown_rx));");
                fn_writer.block("if let Some(callback) = callback", |arm_writer| {
                    arm_writer.line("callback()?;");
                });
                fn_writer.line("Ok(self.clone())");
            },
        );
        impl_writer.line("/// `server.close()`: stop accepting and release the process.");
        impl_writer.block(
            "pub fn close(&self) -> Result<Self, Box<dyn ::std::error::Error>>",
            |fn_writer| {
                fn_writer.block("if let Some(shutdown) = self.shutdown.borrow_mut().take()", |arm_writer| {
                    // A dropped receiver means the accept loop is already gone;
                    // closing a server twice is not an error in Node either.
                    arm_writer.line("let _ = shutdown.send(());");
                    arm_writer.line("smelt_release_handle();");
                });
                fn_writer.line("self.port.set(None);");
                fn_writer.line("Ok(self.clone())");
            },
        );
        impl_writer.line("/// `server.address()`: the bound port, or null when not listening.");
        impl_writer.line("pub fn address(&self) -> Option<f64> { self.port.get().map(f64::from) }");
    });
    writer.blank_line();
}

/// Emit the accept loop and the hyper service that drives one request.
fn emit_connection_glue(writer: &mut CodeWriter) {
    writer.line("/// Accept connections until `close` signals, serving each on its own task.");
    writer.block(
        "async fn smelt_http_accept_loop(listener: tokio::net::TcpListener, handler: SmeltHttpHandler, mut shutdown: tokio::sync::oneshot::Receiver<()>)",
        |fn_writer| {
            fn_writer.block("loop", |loop_writer| {
                loop_writer.block("let stream = tokio::select!", |select_writer| {
                    select_writer.line("accepted = listener.accept() => match accepted { Ok((stream, _)) => stream, Err(_) => continue },");
                    select_writer.line("_ = &mut shutdown => break,");
                });
                loop_writer.line(";");
                loop_writer.line("let handler = ::std::rc::Rc::clone(&handler);");
                loop_writer.block("tokio::task::spawn_local(async move", |task_writer| {
                    task_writer.line("let io = hyper_util::rt::TokioIo::new(stream);");
                    task_writer.block("let service = hyper::service::service_fn(move |request: hyper::Request<hyper::body::Incoming>|", |service_writer| {
                        service_writer.line("let handler = ::std::rc::Rc::clone(&handler);");
                        service_writer.line("async move { smelt_http_dispatch(request, handler).await }");
                    });
                    task_writer.line(");");
                    // `http1` on a `LocalSet`: hyper 1's HTTP/1 connection has
                    // no `Send` bound, which is exactly why the non-`Send`
                    // handler can serve it.
                    task_writer.line("let _ = hyper::server::conn::http1::Builder::new().serve_connection(io, service).await;");
                });
                loop_writer.line(");");
            });
        },
    );
    writer.blank_line();
    writer.line("/// Run one request through the program's handler and answer hyper.");
    writer.line("///");
    writer.line("/// Infallible toward hyper because a handler error never gets that far:");
    writer.line("/// see `smelt_http_handler_failed`.");
    writer.block(
        "async fn smelt_http_dispatch(request: hyper::Request<hyper::body::Incoming>, handler: SmeltHttpHandler) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>, ::std::convert::Infallible>",
        |fn_writer| {
            fn_writer.line("let (parts, body) = request.into_parts();");
            fn_writer.line("let method = parts.method.as_str().to_owned();");
            fn_writer.line("let url = parts.uri.path_and_query().map_or_else(|| \"/\".to_owned(), |target| target.as_str().to_owned());");
            // Node joins repeated headers with ", " into one entry of the plain
            // object; `set-cookie` is the documented exception, and it is an
            // array there rather than a string, so it takes the same join here
            // to keep the map's value type a string.
            fn_writer.line("let mut headers: Vec<(String, String)> = Vec::new();");
            fn_writer.block("for (name, value) in &parts.headers", |loop_writer| {
                loop_writer.line("let key = name.as_str().to_ascii_lowercase();");
                loop_writer.line("let text = value.to_str().unwrap_or_default().to_owned();");
                loop_writer.block("match headers.iter_mut().find(|(existing, _)| *existing == key)", |match_writer| {
                    match_writer.line("Some(existing) => { existing.1.push_str(\", \"); existing.1.push_str(&text); }");
                    match_writer.line("None => headers.push((key, text)),");
                });
            });
            fn_writer.line("let collected = <hyper::body::Incoming as http_body_util::BodyExt>::collect(body).await.map(|body| body.to_bytes().to_vec()).unwrap_or_default();");
            fn_writer.line("let message = SmeltIncomingMessage::from_parts(method, url, headers, collected);");
            fn_writer.line("let response = SmeltServerResponse::new();");
            fn_writer.block("if let Err(error) = handler(message.clone(), response.clone())", |arm_writer| {
                arm_writer.line("smelt_http_handler_failed(&*error);");
            });
            // The handler has registered its listeners by now, so the body can
            // be delivered; see this module's request-lifetime note. A listener
            // that throws is the same uncaught exception as a throwing handler,
            // because that is where the real work of a handler happens.
            fn_writer.block("if let Err(error) = message.deliver_body()", |arm_writer| {
                arm_writer.line("smelt_http_handler_failed(&*error);");
            });
            fn_writer.line("response.wait_until_ended().await;");
            fn_writer.line("let (status, headers, body) = response.into_wire_parts();");
            fn_writer.line("let mut builder = hyper::Response::builder().status(status);");
            fn_writer.block("for (name, value) in headers", |loop_writer| {
                loop_writer.line("builder = builder.header(name, value);");
            });
            // A header a program set but hyper rejects (an invalid name, say)
            // is a programming error with no response to send, so it takes the
            // same route as a throwing handler rather than becoming a silent
            // 200 with the header missing.
            fn_writer.line("match builder.body(http_body_util::Full::new(bytes::Bytes::from(body))) {");
            fn_writer.line("    Ok(response) => Ok(response),");
            fn_writer.line("    Err(error) => smelt_http_handler_failed(&error),");
            fn_writer.line("}");
        },
    );
    writer.blank_line();
    writer.line("/// End the program the way Node ends it for a throwing handler.");
    writer.line("///");
    writer.line("/// Measured, not assumed: an uncaught exception thrown inside a Node");
    writer.line("/// request handler is NOT turned into a 500 -- it reaches the process as an");
    writer.line("/// uncaught exception, prints, and exits 1. The server does not carry on");
    writer.line("/// serving.");
    writer.line("///");
    writer.line("/// Answering 500 instead was the tempting shape, and it is worse than");
    writer.line("/// wrong: a generated program would keep serving where the original had");
    writer.line("/// stopped, so the bug that killed the Node process would show up as a run");
    writer.line("/// of quiet 500s instead. Exiting is the honest translation.");
    writer.line("///");
    writer.line("/// Not a panic: a panic inside a `spawn_local` connection task is caught");
    writer.line("/// by the runtime, which would drop just that connection and leave the");
    writer.line("/// program running -- neither Node's behaviour nor a useful one.");
    writer.block(
        "fn smelt_http_handler_failed(error: &dyn ::std::error::Error) -> !",
        |fn_writer| {
            fn_writer.line("eprintln!(\"Uncaught {error}\");");
            fn_writer.line("::std::process::exit(1)");
        },
    );
    writer.blank_line();
}
