//! Identifier-rewriting mini-lexer over already-rendered Rust closure text: shadow-aware substitution of shared-capture uses without touching struct fields, path segments, or shadowing inner closures.

/// Extracts the bare shared-capture cell identifier from an already-rendered
/// shared-capture access.
///
/// Shared captures render as `(*smelt_capture_x.borrow())` (read) or
/// `(*smelt_capture_x.borrow_mut())` (write); this returns the underlying cell
/// binding `smelt_capture_x` so a nested closure can clone the `Rc` cell
/// itself. Returns `None` when `text` is not a shared-capture access.
pub(super) fn shared_capture_cell_name(text: &str) -> Option<&str> {
    let inner = text.strip_prefix("(*")?;
    let inner = inner
        .strip_suffix(".borrow_mut())")
        .or_else(|| inner.strip_suffix(".borrow())"))?;
    // A self-recursive capture holds a `Weak` and upgrades before borrowing, so its
    // access form carries `SELF_RECURSIVE_UPGRADE` between the cell name and the
    // borrow. Strip it: callers use the result as an IDENTIFIER (they emit
    // `let <cell> = <cell>.clone();` for a nested closure), and returning the
    // expression instead produced `let smelt_capture_x.upgrade().expect(..) = ..`,
    // which is not a pattern and does not parse. radash's `async_test` was the case
    // that caught it -- a self-recursive closure that a nested closure also captures.
    Some(
        inner
            .strip_suffix(SELF_RECURSIVE_UPGRADE)
            .unwrap_or(inner),
    )
}

/// The `Weak` upgrade carried by a self-recursive closure's shared-capture access form.
///
/// Emission (`emitter::closures`) and recovery (`shared_capture_cell_name`) must agree
/// on this text exactly, so it lives here once rather than being spelled at each site.
pub(super) const SELF_RECURSIVE_UPGRADE: &str =
    ".upgrade().expect(\"self-recursive closure called after its defining scope returned\")";

/// Rewrites emitted closure text so shared captures use their `RefCell` storage.
///
/// Some closure emission paths wrap an already-rendered closure with a capture
/// prelude. The prelude creates `smelt_capture_<name>`, but the rendered body
/// can still mention the source name. This textual pass is intentionally
/// limited to identifier-boundary replacements for those wrapper-only cases.
pub(super) fn replace_shared_capture_uses(mut text: String, replacements: &[(String, String)]) -> String {
    for (source, target) in replacements {
        text = replace_identifier(&text, source, target);
    }
    text
}

/// Replaces complete Rust identifier occurrences in generated text.
///
/// Only value uses are rewritten. An identifier in a struct-literal field or
/// map-key position (`name:` with a single colon, e.g. the `length:` field of
/// `SmeltErasedFunction`) is a binding-position name, never a captured value, so
/// it is left untouched; rewriting it would emit an invalid place expression as
/// a field name (`(*smelt_capture_length.borrow_mut()): 0.0`). A path segment
/// (`name::`, double colon) is excluded from this guard and handled by the
/// identifier-boundary check below.
///
/// Occurrences inside a Rust string, byte-string, char, or raw-string literal are
/// left untouched: literal bytes are data, not identifiers. Both emitter-authored
/// literals (a `panic!("recursive closure …")` message inside a user closure
/// named `recursive`) and user program strings can contain the source name, and
/// rewriting them silently corrupts the emitted text. See [`literal_intervals`].
///
/// Occurrences inside a closure that rebinds `source` as one of its parameters
/// are also left untouched. Synthesized coercion adapters bind throwaway
/// parameters (`|value| …`, `|(key, value)| …`) whose names can collide with a
/// user shared-capture local; such a parameter shadows the outer local for the
/// whole closure, so rewriting either the binding or a body use is wrong.
/// Rewriting the binding emits invalid syntax
/// (`|(*smelt_capture_value.borrow_mut())|`), and rewriting a body use would
/// silently read the captured value instead of the closure argument. See
/// [`closure_shadow_intervals`].
fn replace_identifier(text: &str, source: &str, target: &str) -> String {
    let literals = literal_intervals(text);
    let shadows = closure_shadow_intervals(text, source);
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while let Some(offset) = text[index..].find(source) {
        let start = index.saturating_add(offset);
        let end = start.saturating_add(source.len());
        out.push_str(&text[index..start]);
        let before = text[..start].chars().next_back();
        let mut trailing = text[end..].chars();
        let after = trailing.next();
        let is_field_key = after == Some(':') && trailing.next() != Some(':');
        let is_shadowed = contains_offset(&shadows, start);
        let is_literal_data = contains_offset(&literals, start);
        if before.is_some_and(is_rust_ident_char)
            || after.is_some_and(is_rust_ident_char)
            || is_field_key
            || is_shadowed
            || is_literal_data
        {
            out.push_str(source);
        } else {
            out.push_str(target);
        }
        index = end;
    }
    out.push_str(&text[index..]);
    out
}

