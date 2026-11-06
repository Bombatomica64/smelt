// The JavaScript string-to-number rule, as one emitted runtime helper.
//
// ECMA-262 `StringToNumber` is what `Number(s)`, unary `+s`, arithmetic and
// relational coercion of a string, and every erased-value-to-number
// conversion apply to a string. Rust's `str::parse::<f64>` is a different
// grammar, and the generated code used to call it in about fifteen places
// with three different wrappers, so the same source construct answered
// differently depending on which path reached it:
//
// | input | Node | `parse::<f64>()` |
// | --- | --- | --- |
// | `""` | `0` | error |
// | `" 1 "`, `"\n2\t"` | `1`, `2` | error (no trimming) |
// | `"0x10"`, `"0o17"`, `"0b11"` | `16`, `15`, `3` | error |
// | `"inf"`, `"nan"`, `"infinity"` | `NaN` | `inf`, `NaN`, `inf` |
// | `"\u{feff}1"` | `1` (U+FEFF is JS whitespace) | error |
// | `"\u{85}1"` | `NaN` (U+0085 is not) | error, but `str::trim` strips it |
//
// ## The grammar
//
// After trimming `StrWhiteSpaceChar` (the `WhiteSpace` and `LineTerminator`
// code points, which is not Rust's `char::is_whitespace`): empty is `0`; an
// unsigned `0x`/`0o`/`0b` literal is that integer; `Infinity` with an optional
// sign is infinite; otherwise the text must be an optionally signed decimal
// literal (`1`, `1.`, `.5`, `1e3`, `1E-3`, ASCII digits only, no `_`), else
// `NaN`. The decimal value comes from Rust's parser once the text is known to
// be in the JS grammar: there the two agree, and digit-to-float rounding is
// the part worth not reimplementing.

// Emit `smelt_string_to_number(&str) -> f64`.
//
// Emitted unconditionally, like `smelt_number_to_string`: it is core coercion
// machinery, reached by `Number(s)`/`+s` on a static string as well as by the
// erased-value conversions, so no single feature flag owns it.
// @smelt:item smelt_string_to_number
/// ECMA-262 `StringToNumber`: the number JavaScript's `Number(text)` answers.
#[allow(dead_code)]
fn smelt_string_to_number(text: &str) -> f64 {
    let text = text.trim_matches(|ch: char| matches!(ch, '\u{9}'..='\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'));
    if text.is_empty() { return 0.0; }
    //~ A radix literal is unsigned in the JS grammar (`-0x10` is NaN),
    //~ so it is matched before the sign is taken.
    let radix = match text.get(..2) { Some("0x" | "0X") => 16, Some("0o" | "0O") => 8, Some("0b" | "0B") => 2, _ => 10 };
    if radix != 10 {
        let digits = &text[2..];
        if digits.is_empty() || !digits.chars().all(|ch| ch.is_digit(radix)) { return f64::NAN; }
        return digits.chars().fold(0.0, |value, ch| value * f64::from(radix) + f64::from(ch.to_digit(radix).unwrap_or(0)));
    }
    let (sign, unsigned) = match text.as_bytes()[0] { b'+' => (1.0, &text[1..]), b'-' => (-1.0, &text[1..]), _ => (1.0, text) };
    if unsigned == "Infinity" { return sign * f64::INFINITY; }
    let bytes = unsigned.as_bytes();
    let digits_from = |mut at: usize| { while at < bytes.len() && bytes[at].is_ascii_digit() { at += 1; } at };
    let int_end = digits_from(0);
    let (frac_start, frac_end) = if bytes.get(int_end) == Some(&b'.') { (int_end + 1, digits_from(int_end + 1)) } else { (int_end, int_end) };
    if int_end == 0 && frac_end == frac_start { return f64::NAN; }
    let mut end = frac_end;
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let exp_start = if matches!(bytes.get(end + 1), Some(b'+' | b'-')) { end + 2 } else { end + 1 };
        end = digits_from(exp_start);
        if end == exp_start { return f64::NAN; }
    }
    if end != bytes.len() { return f64::NAN; }
    unsigned.parse::<f64>().map_or(f64::NAN, |value| sign * value)
}

// @smelt:item-end
