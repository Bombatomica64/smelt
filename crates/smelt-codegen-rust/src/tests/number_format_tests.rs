//! The emitted number-to-string helper (`runtime/number_format.rs`).

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