/// Returns byte ranges `[param_open, body_end)` for closures that rebind
/// `source` as a parameter, so [`replace_identifier`] can leave their contents
/// alone.
///
/// The scan recognizes closure parameter lists of the form `|pattern|` (a `|`
/// that is neither part of `||` nor a bitwise operator, followed only by
/// pattern characters up to the closing `|`). Literals are skipped whole so a
/// `|` inside string data cannot be mistaken for a parameter list.
///
/// When the pattern binds `source`, the closure body — a single expression
/// extending to the enclosing top-level comma, statement terminator, or closing
/// delimiter — is included so that both the binding and every shadowed body use
/// are protected. String and character literals in the body are skipped so their
/// delimiters do not end the body prematurely.
fn closure_shadow_intervals(text: &str, source: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut intervals = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(literal_end) = literal_end(bytes, i) {
            i = literal_end;
            continue;
        }
        if bytes[i] == b'|'
            && bytes.get(i + 1) != Some(&b'|')
            && (i == 0 || bytes[i - 1] != b'|')
            && let Some(close) = closure_param_close(bytes, i)
        {
            if pattern_binds_identifier(&text[i + 1..close], source) {
                let body_end = closure_body_end(bytes, close + 1);
                intervals.push((i, body_end));
                i = body_end;
                continue;
            }
            // Skip past a non-binding parameter list so its interior `|` (none
            // occur today, but paths change) cannot be mistaken for a new list.
            i = close + 1;
            continue;
        }
        i += 1;
    }
    intervals
}

/// Returns true when `offset` falls inside one of `intervals`.
fn contains_offset(intervals: &[(usize, usize)], offset: usize) -> bool {
    intervals
        .iter()
        .any(|&(begin, finish)| offset >= begin && offset < finish)
}

/// Returns byte ranges `[open, close)` for every Rust literal in `text` whose
/// contents are data rather than code.
///
/// Covers string (`"…"`), byte-string (`b"…"`), raw-string (`r"…"`, `r#"…"#`,
/// `br#"…"#`), char, and byte-char literals. Identifier occurrences inside these
/// ranges must never be rewritten: they are emitted verbatim into the generated
/// program's string data.
fn literal_intervals(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut intervals = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = literal_end(bytes, i) {
            intervals.push((i, end));
            i = end;
            continue;
        }
        i = i.saturating_add(1);
    }
    intervals
}

