// JavaScript strings: UTF-16 code units, shared by reference count.
//
// A JavaScript string is a sequence of UTF-16 code units, and V8 stores it that
// way. Every index (`length`, `slice`, `charCodeAt`, a match's `index`) counts
// code units, a lone surrogate is an ordinary value (`"😀".slice(0, 1)` is
// `"\ud83d"`), and `<` and `sort()` compare code unit by code unit. Generated
// crates store a JavaScript string the same way, so each of those is O(1) per
// index and exact, with no translation from UTF-8 offsets.
//
// `JsString` is the owned string and `JsStr` the borrowed view, as `String` and
// `str` are: `&JsString` derefs to `&JsStr`, read-only operations live on
// `JsStr`, and `JsString` adds what needs ownership. A clone shares the buffer.
// Appending (`a + &b`, `push_str`) writes in place when the handle is the only
// one and copies first when the buffer is shared: the emitter compiles
// `text += piece` to `text = text + &piece` with `text` moved into the `+`, so a
// builder loop stays linear.
//
// Text leaves through `Display`, which writes UTF-8 and replaces a lone
// surrogate with U+FFFD, as Node does on stdout.

// @smelt:item JsString when js_string
/// A JavaScript string: immutable UTF-16 code units, shared by reference count.
///
/// Lone surrogates are representable. Equality, hashing and ordering are by
/// code unit, which is ECMAScript's order (`"😀" < "\u{FFFF}"`). A clone shares
/// the buffer; [`JsString::push_str`] and `+` append in place when this handle
/// is the buffer's only owner and copy it first when it is shared.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct JsString(::std::rc::Rc<Vec<u16>>);

/// A borrowed JavaScript string: a slice of UTF-16 code units.
///
/// The view `&JsString` derefs to, as `&String` derefs to `&str`. Its equality,
/// hash and order are the code units', so a `JsString` key is found by a
/// `&JsStr` lookup.
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
struct JsStr([u16]);

impl JsStr {
    /// Views code units as a string.
    #[allow(unsafe_code)]
    const fn from_units(units: &[u16]) -> &Self {
        //~ `JsStr` is `repr(transparent)` over `[u16]`, so the two pointers
        //~ have the same layout and metadata; this is how `std` builds a
        //~ `&Path` from a `&OsStr`.
        unsafe { &*(::std::ptr::from_ref::<[u16]>(units) as *const Self) }
    }

    /// The code units.
    const fn units(&self) -> &[u16] {
        &self.0
    }

    /// The JavaScript `length`: the number of code units.
    const fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether this is the empty string.
    const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The code unit at `index` (`charCodeAt`), if any.
    fn unit_at(&self, index: usize) -> Option<u16> {
        self.0.get(index).copied()
    }

    /// The one-unit string at `index` (`charAt`, `s[i]`), if any. At either
    /// half of a surrogate pair this is that lone half.
    fn char_at(&self, index: usize) -> Option<JsString> {
        self.0.get(index).map(|unit| JsString::from_units(&[*unit]))
    }

    /// The code point at `index` (`codePointAt`): a whole surrogate pair when
    /// `index` holds its leading half, else the unit there.
    fn code_point_at(&self, index: usize) -> Option<u32> {
        let first = *self.0.get(index)?;
        if (0xD800..0xDC00).contains(&first)
            && let Some(second) = self.0.get(index + 1).copied().filter(|unit| (0xDC00..0xE000).contains(unit))
        {
            return Some(0x10000 + ((u32::from(first) - 0xD800) << 10) + (u32::from(second) - 0xDC00));
        }
        Some(u32::from(first))
    }

    /// The code units `start..end`, clamped to the string; empty when
    /// `end <= start`. A cut may split a surrogate pair, leaving lone halves.
    fn slice(&self, start: usize, end: usize) -> JsString {
        let end = end.min(self.0.len());
        if end <= start { return JsString::default(); }
        JsString::from_units(&self.0[start..end])
    }

