// Python's `json.loads` into a statically typed value, and `json.dumps` with
// an `indent`.
//
// `json.loads` reads the text the way CPython's scanner does (`Lib/json/
// decoder.py`, `Modules/_json.c`): an integer literal is a Python `int` at any
// magnitude (`12345678901234567890123` stays exact), a literal with a fraction
// or an exponent is a `float`, `NaN`/`Infinity`/`-Infinity` are accepted,
// a later duplicate key replaces the earlier value, and a malformed document
// raises `json.JSONDecodeError` (a `ValueError`) with CPython's message and
// `line L column C (char N)` position. The parsed tree is then decoded into
// the destination's static type (`SmeltPyJsonDecode`), never through the
// erased carrier, which holds every number as an `f64`.

// @smelt:item smelt_py_json_loads when py_json_loads
/// One JSON value as `CPython`'s scanner reads it; an integer keeps its digits.
#[allow(dead_code)]
enum SmeltPyJsonValue {
    /// `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// An integer literal's text, decoded to a Python `int` at any size.
    Int(String),
    /// A literal with a fraction or an exponent, or `NaN`/`Infinity`.
    Float(f64),
    /// A string.
    Str(String),
    /// An array.
    Array(Vec<Self>),
    /// An object's members, in document order (duplicates included).
    Object(Vec<(String, Self)>),
}

/// A destination type `json.loads` can decode into.
trait SmeltPyJsonDecode: Sized {
    /// The value of this type `value` stands for, or the `TypeError` naming
    /// the mismatch when the document holds another shape.
    fn py_json_decode(value: SmeltPyJsonValue) -> Result<Self, Box<dyn ::std::error::Error>>;
}

/// The `TypeError` of a document whose shape is not the annotated type's.
#[allow(dead_code)]
fn smelt_py_json_shape_error(expected: &str) -> Box<dyn ::std::error::Error> {
    SmeltPyException::new(&["TypeError", "Exception", "BaseException"], vec![SmeltPyException::arg(&format!("json.loads() result is not {expected}"))]).into_error()
}

impl SmeltPyJsonDecode for SmeltPyInt {
    fn py_json_decode(value: SmeltPyJsonValue) -> Result<Self, Box<dyn ::std::error::Error>> {
        match value { SmeltPyJsonValue::Int(digits) => Self::parse(&digits, 10), _ => Err(smelt_py_json_shape_error("an int")) }
    }
}
impl SmeltPyJsonDecode for f64 {
    //~ A JSON integer at a `float` destination is promoted, as Python's
    //~ numeric tower lets an `int` stand where a `float` is declared.
    fn py_json_decode(value: SmeltPyJsonValue) -> Result<Self, Box<dyn ::std::error::Error>> {
        match value { SmeltPyJsonValue::Float(number) => Ok(number), SmeltPyJsonValue::Int(digits) => SmeltPyInt::parse(&digits, 10)?.to_f64(), _ => Err(smelt_py_json_shape_error("a float")) }
    }
}
impl SmeltPyJsonDecode for bool {
    fn py_json_decode(value: SmeltPyJsonValue) -> Result<Self, Box<dyn ::std::error::Error>> {
        match value { SmeltPyJsonValue::Bool(flag) => Ok(flag), _ => Err(smelt_py_json_shape_error("a bool")) }
    }
}
impl SmeltPyJsonDecode for String {
    fn py_json_decode(value: SmeltPyJsonValue) -> Result<Self, Box<dyn ::std::error::Error>> {
        match value { SmeltPyJsonValue::Str(text) => Ok(text), _ => Err(smelt_py_json_shape_error("a str")) }
    }
}
impl SmeltPyJsonDecode for () {
    fn py_json_decode(value: SmeltPyJsonValue) -> Result<Self, Box<dyn ::std::error::Error>> {
        match value { SmeltPyJsonValue::Null => Ok(()), _ => Err(smelt_py_json_shape_error("None")) }
    }
}
impl<T: SmeltPyJsonDecode> SmeltPyJsonDecode for Option<T> {
    fn py_json_decode(value: SmeltPyJsonValue) -> Result<Self, Box<dyn ::std::error::Error>> {
        match value { SmeltPyJsonValue::Null => Ok(None), other => T::py_json_decode(other).map(Some) }
    }
}
// @smelt:if smelt_list
impl<T: SmeltPyJsonDecode> SmeltPyJsonDecode for SmeltList<T> {
    fn py_json_decode(value: SmeltPyJsonValue) -> Result<Self, Box<dyn ::std::error::Error>> {
        match value { SmeltPyJsonValue::Array(items) => items.into_iter().map(T::py_json_decode).collect::<Result<Vec<_>, _>>().map(Self::from), _ => Err(smelt_py_json_shape_error("a list")) }
    }
}
// @smelt:end
// @smelt:if record
impl<V: SmeltPyJsonDecode> SmeltPyJsonDecode for SmeltRecord<String, V> {
    fn py_json_decode(value: SmeltPyJsonValue) -> Result<Self, Box<dyn ::std::error::Error>> {
        let SmeltPyJsonValue::Object(members) = value else { return Err(smelt_py_json_shape_error("a dict")); };
        let record = Self::new();
        for (key, member) in members { record.insert(key, V::py_json_decode(member)?); }
        Ok(record)
    }
}
// @smelt:end
// @smelt:if dep_indexmap
impl<V: SmeltPyJsonDecode> SmeltPyJsonDecode for ::indexmap::IndexMap<String, V> {
    fn py_json_decode(value: SmeltPyJsonValue) -> Result<Self, Box<dyn ::std::error::Error>> {
        let SmeltPyJsonValue::Object(members) = value else { return Err(smelt_py_json_shape_error("a dict")); };
        let mut map = Self::new();
        for (key, member) in members { map.insert(key, V::py_json_decode(member)?); }
        Ok(map)
    }
}
// @smelt:end

