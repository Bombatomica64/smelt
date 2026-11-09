// Runtime prelude for `node:http`'s server surface, backed by hyper 1.
//
// Three generated types, one per object the module hands a program:
//
// * [`SmeltHttpServer`] — a handler, a bound port, and a shutdown signal;
// * [`SmeltIncomingMessage`] — one request, plus the listener list that makes
//   `req.on('data', ..)` work;
// * [`SmeltServerResponse`] — one response being assembled, and the only
//   modeled object with settable members.
//
// # Nothing here is erased
//
// Unlike the `EventEmitter` prelude this module leans on, `node:http` has no
// dynamic boundary of its own. Every signature the module defines is fixed by
// the module: a request handler is always `(IncomingMessage, ServerResponse)`,
// a listening callback always takes nothing, and a header name and value are
// always strings. So the handler and the listening callback are stored at
// their real Rust types (`Rc<dyn Fn(SmeltIncomingMessage, SmeltServerResponse)
// -> Result<(), _>>` and `Rc<dyn Fn() -> Result<(), _>>`) rather than through
// the erased callable ABI. The one `SmeltUnknown` that reaches this file
// arrives through the composed [`SmeltEventEmitter`], whose listener store is
// erased for reasons documented there and nowhere else.
//
// # Why the emitter is composed rather than inherited
//
// Node's `IncomingMessage` extends `EventEmitter`, and a body is read through
// that inheritance:
//
// ```text
// req.on('data', (chunk) => { body += chunk; });
// req.on('end',  () => { res.end(body); });
// ```
//
// Rather than teach the frontend a notion of "a modeled class extends another
// modeled class", `SmeltIncomingMessage` HOLDS a `SmeltEventEmitter` and
// forwards the five emitter methods to it under the same names. Codegen's
// `EventEmitterOp` rendering is then unchanged — `recv.add("data", cb, false)`
// is valid on both receivers — and there is exactly one listener list
// implementation, so the two receivers cannot drift apart on ordering,
// `once` removal, or the emit snapshot.
//
// # The one place this differs from Node, and why
//
// `res.write(chunk)` BUFFERS instead of streaming: the response is sent to the
// socket in one piece when `end` is called. A client sees identical bytes and
// an identical status line; what it does not see is the chunked framing Node
// would produce for a response written in several calls before `end`. Doing
// better needs a streaming body channel, which is a separate feature from
// serving a request at all, so this is stated here rather than hidden.
//
// # Request lifetime, in the order it happens
//
// 1. hyper accepts a connection and hands over the head and a body stream;
// 2. the body is collected in full, then the handler is called;
// 3. the handler registers `data`/`end` listeners and returns;
// 4. the collected body is DELIVERED — `data` once (when non-empty), then
//    `end`. Delivering after the handler returns is what Node does too: a
//    `data` event never fires synchronously inside the handler, so a program
//    that registers its listeners at the end of the handler still sees the
//    whole body;
// 5. the response is awaited until `end` has been called, then sent.

// Emit `SmeltIncomingMessage`: one request, and its listener list.
// @smelt:item SmeltIncomingMessage when http_server
/// A `node:http` `IncomingMessage`: the request half of one exchange.
#[derive(Clone)]
pub struct SmeltIncomingMessage {
    id: usize,
    method: String,
    url: String,
    //~ Header names lower-cased, duplicates already joined.
    headers: ::std::rc::Rc<Vec<(String, String)>>,
    //~ The emitter this message IS, in Node's type hierarchy.
    emitter: SmeltEventEmitter,
    //~ The collected body, until `deliver_body` hands it to the listeners.
    pending_body: ::std::rc::Rc<::std::cell::RefCell<Option<Vec<u8>>>>,
}

