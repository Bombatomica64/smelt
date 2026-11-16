// Python `str` methods that need CPython's own rules: title case, padding,
// counting and searching with bounds, whitespace and bounded splitting,
// counted replacement, and stripping a character set.
//
// A Python `str` is a `String` and every position here counts CODE POINTS,
// as CPython's do. Bounds are adjusted as CPython's `ADJUST_INDICES` does: a
// negative bound counts from the end, and both clamp to `[0, len]`. The
// frontend hands a bound past a machine word over already saturated, which
// clamps the same way.

// @smelt:item smelt_py_str_title when py_str_methods
/// The characters whose Unicode title case differs from their upper case, with
/// that title case (Unicode 16, `CPython` 3.14).
const SMELT_PY_TITLE_CASE: [(u32, &str); 89] = [(0xDF, "\u{53}\u{73}"), (0x1C4, "\u{1C5}"), (0x1C5, "\u{1C5}"), (0x1C6, "\u{1C5}"), (0x1C7, "\u{1C8}"), (0x1C8, "\u{1C8}"), (0x1C9, "\u{1C8}"), (0x1CA, "\u{1CB}"), (0x1CB, "\u{1CB}"), (0x1CC, "\u{1CB}"), (0x1F1, "\u{1F2}"), (0x1F2, "\u{1F2}"), (0x1F3, "\u{1F2}"), (0x587, "\u{535}\u{582}"), (0x1F80, "\u{1F88}"), (0x1F81, "\u{1F89}"), (0x1F82, "\u{1F8A}"), (0x1F83, "\u{1F8B}"), (0x1F84, "\u{1F8C}"), (0x1F85, "\u{1F8D}"), (0x1F86, "\u{1F8E}"), (0x1F87, "\u{1F8F}"), (0x1F88, "\u{1F88}"), (0x1F89, "\u{1F89}"), (0x1F8A, "\u{1F8A}"), (0x1F8B, "\u{1F8B}"), (0x1F8C, "\u{1F8C}"), (0x1F8D, "\u{1F8D}"), (0x1F8E, "\u{1F8E}"), (0x1F8F, "\u{1F8F}"), (0x1F90, "\u{1F98}"), (0x1F91, "\u{1F99}"), (0x1F92, "\u{1F9A}"), (0x1F93, "\u{1F9B}"), (0x1F94, "\u{1F9C}"), (0x1F95, "\u{1F9D}"), (0x1F96, "\u{1F9E}"), (0x1F97, "\u{1F9F}"), (0x1F98, "\u{1F98}"), (0x1F99, "\u{1F99}"), (0x1F9A, "\u{1F9A}"), (0x1F9B, "\u{1F9B}"), (0x1F9C, "\u{1F9C}"), (0x1F9D, "\u{1F9D}"), (0x1F9E, "\u{1F9E}"), (0x1F9F, "\u{1F9F}"), (0x1FA0, "\u{1FA8}"), (0x1FA1, "\u{1FA9}"), (0x1FA2, "\u{1FAA}"), (0x1FA3, "\u{1FAB}"), (0x1FA4, "\u{1FAC}"), (0x1FA5, "\u{1FAD}"), (0x1FA6, "\u{1FAE}"), (0x1FA7, "\u{1FAF}"), (0x1FA8, "\u{1FA8}"), (0x1FA9, "\u{1FA9}"), (0x1FAA, "\u{1FAA}"), (0x1FAB, "\u{1FAB}"), (0x1FAC, "\u{1FAC}"), (0x1FAD, "\u{1FAD}"), (0x1FAE, "\u{1FAE}"), (0x1FAF, "\u{1FAF}"), (0x1FB2, "\u{1FBA}\u{345}"), (0x1FB3, "\u{1FBC}"), (0x1FB4, "\u{386}\u{345}"), (0x1FB7, "\u{391}\u{342}\u{345}"), (0x1FBC, "\u{1FBC}"), (0x1FC2, "\u{1FCA}\u{345}"), (0x1FC3, "\u{1FCC}"), (0x1FC4, "\u{389}\u{345}"), (0x1FC7, "\u{397}\u{342}\u{345}"), (0x1FCC, "\u{1FCC}"), (0x1FF2, "\u{1FFA}\u{345}"), (0x1FF3, "\u{1FFC}"), (0x1FF4, "\u{38F}\u{345}"), (0x1FF7, "\u{3A9}\u{342}\u{345}"), (0x1FFC, "\u{1FFC}"), (0xFB00, "\u{46}\u{66}"), (0xFB01, "\u{46}\u{69}"), (0xFB02, "\u{46}\u{6C}"), (0xFB03, "\u{46}\u{66}\u{69}"), (0xFB04, "\u{46}\u{66}\u{6C}"), (0xFB05, "\u{53}\u{74}"), (0xFB06, "\u{53}\u{74}"), (0xFB13, "\u{544}\u{576}"), (0xFB14, "\u{544}\u{565}"), (0xFB15, "\u{544}\u{56B}"), (0xFB16, "\u{54E}\u{576}"), (0xFB17, "\u{544}\u{56D}")];