/// A tuple decodes from an array of exactly its length.
macro_rules! smelt_py_json_decode_tuple {
    ($len:expr; $($name:ident),+) => {
        impl<$($name: SmeltPyJsonDecode),+> SmeltPyJsonDecode for ($($name,)+) {
            fn py_json_decode(value: SmeltPyJsonValue) -> Result<Self, Box<dyn ::std::error::Error>> {
                let SmeltPyJsonValue::Array(items) = value else { return Err(smelt_py_json_shape_error("a tuple")); };
                if items.len() != $len { return Err(smelt_py_json_shape_error("a tuple of this length")); }
                let mut items = items.into_iter();
                Ok(($($name::py_json_decode(items.next().ok_or_else(|| smelt_py_json_shape_error("a tuple"))?)?,)+))
            }
        }
    };
}
smelt_py_json_decode_tuple!(1; A);
smelt_py_json_decode_tuple!(2; A, B);
smelt_py_json_decode_tuple!(3; A, B, C);
smelt_py_json_decode_tuple!(4; A, B, C, D);
smelt_py_json_decode_tuple!(5; A, B, C, D, E);
smelt_py_json_decode_tuple!(6; A, B, C, D, E, F);

/// `json.loads(text)` at the destination type `T`.
#[allow(dead_code)]
fn smelt_py_json_loads<T: SmeltPyJsonDecode>(text: &str) -> Result<T, Box<dyn ::std::error::Error>> {
    let chars: Vec<char> = text.chars().collect();
    let mut reader = SmeltPyJsonReader { chars: &chars };
    let start = reader.skip_ws(0);
    let (value, end) = reader.value(start).map_err(|(message, at)| smelt_py_json_decode_error(&chars, message, at))?;
    let end = reader.skip_ws(end);
    if end != chars.len() {
        return Err(smelt_py_json_decode_error(&chars, "Extra data", end));
    }
    T::py_json_decode(value)
}

/// `json.JSONDecodeError(message, doc, at)`: `str(e)` is
/// `"{message}: line {L} column {C} (char {at})"`, positions in code points.
#[allow(dead_code)]
fn smelt_py_json_decode_error(chars: &[char], message: &str, at: usize) -> Box<dyn ::std::error::Error> {
    let before = &chars[..at.min(chars.len())];
    let line = before.iter().filter(|ch| **ch == '\n').count() + 1;
    let column = before.iter().rposition(|ch| *ch == '\n').map_or(at + 1, |newline| at - newline);
    let text = format!("{message}: line {line} column {column} (char {at})");
    SmeltPyException::new(&["JSONDecodeError", "ValueError", "Exception", "BaseException"], vec![SmeltPyException::arg(&text)]).into_error()
}

/// A scan failure: `CPython`'s message and the position it names.
type SmeltPyJsonFailure = (&'static str, usize);

/// The scanner over the document's code points (`scan_once` and friends).
struct SmeltPyJsonReader<'a> {
    /// The document.
    chars: &'a [char],
}