/// Returns the byte index just past the literal starting at `start`, or `None`
/// when no literal starts there.
///
/// A lifetime tick (`'a`) is not a literal, so it yields `None`. A `b`/`r`
/// prefix only opens a literal when it sits at a token boundary and is actually
/// followed by the expected quote (so raw identifiers like `r#type` and ordinary
/// identifiers ending in `r` are not mistaken for literal openers).
fn literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes.get(start)? {
        b'"' => Some(skip_string_literal(bytes, start)),
        b'\'' => {
            let end = skip_char_or_lifetime(bytes, start);
            // `start + 1` means only the tick was consumed: a lifetime, not a literal.
            (end > start.saturating_add(1)).then_some(end)
        }
        b'b' | b'r' => {
            if start
                .checked_sub(1)
                .and_then(|prev| bytes.get(prev))
                .is_some_and(|&prev| is_rust_ident_char(char::from(prev)))
            {
                return None;
            }
            let mut cursor = start;
            if bytes.get(cursor) == Some(&b'b') {
                cursor = cursor.saturating_add(1);
            }
            if bytes.get(cursor) == Some(&b'r') {
                cursor = cursor.saturating_add(1);
                let mut hashes = 0_usize;
                while bytes.get(cursor) == Some(&b'#') {
                    cursor = cursor.saturating_add(1);
                    hashes = hashes.saturating_add(1);
                }
                if bytes.get(cursor) != Some(&b'"') {
                    return None;
                }
                return Some(skip_raw_string_literal(bytes, cursor, hashes));
            }
            match bytes.get(cursor) {
                Some(&b'"') => Some(skip_string_literal(bytes, cursor)),
                Some(&b'\'') => {
                    let end = skip_char_or_lifetime(bytes, cursor);
                    (end > cursor.saturating_add(1)).then_some(end)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Returns the index just past a raw string literal whose opening quote is at
/// `quote` and which is delimited by `hashes` `#` characters.
///
/// Raw strings have no escapes: the literal ends at the first `"` followed by
/// exactly the opening hash count.
fn skip_raw_string_literal(bytes: &[u8], quote: usize, hashes: usize) -> usize {
    let mut i = quote.saturating_add(1);
    while i < bytes.len() {
        let closing = i.saturating_add(1).saturating_add(hashes);
        if bytes.get(i) == Some(&b'"')
            && closing <= bytes.len()
            && bytes
                .get(i.saturating_add(1)..closing)
                .is_some_and(|run| run.iter().all(|&ch| ch == b'#'))
        {
            return closing;
        }
        i = i.saturating_add(1);
    }
    bytes.len()
}

/// Returns the byte index of the `|` closing a closure parameter list that opens
/// at `open`, or `None` when the run after `open` is not a parameter list.
///
/// Only characters that can appear in the emitted adapter parameter patterns are
/// permitted (identifiers, `_`, `,`, whitespace, parentheses, `&`, `:`, and the
/// `<`/`>` of type annotations). Any other character means the `|` was a bitwise
/// operator, not a closure.
fn closure_param_close(bytes: &[u8], open: usize) -> Option<usize> {
    let mut i = open + 1;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'|' {
            return Some(i);
        }
        let is_param_char = ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                b'_' | b',' | b' ' | b'\t' | b'\n' | b'(' | b')' | b'&' | b':' | b'<' | b'>'
            );
        if !is_param_char {
            return None;
        }
        i += 1;
    }
    None
}

/// Returns true when a closure parameter pattern binds `source` as one of its
/// names (a whole-identifier occurrence bounded by non-identifier characters).
fn pattern_binds_identifier(pattern: &str, source: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while let Some(offset) = pattern[index..].find(source) {
        let start = index + offset;
        let end = start + source.len();
        let before = start.checked_sub(1).map(|prev| bytes[prev] as char);
        let after = pattern[end..].chars().next();
        if !before.is_some_and(is_rust_ident_char) && !after.is_some_and(is_rust_ident_char) {
            return true;
        }
        index = end;
    }
    false
}

/// Returns the byte index at which a closure body starting at `start` ends.
///
/// The body is a single expression; it ends at the first top-level comma,
/// semicolon, or closing delimiter (which belongs to an enclosing group), or at
/// end of input. Bracket depth is tracked so delimiters inside nested groups do
/// not terminate the body, and string/character literals are skipped whole so
/// their delimiters are ignored.
fn closure_body_end(bytes: &[u8], start: usize) -> usize {
    let mut depth: i32 = 0;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i = skip_string_literal(bytes, i);
                continue;
            }
            b'\'' => {
                i = skip_char_or_lifetime(bytes, i);
                continue;
            }
            b'(' | b'[' | b'{' => depth = depth.saturating_add(1),
            b')' | b']' | b'}' if depth == 0_i32 => return i,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' | b';' if depth == 0_i32 => return i,
            _ => {}
        }
        i = i.saturating_add(1);
    }
    bytes.len()
}

/// Returns the index just past a double-quoted string literal starting at the
/// opening quote `start`, honoring backslash escapes.
fn skip_string_literal(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Returns the index just past a character literal, or just past the tick of a
/// lifetime, starting at the tick `start`.
///
/// A character literal (`'x'`, `'\n'`) is skipped whole; a lifetime (`'a`) has
/// no closing tick, so only the tick is consumed and the following identifier is
/// scanned normally.
fn skip_char_or_lifetime(bytes: &[u8], start: usize) -> usize {
    // Escaped char literal: `'\n'`, `'\\'`, `'\''`.
    if bytes.get(start + 1) == Some(&b'\\') {
        let mut i = start + 2;
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                return i + 1;
            }
            i += 1;
        }
        return bytes.len();
    }
    // Simple char literal: a single byte followed by a closing tick.
    if bytes.get(start + 2) == Some(&b'\'') {
        return start + 3;
    }
    // Otherwise a lifetime tick with no closing quote.
    start + 1
}

/// Returns true for characters that can be part of emitted Rust identifiers.
fn is_rust_ident_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

