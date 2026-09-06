//! Translation of a JavaScript `RegExp` *pattern* into Rust `regex` syntax.
//!
//! The two syntaxes agree on almost everything — the same metacharacters, the
//! same quantifiers, the same escapes — but they disagree on a small, closed set
//! of constructs, and each disagreement is a fact about the two grammars rather
//! than about any particular pattern:
//!
//! | JavaScript | Rust `regex` / `regex-syntax` |
//! | --- | --- |
//! | `[` inside a character class is a literal `[` | opens a *nested* class, so the outer class is left unterminated |
//! | `&&` inside a class is two literal `&` | class *intersection* |
//! | `~~` inside a class is two literal `~` | reserved class operator |
//! | a non-leading `^` inside a class is a literal `^` | also literal, but escaping it is always safe |
//! | `[]` is the empty class (matches nothing) | an unterminated class |
//! | `[^]` is the negated empty class (matches anything) | an unterminated class |
//! | `(?<name>…)` names a group | spelled `(?P<name>…)` |
//! | `(?<=…)` / `(?<!…)` are lookbehind | same, and must NOT be renamed |
//!
//! Everything else is copied through verbatim. This is deliberately a *grammar*
//! translation and not a table of pattern texts: the code it replaces did four
//! literal `str::replace` calls, one of which (`"[^.[\\]]"`) matched a single
//! library's exact spelling, so a differently spelled pattern with the same
//! JavaScript meaning silently failed to compile — and, because the runtime
//! treated an uncompilable pattern as a no-op, silently did nothing at all.
//!
//! A pattern this module cannot translate is reported as an error, never
//! returned unchanged: a `replace` that quietly returns its input is worse than
//! a loud failure.

/// The `[^]` (negated empty class) equivalent: any character, newline included.
const ANY_CHARACTER: &str = "(?s:.)";

/// The `[]` (empty class) equivalent: a class no character is a member of.
const NO_CHARACTER: &str = "[^\\s\\S]";

/// Translate a JavaScript `RegExp` pattern into Rust `regex` syntax.
///
/// # Errors
///
/// Returns a message describing the untranslatable construct when the pattern
/// is not a well-formed JavaScript pattern this translation understands: a
/// character class that is never closed, or a trailing backslash.
pub fn to_rust_pattern(pattern: &str) -> Result<String, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len());
    let mut index = 0usize;
    // Set while the scanner is between `[` and its closing `]`; the character
    // grammar inside a class is a different grammar, which is the whole reason
    // a `str::replace` cannot do this job.
    let mut in_class = false;
    while let Some(&ch) = chars.get(index) {
        match ch {
            // An escape is two characters in BOTH grammars and means the same
            // thing in both for every escape JavaScript can write, so it is
            // copied whole -- and copying it whole is also what keeps an escaped
            // `\[` or `\]` from being mistaken for class punctuation below.
            '\\' => {
                let Some(&escaped) = chars.get(index.saturating_add(1)) else {
                    return Err("pattern ends with a trailing backslash".to_owned());
                };
                out.push('\\');
                out.push(escaped);
                index = index.saturating_add(2);
            }
            '[' if !in_class => {
                in_class = open_character_class(&chars, &mut index, &mut out);
            }
            // A bare `[` inside a class is a literal in JavaScript and a nested
            // class in Rust.
            '[' => {
                out.push_str("\\[");
                index = index.saturating_add(1);
            }
            ']' if in_class => {
                out.push(']');
                in_class = false;
                index = index.saturating_add(1);
            }
            // Reserved class-set operators in Rust, ordinary literals in
            // JavaScript. Escaping the character is valid Rust wherever it
            // appears, so the rule needs no lookahead for the doubled form.
            '&' | '~' | '^' if in_class => {
                out.push('\\');
                out.push(ch);
                index = index.saturating_add(1);
            }
            '(' if !in_class => open_group(&chars, &mut index, &mut out),
            _ => {
                out.push(ch);
                index = index.saturating_add(1);
            }
        }
    }
    if in_class {
        return Err("character class is never closed".to_owned());
    }
    Ok(out)
}

