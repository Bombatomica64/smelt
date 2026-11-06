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

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

