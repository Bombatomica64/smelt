// Python's format-spec mini-language: `f"{x:.2f}"`, `format(n, ">8,")`.
//
// `[[fill]align][sign]["z"]["#"]["0"][width][grouping]["." precision][type]`,
// applied to an `int`, a `float` or a `str` with CPython's rules and its
// `ValueError`s for a spec the type does not take. Widths, precisions and
// truncation count code points.

// @smelt:item SmeltPyFormatSpec when py_format
/// A parsed format spec.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct SmeltPyFormatSpec {
    //~ The padding character, `None` when the spec names none.
    fill: Option<char>,
    //~ `<`, `>`, `^` or `=`, `None` for the type's default.
    align: Option<char>,
    //~ `+`, `-` or ` `, `None` when the spec names no sign.
    sign: Option<char>,
    //~ `z`: a negative zero formats as zero.
    no_negative_zero: bool,
    //~ `#`: the alternate form.
    alternate: bool,
    //~ The minimum width in code points.
    width: usize,
    //~ `,` or `_`.
    grouping: Option<char>,
    //~ The precision.
    precision: Option<usize>,
    //~ The presentation type.
    kind: Option<char>,
}

/// A `ValueError` with `message`, the error every bad spec raises.
#[allow(dead_code)]
fn smelt_py_format_error(message: &str) -> Box<dyn ::std::error::Error> {
    smelt_py_value_error(message)
}

/// Parse `spec`, refusing one that is malformed for an object of type
/// `type_name`.
#[allow(dead_code)]
fn smelt_py_parse_format_spec(spec: &str, type_name: &str) -> Result<SmeltPyFormatSpec, Box<dyn ::std::error::Error>> {
    let chars: Vec<char> = spec.chars().collect();
    let mut at = 0;
    let is_align = |ch: char| matches!(ch, '<' | '>' | '=' | '^');
    let (mut fill, mut align) = (None, None);
    if chars.len() >= 2 && is_align(chars[1]) {
        fill = Some(chars[0]);
        align = Some(chars[1]);
        at = 2;
    } else if chars.first().copied().is_some_and(is_align) {
        align = chars.first().copied();
        at = 1;
    }
    let sign = chars.get(at).copied().filter(|ch| matches!(ch, '+' | '-' | ' '));
    at += usize::from(sign.is_some());
    let no_negative_zero = chars.get(at) == Some(&'z');
    at += usize::from(no_negative_zero);
    let alternate = chars.get(at) == Some(&'#');
    at += usize::from(alternate);
    //~ A `0` before the width pads with zeros after the sign, unless the
    //~ spec already names a fill or an alignment.
    if chars.get(at) == Some(&'0') {
        fill = fill.or(Some('0'));
        align = align.or(Some('='));
        at += 1;
    }
    let digits = |from: usize| chars[from..].iter().take_while(|ch| ch.is_ascii_digit()).count();
    let width_digits = digits(at);
    let width = chars[at..at + width_digits].iter().collect::<String>().parse::<usize>().unwrap_or(0);
    at += width_digits;
    let grouping = chars.get(at).copied().filter(|ch| matches!(ch, ',' | '_'));
    at += usize::from(grouping.is_some());
    let mut precision = None;
    if chars.get(at) == Some(&'.') {
        let precision_digits = digits(at + 1);
        if precision_digits == 0 {
            return Err(smelt_py_format_error("Format specifier missing precision"));
        }
        precision = chars[at + 1..at + 1 + precision_digits].iter().collect::<String>().parse::<usize>().ok();
        at += 1 + precision_digits;
    }
    let kind = chars.get(at).copied();
    if chars.len() > at + usize::from(kind.is_some()) {
        return Err(smelt_py_format_error(&format!("Invalid format specifier '{spec}' for object of type '{type_name}'")));
    }
    Ok(SmeltPyFormatSpec { fill, align, sign, no_negative_zero, alternate, width, grouping, precision, kind })
}

/// `digits` (ASCII) with `separator` every `every` digits from the right.
#[allow(dead_code)]
fn smelt_py_group_digits(digits: &str, separator: char, every: usize) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / every);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(every) {
            out.push(separator);
        }
        out.push(ch);
    }
    out
}

