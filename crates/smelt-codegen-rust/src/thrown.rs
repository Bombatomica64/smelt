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

use crate::rust::RustExpr;
use smelt_stdlib::runtime_symbols::markers::SMELT_ERROR;


/// The error type of every fallible generated function.
pub(crate) const ERROR_BOX: &str = "Box<dyn std::error::Error>";

/// The return type of a constructor that can throw.
pub(crate) const RESULT_SELF: &str = "Result<Self, Box<dyn std::error::Error>>";

/// Name of the generated adapter that enters the error channel.
///
/// `smelt_throw(value: SmeltUnknown) -> Box<dyn std::error::Error>`.
pub(crate) const THROW_FN: &str = "smelt_throw";

/// Name of the generated adapter that leaves the error channel.
///
/// `smelt_thrown_value(&dyn std::error::Error) -> SmeltUnknown`.
pub(crate) const THROWN_VALUE_FN: &str = "smelt_thrown_value";

/// Renders the expression that enters the error channel with `value_text`.
///
/// `value_text` must already be `SmeltUnknown`-typed; callers erase their
/// operand first (see `FunctionEmitter::value_at_type`).
pub(crate) fn throw_expr(value_text: &str) -> RustExpr {
    RustExpr::postfix_on(&THROW_FN, format!("{THROW_FN}({value_text})"))
}

/// Renders the expression that recovers the original thrown payload.
///
/// `error_text` names a binding holding the caught `Box<dyn std::error::Error>`;
/// the generated call reborrows it as a trait object so the adapter can downcast.
pub(crate) fn thrown_value_expr(error_text: &str) -> RustExpr {
    RustExpr::postfix_on(&THROWN_VALUE_FN, format!("{THROWN_VALUE_FN}(&*{error_text})"))
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
        "SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"{SMELT_ERROR}\".to_owned(), SmeltUnknown::String({class_text}.into())), (\"message\".to_owned(), SmeltUnknown::String({message_text}.into())), (\"stack\".to_owned(), SmeltUnknown::Undefined), (\"cause\".to_owned(), SmeltUnknown::Undefined)])))"
    )
}

/// Name of the generated fallible `JSON.parse` adapter.
///
/// `smelt_json_parse(&str) -> Result<SmeltUnknown, Box<dyn std::error::Error>>`.
pub(crate) const JSON_PARSE_FN: &str = "smelt_json_parse";

/// Name of the generated fallible `decodeURI` adapter.
pub(crate) const DECODE_URI_FN: &str = "smelt_decode_uri_throwing";

/// Name of the generated fallible `decodeURIComponent` adapter.
pub(crate) const DECODE_URI_COMPONENT_FN: &str = "smelt_decode_uri_component_throwing";

/// Name of the generated fallible `encodeURI` adapter.
pub(crate) const ENCODE_URI_FN: &str = "smelt_encode_uri_throwing";

/// Name of the generated fallible `encodeURIComponent` adapter.
pub(crate) const ENCODE_URI_COMPONENT_FN: &str = "smelt_encode_uri_component_throwing";

/// Name of the generated fallible `DataView` READ adapter.
pub(crate) const DATA_VIEW_GET_FN: &str = "smelt_data_view_get_throwing";

/// Name of the generated fallible `DataView` WRITE adapter.
pub(crate) const DATA_VIEW_SET_FN: &str = "smelt_data_view_set_throwing";

/// Name of the generated fallible `DataView` BIGINT read adapter.
pub(crate) const DATA_VIEW_GET_BIGINT_FN: &str = "smelt_data_view_get_bigint_throwing";

/// Name of the generated fallible `DataView` BIGINT write adapter.
pub(crate) const DATA_VIEW_SET_BIGINT_FN: &str = "smelt_data_view_set_bigint_throwing";

/// Name of the generated fallible `btoa` adapter.
pub(crate) const BTOA_FN: &str = "smelt_btoa_throwing";

/// Name of the generated fallible `atob` adapter.
pub(crate) const ATOB_FN: &str = "smelt_atob_throwing";

/// Name of the generated accessor for the shared global object.
pub(crate) const GLOBAL_OBJECT_FN: &str = "smelt_global_object";

/// Name of the generated erased `Object.assign` helper.
pub(crate) const UNKNOWN_ASSIGN_FN: &str = "smelt_unknown_assign";

/// Name of the generated fallible run-time `RegExp` compiler.
///
/// `smelt_regexp_compile(String, String) -> Result<SmeltRegExp, Box<dyn Error>>`.
pub(crate) const REGEXP_COMPILE_FN: &str = "smelt_regexp_compile";

/// Name of the generated helper that recovers a caught panic's message text.
const PANIC_MESSAGE_FN: &str = "smelt_panic_message";

/// Name of the generated helper that presents a caught panic as an erased error.
const PANIC_ERROR_VALUE_FN: &str = "smelt_panic_error_value";

/// Renders the message text a `catch` observes for a caught panic.
///
/// `panic_text` names the `Box<dyn Any + Send>` a `catch_unwind` answered with.
pub(crate) fn caught_panic_message_expr(panic_text: &str) -> RustExpr {
    RustExpr::postfix_on(&PANIC_MESSAGE_FN, format!("{PANIC_MESSAGE_FN}(&*{panic_text})"))
}

/// Renders the erased error record a `catch` observes for a caught panic.
///
/// Unlike [`panic_payload_record_expr`], the class is recovered from the panic
/// payload rather than hard-coded to `Error`, so a `URIError` routed through the
/// panic channel still answers `error.name === 'URIError'`.
pub(crate) fn caught_panic_error_value_expr(panic_text: &str) -> RustExpr {
    RustExpr::postfix_on(&PANIC_ERROR_VALUE_FN, format!("{PANIC_ERROR_VALUE_FN}(&*{panic_text})"))
}
