//! The JavaScript string-to-number rule, as one emitted runtime helper.
//!
//! ECMA-262 `StringToNumber` is what `Number(s)`, unary `+s`, arithmetic and
//! relational coercion of a string, and every erased-value-to-number
//! conversion apply to a string. Rust's `str::parse::<f64>` is a different
//! grammar, and the generated code used to call it in about fifteen places
//! with three different wrappers, so the same source construct answered
//! differently depending on which path reached it:
//!
//! | input | Node | `parse::<f64>()` |
//! | --- | --- | --- |
//! | `""` | `0` | error |
//! | `" 1 "`, `"\n2\t"` | `1`, `2` | error (no trimming) |
//! | `"0x10"`, `"0o17"`, `"0b11"` | `16`, `15`, `3` | error |
//! | `"inf"`, `"nan"`, `"infinity"` | `NaN` | `inf`, `NaN`, `inf` |
//! | `"\u{feff}1"` | `1` (U+FEFF is JS whitespace) | error |
//! | `"\u{85}1"` | `NaN` (U+0085 is not) | error, but `str::trim` strips it |
//!
//! ## The grammar
//!
//! After trimming `StrWhiteSpaceChar` (the `WhiteSpace` and `LineTerminator`
//! code points, which is not Rust's `char::is_whitespace`): empty is `0`; an
//! unsigned `0x`/`0o`/`0b` literal is that integer; `Infinity` with an optional
//! sign is infinite; otherwise the text must be an optionally signed decimal
//! literal (`1`, `1.`, `.5`, `1e3`, `1E-3`, ASCII digits only, no `_`), else
//! `NaN`. The decimal value comes from Rust's parser once the text is known to
//! be in the JS grammar: there the two agree, and digit-to-float rounding is
//! the part worth not reimplementing.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]


/// The emitted helper's name, so emit sites and the prelude agree on it.
pub(crate) const STRING_TO_NUMBER_FN: &str = "smelt_string_to_number";

/// The emitted helper over a JavaScript string (`&JsStr`).
pub(crate) const JS_STRING_TO_NUMBER_FN: &str = "smelt_js_string_to_number";

/// The call text that converts the string expression `text` with the
/// JavaScript rule, for an emit site that builds its code with `format!`.
///
/// `js_text` says whether `text` is a JavaScript string (`JsString`, read
/// through `smelt_js_string_to_number`) or a Rust `String`/`&str`.
pub(crate) fn string_to_number_call(text: &str, js_text: bool) -> String {
    if js_text {
        format!("{JS_STRING_TO_NUMBER_FN}(&({text}))")
    } else {
        format!("{STRING_TO_NUMBER_FN}(&({text}))")
    }
}
