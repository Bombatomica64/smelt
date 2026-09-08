//! Exception-payload ABI for Smelt's generated Rust.
//!
//! Every fallible or `async` Smelt function models its error channel as
//! `Result<T, Box<dyn std::error::Error>>`. A JavaScript `throw`, however,
//! carries an arbitrary *value*, not a message: `throw new TypeError(m)`,
//! `throw 'a string'`, `throw {code: 42}` and `throw someErrorInstance` are all
//! legal and are all observable in the corresponding `catch`.
//!
//! Before this module, the two `Terminator::Throw` emit sites and the
//! `new Promise(reject)` bridge collapsed the thrown value with
//! `format!("{}", value)` into a `std::io::Error`. Because an erased JavaScript
//! `Error` is a marker-bearing `SmeltUnknown::Object`, `Display` rendered it as
//! the literal text `[object Object]`, so the payload's class, `name`,
//! `message`, `cause` and any custom fields were destroyed at the throw site and
//! could never be recovered by a `catch`.
//!
//! This module emits a payload-carrying error type, `SmeltThrown`, and the two
//! adapters that bracket the boundary: [`THROW_FN`] to enter it and
//! [`THROWN_VALUE_FN`] to leave it. The representation is a general rule for
//! *every* `throw`, not a per-library special case.
//!
//! # Why the payload is a `SmeltUnknown`
//!
//! This is a genuine dynamic boundary in the sense of `CLAUDE.md`, not an
//! erasure of convenience:
//!
//! * `throw` is not typed. TypeScript deliberately gives a `catch` binding the
//!   static type `any` (or `unknown` under `useUnknownInCatchVariables`); the
//!   language has no throws-clause, so the set of values that can arrive at a
//!   given `catch` is not statically known.
//! * The error channel is *one* Rust type shared by every fallible function in
//!   the crate. A concrete payload type, a generated union, or a scoped generic
//!   would all have to be chosen per `catch` site, but a `catch` may receive a
//!   value thrown by any transitively called function — including through an
//!   erased `SmeltUnknown::Function` callback, whose signature is already
//!   `Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn Error>>>` and
//!   therefore cannot mention a caller-specific error type.
//! * The channel must also stay open to *foreign* errors (`std::io::Error` and
//!   anything else that reaches it via `?`), which is why the boundary is a
//!   downcast rather than an enum discriminant.
//!
//! `crates/smelt-codegen-rust/src/tests/thrown_tests.rs` carries the regression
//! test that pins this reasoning: it throws two structurally unrelated payloads
//! (an `Error` object and a bare string) through one function's error channel and
//! asserts both survive, which no single concrete type, generated union arm, or
//! function-scoped type parameter could express.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

use crate::rust::CodeWriter;

/// Name of the generated payload-carrying error type.
pub(crate) const THROWN_TYPE: &str = "SmeltThrown";

/// Name of the generated adapter that enters the error channel.
///
/// `smelt_throw(value: SmeltUnknown) -> Box<dyn std::error::Error>`.
pub(crate) const THROW_FN: &str = "smelt_throw";

/// Name of the generated adapter that leaves the error channel.
///
/// `smelt_thrown_value(&dyn std::error::Error) -> SmeltUnknown`.
pub(crate) const THROWN_VALUE_FN: &str = "smelt_thrown_value";

/// Name of the generated helper that projects a thrown payload to its message.
///
/// `smelt_thrown_message(&SmeltUnknown) -> String`.
pub(crate) const THROWN_MESSAGE_FN: &str = "smelt_thrown_message";

/// Renders the expression that enters the error channel with `value_text`.
///
/// `value_text` must already be `SmeltUnknown`-typed; callers erase their
/// operand first (see `FunctionEmitter::value_at_type`).
pub(crate) fn throw_expr(value_text: &str) -> String {
    format!("{THROW_FN}({value_text})")
}

/// Renders the expression that recovers the original thrown payload.
///
/// `error_text` names a binding holding the caught `Box<dyn std::error::Error>`;
/// the generated call reborrows it as a trait object so the adapter can downcast.
pub(crate) fn thrown_value_expr(error_text: &str) -> String {
    format!("{THROWN_VALUE_FN}(&*{error_text})")
}

