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
//~
//~ Exponential notation at `n > 21` and `n <= -6`, where `n` is the
//~ decimal exponent of the shortest round-trip digit string — which is
//~ what Rust's `{:e}` already produces.
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

// `Number.prototype`'s formatting methods: `toFixed`, `toPrecision`,
// `toExponential` and `toString(radix)`.
//
// One family for the typed op (`Rvalue::NumericFormat`) and the erased member
// read (`(x as any).toFixed`), so the two spellings cannot round differently.
// The spec rounds an exact tie AWAY from zero ("pick the larger n"), Rust's
// float formatting rounds it to EVEN, and that is the whole difference: Rust
// formats the exact binary value correctly otherwise, so the hand rounding
// runs only when a tie is possible. Checked against Node on 150k values
// (ties, subnormals, `1e308`, every radix) when written.
// @smelt:item smelt_number_format_checked when number_methods
/// The digits of a positive finite `value` rounded to `count` significant
/// digits the way `Number.prototype.toPrecision` rounds: to nearest, an
/// exact tie going AWAY from zero (the spec's "if there are two such sets
/// of e and n, pick the e and n for which n × 10^(e–p+1) is larger").
//~
//~ Answers `(digits, exponent)` with `digits.len() == count` and
//~ `value ≈ d.ddd × 10^exponent`. Rust's `{:.*e}` rounds the EXACT binary
//~ value correctly, but breaks an exact tie to even, so it is right
//~ whenever the value is not a tie. A tie shows as a trailing `5` one
//~ digit further out, and only then is the exact decimal expansion (at
//~ most 767 significant digits for an `f64`) rounded by hand.
fn smelt_number_round_significant(value: f64, count: usize) -> (Vec<u8>, i32) {
    let split = |text: String| -> (Vec<u8>, i32) {
        let (mantissa, exponent) = text.split_once('e').unwrap_or((text.as_str(), "0"));
        (mantissa.bytes().filter(u8::is_ascii_digit).collect(), exponent.parse().unwrap_or(0))
    };
    let (probe, _) = split(format!("{value:.count$e}"));
    if probe.last() != Some(&b'5') { return split(format!("{value:.precision$e}", precision = count - 1)); }
    let (exact, exponent) = split(format!("{value:.800e}"));
    let mut kept = exact[..count].to_vec();
    if exact[count] < b'5' { return (kept, exponent); }
    for digit in kept.iter_mut().rev() {
        if *digit == b'9' { *digit = b'0'; } else { *digit += 1; return (kept, exponent); }
    }
    kept.insert(0, b'1');
    kept.pop();
    (kept, exponent + 1)
}

/// `Number.prototype.toFixed(digits)` with `digits` already checked to be in `0..=100`.
//~
//~ Rounds to nearest with an exact tie going away from zero, as the spec's
//~ "if there are two such n, pick the larger n" does: `(2.5).toFixed(0)` is
//~ `"3"` and `(1.25).toFixed(1)` is `"1.3"`, where Rust's `{:.1}` answers
//~ `1.2`. Rust's fixed formatting is otherwise exact, so the hand rounding
//~ runs only on a tie (see `smelt_number_round_significant`). A magnitude
//~ of `1e21` or more, and a non-finite value, print as `String(value)`.
fn smelt_number_to_fixed(value: f64, digits: usize) -> String {
    if !value.is_finite() || value.abs() >= 1e21 { return smelt_number_to_string(value); }
    let sign = if value < 0.0 { "-" } else { "" };
    let magnitude = value.abs();
    let probe = format!("{magnitude:.probe_digits$}", probe_digits = digits + 1);
    if !probe.ends_with('5') { return format!("{sign}{magnitude:.digits$}"); }
    let exact = format!("{magnitude:.1100}");
    let (whole, fraction) = exact.split_once('.').unwrap_or((exact.as_str(), ""));
    let mut kept: Vec<u8> = whole.bytes().chain(fraction.bytes().take(digits)).collect();
    if fraction.as_bytes()[digits] >= b'5' {
        let mut carry = true;
        for digit in kept.iter_mut().rev() {
            if *digit == b'9' { *digit = b'0'; } else { *digit += 1; carry = false; break; }
        }
        if carry { kept.insert(0, b'1'); }
    }
    let point = kept.len() - digits;
    let text = String::from_utf8_lossy(&kept);
    if digits == 0 { format!("{sign}{text}") } else { format!("{sign}{}.{}", &text[..point], &text[point..]) }
}