/// The Unicode `Lt` (titlecase letter) characters.
const SMELT_PY_TITLECASE_LETTERS: [u32; 31] = [0x1C5, 0x1C8, 0x1CB, 0x1F2, 0x1F88, 0x1F89, 0x1F8A, 0x1F8B, 0x1F8C, 0x1F8D, 0x1F8E, 0x1F8F, 0x1F98, 0x1F99, 0x1F9A, 0x1F9B, 0x1F9C, 0x1F9D, 0x1F9E, 0x1F9F, 0x1FA8, 0x1FA9, 0x1FAA, 0x1FAB, 0x1FAC, 0x1FAD, 0x1FAE, 0x1FAF, 0x1FBC, 0x1FCC, 0x1FFC];

/// Append the Unicode title case of `ch` to `out`.
#[allow(dead_code)]
fn smelt_py_push_title(ch: char, out: &mut String) {
    let code = u32::from(ch);
    if (0x10D0..=0x10FA).contains(&code) || (0x10FD..=0x10FF).contains(&code) {
        out.push(ch);
    } else if let Some((_, title)) = SMELT_PY_TITLE_CASE.iter().find(|(entry, _)| *entry == code) {
        out.push_str(title);
    } else {
        out.extend(ch.to_uppercase());
    }
}

/// Whether `ch` is cased (`Lu`, `Ll` or `Lt`), as `str.title()` asks.
#[allow(dead_code)]
fn smelt_py_is_cased(ch: char) -> bool {
    ch.is_uppercase() || ch.is_lowercase() || SMELT_PY_TITLECASE_LETTERS.contains(&u32::from(ch))
}

/// `str.title()`: each run of cased characters starts in title case and
/// continues in lower case (`"they're"` is `"They'Re"`).
#[allow(dead_code)]
fn smelt_py_str_title(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_cased = false;
    for ch in text.chars() {
        if previous_cased {
            out.extend(ch.to_lowercase());
        } else {
            smelt_py_push_title(ch, &mut out);
        }
        previous_cased = smelt_py_is_cased(ch);
    }
    out
}

/// `str.capitalize()`: the first character in title case, the rest lower.
#[allow(dead_code)]
fn smelt_py_str_capitalize(text: &str) -> String {
    let mut chars = text.chars();
    let mut out = String::with_capacity(text.len());
    if let Some(first) = chars.next() {
        smelt_py_push_title(first, &mut out);
        out.push_str(&chars.as_str().to_lowercase());
    }
    out
}