#[cfg(test)]
mod shared_capture_cell_name_tests {
    use super::{SELF_RECURSIVE_UPGRADE, shared_capture_cell_name};

    /// The ordinary shared-capture access form yields its cell name.
    #[test]
    fn recovers_cell_from_borrow_forms() {
        assert_eq!(
            shared_capture_cell_name("(*smelt_capture_total.borrow_mut())"),
            Some("smelt_capture_total")
        );
        assert_eq!(
            shared_capture_cell_name("(*smelt_capture_total.borrow())"),
            Some("smelt_capture_total")
        );
    }

    /// A self-recursive capture upgrades its `Weak` before borrowing, and the cell
    /// name must still come back as a bare IDENTIFIER.
    ///
    /// Callers use the result in a binding position -- they emit
    /// `let <cell> = <cell>.clone();` so a nested closure gets its own handle -- so
    /// returning the expression emitted
    /// `let smelt_capture_x.upgrade().expect(..) = ..`, which is not a pattern and
    /// does not parse. That reached radash's generated `async_test` (a self-recursive
    /// closure which a nested closure also captures) and broke the build, while
    /// remeda and es-toolkit stayed green.
    #[test]
    fn recovers_cell_from_the_self_recursive_upgrade_form() {
        let text = format!("(*smelt_capture_fake_work{SELF_RECURSIVE_UPGRADE}.borrow_mut())");
        assert_eq!(
            shared_capture_cell_name(&text),
            Some("smelt_capture_fake_work")
        );
    }

    /// Text that is not an access form at all is not a cell.
    #[test]
    fn rejects_non_access_forms() {
        assert_eq!(shared_capture_cell_name("smelt_capture_total"), None);
        assert_eq!(shared_capture_cell_name("(*smelt_capture_total)"), None);
    }
}

#[cfg(test)]
mod replace_identifier_tests {
    use super::replace_identifier;

    /// A plain value use is rewritten to its shared-capture lvalue form.
    #[test]
    fn rewrites_value_use() {
        assert_eq!(
            replace_identifier("length + 1", "length", "(*smelt_capture_length.borrow_mut())"),
            "(*smelt_capture_length.borrow_mut()) + 1"
        );
    }

    /// A struct-literal field name (`length:`, a single colon) is a binding
    /// position, never a captured value, so it must be left untouched — rewriting
    /// it would emit an invalid place expression as a field name
    /// (`(*smelt_capture_length.borrow_mut()): 0.0`).
    #[test]
    fn preserves_struct_field_name() {
        assert_eq!(
            replace_identifier(
                "SmeltErasedFunction { callback: cb, length: 0.0, object: None }",
                "length",
                "(*smelt_capture_length.borrow_mut())",
            ),
            "SmeltErasedFunction { callback: cb, length: 0.0, object: None }"
        );
    }

    /// A path segment (`Ty::assoc`, double colon) is not a field key and stays
    /// subject to the ordinary identifier-boundary rule.
    #[test]
    fn rewrites_path_segment_value() {
        assert_eq!(
            replace_identifier("length::MAX", "length", "cell"),
            "cell::MAX"
        );
    }

    /// A shared-capture local named `value` flowing through a list coercion
    /// adapter must not clobber the adapter's own `|value|` parameter. Both the
    /// binding and the shadowed body use belong to the closure, not the capture,
    /// so the whole closure is left untouched. Regression for es-toolkit `wrap`.
    #[test]
    fn preserves_shadowing_closure_adapter() {
        assert_eq!(
            replace_identifier(
                "arg1.clone().into_iter().map(|value| value.into_smelt_unknown()).collect::<SmeltList<_>>()",
                "value",
                "(*smelt_capture_value.borrow_mut())",
            ),
            "arg1.clone().into_iter().map(|value| value.into_smelt_unknown()).collect::<SmeltList<_>>()"
        );
    }

    /// A free use of the capture outside any shadowing closure is still
    /// rewritten, even when a shadowing adapter closure precedes it.
    #[test]
    fn rewrites_free_use_alongside_shadowing_closure() {
        assert_eq!(
            replace_identifier(
                "xs.map(|value| value.len()) + value",
                "value",
                "(*smelt_capture_value.borrow_mut())",
            ),
            "xs.map(|value| value.len()) + (*smelt_capture_value.borrow_mut())"
        );
    }