/// Renders the erased `Error` record a caught *panic* is presented as.
///
/// `message_text` names a binding holding the recovered panic message. A Rust
/// `panic!` carries no Smelt payload, so the value a `catch` observes is rebuilt
/// as the same branded `Error` record `smelt_thrown_value` synthesizes for a
/// foreign error: `{ __smelt_error: "Error", message }`.
///
/// **Dynamic boundary.** This record is the exception-payload ABI, not program
/// storage: `throw` accepts any JavaScript value and a `catch` binding has no
/// static type (TypeScript types it `unknown`), so no concrete struct, generated
/// union arm, or scoped generic can stand in for it — the payload's shape is
/// only known at run time, on the run-time branch that produced it. This is the
/// documented boundary behind the `SmeltUnknown::String({message})` marker in
/// `crates/smelt-transpiler/src/unknown_report.rs`, which classifies these lines
/// as `legitimate-boundary` rather than avoidable erasure; the regression test
/// `panic_recovery_payload_is_a_boundary` there covers the classification, and
/// `thrown_tests::one_channel_carries_structurally_unrelated_payloads` covers
/// the ABI itself.
pub(crate) fn panic_payload_record_expr(message_text: &str) -> String {
    error_payload_record_expr("Error", message_text)
}

/// Renders the erased error record for one builtin error class and message.
///
/// The field set is the one `new <ErrorClass>(message)` itself produces (see the
/// error-construction arm in `reflection_prelude`): the `__smelt_error` class
/// brand, `message`, and absent `stack`/`cause`. Sharing the shape here is what
/// lets a `catch` observe a runtime-raised error exactly as it observes a
/// source-level `throw new SyntaxError(..)`.
pub(crate) fn error_payload_record_expr(class: &str, message_text: &str) -> String {
    error_payload_record_expr_dyn(&format!("{class:?}"), message_text)
}

/// Renders the erased error record with both fields given as Rust expressions.
///
/// The class is a *rendered expression* rather than a literal, which is what the
/// panic route needs: a caught panic's class is only known at run time, from the
/// payload that crossed the unwind. [`error_payload_record_expr`] is the literal
/// spelling of the same record.
pub(crate) fn error_payload_record_expr_dyn(class_text: &str, message_text: &str) -> String {
    format!(
        "SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"__smelt_error\".to_owned(), SmeltUnknown::String({class_text}.into())), (\"message\".to_owned(), SmeltUnknown::String({message_text}.into())), (\"stack\".to_owned(), SmeltUnknown::Undefined), (\"cause\".to_owned(), SmeltUnknown::Undefined)])))"
    )
}

/// Name of the generated fallible `JSON.parse` adapter.
///
/// `smelt_json_parse(&str) -> Result<SmeltUnknown, Box<dyn std::error::Error>>`.
pub(crate) const JSON_PARSE_FN: &str = "smelt_json_parse";

/// Emits the fallible `JSON.parse` adapter into the generated prelude.
///
/// `JSON.parse` throws a `SyntaxError` on malformed text and JavaScript code
/// catches it (`try { JSON.parse(s) } catch { return false }`). The adapter
/// reports failure through the same error channel a source-level `throw` uses,
/// carrying the same `SyntaxError` record `new SyntaxError(message)` builds — so
/// a `catch` binding cannot tell a runtime-raised parse error from a
/// hand-written one.
pub(crate) fn emit_json_parse_support(writer: &mut CodeWriter) {
    writer.blank_line();
    writer.line("/// `JSON.parse`: parse JSON text, throwing a catchable `SyntaxError`.");
    writer.line(format!(
        "fn {JSON_PARSE_FN}(text: &str) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>> {{ match serde_json::from_str::<SmeltUnknown>(text) {{ Ok(value) => Ok(value), Err(error) => Err({THROW_FN}({})) }} }}",
        error_payload_record_expr("SyntaxError", "error.to_string()")
    ));
}

/// Name of the generated fallible `decodeURI` adapter.
pub(crate) const DECODE_URI_FN: &str = "smelt_decode_uri_throwing";

/// Name of the generated fallible `decodeURIComponent` adapter.
pub(crate) const DECODE_URI_COMPONENT_FN: &str = "smelt_decode_uri_component_throwing";

