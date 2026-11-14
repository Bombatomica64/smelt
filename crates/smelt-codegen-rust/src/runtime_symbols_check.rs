//! Check that every runtime symbol a generated crate names has a definition.
//!
//! The runtime is selected by [`crate::runtime_demand`], so a missing edge in its
//! `requires` graph shows up as a call to a helper the crate never declares. That
//! is an E0425 in the user's build, far from the cause. This check reads the
//! finished text instead, and reports the missing names while the emitter still
//! knows it was the one that named them.
//!
//! It is a lexer, not a parser: it skips comments and string literals, and treats
//! anything that could bind a name (an item keyword, a `let`, a pattern, a
//! parameter) as a definition. It can therefore miss a defect, but it cannot
//! report a name that is bound somewhere in the crate.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible like every other emitter helper"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "every offset is a cursor bounded by the text it walks"
)]

use std::collections::BTreeSet;

/// One token of the generated text the check cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token<'a> {
    /// An identifier or keyword.
    Ident(&'a str),
    /// A single punctuation character.
    Punct(char),
}

/// Returns the emitter-prefixed names (`smelt_*`, `__smelt_*`, `Smelt*`;
/// see [`crate::names::reserved::has_emitter_prefix`]) `sources` use without
/// defining, sorted.
///
/// All of a crate's sources are read together: a module may call a helper the
/// crate root defines.
#[must_use]
pub(crate) fn undefined_runtime_symbols<'a>(
    sources: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut defined = BTreeSet::new();
    let mut used = BTreeSet::new();
    for source in sources {
        let tokens = tokenize(source);
        for (index, token) in tokens.iter().enumerate() {
            let Token::Ident(name) = *token else {
                continue;
            };
            if !is_runtime_symbol(name) {
                continue;
            }
            let previous = index.checked_sub(1).and_then(|at| tokens.get(at)).copied();
            let next = tokens.get(index.saturating_add(1)).copied();
            let after_next = tokens.get(index.saturating_add(2)).copied();
            if binds_a_name(previous, next, after_next) {
                defined.insert(name);
            } else if is_a_use(name, previous, next) {
                used.insert(name);
            }
        }
    }
    used.difference(&defined)
        .map(|name| (*name).to_owned())
        .collect()
}

/// Whether `name` is one the emitter or runtime mints: the reserved prefixes,
/// which no source name renders to. The helpers a reservation declines are
/// `__smelt_*` items, so a check of `smelt_`/`Smelt` alone missed exactly the
/// reference a released ordinal leaves behind.
fn is_runtime_symbol(name: &str) -> bool {
    crate::names::reserved::has_emitter_prefix(name)
}

/// Whether an identifier in this position introduces a name.
fn binds_a_name(
    previous: Option<Token<'_>>,
    next: Option<Token<'_>>,
    after_next: Option<Token<'_>>,
) -> bool {
    if let Some(Token::Ident(
        "fn" | "struct" | "enum" | "union" | "trait" | "type" | "const" | "static" | "mod" | "let"
        | "mut" | "ref" | "for" | "macro_rules",
    )) = previous
    {
        return true;
    }
    // A parameter, field or typed binding, but not a path.
    if next == Some(Token::Punct(':')) && after_next != Some(Token::Punct(':')) {
        return true;
    }
    // A closure parameter or a name bound inside a pattern.
    matches!(previous, Some(Token::Punct('|' | '(' | ',' | '<')))
        && matches!(next, Some(Token::Punct('|' | ')' | ',' | '>')))
}

/// Whether an identifier in this position needs a definition in the crate.
///
/// A `smelt_*` name is checked where it is called: bare, it can be a local a
/// lowered body bound in a shape this lexer does not model. A `Smelt*` name is a
/// type, trait or constructor wherever it stands. A name after `.` or `::` is a
/// member of something else.
fn is_a_use(name: &str, previous: Option<Token<'_>>, next: Option<Token<'_>>) -> bool {
    if matches!(previous, Some(Token::Punct('.' | ':'))) {
        return false;
    }
    name.starts_with("Smelt") || next == Some(Token::Punct('('))
}

/// Splits Rust text into identifiers and punctuation, dropping comments and
/// string, raw string, byte string and character literals.
fn tokenize(source: &str) -> Vec<Token<'_>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0_usize;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'/' if bytes.get(at + 1) == Some(&b'/') => at = skip_line(bytes, at),
            b'/' if bytes.get(at + 1) == Some(&b'*') => at = skip_block_comment(bytes, at),
            b'"' => at = skip_string(bytes, at),
            b'\'' => at = skip_char_or_lifetime(bytes, at),
            b'r' | b'b' if raw_string_hashes(bytes, at).is_some() => {
                at = skip_raw_string(bytes, at);
            }
            _ if byte == b'_' || byte.is_ascii_alphabetic() => {
                let end = bytes
                    .iter()
                    .skip(at)
                    .position(|next| *next != b'_' && !next.is_ascii_alphanumeric())
                    .map_or(bytes.len(), |length| at + length);
                if let Some(ident) = source.get(at..end) {
                    tokens.push(Token::Ident(ident));
                }
                at = end;
            }
            _ if byte.is_ascii_digit() => {
                at = bytes
                    .iter()
                    .skip(at)
                    .position(|next| *next != b'_' && !next.is_ascii_alphanumeric())
                    .map_or(bytes.len(), |length| at + length);
            }
            _ if byte.is_ascii_whitespace() => at += 1,
            _ => {
                tokens.push(Token::Punct(char::from(byte)));
                at += 1;
            }
        }
    }
    tokens
}