/// Lay out a formatted number: `sign` and `prefix` (`0x`), the `digits`
/// (grouped when the spec asks, and zero-padded through the grouping for
/// `0` padding), then `suffix` (a fraction, an exponent, `%`), padded to the
/// width with the spec's alignment (right by default).
#[allow(dead_code)]
fn smelt_py_format_number(spec: &SmeltPyFormatSpec, sign: &str, prefix: &str, digits: &str, suffix: &str, every: usize) -> String {
    let group = |digits: &str| spec.grouping.map_or_else(|| digits.to_owned(), |separator| smelt_py_group_digits(digits, separator, every));
    let fill = spec.fill.unwrap_or(' ');
    let align = spec.align.unwrap_or('>');
    let mut body = group(digits);
    if align == '=' && fill == '0' && spec.grouping.is_some() {
        //~ Zero padding grows the digits, separators included.
        let mut padded = digits.to_owned();
        while sign.chars().count() + prefix.chars().count() + body.chars().count() + suffix.chars().count() < spec.width {
            padded.insert(0, '0');
            body = group(&padded);
        }
    }
    let content = format!("{body}{suffix}");
    let used = sign.chars().count() + prefix.chars().count() + content.chars().count();
    let pad = spec.width.saturating_sub(used);
    let repeat = |count: usize| ::std::iter::repeat_n(fill, count).collect::<String>();
    match align {
        '<' => format!("{sign}{prefix}{content}{}", repeat(pad)),
        '^' => format!("{}{sign}{prefix}{content}{}", repeat(pad / 2), repeat(pad - pad / 2)),
        '=' => format!("{sign}{prefix}{}{content}", repeat(pad)),
        _ => format!("{}{sign}{prefix}{content}", repeat(pad)),
    }
}

/// The sign text of a number that is `negative`, under the spec's sign.
#[allow(dead_code)]
const fn smelt_py_format_sign(spec: &SmeltPyFormatSpec, negative: bool) -> &'static str {
    match (negative, spec.sign) {
        (true, _) => "-",
        (false, Some('+')) => "+",
        (false, Some(' ')) => " ",
        _ => "",
    }
}

/// `format(n, spec)` for a Python `int`.
#[allow(dead_code)]
fn smelt_py_format_int(value: &SmeltPyInt, spec: &str) -> Result<String, Box<dyn ::std::error::Error>> {
    let parsed = smelt_py_parse_format_spec(spec, "int")?;
    match parsed.kind {
        Some('e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%') => return smelt_py_format_float_spec(value.to_f64()?, &parsed),
        Some('d' | 'n' | 'b' | 'o' | 'x' | 'X' | 'c') | None => {}
        Some(code) => return Err(smelt_py_format_error(&format!("Unknown format code '{code}' for object of type 'int'"))),
    }
    if parsed.precision.is_some() {
        return Err(smelt_py_format_error("Precision not allowed in integer format specifier"));
    }
    let kind = parsed.kind.unwrap_or('d');
    if parsed.grouping == Some(',') && !matches!(kind, 'd' | 'n') {
        return Err(smelt_py_format_error(&format!("Cannot specify ',' with '{kind}'.")));
    }
    if kind == 'c' {
        if parsed.sign.is_some() {
            return Err(smelt_py_format_error("Sign not allowed with integer format specifier 'c'"));
        }
        let code = value.as_small().and_then(|code| u32::try_from(code).ok()).and_then(char::from_u32);
        let text = code.ok_or_else(|| smelt_py_format_error("%c arg not in range(0x110000)"))?.to_string();
        return Ok(smelt_py_format_number(&parsed, "", "", &text, "", 3));
    }
    let magnitude = value.abs().to_big();
    let (digits, prefix, every) = match kind {
        'b' => (magnitude.to_str_radix(2), "0b", 4),
        'o' => (magnitude.to_str_radix(8), "0o", 4),
        'x' => (magnitude.to_str_radix(16), "0x", 4),
        'X' => (magnitude.to_str_radix(16).to_uppercase(), "0X", 4),
        _ => (magnitude.to_string(), "", 3),
    };
    let sign = smelt_py_format_sign(&parsed, value.is_negative());
    Ok(smelt_py_format_number(&parsed, sign, if parsed.alternate { prefix } else { "" }, &digits, "", every))
}

/// `format(x, spec)` for a `float`.
#[allow(dead_code)]
fn smelt_py_format_float(value: f64, spec: &str) -> Result<String, Box<dyn ::std::error::Error>> {
    let parsed = smelt_py_parse_format_spec(spec, "float")?;
    smelt_py_format_float_spec(value, &parsed)
}

/// Rewrite Rust's `{:e}` exponent (`1.5e3`) the way C writes it (`1.5e+03`).
#[allow(dead_code)]
fn smelt_py_c_exponent(text: &str, upper: bool) -> String {
    let (mantissa, exponent) = text.split_once('e').unwrap_or((text, "0"));
    let (sign, digits) = exponent.strip_prefix('-').map_or(("+", exponent), |digits| ("-", digits));
    let marker = if upper { 'E' } else { 'e' };
    format!("{mantissa}{marker}{sign}{digits:0>2}")
}