impl SmeltPyJsonReader<'_> {
    /// The position after any JSON whitespace (`' \t\n\r'`) from `at`.
    fn skip_ws(&self, mut at: usize) -> usize {
        while matches!(self.chars.get(at), Some(' ' | '\t' | '\n' | '\r')) { at += 1; }
        at
    }

    /// Whether the document spells `word` at `at`.
    fn spells(&self, at: usize, word: &str) -> bool {
        word.chars().enumerate().all(|(offset, ch)| self.chars.get(at + offset) == Some(&ch))
    }

    /// `scan_once`: the value starting exactly at `at` and where it ends.
    fn value(&self, at: usize) -> Result<(SmeltPyJsonValue, usize), SmeltPyJsonFailure> {
        match self.chars.get(at) {
            Some('"') => self.string(at + 1).map(|(text, end)| (SmeltPyJsonValue::Str(text), end)),
            Some('{') => self.object(at + 1),
            Some('[') => self.array(at + 1),
            Some('n') if self.spells(at, "null") => Ok((SmeltPyJsonValue::Null, at + 4)),
            Some('t') if self.spells(at, "true") => Ok((SmeltPyJsonValue::Bool(true), at + 4)),
            Some('f') if self.spells(at, "false") => Ok((SmeltPyJsonValue::Bool(false), at + 5)),
            Some('N') if self.spells(at, "NaN") => Ok((SmeltPyJsonValue::Float(f64::NAN), at + 3)),
            Some('I') if self.spells(at, "Infinity") => Ok((SmeltPyJsonValue::Float(f64::INFINITY), at + 8)),
            Some('-') if self.spells(at, "-Infinity") => Ok((SmeltPyJsonValue::Float(f64::NEG_INFINITY), at + 9)),
            _ => self.number(at).ok_or(("Expecting value", at)),
        }
    }

    /// `NUMBER_RE`, `-?(0|[1-9]\d*)(\.\d+)?([eE][-+]?\d+)?`: an `int` without
    /// a fraction or exponent, otherwise a correctly rounded `float`.
    fn number(&self, at: usize) -> Option<(SmeltPyJsonValue, usize)> {
        let digit = |index: usize| self.chars.get(index).is_some_and(char::is_ascii_digit);
        let mut end = at;
        if self.chars.get(end) == Some(&'-') { end += 1; }
        match self.chars.get(end) {
            Some('0') => end += 1,
            Some('1'..='9') => { while digit(end) { end += 1; } }
            _ => return None,
        }
        let has_fraction = self.chars.get(end) == Some(&'.') && digit(end + 1);
        if has_fraction {
            end += 1;
            while digit(end) { end += 1; }
        }
        let mut has_exponent = false;
        if matches!(self.chars.get(end), Some('e' | 'E')) {
            let mut exponent = end + 1;
            if matches!(self.chars.get(exponent), Some('+' | '-')) { exponent += 1; }
            has_exponent = digit(exponent);
            if has_exponent {
                while digit(exponent) { exponent += 1; }
                end = exponent;
            }
        }
        let is_float = has_fraction || has_exponent;
        let text: String = self.chars[at..end].iter().collect();
        Some((if is_float { SmeltPyJsonValue::Float(text.parse().ok()?) } else { SmeltPyJsonValue::Int(text) }, end))
    }

    /// `scanstring`: the string whose opening quote is just before `start`.
    fn string(&self, start: usize) -> Result<(String, usize), SmeltPyJsonFailure> {
        let mut out = String::new();
        let mut at = start;
        loop {
            let Some(&ch) = self.chars.get(at) else { return Err(("Unterminated string starting at", start - 1)); };
            match ch {
                '"' => return Ok((out, at + 1)),
                '\\' => {
                    let Some(&escape) = self.chars.get(at + 1) else { return Err(("Unterminated string starting at", start - 1)); };
                    at += 2;
                    match escape {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let unit = self.hex4(at).ok_or(("Invalid \\uXXXX escape", at - 1))?;
                            at += 4;
                            let mut code = unit;
                            if (0xd800..0xdc00).contains(&unit) && self.chars.get(at) == Some(&'\\') && self.chars.get(at + 1) == Some(&'u') {
                                let low = self.hex4(at + 2).ok_or(("Invalid \\uXXXX escape", at + 1))?;
                                if (0xdc00..0xe000).contains(&low) {
                                    code = 0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00);
                                    at += 6;
                                }
                            }
                            //~ A lone surrogate is a valid Python `str` but
                            //~ no Rust `String` holds one.
                            out.push(char::from_u32(code).ok_or(("Invalid \\uXXXX escape", at - 1))?);
                        }
                        _ => return Err(("Invalid \\escape", at - 2)),
                    }
                }
                control if u32::from(control) < 0x20 => return Err(("Invalid control character at", at)),
                other => { out.push(other); at += 1; }
            }
        }
    }

    /// Four hex digits at `at`, as a code unit.
    fn hex4(&self, at: usize) -> Option<u32> {
        let digits: String = self.chars.get(at..at + 4)?.iter().collect();
        if digits.chars().all(|ch| ch.is_ascii_hexdigit()) { u32::from_str_radix(&digits, 16).ok() } else { None }
    }

    /// `JSONObject`: the members after the `{` just before `start`.
    fn object(&self, start: usize) -> Result<(SmeltPyJsonValue, usize), SmeltPyJsonFailure> {
        let mut members = Vec::new();
        let mut at = self.skip_ws(start);
        match self.chars.get(at) {
            Some('}') => return Ok((SmeltPyJsonValue::Object(members), at + 1)),
            Some('"') => {}
            _ => return Err(("Expecting property name enclosed in double quotes", at)),
        }
        loop {
            let (key, after_key) = self.string(at + 1)?;
            at = self.skip_ws(after_key);
            if self.chars.get(at) != Some(&':') { return Err(("Expecting ':' delimiter", at)); }
            at = self.skip_ws(at + 1);
            let (member, after_value) = self.value(at)?;
            members.push((key, member));
            at = self.skip_ws(after_value);
            match self.chars.get(at) {
                Some('}') => return Ok((SmeltPyJsonValue::Object(members), at + 1)),
                Some(',') => {}
                _ => return Err(("Expecting ',' delimiter", at)),
            }
            let comma = at;
            at = self.skip_ws(at + 1);
            match self.chars.get(at) {
                Some('"') => {}
                Some('}') => return Err(("Illegal trailing comma before end of object", comma)),
                _ => return Err(("Expecting property name enclosed in double quotes", at)),
            }
        }
    }

    /// `JSONArray`: the items after the `[` just before `start`.
    fn array(&self, start: usize) -> Result<(SmeltPyJsonValue, usize), SmeltPyJsonFailure> {
        let mut items = Vec::new();
        let mut at = self.skip_ws(start);
        if self.chars.get(at) == Some(&']') { return Ok((SmeltPyJsonValue::Array(items), at + 1)); }
        loop {
            let (item, after_item) = self.value(at)?;
            items.push(item);
            at = self.skip_ws(after_item);
            match self.chars.get(at) {
                Some(']') => return Ok((SmeltPyJsonValue::Array(items), at + 1)),
                Some(',') => {}
                _ => return Err(("Expecting ',' delimiter", at)),
            }
            let comma = at;
            at = self.skip_ws(at + 1);
            if self.chars.get(at) == Some(&']') { return Err(("Illegal trailing comma before end of array", comma)); }
        }
    }
}
// @smelt:item-end