/// `str.swapcase()`: upper case to lower and lower to upper; a title case
/// or uncased character is unchanged.
#[allow(dead_code)]
fn smelt_py_str_swapcase(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_uppercase() {
            out.extend(ch.to_lowercase());
        } else if ch.is_lowercase() {
            out.extend(ch.to_uppercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// The code-point range `[start, end)` `CPython` reads for bounds `start` and
/// `end` over `len` code points (`ADJUST_INDICES`), or `None` when
/// `start` lies past the end.
#[allow(dead_code)]
fn smelt_py_str_bounds(len: usize, start: i64, end: i64) -> Option<(usize, usize)> {
    let len = i64::try_from(len).unwrap_or(i64::MAX);
    let adjust = |bound: i64| if bound < 0 { bound.saturating_add(len).max(0) } else { bound.min(len) };
    let (first, last) = (adjust(start), adjust(end));
    if start > len && start >= 0 {
        return None;
    }
    Some((usize::try_from(first).unwrap_or(0), usize::try_from(last).unwrap_or(0)))
}

/// `str.zfill(width)`: zeros after a leading sign, up to `width` code points.
#[allow(dead_code)]
fn smelt_py_str_zfill(text: &str, width: i64) -> String {
    let len = text.chars().count();
    let width = usize::try_from(width).unwrap_or(0);
    if width <= len {
        return text.to_owned();
    }
    let zeros = "0".repeat(width - len);
    text.strip_prefix(['+', '-']).map_or_else(|| format!("{zeros}{text}"), |rest| format!("{}{zeros}{rest}", &text[..1]))
}

/// `str.center`/`ljust`/`rjust(width, fill)` (`side` is `'c'`, `'l'` or
/// `'r'`): a `fill` that is not exactly one character is `CPython`'s
/// `TypeError`.
#[allow(dead_code)]
fn smelt_py_str_pad(text: &str, width: i64, fill: &str, side: char) -> Result<String, Box<dyn ::std::error::Error>> {
    let mut fill_chars = fill.chars();
    let (Some(fill_char), None) = (fill_chars.next(), fill_chars.next()) else {
        return Err(SmeltPyException::new(&["TypeError", "Exception", "BaseException"], vec![SmeltPyException::arg(&"The fill character must be exactly one character long".to_owned())]).into_error());
    };
    let len = text.chars().count();
    let width = usize::try_from(width).unwrap_or(0);
    if width <= len {
        return Ok(text.to_owned());
    }
    let margin = width - len;
    //~ CPython's `center` puts the odd pad on the left exactly when the
    //~ margin and the width are both odd.
    let left = match side {
        'l' => 0,
        'r' => margin,
        _ => margin / 2 + (margin & width & 1),
    };
    let pad = |count: usize| ::std::iter::repeat_n(fill_char, count).collect::<String>();
    Ok(format!("{}{text}{}", pad(left), pad(margin - left)))
}

/// `str.count(sub, start, end)`: non-overlapping occurrences in the bounds;
/// an empty `sub` counts the positions, one more than the code points.
#[allow(dead_code)]
fn smelt_py_str_count(text: &str, sub: &str, start: i64, end: i64) -> i64 {
    let chars: Vec<char> = text.chars().collect();
    let Some((first, last)) = smelt_py_str_bounds(chars.len(), start, end) else {
        return 0;
    };
    if first > last {
        return 0;
    }
    let needle: Vec<char> = sub.chars().collect();
    if needle.is_empty() {
        return i64::try_from(last - first + 1).unwrap_or(i64::MAX);
    }
    let (mut count, mut at) = (0_i64, first);
    while at + needle.len() <= last {
        if chars[at..at + needle.len()] == needle[..] {
            count += 1;
            at += needle.len();
        } else {
            at += 1;
        }
    }
    count
}

/// `str.find`/`rfind(sub, start, end)`: the code-point position of the first
/// (last, when `reverse`) occurrence in the bounds, or `-1`.
#[allow(dead_code)]
fn smelt_py_str_find(text: &str, sub: &str, start: i64, end: i64, reverse: bool) -> i64 {
    let chars: Vec<char> = text.chars().collect();
    let Some((first, last)) = smelt_py_str_bounds(chars.len(), start, end) else {
        return -1;
    };
    let needle: Vec<char> = sub.chars().collect();
    if first > last || last - first < needle.len() {
        return -1;
    }
    let candidates = first..=last - needle.len();
    let matches = |at: &usize| chars[*at..*at + needle.len()] == needle[..];
    let found = if reverse { candidates.rev().find(matches) } else { candidates.into_iter().find(matches) };
    found.map_or(-1, |at| i64::try_from(at).unwrap_or(i64::MAX))
}

/// `str.index`/`rindex`: `find` that raises `ValueError` when absent.
#[allow(dead_code)]
fn smelt_py_str_index(text: &str, sub: &str, start: i64, end: i64, reverse: bool) -> Result<i64, Box<dyn ::std::error::Error>> {
    match smelt_py_str_find(text, sub, start, end, reverse) {
        -1 => Err(smelt_py_value_error("substring not found")),
        at => Ok(at),
    }
}

/// Whether `ch` is Python whitespace (`str.isspace`), which also counts
/// the information separators U+001C..U+001F.
#[allow(dead_code)]
fn smelt_py_is_space(ch: char) -> bool {
    ch.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&ch)
}

/// `str.split()`/`rsplit()` with no separator: runs of whitespace separate,
/// leading and trailing whitespace yields nothing, at most `maxsplit` splits
/// (negative is unbounded) from the left (from the right when `reverse`).
#[allow(dead_code)]
fn smelt_py_str_split_whitespace(text: &str, maxsplit: i64, reverse: bool) -> Vec<String> {
    let chars: Vec<char> = if reverse { text.chars().rev().collect() } else { text.chars().collect() };
    let mut parts = Vec::new();
    let mut at = 0;
    let mut splits = 0_i64;
    loop {
        while at < chars.len() && smelt_py_is_space(chars[at]) {
            at += 1;
        }
        if at >= chars.len() {
            break;
        }
        //~ Past the last split the remainder is kept as it stands, its
        //~ trailing whitespace included (`" a b c ".split(None, 1)` is
        //~ `['a', 'b c ']`).
        if maxsplit >= 0 && splits >= maxsplit {
            parts.push(chars[at..].iter().collect::<String>());
            break;
        }
        let begin = at;
        while at < chars.len() && !smelt_py_is_space(chars[at]) {
            at += 1;
        }
        parts.push(chars[begin..at].iter().collect::<String>());
        splits += 1;
    }
    if reverse {
        for part in &mut parts {
            *part = part.chars().rev().collect();
        }
        parts.reverse();
    }
    parts
}

/// `str.split(sep, maxsplit)`/`rsplit`: an empty `sep` is `CPython`'s
/// `ValueError`.
#[allow(dead_code)]
fn smelt_py_str_split(text: &str, sep: &str, maxsplit: i64, reverse: bool) -> Result<Vec<String>, Box<dyn ::std::error::Error>> {
    if sep.is_empty() {
        return Err(smelt_py_value_error("empty separator"));
    }
    let limit = usize::try_from(maxsplit).ok();
    Ok(match (reverse, limit) {
        (_, None) => text.split(sep).map(str::to_owned).collect(),
        (false, Some(limit)) => text.splitn(limit.saturating_add(1), sep).map(str::to_owned).collect(),
        (true, Some(limit)) => {
            let mut parts: Vec<String> = text.rsplitn(limit.saturating_add(1), sep).map(str::to_owned).collect();
            parts.reverse();
            parts
        }
    })
}

/// `str.replace(old, new, count)`: at most `count` replacements (negative
/// is every one); an empty `old` inserts `new` before each code point and
/// at the end.
#[allow(dead_code)]
fn smelt_py_str_replace(text: &str, old: &str, new: &str, count: i64) -> String {
    let limit = usize::try_from(count).ok();
    if !old.is_empty() {
        return limit.map_or_else(|| text.replace(old, new), |limit| text.replacen(old, new, limit));
    }
    let mut out = String::new();
    let mut inserted = 0;
    let mut chars = text.chars();
    loop {
        if limit.is_none_or(|limit| inserted < limit) {
            out.push_str(new);
            inserted += 1;
        }
        match chars.next() {
            Some(ch) => out.push(ch),
            None => break,
        }
    }
    out
}

/// `str.strip`/`lstrip`/`rstrip(chars)` (`side` is `'b'`, `'l'` or
/// `'r'`): remove the characters of `chars` from the given ends.
#[allow(dead_code)]
fn smelt_py_str_strip_chars(text: &str, chars: &str, side: char) -> String {
    let strip = |ch: char| chars.contains(ch);
    match side {
        'l' => text.trim_start_matches(strip),
        'r' => text.trim_end_matches(strip),
        _ => text.trim_matches(strip),
    }
    .to_owned()
}

/// `chr(code)`: the character of a code point; outside `range(0x110000)` is
/// `CPython`'s `ValueError`. A lone surrogate has no Rust `char`, so it is the
/// same `ValueError` here (a documented divergence: `CPython` builds one).
#[allow(dead_code)]
fn smelt_py_chr(code: i64) -> Result<String, Box<dyn ::std::error::Error>> {
    u32::try_from(code)
        .ok()
        .and_then(char::from_u32)
        .map(String::from)
        .ok_or_else(|| smelt_py_value_error("chr() arg not in range(0x110000)"))
}

/// `ord(text)`: the code point of a one-character string; any other length is
/// `CPython`'s `TypeError`.
#[allow(dead_code)]
fn smelt_py_ord(text: &str) -> Result<i64, Box<dyn ::std::error::Error>> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(only), None) => Ok(i64::from(u32::from(only))),
        _ => Err(SmeltPyException::new(&["TypeError", "Exception", "BaseException"], vec![SmeltPyException::arg(&format!("ord() expected a character, but string of length {} found", text.chars().count()))]).into_error()),
    }
}

// @smelt:item-end