/// How many CAPTURE groups a JavaScript `RegExp` pattern declares.
///
/// This is the `N` in ECMA-262's replacer argument list
/// `(matched, p1, …, pN, position, string)`, so it is what decides whether the
/// second parameter of a `.replace(re, (a, b) => …)` callback is capture group
/// 1 or the match position. Counting requires the same grammar knowledge as
/// [`to_rust_pattern`] and therefore lives beside it: a `(` only opens a
/// capture group when it is outside a character class, not escaped, and not
/// followed by one of the non-capturing prefixes.
///
/// Capturing: `(…)` and the named form `(?<name>…)`.
/// Non-capturing: `(?:…)`, lookahead `(?=…)` / `(?!…)`, lookbehind
/// `(?<=…)` / `(?<!…)`, and inline flags `(?i)`.
///
/// A malformed pattern is not rejected here — an unterminated class simply
/// stops counting groups, because a caller that needs validity calls
/// [`to_rust_pattern`], which reports it.
#[must_use]
pub fn capture_group_count(pattern: &str) -> u32 {
    let chars: Vec<char> = pattern.chars().collect();
    let mut count = 0u32;
    let mut index = 0usize;
    let mut in_class = false;
    while let Some(&ch) = chars.get(index) {
        match ch {
            // An escape spans two characters in both grammars, so `\(` is a
            // literal parenthesis and `\[` is not class punctuation.
            '\\' => index = index.saturating_add(2),
            '[' if !in_class => {
                // `[]` and `[^]` are the member-less classes; neither opens a
                // class that a later `]` would close.
                if chars.get(index.saturating_add(1)) == Some(&']') {
                    index = index.saturating_add(2);
                } else if chars.get(index.saturating_add(1)) == Some(&'^')
                    && chars.get(index.saturating_add(2)) == Some(&']')
                {
                    index = index.saturating_add(3);
                } else {
                    in_class = true;
                    index = index.saturating_add(1);
                }
            }
            ']' if in_class => {
                in_class = false;
                index = index.saturating_add(1);
            }
            '(' if !in_class => {
                if is_capturing_group_prefix(&chars, index) {
                    count = count.saturating_add(1);
                }
                index = index.saturating_add(1);
            }
            _ => index = index.saturating_add(1),
        }
    }
    count
}

/// Whether the `(` at `index` opens a capture group rather than a group
/// modifier.
///
/// Every non-capturing form starts `(?`; the one exception is `(?<name>`, which
/// captures. `(?<=` and `(?<!` are lookbehind and do not.
fn is_capturing_group_prefix(chars: &[char], index: usize) -> bool {
    if chars.get(index.saturating_add(1)) != Some(&'?') {
        return true;
    }
    // `(?<…` is either a named capture group or a lookbehind assertion.
    chars.get(index.saturating_add(2)) == Some(&'<')
        && !matches!(chars.get(index.saturating_add(3)), Some(&'=' | &'!'))
}

/// Translate a `[` that opens a character class, returning whether one is open.
///
/// `[]` and `[^]` are the two JavaScript classes with no members listed: the
/// first matches nothing, the second matches anything. Rust spells neither as a
/// class, and reading either as the start of one leaves it unterminated -- so
/// both are rewritten whole and no class is opened. Otherwise the `[` and an
/// immediately following negating `^` are copied, since both grammars agree on
/// those.
fn open_character_class(chars: &[char], index: &mut usize, out: &mut String) -> bool {
    if chars.get(index.saturating_add(1)) == Some(&']') {
        out.push_str(NO_CHARACTER);
        *index = index.saturating_add(2);
        return false;
    }
    if chars.get(index.saturating_add(1)) == Some(&'^')
        && chars.get(index.saturating_add(2)) == Some(&']')
    {
        out.push_str(ANY_CHARACTER);
        *index = index.saturating_add(3);
        return false;
    }
    out.push('[');
    *index = index.saturating_add(1);
    if chars.get(*index) == Some(&'^') {
        out.push('^');
        *index = index.saturating_add(1);
    }
    true
}