    /// [`Self::slice`] as a borrowed view.
    fn sub(&self, start: usize, end: usize) -> &Self {
        let end = end.min(self.0.len());
        Self::from_units(self.0.get(start.min(end)..end).unwrap_or(&[]))
    }

    /// The first index at or after `from` where `needle` occurs (`indexOf`).
    ///
    /// `from` past the end is the end, so an empty needle is found at
    /// `min(from, length)`, as in JavaScript.
    fn index_of(&self, needle: &Self, from: usize) -> Option<usize> {
        let (haystack, needle) = (&self.0, &needle.0);
        let from = from.min(haystack.len());
        let Some((&first, rest)) = needle.split_first() else { return Some(from) };
        let last_start = haystack.len().checked_sub(needle.len())?;
        let mut at = from;
        while at <= last_start {
            //~ Scan for the first unit, then compare the rest in place.
            at += haystack[at..=last_start].iter().position(|unit| *unit == first)?;
            if haystack[at + 1..at + needle.len()] == *rest { return Some(at); }
            at += 1;
        }
        None
    }

    /// The last index at or before `from` where `needle` occurs (`lastIndexOf`).
    fn last_index_of(&self, needle: &Self, from: usize) -> Option<usize> {
        let (haystack, needle) = (&self.0, &needle.0);
        let last_start = haystack.len().checked_sub(needle.len())?;
        (0..=from.min(last_start)).rev().find(|at| haystack[*at..*at + needle.len()] == *needle)
    }

    /// Whether `needle` occurs anywhere (`includes`).
    fn contains(&self, needle: &Self) -> bool {
        self.index_of(needle, 0).is_some()
    }

    /// Whether the string starts with `prefix` (`startsWith`).
    fn starts_with(&self, prefix: &Self) -> bool {
        self.0.starts_with(&prefix.0)
    }

    /// Whether the string ends with `suffix` (`endsWith`).
    fn ends_with(&self, suffix: &Self) -> bool {
        self.0.ends_with(&suffix.0)
    }

    /// The string with JavaScript white space removed from both ends (`trim`).
    fn trim(&self) -> &Self {
        self.trim_start().trim_end()
    }

    /// The string with leading JavaScript white space removed (`trimStart`).
    fn trim_start(&self) -> &Self {
        let start = self.0.iter().position(|unit| !js_is_white_space(*unit)).unwrap_or(self.0.len());
        Self::from_units(&self.0[start..])
    }

    /// The string with trailing JavaScript white space removed (`trimEnd`).
    fn trim_end(&self) -> &Self {
        let end = self.0.iter().rposition(|unit| !js_is_white_space(*unit)).map_or(0, |last| last + 1);
        Self::from_units(&self.0[..end])
    }

    /// The string repeated `count` times (`repeat`).
    fn repeat(&self, count: usize) -> JsString {
        JsString::from(self.0.repeat(count))
    }

    /// The pieces between occurrences of `separator` (`split`), at most
    /// `limit` of them. An empty separator splits into single code units.
    fn split(&self, separator: &Self, limit: usize) -> Vec<JsString> {
        let mut pieces = Vec::new();
        if limit == 0 { return pieces; }
        if separator.is_empty() {
            pieces.extend(self.0.iter().take(limit).map(|unit| JsString::from_units(&[*unit])));
            return pieces;
        }
        let mut start = 0;
        while let Some(at) = self.index_of(separator, start) {
            pieces.push(self.slice(start, at));
            if pieces.len() == limit { return pieces; }
            start = at + separator.len();
        }
        pieces.push(self.slice(start, self.len()));
        pieces
    }

    /// The string with its first occurrence of `pattern` replaced by
    /// `replacement` (`replace` with a string pattern and a plain
    /// replacement, no `$` substitutions).
    fn replace_first(&self, pattern: &Self, replacement: &Self) -> JsString {
        self.index_of(pattern, 0).map_or_else(|| self.to_owned(), |at| {
            let mut out = Vec::with_capacity(self.len() - pattern.len() + replacement.len());
            out.extend_from_slice(&self.0[..at]);
            out.extend_from_slice(&replacement.0);
            out.extend_from_slice(&self.0[at + pattern.len()..]);
            JsString::from(out)
        })
    }

