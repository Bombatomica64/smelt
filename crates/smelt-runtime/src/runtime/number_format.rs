// The JavaScript number-to-string rule, as one emitted runtime helper.
//
// Rust's `f64` `Display` and JavaScript's `Number::toString` agree on most
// values and disagree at both ends of the range, because Rust never switches
// to exponential notation and JavaScript does:
//
// | value | Node | Rust `Display` |
// | --- | --- | --- |
// | `1e21` | `1e+21` | `1000000000000000000000` |
// | `1e-7` | `1e-7` | `0.0000001` |
// | `3.13984e-319` | `3.13984e-319` | 321 characters of zeros and digits |
//
// So every program that printed a very large or very small number printed a
// different string from Node — a general formatting rule, wrong in one place,
// independent of every type that reaches it. It was found while making
// `DataView` concrete (a `getFloat64` over unrelated bytes lands on
// subnormals) and recorded in `blocker-logs/hono-fetch-demand.md`.
//
// ## The spec's rule, and why the digits come from `{:e}`
//
// ECMA-262 `Number::toString(x, 10)` picks integers `s`, `k`, `n` with
// `10^(k-1) <= s < 10^k` and `s * 10^(n-k) = x`, choosing the SMALLEST `k` —
// that is, the shortest decimal digit string that round-trips — and then lays
// those digits out by comparing `n` against 21 and -6.
//
// Rust's `{:e}` already produces exactly that digit string: `LowerExp` uses
// the same shortest-round-trip algorithm as `Display`, printed as
// `d[.ddd]e<exp>`. So the helper reads `s` and `k` off the mantissa and `n`
// off the exponent (`n = exp + 1`, since the mantissa carries one digit before
// the point) rather than reimplementing digit generation, which is the part
// that would be easy to get subtly wrong.

// Emit `smelt_number_to_string(f64) -> String`.
//
// Emitted unconditionally: it is core coercion machinery, reached by
// `Display for SmeltUnknown`, every static `String(x)`/template coercion, a
// property key, an array join and `console.log`. A crate that never
// stringifies a number carries it unused, which is inert under the generated
// crate's `#![allow(dead_code)]` — the same trade the `typeof` helper makes.
// @smelt:item smelt_number_to_string
/// `Number::toString(value, 10)`: JavaScript's number formatting.
///
/// Exponential notation at `n > 21` and `n <= -6`, where `n` is the
/// decimal exponent of the shortest round-trip digit string — which is
/// what Rust's `{:e}` already produces.
//~ `Borrow<f64>` rather than `f64`, so the one helper serves both call
//~ shapes the emitter produces: a by-value number, and the `&f64` a
//~ `match` over a BORROWED erased value binds (an erased list's items are
//~ iterated by reference, so `SmeltUnknown::Number(value)` there is a
//~ `&f64`). The alternative was for every emit site to know which shape its
//~ scrutinee had, which is exactly the kind of guess that compiles in the
//~ examples and fails on the first library.
fn smelt_number_to_string<N: ::std::borrow::Borrow<f64>>(value: N) -> String {
    let value = *value.borrow();
    if value.is_nan() { return "NaN".to_owned(); }
    //~ `-0.0 == 0.0` is true, so negative zero takes this arm and prints
    //~ `0`, which is what `String(-0)` is in JavaScript.
    if value == 0.0 { return "0".to_owned(); }
    if value < 0.0 { return format!("-{}", smelt_number_to_string(-value)); }
    if value.is_infinite() { return "Infinity".to_owned(); }
    let exponential = format!("{value:e}");
    let (mantissa, exponent) = exponential.split_once('e').unwrap_or((exponential.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let k = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let n = exponent.parse::<i32>().unwrap_or(0) + 1;
    //~ The four layouts of the spec's step 5, in its order: an integer
    //~ with trailing zeros, a decimal point inside the digits, a leading
    //~ `0.` with padding, and exponential form.
    if k <= n && n <= 21 {
        let zeros = usize::try_from(n - k).unwrap_or(0);
        return digits + &"0".repeat(zeros);
    }
    if 0 < n && n <= 21 {
        let split = usize::try_from(n).unwrap_or(0);
        return format!("{}.{}", &digits[..split], &digits[split..]);
    }
    if -6 < n && n <= 0 {
        let zeros = usize::try_from(-n).unwrap_or(0);
        return format!("0.{}{}", "0".repeat(zeros), digits);
    }
    let sign = if n - 1 < 0 { '-' } else { '+' };
    let magnitude = (n - 1).abs();
    if k == 1 { return format!("{digits}e{sign}{magnitude}"); }
    format!("{}.{}e{sign}{magnitude}", &digits[..1], &digits[1..])
}

//~ `console.log` is `util.inspect`, not `String()`, and the two disagree
//~ about exactly one value.
/// `console.log`'s number formatting: the spec's rule, but `-0` prints `-0`.
fn smelt_console_number<N: ::std::borrow::Borrow<f64>>(value: N) -> String { let value = *value.borrow(); if value == 0.0 && value.is_sign_negative() { return "-0".to_owned(); } smelt_number_to_string(value) }

// @smelt:item-end

// @smelt:item smelt_py_str_repr when python_number_format
/// Python's `repr(str)`: single quotes, unless the text holds one and no double
/// quote.
fn smelt_py_str_repr<S: AsRef<str>>(value: S) -> String {
    let value = value.as_ref();
    let quote = if value.contains('\'') && !value.contains('"') { '"' } else { '\'' };
    let mut text = String::with_capacity(value.len().saturating_add(2));
    text.push(quote);
    for character in value.chars() {
        match character {
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\r' => text.push_str("\\r"),
            '\t' => text.push_str("\\t"),
            other if other == quote => { text.push('\\'); text.push(other); }
            other => text.push(other),
        }
    }
    text.push(quote);
    text
}

// @smelt:item-end

// Emit [`PY_FLOAT_REPR_FN`].
//
// Emitted only for a crate with Python code (a function whose absent value
// is spelled `None`), so TypeScript programs do not carry it.
// @smelt:item smelt_py_float_repr when python_number_format
/// Python's `repr(float)`: `12.0`, `1e+16`, `1.5e-07`, `inf`, `nan`.
fn smelt_py_float_repr<N: ::std::borrow::Borrow<f64>>(value: N) -> String {
    let value = *value.borrow();
    if value.is_nan() { return "nan".to_owned(); }
    if value.is_infinite() { return if value > 0.0 { "inf" } else { "-inf" }.to_owned(); }
    let text = format!("{value:?}");
    match text.split_once('e') {
        Some((mantissa, exponent)) => { let (sign, digits) = exponent.strip_prefix('-').map_or(("+", exponent), |digits| ("-", digits)); format!("{mantissa}e{sign}{digits:0>2}") }
        None => text,
    }
}

// @smelt:item-end
