// JavaScript string indices, over Rust's UTF-8 `String`.
//
// A JavaScript string is a sequence of UTF-16 code units: `length`, every
// index an operation answers (`indexOf`, `search`, a match's `index`) and
// every index it takes (`slice`, `charAt`, `s[i]`, `padEnd`'s length) count
// code units. Generated crates keep strings as UTF-8 `String`, so each such
// operation converts at its boundary through these helpers, and every helper
// reads ASCII text byte for byte, since there a byte is a code unit.
//
// A Rust string cannot hold a lone surrogate. A cut that falls inside a
// surrogate pair gives the whole character to the piece on its LEFT:
// `"😀".slice(0, 1)` and `"😀"[0]` are `"😀"`, `"😀".slice(1)` and `"😀"[1]`
// are `""`. Every index and length still counts code units, and joining the
// pieces of a cut string gives back the string (`s[0] + s.slice(1) === s`),
// which is what code that cuts at an index relies on; what differs from
// JavaScript is a lone half looked at on its own.

// @smelt:item smelt_utf16_len when string_units
/// The JavaScript `length` of `text`: its UTF-16 code units.
#[allow(dead_code)]
fn smelt_utf16_len(text: &str) -> usize {
    //~ Every byte that starts a character is one unit and a four-byte
    //~ character is two: a byte count with no decoding, which vectorizes.
    if text.is_ascii() { text.len() } else { smelt_utf16_units_in(text.as_bytes()) }
}

/// A count or index of code units as a JavaScript number: exact, since a
/// string's length is far below 2^53.
#[allow(dead_code)]
const fn smelt_units_to_number(units: usize) -> f64 {
    units as f64
}

/// A JavaScript number as a count or index of code units: truncated toward
/// zero, NaN and negatives `0`, anything past `usize::MAX` saturating.
#[allow(dead_code)]
const fn smelt_units_from_number(number: f64) -> usize {
    number as usize
}

/// The UTF-16 index of the byte offset `byte` of `text`, which is a char boundary.
#[allow(dead_code)]
fn smelt_utf16_index_of_byte(text: &str, byte: usize) -> usize {
    smelt_utf16_len(&text[..byte])
}

/// The UTF-16 code units of the characters whose first byte is in the word
/// `word` (eight bytes of a string, little-endian).
//~ Bit 7 of each byte of the masks: a continuation byte is `10xxxxxx` and
//~ counts nothing; any other byte starts a character, one unit, and a
//~ four-byte character's `11110xxx` lead counts a second unit.
#[allow(dead_code)]
const fn smelt_utf16_units_in_word(word: u64) -> usize {
    const HIGH: u64 = 0x8080_8080_8080_8080;
    let continuation = (word & !(word << 1) & HIGH).count_ones() as usize;
    let four_byte = (word & (word << 1) & (word << 2) & (word << 3) & HIGH).count_ones() as usize;
    8 - continuation + four_byte
}

/// The UTF-16 code units of the characters whose first byte is in `bytes`.
#[allow(dead_code)]
fn smelt_utf16_units_in(bytes: &[u8]) -> usize {
    let (words, rest) = bytes.as_chunks::<8>();
    let whole: usize = words.iter().map(|word| smelt_utf16_units_in_word(u64::from_le_bytes(*word))).sum();
    whole + rest.iter().map(|byte| usize::from(byte & 0xC0 != 0x80) + usize::from(*byte >= 0xF0)).sum::<usize>()
}

/// The byte offset where UTF-16 index `unit` of `text` starts, clamped to the
/// end; `Err` with the offset of the surrogate pair `unit` falls inside.
#[allow(dead_code)]
fn smelt_utf16_byte_of(text: &str, unit: usize) -> Result<usize, usize> {
    let bytes = text.as_bytes();
    if bytes.get(..unit).is_some_and(<[u8]>::is_ascii) { return Ok(unit); }
    //~ Skip whole eight-byte words that end before `unit`, then step.
    let (mut at, mut units) = (0, 0);
    while let Some(word) = bytes.get(at..at + 8) {
        let count = smelt_utf16_units_in_word(u64::from_le_bytes([word[0], word[1], word[2], word[3], word[4], word[5], word[6], word[7]]));
        if units + count >= unit { break; }
        units += count;
        at += 8;
    }
    for (offset, &byte) in bytes[at..].iter().enumerate() {
        if byte & 0xC0 == 0x80 { continue; }
        if units == unit { return Ok(at + offset); }
        units += 1 + usize::from(byte >= 0xF0);
        if units > unit { return Err(at + offset); }
    }
    Ok(bytes.len())
}

/// The byte offset of the cut at UTF-16 index `unit` of `text`: a cut inside
/// a surrogate pair falls after the pair.
#[allow(dead_code)]
fn smelt_utf16_cut(text: &str, unit: usize) -> usize {
    smelt_utf16_byte_of(text, unit).unwrap_or_else(|byte| byte + 4)
}

/// The code units `start..end` of `text` (clamped; empty when `end <= start`),
/// a pair cut at either end belonging to the piece on the cut's left.
#[allow(dead_code)]
fn smelt_utf16_slice(text: &str, start: usize, end: usize) -> String {
    if end <= start { return String::new(); }
    let from = smelt_utf16_cut(text, start);
    let to = smelt_utf16_cut(text, end);
    text.get(from..to.max(from)).unwrap_or("").to_owned()
}

/// The code unit at UTF-16 index `unit` of `text` (`charCodeAt`), if any.
#[allow(dead_code)]
fn smelt_utf16_unit_at(text: &str, unit: usize) -> Option<u16> {
    if let Some(prefix) = text.as_bytes().get(..=unit).filter(|prefix| prefix.is_ascii()) { return Some(u16::from(prefix[unit])); }
    let (byte, half) = match smelt_utf16_byte_of(text, unit) { Ok(byte) => (byte, 0), Err(byte) => (byte, 1) };
    let mut units = [0; 2];
    text[byte..].chars().next().map(|ch| ch.encode_utf16(&mut units)[half])
}

/// The string at UTF-16 index `unit` of `text` (`charAt`, `s[i]`), if any:
/// one unit, or a whole pair at its first unit and `""` at its second.
#[allow(dead_code)]
fn smelt_utf16_char_at(text: &str, unit: usize) -> Option<String> {
    if let Some(prefix) = text.as_bytes().get(..=unit).filter(|prefix| prefix.is_ascii()) { return Some(char::from(prefix[unit]).to_string()); }
    smelt_utf16_byte_of(text, unit).map_or_else(|_| Some(String::new()), |byte| text[byte..].chars().next().map(String::from))
}

/// The code point starting at UTF-16 index `unit` of `text` (`codePointAt`):
/// a whole surrogate pair, else the lone unit there.
#[allow(dead_code)]
fn smelt_utf16_code_point_at(text: &str, unit: usize) -> Option<u32> {
    smelt_utf16_byte_of(text, unit).map_or_else(|_| smelt_utf16_unit_at(text, unit).map(u32::from), |byte| text[byte..].chars().next().map(u32::from))
}

/// One string per code unit of `text` (`s.split("")`): a pair is its whole
/// character followed by `""`.
#[allow(dead_code)]
fn smelt_utf16_units(text: &str) -> Vec<String> {
    let mut units = Vec::with_capacity(text.len());
    for ch in text.chars() {
        units.push(ch.to_string());
        if ch.len_utf16() == 2 { units.push(String::new()); }
    }
    units
}

// @smelt:item-end