    /// A destructuring adapter parameter (`|(key, value)|`) also shadows the
    /// capture across the closure body.
    #[test]
    fn preserves_destructuring_closure_adapter() {
        assert_eq!(
            replace_identifier(
                "m.into_iter().map(|(key, value)| (key, value)).collect()",
                "value",
                "(*smelt_capture_value.borrow_mut())",
            ),
            "m.into_iter().map(|(key, value)| (key, value)).collect()"
        );
    }

    /// A string literal inside a shadowing closure body must not end the body
    /// early: its delimiters (`)`, `,`) are ignored so the whole closure stays
    /// protected.
    #[test]
    fn string_literal_does_not_truncate_shadow() {
        assert_eq!(
            replace_identifier(
                "xs.map(|value| panic!(\"bad, value)\", value)) + value",
                "value",
                "CAP",
            ),
            "xs.map(|value| panic!(\"bad, value)\", value)) + CAP"
        );
    }

    /// An occurrence inside a string literal is program data, not an identifier,
    /// so it must survive verbatim. Regression for the es-toolkit `flatten`
    /// closure named `recursive`, whose name corrupted the emitter's own
    /// `panic!("recursive closure control flow …")` message.
    #[test]
    fn preserves_string_literal_contents() {
        assert_eq!(
            replace_identifier(
                "panic!(\"recursive closure control flow is not structured yet\")",
                "recursive",
                "(*smelt_capture_recursive.borrow_mut())",
            ),
            "panic!(\"recursive closure control flow is not structured yet\")"
        );
    }

    /// A literal is skipped, but a real value use after it is still rewritten.
    #[test]
    fn rewrites_use_after_string_literal() {
        assert_eq!(
            replace_identifier("log(\"value\"); value", "value", "CAP"),
            "log(\"value\"); CAP"
        );
    }

    /// An escaped quote inside a literal must not end the literal early, or the
    /// following literal text would be treated as code.
    #[test]
    fn preserves_escaped_quote_string_literal() {
        assert_eq!(
            replace_identifier("f(\"a \\\" value b\", value)", "value", "CAP"),
            "f(\"a \\\" value b\", CAP)"
        );
    }

    /// Raw string literals (`r"…"`, `r#"…"#`) have no escapes and are skipped
    /// whole, including their embedded quotes.
    #[test]
    fn preserves_raw_string_literal_contents() {
        assert_eq!(
            replace_identifier("f(r#\"a \"value\" b\"#, value)", "value", "CAP"),
            "f(r#\"a \"value\" b\"#, CAP)"
        );
        assert_eq!(
            replace_identifier("f(r\"value\", value)", "value", "CAP"),
            "f(r\"value\", CAP)"
        );
    }

    /// A `r`/`b` that merely ends or begins an identifier does not open a
    /// literal, so a following value use is still rewritten.
    #[test]
    fn identifier_ending_in_r_does_not_open_literal() {
        assert_eq!(
            replace_identifier("other\"value\" + value", "value", "CAP"),
            "other\"value\" + CAP"
        );
    }

    /// A `|` inside string data is not a closure parameter list, so it cannot
    /// fabricate a shadow interval that suppresses a later real use.
    #[test]
    fn pipe_inside_string_literal_is_not_a_closure() {
        assert_eq!(
            replace_identifier("f(\"|value| x\", value)", "value", "CAP"),
            "f(\"|value| x\", CAP)"
        );
    }

    /// A bitwise/logical `|` between value uses is not a closure parameter list,
    /// so both operands are still rewritten.
    #[test]
    fn does_not_treat_bitwise_or_as_closure() {
        assert_eq!(
            replace_identifier("flag || value", "value", "CAP"),
            "flag || CAP"
        );
    }
}

