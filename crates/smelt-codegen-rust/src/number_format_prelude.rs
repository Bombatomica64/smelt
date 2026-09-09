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

use crate::rust::CodeWriter;

/// The emitted helper's name, so emit sites and the prelude agree on it.
pub const NUMBER_TO_STRING_FN: &str = "smelt_number_to_string";

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
pub const CONSOLE_NUMBER_FN: &str = "smelt_console_number";


/// Emit `smelt_number_to_string(f64) -> String`.
///
/// Emitted unconditionally: it is core coercion machinery, reached by
/// `Display for SmeltUnknown`, every static `String(x)`/template coercion, a
/// property key, an array join and `console.log`. A crate that never
/// stringifies a number carries it unused, which is inert under the generated
/// crate's `#![allow(dead_code)]` — the same trade the `typeof` helper makes.
pub fn emit(writer: &mut CodeWriter) {
    writer.line("/// `Number::toString(value, 10)`: JavaScript's number formatting.");
    writer.line("///");
    writer.line("/// Exponential notation at `n > 21` and `n <= -6`, where `n` is the");
    writer.line("/// decimal exponent of the shortest round-trip digit string — which is");
    writer.line("/// what Rust's `{:e}` already produces.");
    // `Borrow<f64>` rather than `f64`, so the one helper serves both call
    // shapes the emitter produces: a by-value number, and the `&f64` a
    // `match` over a BORROWED erased value binds (an erased list's items are
    // iterated by reference, so `SmeltUnknown::Number(value)` there is a
    // `&f64`). The alternative was for every emit site to know which shape its
    // scrutinee had, which is exactly the kind of guess that compiles in the
    // examples and fails on the first library.
    writer.block(
        format!(
            "fn {NUMBER_TO_STRING_FN}<N: ::std::borrow::Borrow<f64>>(value: N) -> String"
        ),
        |fn_writer| {
            fn_writer.line("let value = *value.borrow();");
            fn_writer.line("if value.is_nan() { return \"NaN\".to_owned(); }");
            // `-0.0 == 0.0` is true, so negative zero takes this arm and prints
            // `0`, which is what `String(-0)` is in JavaScript.
            fn_writer.line("if value == 0.0 { return \"0\".to_owned(); }");
            fn_writer
                .line(format!("if value < 0.0 {{ return format!(\"-{{}}\", {NUMBER_TO_STRING_FN}(-value)); }}"));
            fn_writer.line("if value.is_infinite() { return \"Infinity\".to_owned(); }");
            fn_writer.line("let exponential = format!(\"{value:e}\");");
            fn_writer.line("let (mantissa, exponent) = exponential.split_once('e').unwrap_or((exponential.as_str(), \"0\"));");
            fn_writer.line("let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();");
            fn_writer.line("let k = i32::try_from(digits.len()).unwrap_or(i32::MAX);");
            fn_writer.line("let n = exponent.parse::<i32>().unwrap_or(0) + 1;");
            // The four layouts of the spec's step 5, in its order: an integer
            // with trailing zeros, a decimal point inside the digits, a leading
            // `0.` with padding, and exponential form.
            fn_writer.block("if k <= n && n <= 21", |branch| {
                branch.line("let zeros = usize::try_from(n - k).unwrap_or(0);");
                branch.line("return digits + &\"0\".repeat(zeros);");
            });
            fn_writer.block("if 0 < n && n <= 21", |branch| {
                branch.line("let split = usize::try_from(n).unwrap_or(0);");
                branch.line("return format!(\"{}.{}\", &digits[..split], &digits[split..]);");
            });
            fn_writer.block("if -6 < n && n <= 0", |branch| {
                branch.line("let zeros = usize::try_from(-n).unwrap_or(0);");
                branch.line("return format!(\"0.{}{}\", \"0\".repeat(zeros), digits);");
            });
            fn_writer.line("let sign = if n - 1 < 0 { '-' } else { '+' };");
            fn_writer.line("let magnitude = (n - 1).abs();");
            fn_writer.line("if k == 1 { return format!(\"{digits}e{sign}{magnitude}\"); }");
            fn_writer.line("format!(\"{}.{}e{sign}{magnitude}\", &digits[..1], &digits[1..])");
        },
    );
    writer.blank_line();
    // `console.log` is `util.inspect`, not `String()`, and the two disagree
    // about exactly one value.
    writer.line("/// `console.log`'s number formatting: the spec's rule, but `-0` prints `-0`.");
    writer.line(format!(
        "fn {CONSOLE_NUMBER_FN}<N: ::std::borrow::Borrow<f64>>(value: N) -> String {{ let value = *value.borrow(); if value == 0.0 && value.is_sign_negative() {{ return \"-0\".to_owned(); }} {NUMBER_TO_STRING_FN}(value) }}"
    ));
    writer.blank_line();
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The emitted helper covers all four of the spec's layouts and both
    /// notation switches.
    ///
    /// The switch points are the whole reason this exists, so the test names
    /// them: `n > 21` and `n <= -6` are where JavaScript leaves positional
    /// notation and Rust's `Display` never does.
    #[test]
    fn the_helper_implements_the_four_layouts() {
        let mut writer = CodeWriter::new();
        emit(&mut writer);
        let text = writer.finish();
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