/// `Number.prototype.toExponential(digits)`, `digits` checked to be in `0..=100`;
/// `None` is the shortest digit string that round-trips.
fn smelt_number_to_exponential(value: f64, digits: Option<usize>) -> String {
    if !value.is_finite() { return smelt_number_to_string(value); }
    let sign = if value < 0.0 { "-" } else { "" };
    let magnitude = value.abs();
    let (mantissa, exponent) = if magnitude == 0.0 {
        (vec![b'0'; digits.unwrap_or(0) + 1], 0)
    } else if let Some(digits) = digits {
        smelt_number_round_significant(magnitude, digits + 1)
    } else {
        let text = format!("{magnitude:e}");
        let (mantissa, exponent) = text.split_once('e').unwrap_or((text.as_str(), "0"));
        (mantissa.bytes().filter(u8::is_ascii_digit).collect(), exponent.parse().unwrap_or(0))
    };
    let mantissa = String::from_utf8_lossy(&mantissa);
    let point = if mantissa.len() > 1 { format!("{}.{}", &mantissa[..1], &mantissa[1..]) } else { mantissa.into_owned() };
    format!("{sign}{point}e{}{}", if exponent < 0 { '-' } else { '+' }, exponent.abs())
}

/// `Number.prototype.toPrecision(digits)`, `digits` checked to be in `1..=100`;
/// `None` is `String(value)`.
//~
//~ Exponential form when the exponent is below -6 or not below the
//~ precision, fixed form otherwise -- the spec's steps 10-12.
fn smelt_number_to_precision(value: f64, digits: Option<usize>) -> String {
    let Some(digits) = digits else { return smelt_number_to_string(value) };
    if !value.is_finite() { return smelt_number_to_string(value); }
    let sign = if value < 0.0 { "-" } else { "" };
    let magnitude = value.abs();
    let (mantissa, exponent) = if magnitude == 0.0 { (vec![b'0'; digits], 0) } else { smelt_number_round_significant(magnitude, digits) };
    let mantissa = String::from_utf8_lossy(&mantissa);
    let precision = i32::try_from(digits).unwrap_or(i32::MAX);
    if exponent < -6 || exponent >= precision {
        let point = if digits > 1 { format!("{}.{}", &mantissa[..1], &mantissa[1..]) } else { mantissa.into_owned() };
        return format!("{sign}{point}e{}{}", if exponent < 0 { '-' } else { '+' }, exponent.abs());
    }
    if exponent == precision - 1 { return format!("{sign}{mantissa}"); }
    if exponent >= 0 {
        let point = usize::try_from(exponent + 1).unwrap_or(0);
        return format!("{sign}{}.{}", &mantissa[..point], &mantissa[point..]);
    }
    format!("{sign}0.{}{mantissa}", "0".repeat(usize::try_from(-(exponent + 1)).unwrap_or(0)))
}

