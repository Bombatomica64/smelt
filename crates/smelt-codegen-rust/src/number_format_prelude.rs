//! The JavaScript number-to-string rule, as one emitted runtime helper.
//!
//! Rust's `f64` `Display` and JavaScript's `Number::toString` agree on most
//! values and disagree at both ends of the range, because Rust never switches
//! to exponential notation and JavaScript does:
//!
//! | value | Node | Rust `Display` |
//! | --- | --- | --- |
//! | `1e21` | `1e+21` | `1000000000000000000000` |
//! | `1e-7` | `1e-7` | `0.0000001` |
//! | `3.13984e-319` | `3.13984e-319` | 321 characters of zeros and digits |
//!
//! So every program that printed a very large or very small number printed a
//! different string from Node — a general formatting rule, wrong in one place,
//! independent of every type that reaches it. It was found while making
//! `DataView` concrete (a `getFloat64` over unrelated bytes lands on
//! subnormals) and recorded in `blocker-logs/hono-fetch-demand.md`.
//!
//! ## The spec's rule, and why the digits come from `{:e}`
//!
//! ECMA-262 `Number::toString(x, 10)` picks integers `s`, `k`, `n` with
//! `10^(k-1) <= s < 10^k` and `s * 10^(n-k) = x`, choosing the SMALLEST `k` —
//! that is, the shortest decimal digit string that round-trips — and then lays
//! those digits out by comparing `n` against 21 and -6.
//!
//! Rust's `{:e}` already produces exactly that digit string: `LowerExp` uses
//! the same shortest-round-trip algorithm as `Display`, printed as
//! `d[.ddd]e<exp>`. So the helper reads `s` and `k` off the mantissa and `n`
//! off the exponent (`n = exp + 1`, since the mantissa carries one digit before
//! the point) rather than reimplementing digit generation, which is the part
//! that would be easy to get subtly wrong.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]


/// The emitted helper's name, so emit sites and the prelude agree on it.
pub(crate) const NUMBER_TO_STRING_FN: &str = "smelt_number_to_string";

/// The console's number formatter, which differs from the spec's in one value.
///
/// `String(-0)` is `"0"` — the spec's rule folds the sign of zero — but
/// `console.log(-0)` prints `-0`, because Node's `util.inspect` shows negative
/// zero rather than coercing it. Both are Node's behaviour, so both are here,
/// and the console path is the only caller that wants the second.
///
/// It reaches a console argument whose STATIC type is a number. An operand
/// typed `Unknown` keeps `Display for SmeltUnknown` (the spec's rule), because
/// such an operand does not always emit a `SmeltUnknown` expression — a
/// typed-array element read is an `f64` — so a formatter taking the carrier by
/// reference cannot be applied on the static type alone. The gap is one value:
/// an ERASED negative zero prints `0` where Node prints `-0`. Closing it wants
/// a `SmeltConsoleFormat` trait with one impl per reachable Rust type, so the
/// impl is chosen by the expression's real type instead of by the emitter's
/// guess at it.
pub(crate) const CONSOLE_NUMBER_FN: &str = "smelt_console_number";


/// Python's `repr(float)` (which `str(float)` and `print` use).
///
/// The shortest round-trip digits, like JavaScript, but spelled Python's way:
/// a whole float keeps its `.0` (`12.0`, where JavaScript prints `12`),
/// exponential notation applies below `1e-4` and from `1e16` with a signed,
/// two-digit exponent (`1e+16`, `1.5e-07`), and the special values are `inf`,
/// `-inf` and `nan`. Rust's `Debug` for `f64` already picks the same digits
/// and the same thresholds, so only the exponent and the special values are
/// respelled.
pub const PY_FLOAT_REPR_FN: &str = "smelt_py_float_repr";

#[cfg(test)]
mod tests {
    /// The emitted helper covers all four of the spec's layouts and both
    /// notation switches.
    ///
    /// The switch points are the whole reason this exists, so the test names
    /// them: `n > 21` and `n <= -6` are where JavaScript leaves positional
    /// notation and Rust's `Display` never does.
    #[test]
    fn the_helper_implements_the_four_layouts() {
        let text = crate::runtime_prelude::rendered_text(&|_| false);
        for expected in [
            "fn smelt_number_to_string<N: ::std::borrow::Borrow<f64>>(value: N) -> String",
            // One helper for both call shapes: a by-value number and the
            // `&f64` a borrowed erased match binds.
            "let value = *value.borrow();",
            // NaN, zero, sign and infinity come before the digit work.
            "if value.is_nan() { return \"NaN\".to_owned(); }",
            "if value == 0.0 { return \"0\".to_owned(); }",
            "if value.is_infinite() { return \"Infinity\".to_owned(); }",
            // The digits are Rust's shortest round-trip ones, not hand-rolled.
            "let exponential = format!(\"{value:e}\");",
            // The four layouts, at the spec's own boundaries.
            "if k <= n && n <= 21",
            "if 0 < n && n <= 21",
            "if -6 < n && n <= 0",
            "format!(\"{}.{}e{sign}{magnitude}\", &digits[..1], &digits[1..])",
            // `console.log(-0)` is `-0` where `String(-0)` is `0`.
            "if value == 0.0 && value.is_sign_negative() { return \"-0\".to_owned(); }",
        ] {
            assert!(text.contains(expected), "missing `{expected}`");
        }
    }

}