    /// The string with every occurrence of `pattern` replaced by `replacement`
    /// (`replaceAll` with a string pattern and a plain replacement). An empty
    /// pattern matches before every unit and at the end.
    fn replace_all(&self, pattern: &Self, replacement: &Self) -> JsString {
        let mut out = Vec::with_capacity(self.len());
        if pattern.is_empty() {
            for unit in &self.0 {
                out.extend_from_slice(&replacement.0);
                out.push(*unit);
            }
            out.extend_from_slice(&replacement.0);
            return JsString::from(out);
        }
        let mut start = 0;
        while let Some(at) = self.index_of(pattern, start) {
            out.extend_from_slice(&self.0[start..at]);
            out.extend_from_slice(&replacement.0);
            start = at + pattern.len();
        }
        out.extend_from_slice(&self.0[start..]);
        JsString::from(out)
    }

    /// The characters, decoded: `Ok` for a scalar value, `Err` for a lone
    /// surrogate.
    fn chars_lossless(&self) -> impl Iterator<Item = Result<char, u16>> + '_ {
        char::decode_utf16(self.0.iter().copied()).map(|decoded| decoded.map_err(|lone| lone.unpaired_surrogate()))
    }

    /// The code points (`for...of`, `Array.from`, `[...s]`): a surrogate pair
    /// is one code point, a lone surrogate is its own.
    fn code_points(&self) -> impl Iterator<Item = u32> + '_ {
        self.chars_lossless().map(|decoded| decoded.map_or_else(u32::from, u32::from))
    }

    /// One string per code point (`[...s]`): a surrogate pair is one piece.
    fn code_point_strings(&self) -> Vec<JsString> {
        let mut pieces = Vec::with_capacity(self.len());
        let mut at = 0;
        while at < self.0.len() {
            let width = if self.code_point_at(at).is_some_and(|point| point > 0xFFFF) { 2 } else { 1 };
            pieces.push(JsString::from_units(&self.0[at..at + width]));
            at += width;
        }
        pieces
    }

    /// Whether the string has no lone surrogate (`isWellFormed`).
    fn is_well_formed(&self) -> bool {
        self.chars_lossless().all(|decoded| decoded.is_ok())
    }

    /// The string with each lone surrogate replaced by U+FFFD (`toWellFormed`).
    fn to_well_formed(&self) -> JsString {
        if self.is_well_formed() { return self.to_owned(); }
        let mut out = Vec::with_capacity(self.len());
        for decoded in self.chars_lossless() {
            let mut buffer = [0; 2];
            out.extend_from_slice(decoded.unwrap_or('\u{FFFD}').encode_utf16(&mut buffer));
        }
        JsString::from(out)
    }

    /// UTF-8 text, a lone surrogate becoming U+FFFD: what Node writes to stdout.
    fn to_string_lossy(&self) -> String {
        if self.0.iter().all(|unit| *unit < 0x80) {
            //~ ASCII: every unit is one byte.
            return self.0.iter().map(|unit| char::from(u8::try_from(*unit).unwrap_or(b'?'))).collect();
        }
        self.chars_lossless().map(|decoded| decoded.unwrap_or('\u{FFFD}')).collect()
    }

    /// UTF-8 text when the string is well formed, `None` when it holds a lone
    /// surrogate.
    fn to_str_exact(&self) -> Option<String> {
        self.chars_lossless().collect::<Result<String, u16>>().ok()
    }

    /// The string upper-cased by the Unicode default rules (`toUpperCase`); a
    /// lone surrogate passes through unchanged.
    fn to_upper_case(&self) -> JsString {
        self.map_case(u8::to_ascii_uppercase, str::to_uppercase)
    }

    /// The string lower-cased by the Unicode default rules (`toLowerCase`); a
    /// lone surrogate passes through unchanged.
    fn to_lower_case(&self) -> JsString {
        self.map_case(u8::to_ascii_lowercase, str::to_lowercase)
    }

    /// Case mapping: `ascii` per unit when the text is ASCII, else `full` over
    /// each well-formed run, so the standard library's special casing and
    /// final-sigma rules apply and lone surrogates stay where they are.
    fn map_case(&self, ascii: fn(&u8) -> u8, full: fn(&str) -> String) -> JsString {
        if self.0.iter().all(|unit| *unit < 0x80) {
            return JsString::from(self.0.iter().map(|unit| u16::from(ascii(&u8::try_from(*unit).unwrap_or(b'?')))).collect::<Vec<u16>>());
        }
        let mut out = Vec::with_capacity(self.len());
        let mut run = String::new();
        for decoded in self.chars_lossless() {
            match decoded {
                Ok(ch) => run.push(ch),
                Err(lone) => {
                    out.extend(full(&run).encode_utf16());
                    run.clear();
                    out.push(lone);
                }
            }
        }
        out.extend(full(&run).encode_utf16());
        JsString::from(out)
    }

    /// Whether the string holds exactly the text `other`.
    fn eq_str(&self, other: &str) -> bool {
        //~ Compare without encoding `other` into a buffer: equal strings
        //~ have equal unit counts, and the encoder yields units in order.
        let mut units = other.encode_utf16();
        self.0.iter().all(|unit| units.next() == Some(*unit)) && units.next().is_none()
    }
}