/// `Number.prototype.toString(radix)` for a radix other than 10: the integer
/// part and as many fraction digits as tell the value apart from its neighbours.
//~
//~ The digit generation is V8's `DoubleToRadixCString`, the reference
//~ implementation Node runs: fraction digits are produced until the
//~ remaining fraction is smaller than half the gap to the next `f64`
//~ (`delta`), rounding the last digit up -- with a carry into the integer
//~ part -- when the rest exceeds one half. `radix` is checked to be in
//~ `2..=36` by the caller.
fn smelt_number_to_string_radix(value: f64, radix: u32) -> String {
    if radix == 10 || !value.is_finite() || value == 0.0 { return smelt_number_to_string(value); }
    let base = f64::from(radix);
    let negative = value < 0.0;
    let value = value.abs();
    let digit_char = |digit: u32| char::from_digit(digit, radix).unwrap_or('0');
    let mut integer = value.floor();
    let mut fraction = value - integer;
    let mut delta = (0.5 * (f64::from_bits(value.to_bits() + 1) - value)).max(f64::from_bits(1));
    let mut fraction_digits: Vec<u32> = Vec::new();
    if fraction >= delta {
        loop {
            fraction *= base;
            delta *= base;
            //~ `fraction` is in `[0, 1)` before the multiply, so the digit is in `0..radix`.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let digit = fraction as u32;
            fraction_digits.push(digit);
            fraction -= f64::from(digit);
            //~ V8 compares the remainder with one half EXACTLY: a tie rounds to an odd digit's neighbour.
            #[allow(clippy::float_cmp)]
            let round_up = (fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1)) && fraction + delta > 1.0;
            if round_up {
                loop {
                    match fraction_digits.pop() {
                        None => { integer += 1.0; break; }
                        Some(last) if last + 1 < radix => { fraction_digits.push(last + 1); break; }
                        Some(_) => {}
                    }
                }
                break;
            }
            if fraction < delta { break; }
        }
    }
    let mut integer_digits: Vec<char> = Vec::new();
    //~ Above 2^53 the low integer digits are not represented; V8 prints them as zeros.
    while (integer / base).log2().total_cmp(&53.0).is_ge() { integer /= base; integer_digits.push('0'); }
    loop {
        let remainder = integer % base;
        //~ `remainder` is a whole number in `0..radix`.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let digit = remainder as u32;
        integer_digits.push(digit_char(digit));
        integer = (integer - remainder) / base;
        if integer <= 0.0 { break; }
    }
    let mut text = String::with_capacity(integer_digits.len() + fraction_digits.len() + 2);
    if negative { text.push('-'); }
    text.extend(integer_digits.iter().rev());
    if !fraction_digits.is_empty() { text.push('.'); text.extend(fraction_digits.into_iter().map(digit_char)); }
    text
}

/// Run `Number.prototype[method](argument)` on `value`, with the spec's
/// `ToIntegerOrInfinity` and range check on the argument: the formatted
/// text, or the method's `RangeError` message.
//~
//~ The order of the checks is each method's own: `toFixed` and
//~ `toString` reject a bad argument even for `NaN`, while `toPrecision` and
//~ `toExponential` answer `String(value)` for a non-finite value first, so
//~ `NaN.toPrecision(0)` is `"NaN"` and `NaN.toFixed(101)` throws.
fn smelt_number_format_checked(method: &str, value: f64, argument: Option<f64>) -> Result<String, String> {
    let integer = argument.map(|argument| if argument.is_nan() { 0.0 } else { argument.trunc() });
    let (low, high, message) = match method {
        "toFixed" => (0.0, 100.0, "toFixed() digits argument must be between 0 and 100"),
        "toPrecision" => (1.0, 100.0, "toPrecision() argument must be between 1 and 100"),
        "toExponential" => (0.0, 100.0, "toExponential() argument must be between 0 and 100"),
        _ => (2.0, 36.0, "toString() radix argument must be between 2 and 36"),
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let checked = |integer: f64| if (low..=high).contains(&integer) { Ok(integer as usize) } else { Err(message.to_owned()) };
    Ok(match method {
        "toFixed" => smelt_number_to_fixed(value, checked(integer.unwrap_or(0.0))?),
        "toPrecision" | "toExponential" if !value.is_finite() => smelt_number_to_string(value),
        "toPrecision" => smelt_number_to_precision(value, integer.map(checked).transpose()?),
        "toExponential" => smelt_number_to_exponential(value, integer.map(checked).transpose()?),
        #[allow(clippy::cast_possible_truncation)]
        _ => smelt_number_to_string_radix(value, checked(integer.unwrap_or(10.0))? as u32),
    })
}

// @smelt:item-end

// @smelt:item smelt_number_range_error when number_methods & unknown
/// A JavaScript `RangeError` from a `Number.prototype` formatting method.
fn smelt_number_range_error(message: String) -> Box<dyn ::std::error::Error> { smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("RangeError".into())), ("message".to_owned(), SmeltUnknown::String(message.into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)])))) }

// @smelt:item-end