/// Emits the fallible URI-decoder adapters into the generated prelude.
///
/// The runtime decoders answer `Option<String>` — `None` for malformed
/// percent-encoding — and JavaScript answers that same input with a *catchable*
/// `URIError`. These adapters convert one into the other through the ABI a
/// source-level `throw` uses, so a `catch` binding cannot tell a
/// runtime-raised `URIError` from a hand-written one, exactly as
/// [`emit_json_parse_support`] arranges for `SyntaxError`.
///
/// Before this existed the emitter wrote
/// `smelt_decode_uri(..).expect("URIError: URI malformed")`, which does not
/// merely fail to be catchable: the handler block ends up with no predecessor,
/// MIR drops it, and a `try`/`catch` the source wrote is *absent* from the
/// generated crate. Hono's `tryDecode` is that shape.
pub(crate) fn emit_uri_decode_support(writer: &mut CodeWriter) {
    use smelt_stdlib::runtime_symbols::strings;

    for (adapter, inner) in [
        (DECODE_URI_FN, strings::DECODE_URI),
        (DECODE_URI_COMPONENT_FN, strings::DECODE_URI_COMPONENT),
    ] {
        writer.blank_line();
        writer.line(format!(
            "/// `{inner}`: decode percent-encoding, throwing a catchable `URIError`."
        ));
        writer.line(format!(
            "fn {adapter}(value: &str) -> Result<String, Box<dyn ::std::error::Error>> {{ \
             match {inner}(value) {{ \
             Some(decoded) => Ok(decoded), \
             None => Err({THROW_FN}({})) }} }}",
            error_payload_record_expr("URIError", "\"URI malformed\".to_owned()")
        ));
    }
}

/// Emits the exception-payload ABI into the generated runtime prelude.
///
/// Only called from inside the prelude's `needs_unknown` region: the payload is
/// a `SmeltUnknown`, so these items are only well-formed where that enum exists.
/// Throw sites in a program with no erased values keep the plain string
/// `std::io::Error` form (see `FunctionEmitter::throw_terminator_text`).
pub(crate) fn emit_thrown_payload_support(writer: &mut CodeWriter, needs_panic_route: bool) {
    writer.blank_line();
    writer.line("/// A JavaScript `throw` payload travelling Smelt's `Box<dyn Error>` channel.");
    writer.line("///");
    writer.line("/// DYNAMIC BOUNDARY: `throw` accepts any JavaScript value and a `catch`");
    writer.line("/// binding has no static type, so the single error channel shared by every");
    writer.line("/// generated fallible function cannot be a concrete type, a generated union,");
    writer.line("/// or a scoped generic. The payload is kept whole here and recovered by");
    writer.line("/// `smelt_thrown_value`; foreign errors reaching the same channel fall back");
    writer.line("/// to their `Display` text.");
    writer.line("#[derive(Debug)]");
    writer.line(format!("struct {THROWN_TYPE} {{ value: SmeltUnknown }}"));
    // `Display` projects the payload's `message` field when present. Erased
    // `Error` values carry `message`, and it is what the runtime's own promise
    // rejection bridge already surfaced, so keeping the same projection means
    // string-typed `catch` bindings and `.toThrow("text")`-style substring
    // checks observe exactly the text they observed before this change --
    // whereas `format!("{}", value)` on an error object rendered the useless
    // literal `[object Object]`.
    writer.line(
        "/// Project a thrown payload to the message text a string-typed `catch` observes.",
    );
    writer.line(format!(
        "fn {THROWN_MESSAGE_FN}(value: &SmeltUnknown) -> String {{ if let SmeltUnknown::Object(object) = value {{ if let Some(SmeltUnknown::String(message)) = object.get(\"message\") {{ return message.to_string(); }} }} format!(\"{{value}}\") }}"
    ));
    writer.line(format!(
        "impl ::std::fmt::Display for {THROWN_TYPE} {{ fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{ formatter.write_str(&{THROWN_MESSAGE_FN}(&self.value)) }} }}"
    ));
    writer.line(format!(
        "impl ::std::error::Error for {THROWN_TYPE} {{}}"
    ));
    writer.line("/// Enter the error channel, keeping the thrown value's structure and identity.");
    writer.line(format!(
        "fn {THROW_FN}(value: SmeltUnknown) -> Box<dyn ::std::error::Error> {{ Box::new({THROWN_TYPE} {{ value }}) }}"
    ));
    writer.line("/// Recover the original thrown payload from the error channel.");
    writer.line("///");
    writer.line("/// A foreign error (anything that reached the channel through `?` rather than");
    writer.line("/// a Smelt `throw`) has no payload, so it is presented as an erased `Error`");
    writer.line("/// record built from its `Display` text -- the shape a `catch` saw before the");
    writer.line("/// payload ABI existed.");
    writer.line(format!(
        "fn {THROWN_VALUE_FN}(error: &(dyn ::std::error::Error + 'static)) -> SmeltUnknown {{ if let Some(thrown) = error.downcast_ref::<{THROWN_TYPE}>() {{ return thrown.value.clone(); }} {} }}",
        panic_payload_record_expr("error.to_string()")
    ));
    // The panic route's payload projection reads a thrown value through
    // `smelt_thrown_value` above, so it belongs to the same gated region; it
    // also names `SmeltPanic`, so it is only well-formed when the route's own
    // items are emitted.
    if needs_panic_route {
        emit_panic_payload_projection(writer);
    }
}