/// `format(x, spec)` for a `float` whose spec is parsed.
#[allow(dead_code)]
fn smelt_py_format_float_spec(value: f64, spec: &SmeltPyFormatSpec) -> Result<String, Box<dyn ::std::error::Error>> {
    let kind = spec.kind;
    if let Some(code) = kind.filter(|code| !matches!(code, 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | 'n' | '%')) {
        return Err(smelt_py_format_error(&format!("Unknown format code '{code}' for object of type 'float'")));
    }
    let upper = matches!(kind, Some('E' | 'F' | 'G'));
    let negative = value.is_sign_negative() && !value.is_nan();
    let magnitude = value.abs();
    let (mut text, percent) = if magnitude.is_finite() {
        match kind {
            Some('f' | 'F') => (format!("{magnitude:.*}", spec.precision.unwrap_or(6)), false),
            Some('%') => (format!("{:.*}", spec.precision.unwrap_or(6), magnitude * 100.0), true),
            Some('e' | 'E') => (smelt_py_c_exponent(&format!("{magnitude:.*e}", spec.precision.unwrap_or(6)), upper), false),
            None if spec.precision.is_none() => (smelt_py_float_repr(magnitude), false),
            _ => {
                //~ General format: `precision` significant digits, fixed
                //~ notation while the exponent lies in `[-4, precision)`.
                let precision = spec.precision.unwrap_or(6).max(1);
                let scientific = format!("{magnitude:.*e}", precision - 1);
                let exponent = scientific.split_once('e').and_then(|(_, exponent)| exponent.parse::<i64>().ok()).unwrap_or(0);
                let fixed = (-4..i64::try_from(precision).unwrap_or(i64::MAX)).contains(&exponent);
                let mut body = if fixed {
                    let decimals = usize::try_from(i64::try_from(precision).unwrap_or(0) - 1 - exponent).unwrap_or(0);
                    format!("{magnitude:.decimals$}")
                } else {
                    scientific
                };
                if !spec.alternate {
                    let (mantissa, rest) = body.split_once('e').map_or((body.as_str(), None), |(mantissa, rest)| (mantissa, Some(rest)));
                    let mut trimmed = mantissa.to_owned();
                    if trimmed.contains('.') {
                        trimmed = trimmed.trim_end_matches('0').trim_end_matches('.').to_owned();
                    }
                    body = rest.map_or_else(|| trimmed.clone(), |rest| format!("{trimmed}e{rest}"));
                }
                if kind.is_none() && fixed && !body.contains('.') {
                    body.push_str(".0");
                }
                (if fixed { body } else { smelt_py_c_exponent(&body, upper) }, false)
            }
        }
    } else {
        let word = if magnitude.is_nan() { "nan" } else { "inf" };
        (if upper { word.to_uppercase() } else { word.to_owned() }, kind == Some('%'))
    };
    if spec.alternate && !text.contains('.') && magnitude.is_finite() {
        match text.find(['e', 'E']) {
            Some(at) => text.insert(at, '.'),
            None => text.push('.'),
        }
    }
    let rounds_to_zero = text.chars().all(|ch| matches!(ch, '0' | '.' | 'e' | 'E' | '+' | '-'));
    let shown_negative = negative && !(spec.no_negative_zero && rounds_to_zero);
    let sign = smelt_py_format_sign(spec, shown_negative);
    let split = text.find(['.', 'e', 'E']).unwrap_or(text.len());
    let (digits, rest) = text.split_at(split);
    let suffix = if percent { format!("{rest}%") } else { rest.to_owned() };
    Ok(smelt_py_format_number(spec, sign, "", digits, &suffix, 3))
}

/// `format(text, spec)` for a `str`.
#[allow(dead_code)]
fn smelt_py_format_str(value: &str, spec: &str) -> Result<String, Box<dyn ::std::error::Error>> {
    let parsed = smelt_py_parse_format_spec(spec, "str")?;
    if let Some(code) = parsed.kind.filter(|code| *code != 's') {
        return Err(smelt_py_format_error(&format!("Unknown format code '{code}' for object of type 'str'")));
    }
    if parsed.sign.is_some() {
        return Err(smelt_py_format_error("Sign not allowed in string format specifier"));
    }
    if let Some(separator) = parsed.grouping {
        return Err(smelt_py_format_error(&format!("Cannot specify '{separator}' with 's'.")));
    }
    if parsed.align == Some('=') {
        return Err(smelt_py_format_error("'=' alignment not allowed in string format specifier"));
    }
    let text: String = parsed.precision.map_or_else(|| value.to_owned(), |precision| value.chars().take(precision).collect());
    let fill = parsed.fill.unwrap_or(' ');
    let pad = parsed.width.saturating_sub(text.chars().count());
    let repeat = |count: usize| ::std::iter::repeat_n(fill, count).collect::<String>();
    Ok(match parsed.align.unwrap_or('<') {
        '>' => format!("{}{text}", repeat(pad)),
        '^' => format!("{}{text}{}", repeat(pad / 2), repeat(pad - pad / 2)),
        _ => format!("{text}{}", repeat(pad)),
    })
}

// @smelt:item-end
