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
    format!(
        "SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([(\"__smelt_error\".to_owned(), SmeltUnknown::String(\"Error\".to_owned())), (\"message\".to_owned(), SmeltUnknown::String({message_text}))])))"
    )
}

/// Emits the exception-payload ABI into the generated runtime prelude.
///
/// Only called from inside the prelude's `needs_unknown` region: the payload is
/// a `SmeltUnknown`, so these items are only well-formed where that enum exists.
/// Throw sites in a program with no erased values keep the plain string
/// `std::io::Error` form (see `FunctionEmitter::throw_terminator_text`).
pub(crate) fn emit_thrown_payload_support(writer: &mut CodeWriter) {
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
        "fn {THROWN_MESSAGE_FN}(value: &SmeltUnknown) -> String {{ if let SmeltUnknown::Object(object) = value {{ if let Some(SmeltUnknown::String(message)) = object.get(\"message\") {{ return message; }} }} format!(\"{{value}}\") }}"
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
}