/// Name of the generated `Send` panic payload that carries a throw's identity.
const PANIC_TYPE: &str = "SmeltPanic";

/// Name of the generated adapter that routes a Smelt error through `panic!`.
///
/// `smelt_panic_throw(error: Box<dyn Error>) -> !`. The emit sites spell the
/// name literally, inside their own `format!` templates; this constant is the
/// single definition the prelude emits against.
const PANIC_THROW_FN: &str = "smelt_panic_throw";

/// Name of the generated helper that recovers a caught panic's message text.
const PANIC_MESSAGE_FN: &str = "smelt_panic_message";

/// Name of the generated helper that recovers a caught panic's error class.
const PANIC_CLASS_FN: &str = "smelt_panic_class";

/// Name of the generated helper that builds a `SmeltPanic` from a channel error.
const PANIC_PAYLOAD_FN: &str = "smelt_panic_payload";

/// Name of the generated helper that presents a caught panic as an erased error.
const PANIC_ERROR_VALUE_FN: &str = "smelt_panic_error_value";

/// Name of the generated one-shot panic-hook installer.
const PANIC_HOOK_FN: &str = "smelt_install_panic_hook";

/// Renders the message text a `catch` observes for a caught panic.
///
/// `panic_text` names the `Box<dyn Any + Send>` a `catch_unwind` answered with.
pub(crate) fn caught_panic_message_expr(panic_text: &str) -> String {
    format!("{PANIC_MESSAGE_FN}(&*{panic_text})")
}

/// Renders the erased error record a `catch` observes for a caught panic.
///
/// Unlike [`panic_payload_record_expr`], the class is recovered from the panic
/// payload rather than hard-coded to `Error`, so a `URIError` routed through the
/// panic channel still answers `error.name === 'URIError'`.
pub(crate) fn caught_panic_error_value_expr(panic_text: &str) -> String {
    format!("{PANIC_ERROR_VALUE_FN}(&*{panic_text})")
}