/// Returns the offset just past the line that starts at `at`.
fn skip_line(bytes: &[u8], at: usize) -> usize {
    bytes
        .iter()
        .skip(at)
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |length| at + length + 1)
}

/// Returns the offset just past the (possibly nested) block comment at `at`.
fn skip_block_comment(bytes: &[u8], mut at: usize) -> usize {
    let mut depth = 0_usize;
    while at < bytes.len() {
        match (bytes.get(at), bytes.get(at + 1)) {
            (Some(b'/'), Some(b'*')) => {
                depth += 1;
                at += 2;
            }
            (Some(b'*'), Some(b'/')) => {
                depth -= 1;
                at += 2;
                if depth == 0 {
                    return at;
                }
            }
            _ => at += 1,
        }
    }
    at
}

/// Returns the offset just past the string literal whose quote is at `at`.
fn skip_string(bytes: &[u8], mut at: usize) -> usize {
    at += 1;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }
    at
}

/// Returns the offset past a character literal, or just past the `'` of a lifetime.
fn skip_char_or_lifetime(bytes: &[u8], at: usize) -> usize {
    if bytes.get(at + 1) == Some(&b'\\') {
        let mut end = at + 3;
        while bytes.get(end).is_some_and(|byte| *byte != b'\'') {
            end += 1;
        }
        return end + 1;
    }
    // A literal closes after one character, which may be multi-byte.
    let mut end = at + 2;
    while bytes.get(end).is_some_and(|byte| byte & 0xC0 == 0x80) {
        end += 1;
    }
    if bytes.get(end) == Some(&b'\'') {
        end + 1
    } else {
        at + 1
    }
}

/// Returns the `#` count when a raw string literal starts at `at`.
fn raw_string_hashes(bytes: &[u8], at: usize) -> Option<usize> {
    let mut cursor = at;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let hashes = bytes
        .iter()
        .skip(cursor)
        .take_while(|byte| **byte == b'#')
        .count();
    (bytes.get(cursor + hashes) == Some(&b'"')).then_some(hashes)
}

/// Returns the offset just past the raw string literal at `at`.
fn skip_raw_string(bytes: &[u8], at: usize) -> usize {
    let hashes = raw_string_hashes(bytes, at).unwrap_or(0);
    let mut cursor = at;
    while bytes.get(cursor).is_some_and(|byte| *byte != b'"') {
        cursor += 1;
    }
    cursor += 1;
    while cursor < bytes.len() {
        if bytes.get(cursor) == Some(&b'"')
            && bytes
                .iter()
                .skip(cursor + 1)
                .take(hashes)
                .filter(|byte| **byte == b'#')
                .count()
                == hashes
        {
            return cursor + 1 + hashes;
        }
        cursor += 1;
    }
    cursor
}

#[cfg(test)]
mod tests {
    use super::undefined_runtime_symbols;

    /// A helper called but never declared is the E0425 the check exists for.
    #[test]
    fn a_called_helper_without_a_definition_is_reported() {
        let missing = undefined_runtime_symbols(["fn main() { smelt_missing(1); }"]);
        assert_eq!(
            missing,
            ["smelt_missing"],
            "the undefined call must be named"
        );
    }

    /// A declined helper reservation leaves a `__smelt_*` call behind: the
    /// emitter's per-program helpers carry that prefix, and a check of
    /// `smelt_`/`Smelt` alone let the E0425 through.
    #[test]
    fn a_called_double_underscore_helper_without_a_definition_is_reported() {
        let missing = undefined_runtime_symbols([
            "fn __smelt_fn_value_1() {} fn main() { __smelt_fn_value_1(); __smelt_record_adapter_7(x); }",
        ]);
        assert_eq!(
            missing,
            ["__smelt_record_adapter_7"],
            "the released helper must be named"
        );
    }

    /// A type named but never declared is the E0412/E0433 sibling.
    #[test]
    fn a_named_type_without_a_definition_is_reported() {
        let missing =
            undefined_runtime_symbols(["fn f() -> SmeltGenerator<f64> { SmeltGenerator::new() }"]);
        assert_eq!(
            missing,
            ["SmeltGenerator"],
            "the undefined type must be named"
        );
    }

    /// A definition in another source of the crate satisfies the use.
    #[test]
    fn a_definition_in_another_module_counts() {
        let missing =
            undefined_runtime_symbols(["fn smelt_helper() {}", "fn main() { smelt_helper(); }"]);
        assert!(
            missing.is_empty(),
            "a crate-level definition must resolve: {missing:?}"
        );
    }

    /// Locals, parameters and closure parameters bind names too.
    #[test]
    fn local_bindings_are_not_reported() {
        let source = "fn f(smelt_a: fn()) { let smelt_b = smelt_a; smelt_b(); let g = |smelt_c: fn()| smelt_c(); }";
        let missing = undefined_runtime_symbols([source]);
        assert!(
            missing.is_empty(),
            "local callables must not be reported: {missing:?}"
        );
    }

    /// Text in strings and comments is not code.
    #[test]
    fn strings_and_comments_are_skipped() {
        let source = r##"// smelt_comment()
            fn f() -> &'static str { let c = '"'; let r = r#"smelt_raw()"#; "smelt_string()" }"##;
        let missing = undefined_runtime_symbols([source]);
        assert!(
            missing.is_empty(),
            "literals and comments must be skipped: {missing:?}"
        );
    }

    /// A method or a path segment belongs to its receiver, not the crate root.
    #[test]
    fn members_are_not_uses() {
        let missing =
            undefined_runtime_symbols(["fn f(x: T) { x.smelt_method(); T::smelt_assoc(); }"]);
        assert!(
            missing.is_empty(),
            "members must not be reported: {missing:?}"
        );
    }
}