/// Renders an owned copy of an already-rendered expression, without cloning a
/// value that is already owned.
///
/// Many emitter helpers need an owned value of a subexpression they were handed
/// as text, so they spell `{text}.clone()`. But `text` very often *already* ends
/// in a `.clone()` -- it came from `operand_text`/`local_value_text`, which clone
/// the local they read. The result is `x.clone().clone()`: a second deep copy of
/// a temporary that nothing else can observe. es-toolkit emitted 2881 of them
/// and remeda 1348.
///
/// `X.clone().clone()` computes exactly `X.clone()` for every `X`: `Clone::clone`
/// takes `&self` and returns an owned value, so the outer call re-copies a value
/// that is already owned and yields an equal one (Smelt's containers derive
/// `Clone`, so identity fields such as `SmeltList::id` are preserved either way).
/// Dropping it is therefore always sound.
///
/// The result is parenthesised only when `text` is not already a postfix chain,
/// so a caller can splice it into a larger expression without worrying about
/// precedence while generated code keeps the plain `value.clone()` spelling a
/// hand-written port would use instead of a noisy `(value).clone()`.
///
/// This is a syntax-directed peephole on text this module already owns, not a
/// per-construct special case: it fires wherever a caller asks for an owned
/// copy, whatever construct produced the inner expression.
pub(super) fn cloned_value_text(text: &str) -> String {
    let trimmed = text.trim();
    let already_owned = trimmed.ends_with(".clone()");
    match (is_postfix_chain(trimmed), already_owned) {
        (true, true) => trimmed.to_owned(),
        (true, false) => format!("{trimmed}.clone()"),
        (false, true) => format!("({trimmed})"),
        (false, false) => format!("({trimmed}).clone()"),
    }
}

/// Renders `text` as a shared borrow of the value it evaluates to.
///
/// The counterpart to [`cloned_value_text`] for a callee that reads a value
/// through `&T` instead of taking it by value: a postfix chain keeps the plain
/// `&value` / `&value.field` spelling a hand-written port would use, while any
/// looser expression is parenthesised so the `&` cannot bind more tightly than
/// the caller meant. When the text already ends in a `.clone()` that clone is
/// dropped -- borrowing a value never needs an owned copy of it -- unless the
/// clone is the whole expression's tail on a non-postfix form, where removing
/// it could change what is borrowed, so it is kept and parenthesised instead.
///
/// Like [`cloned_value_text`] this is a syntax-directed peephole on text this
/// module already owns, not a per-construct special case.
pub(super) fn borrowed_value_text(text: &str) -> String {
    let trimmed = text.trim();
    if is_postfix_chain(trimmed) {
        // `a.clone()` and `a` borrow the same value, so the copy is dead weight
        // behind a `&`. Only strip it from a postfix chain, where the prefix is
        // itself a complete expression naming the same place.
        let borrowed = trimmed.strip_suffix(".clone()").unwrap_or(trimmed);
        if is_postfix_chain(borrowed) {
            return format!("&{borrowed}");
        }
        return format!("&{trimmed}");
    }
    format!("&({trimmed})")
}

/// Whether `text` is a primary expression followed only by field, method, and
/// index postfixes -- `a`, `a.b`, `f(x).g()`, `xs[0].y`.
///
/// Such an expression binds tighter than every Rust operator, so appending
/// `.clone()` to it, or splicing it into a larger expression, cannot change how
/// either parses. Anything else (a `match`, an `if`, a block, a binary
/// operation, a unary `*`, a turbofish path) is wrapped in parentheses instead.
///
/// The test is deliberately conservative and purely lexical: at bracket depth
/// zero the text must start with an identifier character and contain no
/// whitespace and no operator character. A false negative only costs a pair of
/// parentheses; a false positive would change the meaning of emitted code, so
/// every construct that is not obviously a postfix chain is rejected.
fn is_postfix_chain(text: &str) -> bool {
    // A string literal is a primary expression too, and static property keys
    // render as one (`"b".to_owned()`), so accept a leading literal and check
    // the postfixes that follow it.
    let rest = match strip_leading_string_literal(text) {
        Some(rest) => rest,
        None => match text.chars().next() {
            Some(first) if first.is_alphanumeric() || first == '_' => text,
            _ => return false,
        },
    };
    let mut depth = 0usize;
    for ch in rest.chars() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => match depth.checked_sub(1) {
                Some(next) => depth = next,
                // Unbalanced here means the text closes a group it never opened;
                // treat it as not a chain rather than guessing.
                None => return false,
            },
            _ if depth > 0 => {}
            ch if ch.is_whitespace() => return false,
            '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^' | '!' | '<' | '>' | '=' | '?' | ','
            | ':' | ';' | '\'' | '"' | '#' | '@' => return false,
            _ => {}
        }
    }
    depth == 0
}

/// Strips a leading Rust string literal from `text`, returning what follows it.
///
/// Returns `None` when `text` does not start with a `"` or the literal is not
/// terminated. Escapes are honoured so a `\"` inside the literal does not end
/// it early.
fn strip_leading_string_literal(text: &str) -> Option<&str> {
    let body = text.strip_prefix('"')?;
    let mut escaped = false;
    for (offset, ch) in body.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(&body[offset + 1..]);
        }
    }
    None
}