impl PartialEq for SmeltIncomingMessage { fn eq(&self, other: &Self) -> bool { self.id == other.id } }
impl ::std::fmt::Debug for SmeltIncomingMessage { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltIncomingMessage").field("method", &self.method).field("url", &self.url).finish() } }

#[allow(dead_code)]
impl SmeltIncomingMessage {
    //~ Build a request from the parts hyper delivered.
    pub fn from_parts(method: String, url: String, headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        Self { id: smelt_next_object_id(), method, url, headers: ::std::rc::Rc::new(headers), emitter: SmeltEventEmitter::new(), pending_body: ::std::rc::Rc::new(::std::cell::RefCell::new(Some(body))) }
    }
    //~ JS reference identity of this message.
    pub fn id(&self) -> usize { self.id }
    //~ `req.method`.
    pub fn method(&self) -> String { self.method.clone() }
    //~ `req.url`: the request target, path and query only.
    pub fn url(&self) -> String { self.url.clone() }
    //~ `req.headers`: the lower-cased name/value pairs, in arrival order.
    //~
    //~ PAIRS rather than a finished map, because the container a
    //~ `Record<string, string>` uses is decided per crate -- a plain
    //~ `HashMap`, or the reference-semantics `SmeltRecord` when the
    //~ program stores the value somewhere that needs JS object identity.
    //~ Both build from these pairs through `FromIterator`, so the emit
    //~ site names the one it needs and this method commits to neither.
    pub fn headers(&self) -> Vec<(String, String)> { (*self.headers).clone() }
    //~ The five emitter methods, forwarded under the SAME names codegen
    //~ renders for an `EventEmitterOp`. That is the whole of the
    //~ composition: no dispatch site needs to know which of the two
    //~ receivers it holds.
    //~ `on`/`addListener`/`once`, forwarded to the composed emitter.
    pub fn add(&self, event: &(impl SmeltHostText + ?Sized), callback: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>>>, once: bool) -> Self { let event = event.host_text(); let event: &str = &event;
        self.emitter.add(event, callback, once);
        //~ Node's `req.on(..)` answers the REQUEST, not the emitter, so
        //~ `req.on(..).on(..)` chains on the request's own type.
        self.clone()
    }
    //~ `off`/`removeListener`, forwarded to the composed emitter.
    pub fn remove(&self, event: &(impl SmeltHostText + ?Sized), callback: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>>>) -> Self { let event = event.host_text(); let event: &str = &event;
        self.emitter.remove(event, callback);
        self.clone()
    }
    //~ `removeAllListeners`, forwarded to the composed emitter.
    pub fn remove_all(&self, event: &(impl SmeltHostText + ?Sized)) -> Self { let event = event.host_text(); let event: &str = &event;
        self.emitter.remove_all(event);
        self.clone()
    }
    //~ `listenerCount`, forwarded to the composed emitter.
    pub fn listener_count(&self, event: &(impl SmeltHostText + ?Sized)) -> f64 { let event = event.host_text(); let event: &str = &event; self.emitter.listener_count(event) }
    //~ `emit`, forwarded to the composed emitter.
    pub fn emit(&self, event: &(impl SmeltHostText + ?Sized), args: Vec<SmeltUnknown>) -> Result<bool, Box<dyn ::std::error::Error>> { let event = event.host_text(); let event: &str = &event; self.emitter.emit(event, args) }
    //~ Hand the collected body to the listeners the handler registered.
    //~
    //~ One `data` for a non-empty body then one `end`, which is the
    //~ event sequence Node produces for a fully buffered request. The
    //~ chunk is a STRING rather than a byte view because `body += chunk`
    //~ -- the shape every such handler is written in -- stringifies it
    //~ in JavaScript anyway.
    pub fn deliver_body(&self) -> Result<(), Box<dyn ::std::error::Error>> {
        let Some(bytes) = self.pending_body.borrow_mut().take() else { return Ok(()); };
        if !bytes.is_empty() {
            let chunk = String::from_utf8_lossy(&bytes).into_owned();
            self.emitter.emit("data", vec![SmeltUnknown::String(chunk.into())])?;
        }
        self.emitter.emit("end", Vec::new())?;
        Ok(())
    }
}

//~ Emit the whole `node:http` server runtime.
//~
//~ Requires the `SmeltEventEmitter` prelude (composed into
//~ [`SmeltIncomingMessage`]) and the timer prelude's live-handle counter, both
//~ of which `crate::stdlib` gates on this prelude's own use.
/// `SmeltIncomingMessage` with its listener list held weakly.
#[derive(Clone)]
struct SmeltWeakIncomingMessage { id: usize, method: String, url: String, headers: ::std::rc::Rc<Vec<(String, String)>>, emitter: SmeltWeakEventEmitter, pending_body: ::std::rc::Rc<::std::cell::RefCell<Option<Vec<u8>>>> }

impl SmeltEmitterHandle for SmeltIncomingMessage {
    type Weak = SmeltWeakIncomingMessage;
    fn smelt_downgrade(&self) -> Self::Weak { SmeltWeakIncomingMessage { id: self.id, method: self.method.clone(), url: self.url.clone(), headers: self.headers.clone(), emitter: self.emitter.smelt_downgrade(), pending_body: self.pending_body.clone() } }
    fn smelt_upgrade(weak: &Self::Weak) -> Option<Self> { Some(Self { id: weak.id, method: weak.method.clone(), url: weak.url.clone(), headers: weak.headers.clone(), emitter: SmeltEventEmitter::smelt_upgrade(&weak.emitter)?, pending_body: weak.pending_body.clone() }) }
}

//~ Emit `SmeltServerResponse`: one response being assembled.
/// A `node:http` `ServerResponse`: the response half of one exchange.
#[derive(Clone)]
pub struct SmeltServerResponse {
    id: usize,
    status: ::std::rc::Rc<::std::cell::Cell<u16>>,
    //~ Name/value pairs in the order they were set, names lower-cased.
    headers: ::std::rc::Rc<::std::cell::RefCell<Vec<(String, String)>>>,
    body: ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>>,
    ended: ::std::rc::Rc<::std::cell::Cell<bool>>,
    //~ Signalled by `end`, so the connection task wakes as soon as it answers.
    ended_signal: ::std::rc::Rc<tokio::sync::Notify>,
}

impl PartialEq for SmeltServerResponse { fn eq(&self, other: &Self) -> bool { self.id == other.id } }
impl ::std::fmt::Debug for SmeltServerResponse { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltServerResponse").field("status", &self.status.get()).field("ended", &self.ended.get()).finish() } }
impl Default for SmeltServerResponse { fn default() -> Self { Self::new() } }

#[allow(dead_code)]
impl SmeltServerResponse {
    //~ A fresh response: 200, no headers, empty body, not yet sent.
    pub fn new() -> Self { Self { id: smelt_next_object_id(), status: ::std::rc::Rc::new(::std::cell::Cell::new(200)), headers: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), body: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), ended: ::std::rc::Rc::new(::std::cell::Cell::new(false)), ended_signal: ::std::rc::Rc::new(tokio::sync::Notify::new()) } }
    //~ JS reference identity of this response.
    pub fn id(&self) -> usize { self.id }
    //~ `res.statusCode`.
    pub fn status_code(&self) -> f64 { f64::from(self.status.get()) }
    //~ `res.statusCode = n`.
    //~
    //~ A status outside the representable range is clamped rather than
    //~ wrapped: `res.statusCode = 70000` must not become a `4464` on the
    //~ wire, and there is no status line that could carry it.
    pub fn set_status_code(&self, status: f64) -> f64 {
        let clamped = if status.is_finite() { status.clamp(100.0, 599.0) as u16 } else { 200 };
        self.status.set(clamped);
        status
    }
    //~ `res.setHeader(name, value)`: set or REPLACE, case-insensitively.
    pub fn set_header(&self, name: &(impl SmeltHostText + ?Sized), value: &(impl SmeltHostText + ?Sized)) { let name = name.host_text(); let name: &str = &name; let value = value.host_text(); let value: &str = &value;
        let key = name.to_ascii_lowercase();
        let mut headers = self.headers.borrow_mut();
        if let Some(existing) = headers.iter_mut().find(|(entry, _)| *entry == key) {
            existing.1 = value.to_owned();
            return;
        }
        headers.push((key, value.to_owned()));
    }
    //~ `res.getHeader(name)`: the pending value, or null.
    pub fn get_header(&self, name: &(impl SmeltHostText + ?Sized)) -> Option<String> { let name = name.host_text(); let name: &str = &name; let key = name.to_ascii_lowercase(); self.headers.borrow().iter().find(|(entry, _)| *entry == key).map(|(_, value)| value.clone()) }
    //~ `res.writeHead(status, headers?)`: set the status, merge headers.
    //~
    //~ Merge rather than replace, as Node does: `writeHead` applies its
    //~ object on top of whatever `setHeader` already put there.
    pub fn write_head(&self, status: f64, headers: Option<Vec<(String, String)>>) -> Self {
        self.set_status_code(status);
        if let Some(pairs) = headers {
            for (name, value) in pairs {
                self.set_header(&name, &value);
            }
        }
        //~ Node answers the response so `writeHead(..).end(..)` chains.
        self.clone()
    }
    //~ `res.write(chunk)`: append to the buffered body.
    pub fn write(&self, chunk: &(impl SmeltHostText + ?Sized)) -> bool { let chunk = chunk.host_text(); let chunk: &str = &chunk; self.body.borrow_mut().extend_from_slice(chunk.as_bytes()); true }
    //~ `res.end(chunk?)`: append the last chunk and send.
    //~
    //~ Ending twice is a no-op rather than an error, which is how Node
    //~ treats a second `end` on an already-finished response.
    pub fn end(&self, chunk: Option<String>) -> Self {
        if !self.ended.get() {
            if let Some(chunk) = chunk {
                self.body.borrow_mut().extend_from_slice(chunk.as_bytes());
            }
            self.ended.set(true);
            self.ended_signal.notify_one();
        }
        self.clone()
    }
    //~ Whether `end` has been called.
    pub fn is_ended(&self) -> bool { self.ended.get() }
    //~ Wait until the handler (or one of its listeners) calls `end`.
    //~
    //~ A response that is never ended leaves the request hanging, exactly
    //~ as it does in Node, so there is deliberately no timeout here. `end`
    //~ signals the wait directly: this used to poll every millisecond, which
    //~ added up to a millisecond of idle time to every request a handler
    //~ answered asynchronously and capped a loaded server at roughly
    //~ connections per millisecond, whatever its CPU cost per request.
    pub async fn wait_until_ended(&self) {
        while !self.ended.get() {
            self.ended_signal.notified().await;
        }
    }
    //~ The status, headers and body, moved out for the wire.
    //~
    //~ Called once, after `end`, when nothing can write the response any more;
    //~ taking the buffers spares a copy of every header and the whole body.
    pub fn into_wire_parts(&self) -> (u16, Vec<(String, String)>, Vec<u8>) { (self.status.get(), ::std::mem::take(&mut *self.headers.borrow_mut()), ::std::mem::take(&mut *self.body.borrow_mut())) }
}