/// Whether `unit` is JavaScript white space or a line terminator, the set
/// `trim` removes: TAB, VT, FF, SP, NBSP, ZWNBSP, the `Zs` category, LF, CR,
/// LS and PS.
const fn js_is_white_space(unit: u16) -> bool {
    matches!(unit, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}

impl JsString {
    /// The empty string.
    fn new() -> Self {
        Self::default()
    }

    /// A string holding a copy of `units`.
    fn from_units(units: &[u16]) -> Self {
        Self(::std::rc::Rc::new(units.to_vec()))
    }

    /// The borrowed view.
    fn as_js_str(&self) -> &JsStr {
        JsStr::from_units(&self.0)
    }

    /// Appends `other`, in place when this handle is the buffer's only owner.
    fn push_str(&mut self, other: &JsStr) {
        if other.is_empty() { return; }
        ::std::rc::Rc::make_mut(&mut self.0).extend_from_slice(&other.0);
    }

    /// Appends one code unit, in place when this handle is the only owner.
    fn push_unit(&mut self, unit: u16) {
        ::std::rc::Rc::make_mut(&mut self.0).push(unit);
    }

    /// The pieces joined with `separator` between them (`Array.join`).
    fn join<I>(pieces: I, separator: &JsStr) -> Self
    where
        I: IntoIterator,
        I::Item: ::std::borrow::Borrow<JsStr>,
    {
        let mut out = Vec::new();
        for (index, piece) in pieces.into_iter().enumerate() {
            if index > 0 { out.extend_from_slice(&separator.0); }
            out.extend_from_slice(&::std::borrow::Borrow::<JsStr>::borrow(&piece).0);
        }
        Self::from(out)
    }
}

impl ::std::ops::Deref for JsString {
    type Target = JsStr;

    fn deref(&self) -> &JsStr {
        JsStr::from_units(&self.0)
    }
}

impl ::std::borrow::Borrow<JsStr> for JsString {
    fn borrow(&self) -> &JsStr {
        self
    }
}

impl AsRef<JsStr> for JsString {
    fn as_ref(&self) -> &JsStr {
        self
    }
}

impl AsRef<Self> for JsStr {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl ToOwned for JsStr {
    type Owned = JsString;

    fn to_owned(&self) -> JsString {
        JsString::from_units(&self.0)
    }
}

impl From<Vec<u16>> for JsString {
    fn from(units: Vec<u16>) -> Self {
        Self(::std::rc::Rc::new(units))
    }
}

impl From<&str> for JsString {
    fn from(text: &str) -> Self {
        Self::from(text.encode_utf16().collect::<Vec<u16>>())
    }
}

impl From<String> for JsString {
    fn from(text: String) -> Self {
        Self::from(text.as_str())
    }
}

impl From<&String> for JsString {
    fn from(text: &String) -> Self {
        Self::from(text.as_str())
    }
}

impl From<char> for JsString {
    fn from(ch: char) -> Self {
        let mut buffer = [0; 2];
        Self::from_units(ch.encode_utf16(&mut buffer))
    }
}

impl From<&JsStr> for JsString {
    fn from(text: &JsStr) -> Self {
        text.to_owned()
    }
}

impl From<&Self> for JsString {
    fn from(text: &Self) -> Self {
        text.clone()
    }
}

impl ::std::ops::Add<&JsStr> for JsString {
    type Output = Self;

    //~ `text + &piece` moves `text` in, so a builder's handle is the only one
    //~ and the append is in place.
    fn add(mut self, other: &JsStr) -> Self {
        self.push_str(other);
        self
    }
}

impl ::std::ops::AddAssign<&JsStr> for JsString {
    fn add_assign(&mut self, other: &JsStr) {
        self.push_str(other);
    }
}

impl<P: ::std::borrow::Borrow<JsStr>> FromIterator<P> for JsString {
    fn from_iter<I: IntoIterator<Item = P>>(pieces: I) -> Self {
        let mut out = Vec::new();
        for piece in pieces { out.extend_from_slice(&piece.borrow().0); }
        Self::from(out)
    }
}

impl PartialEq<str> for JsStr {
    fn eq(&self, other: &str) -> bool {
        self.eq_str(other)
    }
}

impl PartialEq<&str> for JsStr {
    fn eq(&self, other: &&str) -> bool {
        self.eq_str(other)
    }
}

impl PartialEq<str> for JsString {
    fn eq(&self, other: &str) -> bool {
        self.eq_str(other)
    }
}

impl PartialEq<&str> for JsString {
    fn eq(&self, other: &&str) -> bool {
        self.eq_str(other)
    }
}

impl PartialEq<JsStr> for JsString {
    fn eq(&self, other: &JsStr) -> bool {
        **self == *other
    }
}

impl PartialEq<JsString> for JsStr {
    fn eq(&self, other: &JsString) -> bool {
        *self == **other
    }
}

impl ::std::fmt::Display for JsStr {
    //~ Writes UTF-8 in stack-sized chunks, a lone surrogate as U+FFFD, so
    //~ printing builds no `String`.
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        let mut buffer = [0u8; 256];
        let mut filled = 0;
        for decoded in self.chars_lossless() {
            if filled + 4 > buffer.len() {
                formatter.write_str(::std::str::from_utf8(&buffer[..filled]).map_err(|_utf8| ::std::fmt::Error)?)?;
                filled = 0;
            }
            filled += decoded.unwrap_or('\u{FFFD}').encode_utf8(&mut buffer[filled..]).len();
        }
        formatter.write_str(::std::str::from_utf8(&buffer[..filled]).map_err(|_utf8| ::std::fmt::Error)?)
    }
}

impl ::std::fmt::Display for JsString {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        ::std::fmt::Display::fmt(&**self, formatter)
    }
}

impl ::std::fmt::Debug for JsStr {
    //~ Quoted like `str`'s `Debug`, with a lone surrogate as `\u{d83d}`.
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        use ::std::fmt::Write as _;
        formatter.write_char('"')?;
        for decoded in self.chars_lossless() {
            match decoded {
                Ok(ch) => for escaped in ch.escape_debug() { formatter.write_char(escaped)?; },
                Err(lone) => write!(formatter, "\\u{{{lone:x}}}")?,
            }
        }
        formatter.write_char('"')
    }
}

impl ::std::fmt::Debug for JsString {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        ::std::fmt::Debug::fmt(&**self, formatter)
    }
}

// @smelt:item-end