// @smelt:item smelt_py_json_indent when py_json_indent
/// `json.dumps(value, indent=indent)`'s layout over the default compact text
/// (`", "` and `": "` separators): every item on its own line, one `indent`
/// deeper than its container, `,` ending a line (`CPython` switches the item
/// separator to `","` once `indent` is given), `": "` after a key, and an
/// empty `[]`/`{}` kept whole. Unlike JavaScript, an empty `indent` (or
/// `indent=0`) still breaks lines. Strings are copied untouched.
#[allow(dead_code)]
fn smelt_py_json_indent(text: &str, indent: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() * 2);
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0_usize;
    let newline = |out: &mut String, depth: usize| { out.push('\n'); for _ in 0..depth { out.push_str(indent); } };
    while index < chars.len() {
        let ch = chars[index];
        index += 1;
        if in_string {
            out.push(ch);
            if escaped { escaped = false; } else if ch == '\\' { escaped = true; } else if ch == '"' { in_string = false; }
            continue;
        }
        match ch {
            '"' => { in_string = true; out.push(ch); }
            '{' | '[' => {
                let close = if ch == '{' { '}' } else { ']' };
                out.push(ch);
                if chars.get(index) == Some(&close) { out.push(close); index += 1; continue; }
                depth += 1;
                newline(&mut out, depth);
            }
            '}' | ']' => { depth = depth.saturating_sub(1); newline(&mut out, depth); out.push(ch); }
            ',' => { out.push(','); newline(&mut out, depth); if chars.get(index) == Some(&' ') { index += 1; } }
            _ => out.push(ch),
        }
    }
    out
}
// @smelt:item-end