//~ Emit `SmeltHttpServer` and its `listen`/`close`/`address` surface.
/// The handler `createServer` takes: one call per accepted request.
//~
//~ Stored at its real signature rather than through the erased callable
//~ ABI: `node:http` fixes both parameter types, so there is nothing here a
//~ concrete type cannot say.
pub type SmeltHttpHandler = ::std::rc::Rc<dyn Fn(SmeltIncomingMessage, SmeltServerResponse) -> Result<(), Box<dyn ::std::error::Error>>>;

/// A `node:http` `Server`.
#[derive(Clone)]
pub struct SmeltHttpServer {
    id: usize,
    handler: SmeltHttpHandler,
    //~ The bound socket address once `listen` has run, for `address()`.
    local: ::std::rc::Rc<::std::cell::Cell<Option<::std::net::SocketAddr>>>,
    //~ Sends the accept loop its stop signal; taken by `close`.
    shutdown: ::std::rc::Rc<::std::cell::RefCell<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl PartialEq for SmeltHttpServer { fn eq(&self, other: &Self) -> bool { self.id == other.id } }
impl ::std::fmt::Debug for SmeltHttpServer { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltHttpServer").field("local", &self.local.get()).finish() } }

#[allow(dead_code)]
impl SmeltHttpServer {
    //~ `createServer(handler)`.
    pub fn new(handler: SmeltHttpHandler) -> Self { Self { id: smelt_next_object_id(), handler, local: ::std::rc::Rc::new(::std::cell::Cell::new(None)), shutdown: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }
    //~ JS reference identity of this server.
    pub fn id(&self) -> usize { self.id }
    //~ `server.listen(port[, host][, callback])`.
    //~
    //~ SYNCHRONOUS through the bind, which is what makes
    //~ `server.listen(0); server.address()` a legal pair: the socket is
    //~ bound with `std::net` before this returns, and only the accept
    //~ loop is deferred to a spawned task. Port 0 therefore has a real
    //~ port to report the moment `listen` answers.
    //~
    //~ Listening also registers a LIVE HANDLE, which is what keeps the
    //~ program alive at exit -- Node's ref'd-handle rule (see
    //~ `smelt_run_until_exit`).
    pub fn listen(&self, port: f64, host: Option<String>, callback: Option<::std::rc::Rc<dyn Fn() -> Result<(), Box<dyn ::std::error::Error>>>>) -> Result<Self, Box<dyn ::std::error::Error>> {
        let host = host.unwrap_or_else(|| "127.0.0.1".to_owned());
        let port = if port.is_finite() && port >= 0.0 && port <= 65535.0 { port as u16 } else { 0 };
        let listener = ::std::net::TcpListener::bind((host.as_str(), port))?;
        listener.set_nonblocking(true)?;
        self.local.set(Some(listener.local_addr()?));
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        *self.shutdown.borrow_mut() = Some(shutdown_tx);
        smelt_retain_handle();
        let handler = ::std::rc::Rc::clone(&self.handler);
        //~ `spawn_local`, not `spawn`: every generated value is `Rc`-based,
        //~ so the handler and everything it captured is not `Send`.
        tokio::task::spawn_local(smelt_http_accept_loop(listener, handler, shutdown_rx));
        if let Some(callback) = callback {
            callback()?;
        }
        Ok(self.clone())
    }
    //~ `server.close()`: stop accepting and release the process.
    pub fn close(&self) -> Result<Self, Box<dyn ::std::error::Error>> {
        if let Some(shutdown) = self.shutdown.borrow_mut().take() {
            //~ A dropped receiver means the accept loop is already gone;
            //~ closing a server twice is not an error in Node either.
            let _ = shutdown.send(());
            smelt_release_handle();
        }
        self.local.set(None);
        Ok(self.clone())
    }
    //~ `server.address()`: Node's `AddressInfo` fields of the bound socket --
    //~ `(address, family, port)`, e.g. `("127.0.0.1", "IPv4", 41234.0)` --
    //~ or `None` when not listening. The emitter builds the generated
    //~ `AddressInfo` record from the triple.
    pub fn address(&self) -> Option<(String, String, f64)> { self.local.get().map(|local| (local.ip().to_string(), if local.is_ipv4() { "IPv4".to_owned() } else { "IPv6".to_owned() }, f64::from(local.port()))) }
}

//~ Emit the accept loop and the hyper service that drives one request.
/// Accept connections until `close` signals, serving each on its own task.
async fn smelt_http_accept_loop(listener: tokio::net::TcpListener, handler: SmeltHttpHandler, mut shutdown: tokio::sync::oneshot::Receiver<()>) {
    let mut shutdown_open = true;
    loop {
        let stream = tokio::select! {
            accepted = listener.accept() => match accepted { Ok((stream, _)) => stream, Err(_) => continue },
            //~ Only an explicit `close()` SENDS on the channel. A
            //~ dropped sender (every handle to the server object went
            //~ out of scope — `createServer(..).listen(..)` inside a
            //~ helper that returns) is not a close: Node keeps a
            //~ listening server alive as a ref'd handle whether or not
            //~ the program still references it. So on `RecvError` this
            //~ stops polling the finished channel (polling it again
            //~ panics) and keeps serving.
            closed = &mut shutdown, if shutdown_open => match closed { Ok(()) => break, Err(_) => { shutdown_open = false; continue } },
        }
        ;
        let handler = ::std::rc::Rc::clone(&handler);
        tokio::task::spawn_local(async move {
            let io = hyper_util::rt::TokioIo::new(stream);
            let service = hyper::service::service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                let handler = ::std::rc::Rc::clone(&handler);
                async move { smelt_http_dispatch(request, handler).await }
            }
            );
            //~ `http1` on a `LocalSet`: hyper 1's HTTP/1 connection has
            //~ no `Send` bound, which is exactly why the non-`Send`
            //~ handler can serve it.
            let _ = hyper::server::conn::http1::Builder::new().serve_connection(io, service).await;
        }
        );
    }
}

/// Run one request through the program's handler and answer hyper.
//~
//~ Infallible toward hyper because a handler error never gets that far:
//~ see `smelt_http_handler_failed`.
async fn smelt_http_dispatch(request: hyper::Request<hyper::body::Incoming>, handler: SmeltHttpHandler) -> Result<hyper::Response<http_body_util::Full<bytes::Bytes>>, ::std::convert::Infallible> {
    let (parts, body) = request.into_parts();
    let method = parts.method.as_str().to_owned();
    let url = parts.uri.path_and_query().map_or_else(|| "/".to_owned(), |target| target.as_str().to_owned());
    //~ Node joins repeated headers with ", " into one entry of the plain
    //~ object; `set-cookie` is the documented exception, and it is an
    //~ array there rather than a string, so it takes the same join here
    //~ to keep the map's value type a string.
    let mut headers: Vec<(String, String)> = Vec::new();
    for (name, value) in &parts.headers {
        //~ `http::HeaderName` is lowercase by construction (hyper's parser
        //~ lowercases every name), so the key is the name as it is.
        let key = name.as_str().to_owned();
        let text = value.to_str().unwrap_or_default().to_owned();
        match headers.iter_mut().find(|(existing, _)| *existing == key) {
            Some(existing) => { existing.1.push_str(", "); existing.1.push_str(&text); }
            None => headers.push((key, text)),
        }
    }
    let collected = <hyper::body::Incoming as http_body_util::BodyExt>::collect(body).await.map(|body| body.to_bytes().to_vec()).unwrap_or_default();
    let message = SmeltIncomingMessage::from_parts(method, url, headers, collected);
    let response = SmeltServerResponse::new();
    if let Err(error) = handler(message.clone(), response.clone()) {
        smelt_http_handler_failed(&*error);
    }
    //~ The handler has registered its listeners by now, so the body can
    //~ be delivered; see this module's request-lifetime note. A listener
    //~ that throws is the same uncaught exception as a throwing handler,
    //~ because that is where the real work of a handler happens.
    if let Err(error) = message.deliver_body() {
        smelt_http_handler_failed(&*error);
    }
    response.wait_until_ended().await;
    let (status, headers, body) = response.into_wire_parts();
    let mut builder = hyper::Response::builder().status(status);
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    //~ A header a program set but hyper rejects (an invalid name, say)
    //~ is a programming error with no response to send, so it takes the
    //~ same route as a throwing handler rather than becoming a silent
    //~ 200 with the header missing.
    match builder.body(http_body_util::Full::new(bytes::Bytes::from(body))) {
        Ok(response) => Ok(response),
        Err(error) => smelt_http_handler_failed(&error),
    }
}

/// End the program the way Node ends it for a throwing handler.
//~
//~ Measured, not assumed: an uncaught exception thrown inside a Node
//~ request handler is NOT turned into a 500 -- it reaches the process as an
//~ uncaught exception, prints, and exits 1. The server does not carry on
//~ serving.
//~
//~ Answering 500 instead was the tempting shape, and it is worse than
//~ wrong: a generated program would keep serving where the original had
//~ stopped, so the bug that killed the Node process would show up as a run
//~ of quiet 500s instead. Exiting is the honest translation.
//~
//~ Not a panic: a panic inside a `spawn_local` connection task is caught
//~ by the runtime, which would drop just that connection and leave the
//~ program running -- neither Node's behaviour nor a useful one.
fn smelt_http_handler_failed(error: &dyn ::std::error::Error) -> ! {
    eprintln!("Uncaught {error}");
    ::std::process::exit(1)
}

// @smelt:item-end

// Emit [`FETCH_SERVER`].
// @smelt:item smelt_fetch_server_request when fetch_server
/// The fetch `Request` for one incoming message.
#[allow(dead_code)]
fn smelt_fetch_server_request(message: &SmeltIncomingMessage) -> SmeltRequest {
    let headers = SmeltHeaders::new();
    let mut host = String::from("localhost");
    for (name, value) in message.headers.iter() {
        if name == "host" { host = value.clone(); }
        headers.append(name, value);
    }
    let body = match message.pending_body.borrow_mut().take() {
        Some(bytes) if !bytes.is_empty() && !matches!(message.method.as_str(), "GET" | "HEAD") => SmeltBody::from_bytes(bytes),
        _ => SmeltBody::empty(),
    };
    SmeltRequest::from_parts(&format!("http://{host}{}", message.url), message.method.clone(), headers, body)
}

/// Write a fetch handler's answer to `response` once it settles.
#[allow(dead_code)]
fn smelt_fetch_server_respond(response: SmeltServerResponse, answer: Result<SmeltUnknown, Box<dyn ::std::error::Error>>) {
    //~ A task of its own on the local set, like the connection that waits for
    //~ it: the promise-task queue is drained only by the program's own event
    //~ loop, which a program awaiting its own server (`await fetch(..)` at top
    //~ level) is not running yet.
    tokio::task::spawn_local(async move {
        let settled = match answer {
            Ok(value) => smelt_await_flatten(value).await,
            Err(error) => Err(error),
        };
        smelt_fetch_server_write(&response, settled);
    });
}

/// Write a settled fetch answer (or the error it settled with) and end `response`.
#[allow(dead_code)]
fn smelt_fetch_server_write(response: &SmeltServerResponse, settled: Result<SmeltUnknown, Box<dyn ::std::error::Error>>) {
    {
        match settled {
            Ok(value) => {
                let answered = <SmeltResponse as SmeltFromUnknown>::smelt_from_unknown(value);
                response.status.set(answered.status() as u16);
                let mut has_content_type = false;
                for (name, value) in answered.headers.entries_sorted() {
                    has_content_type |= name == "content-type";
                    response.headers.borrow_mut().push((name, value));
                }
                //~ `@hono/node-server`'s `buildOutgoingHttpHeaders` defaults the
                //~ header (`res["content-type"] ??= "text/plain; charset=UTF-8"`),
                //~ so a bodyless `c.body(null, 204)` carries it too.
                if !has_content_type {
                    response.headers.borrow_mut().push(("content-type".to_owned(), "text/plain; charset=UTF-8".to_owned()));
                }
                response.body.borrow_mut().extend_from_slice(&answered.body.peek_bytes());
            }
            Err(error) => {
                eprintln!("{error}");
                response.status.set(500);
                response.headers.borrow_mut().push(("content-type".to_owned(), "text/plain; charset=UTF-8".to_owned()));
                response.body.borrow_mut().extend_from_slice(b"Internal Server Error");
            }
        }
        response.end(None);
    }
}

// @smelt:item-end