/// Emits the panic-route support items into the generated prelude.
///
/// # Why the panic channel exists at all
///
/// A generated function whose body cannot propagate an error — because its own
/// type says `may_throw: false`, which is the case for every closure coerced to
/// a declared non-throwing callback parameter type — still has to report a
/// `throw`. It does so by panicking, and an enclosing `try` catches it with
/// `std::panic::catch_unwind`. That route is why the generated `Cargo.toml` must
/// never set `panic = "abort"`; `emitted_manifest_never_aborts_on_panic` pins it.
///
/// # Why the payload is a class plus a message, and not the thrown value
///
/// `std::panic::panic_any` requires `Any + Send`, and a `SmeltUnknown` holds
/// `Rc` handles, so the thrown value itself cannot cross an unwind. `SmeltPanic`
/// carries the two parts of the payload that a `catch` observes and that *are*
/// `Send`: the error class brand and the message. Before this existed the route
/// panicked with `format!("{}", error)`, so every panic-routed throw arrived at
/// its `catch` as a bare `Error` — `error.name` was wrong for `URIError`,
/// `TypeError`, and every user error class. Custom fields on a thrown class
/// instance still do not survive the unwind; the statically resolvable cases are
/// meant to stop taking this route at all (see `hono-fallible-ops.md` §9(b)).
///
/// # The hook
///
/// Routing control flow through panics otherwise prints a panic line and the
/// backtrace note on ordinary caught input — per request, for a router decoding
/// untrusted path segments. The installed hook suppresses output *only* for a
/// `SmeltPanic` payload and delegates everything else to the previous hook, so a
/// genuine panic stays as loud as it was.
///
/// `needs_unknown` selects the panic-payload builder's body: the structured form
/// projects the thrown payload's class, and is emitted by
/// [`emit_thrown_payload_support`] because it names `SmeltUnknown`. A crate with
/// no erased values has no structured payloads either — its throw sites carry
/// plain message strings — so the class is `Error` there by construction.
pub(crate) fn emit_panic_route_support(writer: &mut CodeWriter, needs_unknown: bool) {
    // No leading blank line: the caller has just written the crate attribute
    // block, which already ends with one.
    writer.line("/// A Smelt `throw` crossing an unwind boundary, keeping its class.");
    writer.line("#[derive(Debug)]");
    writer.line(format!(
        "struct {PANIC_TYPE} {{ class: String, message: String }}"
    ));
    writer.line(format!(
        "impl ::std::fmt::Display for {PANIC_TYPE} {{ fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{ write!(formatter, \"{{}}: {{}}\", self.class, self.message) }} }}"
    ));
    writer.line("static SMELT_PANIC_HOOK: ::std::sync::Once = ::std::sync::Once::new();");
    writer.line("/// Silence the panic report for Smelt-thrown payloads only.");
    writer.line(format!(
        "fn {PANIC_HOOK_FN}() {{ SMELT_PANIC_HOOK.call_once(|| {{ let previous = ::std::panic::take_hook(); ::std::panic::set_hook(Box::new(move |info| {{ if info.payload().downcast_ref::<{PANIC_TYPE}>().is_some() {{ return; }} previous(info); }})); }}); }}"
    ));
    writer.line("/// Report a Smelt error through the panic channel, keeping its identity.");
    writer.line(format!(
        "fn {PANIC_THROW_FN}(error: Box<dyn ::std::error::Error>) -> ! {{ {PANIC_HOOK_FN}(); ::std::panic::panic_any({PANIC_PAYLOAD_FN}(&*error)) }}"
    ));
    writer.line("/// Recover the message text a `catch` observes from a caught panic.");
    writer.line(format!(
        "fn {PANIC_MESSAGE_FN}(panic: &(dyn ::std::any::Any + Send)) -> String {{ if let Some(payload) = panic.downcast_ref::<{PANIC_TYPE}>() {{ return payload.message.clone(); }} if let Some(message) = panic.downcast_ref::<String>() {{ return message.clone(); }} if let Some(message) = panic.downcast_ref::<&'static str>() {{ return (*message).to_owned(); }} \"JavaScript exception\".to_owned() }}"
    ));
    writer.line("/// Recover the error class a `catch` observes from a caught panic.");
    writer.line(format!(
        "fn {PANIC_CLASS_FN}(panic: &(dyn ::std::any::Any + Send)) -> String {{ panic.downcast_ref::<{PANIC_TYPE}>().map_or_else(|| \"Error\".to_owned(), |payload| payload.class.clone()) }}"
    ));
    if !needs_unknown {
        writer.line("/// Build the unwind payload for a crate with no erased values.");
        writer.line(format!(
            "fn {PANIC_PAYLOAD_FN}(error: &(dyn ::std::error::Error + 'static)) -> {PANIC_TYPE} {{ {PANIC_TYPE} {{ class: \"Error\".to_owned(), message: error.to_string() }} }}"
        ));
    }
}

/// Emits the payload-projecting halves of the panic route.
///
/// Only well-formed inside the prelude's `needs_unknown` region: both items name
/// `SmeltUnknown`. See [`emit_panic_route_support`] for the design.
fn emit_panic_payload_projection(writer: &mut CodeWriter) {
    writer.blank_line();
    writer.line("/// Project a channel error's class and message across an unwind.");
    writer.line("///");
    writer.line("/// The class brand is the one `new <ErrorClass>(message)` writes; a thrown");
    writer.line("/// class instance is read through its `name` property instead, which is what");
    writer.line("/// JavaScript reports for `error.name` on a user error class.");
    writer.line(format!(
        "fn {PANIC_PAYLOAD_FN}(error: &(dyn ::std::error::Error + 'static)) -> {PANIC_TYPE} {{ \
         let value = {THROWN_VALUE_FN}(error); \
         let message = {THROWN_MESSAGE_FN}(&value); \
         let mut class = \"Error\".to_owned(); \
         if let SmeltUnknown::Object(object) = &value {{ \
         if let Some(SmeltUnknown::String(name)) = object.get(\"__smelt_error\") {{ class = name.to_string(); }} \
         else if let Some(SmeltUnknown::String(name)) = object.get(\"name\") {{ class = name.to_string(); }} }} \
         {PANIC_TYPE} {{ class, message }} }}"
    ));
    writer.line("/// Present a caught panic as the erased error record a `catch` binds.");
    writer.line(format!(
        "fn {PANIC_ERROR_VALUE_FN}(panic: &(dyn ::std::any::Any + Send)) -> SmeltUnknown {{ {} }}",
        error_payload_record_expr_dyn(
            &format!("{PANIC_CLASS_FN}(panic)"),
            &format!("{PANIC_MESSAGE_FN}(panic)")
        )
    ));
}