/// Translate a `(` and the group prefix that may follow it.
///
/// `(?<name>` names a group and is spelled `(?P<name>` in Rust, but `(?<=` and
/// `(?<!` are lookbehind in both grammars and must survive intact -- which is
/// exactly what a blind `"(?<" -> "(?P<"` text replacement got wrong.
fn open_group(chars: &[char], index: &mut usize, out: &mut String) {
    *index = index.saturating_add(1);
    out.push('(');
    if chars.get(*index) == Some(&'?')
        && chars.get(index.saturating_add(1)) == Some(&'<')
        && !matches!(chars.get(index.saturating_add(2)), Some(&'=' | &'!'))
    {
        out.push_str("?P<");
        *index = index.saturating_add(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bare `[` inside a class is a literal in JavaScript. This is the
    /// construct `escapeRegExp`'s own pattern is built from, and the one the
    /// replaced `str::replace` hack only handled for a single spelling.
    #[test]
    fn a_bare_bracket_inside_a_class_becomes_a_literal() {
        assert_eq!(
            to_rust_pattern(r"[\\^$.*+?()[\]{}|]").as_deref(),
            Ok(r"[\\\^$.*+?()\[\]{}|]")
        );
        assert_eq!(to_rust_pattern(r"[a[b]").as_deref(), Ok(r"[a\[b]"));
        // The special-cased spelling the four hacks carried, now translated by
        // the general rule.
        assert_eq!(to_rust_pattern(r"[^.[\]]").as_deref(), Ok(r"[^.\[\]]"));
    }

    /// An escaped bracket is copied whole and is not class punctuation.
    #[test]
    fn an_escaped_bracket_is_not_class_punctuation() {
        assert_eq!(to_rust_pattern(r"\[a\]").as_deref(), Ok(r"\[a\]"));
        assert_eq!(to_rust_pattern(r"[\]\[]").as_deref(), Ok(r"[\]\[]"));
    }

    /// The two member-less JavaScript classes have their own Rust spellings.
    #[test]
    fn the_memberless_classes_get_their_rust_equivalents() {
        assert_eq!(to_rust_pattern("[^]").as_deref(), Ok(ANY_CHARACTER));
        assert_eq!(to_rust_pattern("a[]b").as_deref(), Ok("a[^\\s\\S]b"));
    }

    /// Rust's class-set operators are literals in JavaScript.
    #[test]
    fn reserved_class_operators_are_escaped() {
        assert_eq!(to_rust_pattern("[a&&b]").as_deref(), Ok("[a\\&\\&b]"));
        assert_eq!(to_rust_pattern("[a~~b]").as_deref(), Ok("[a\\~\\~b]"));
        assert_eq!(to_rust_pattern("[a^b]").as_deref(), Ok("[a\\^b]"));
        assert_eq!(to_rust_pattern("[^ab]").as_deref(), Ok("[^ab]"));
    }

    /// A named group is renamed; a lookbehind is not.
    #[test]
    fn named_groups_are_renamed_and_lookbehind_is_not() {
        assert_eq!(
            to_rust_pattern("(?<year>\\d{4})").as_deref(),
            Ok("(?P<year>\\d{4})")
        );
        assert_eq!(to_rust_pattern("(?<=a)b").as_deref(), Ok("(?<=a)b"));
        assert_eq!(to_rust_pattern("(?<!a)b").as_deref(), Ok("(?<!a)b"));
    }

    /// An untranslatable pattern is an error, never a silent pass-through.
    #[test]
    fn an_untranslatable_pattern_is_an_error() {
        assert!(to_rust_pattern("[abc").is_err());
        assert!(to_rust_pattern("abc\\").is_err());
    }

    /// Everything the two grammars share is copied unchanged.
    #[test]
    fn shared_syntax_is_copied_unchanged() {
        for pattern in [
            r"^\d+$",
            r"(a|b)*c",
            r"\s{2,4}?",
            r"[a-zA-Z0-9_-]+",
            r"(?:ab)+",
            r"\.",
            r"[\w.]+@[\w.]+",
        ] {
            assert_eq!(to_rust_pattern(pattern).as_deref(), Ok(pattern));
        }
    }

    /// Only real capture groups count: the number decides whether a replacer
    /// callback's second parameter is `p1` or `position`.
    #[test]
    fn capture_groups_are_counted_and_group_modifiers_are_not() {
        // No groups at all -- the shape whose `(a, b) => …` callback receives
        // the match POSITION as `b`.
        assert_eq!(capture_group_count(r"\{[^}]+\}"), 0);
        assert_eq!(capture_group_count(r"(?:%[0-9A-Fa-f]{2})+"), 0);
        assert_eq!(capture_group_count(r"(?=ab)(?!cd)"), 0);
        assert_eq!(capture_group_count(r"(?<=ab)(?<!cd)"), 0);
        // One group -- the shape whose `(a, b) => …` callback receives capture
        // group 1 as `b`.
        assert_eq!(capture_group_count(r###""##(.+?)##""###), 1);
        assert_eq!(capture_group_count(r"(?<name>ab)"), 1);
        // Several, nested and alternated.
        assert_eq!(capture_group_count(r"((a)|(b))"), 3);
        assert_eq!(capture_group_count(r"(a)(?:b)(c)"), 2);
    }

    /// A parenthesis that is not a group opener is not counted: escaped, or
    /// inside a character class where it is an ordinary member.
    #[test]
    fn literal_parentheses_are_not_capture_groups() {
        assert_eq!(capture_group_count(r"\(a\)"), 0);
        assert_eq!(capture_group_count(r"[()]"), 0);
        assert_eq!(capture_group_count(r"[\\^$.*+?()[\]{}|]"), 0);
        // The member-less classes do not open a class, so a following `(` is
        // still a group opener.
        assert_eq!(capture_group_count(r"[](a)"), 1);
        assert_eq!(capture_group_count(r"[^](a)"), 1);
    }
}
