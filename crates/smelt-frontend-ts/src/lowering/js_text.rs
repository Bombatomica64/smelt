//! Conversion of oxc JavaScript string values into Rust text.
//!
//! Since oxc 0.157 the AST stores string values (string literals, cooked template text, static
//! property names) as [`JSStr`], a canonical WTF-8 string that can hold JavaScript lone
//! surrogates such as `"\uD800"`. Rust's `str`/`String` cannot represent lone surrogates, and
//! Smelt never modeled them: the HIR, the stdlib and the generated Rust all carry UTF-8 text.
//!
//! This module is the single place where the frontend crosses from `JSStr` to Rust text. Every
//! value that is valid UTF-8 (all ordinary source) is borrowed unchanged, so lowering behaves
//! exactly as it did when oxc handed out `&str`. A value containing a lone surrogate is converted
//! lossily, replacing each lone surrogate with U+FFFD — the same result as
//! `String::from_utf16_lossy` applied to the value's UTF-16 encoding. Never use `display()` for
//! values, keys or comparisons: its escaping is for diagnostics and is not injective.

use std::borrow::Cow;

use oxc::ast::StaticPropertyName;
use oxc::ast::ast::TemplateElementValue;
use oxc::str::JSStr;

/// Returns the Rust text of a JavaScript string value.
///
/// Borrows the value when it is valid UTF-8; otherwise builds a `String` in which every lone
/// surrogate becomes U+FFFD (identical to `String::from_utf16_lossy` on the UTF-16 encoding,
/// because canonical WTF-8 never stores a surrogate pair as two halves).
pub(in crate::lowering) fn js_str_text(value: JSStr<'_>) -> Cow<'_, str> {
    value.as_str().map_or_else(
        || {
            Cow::Owned(
                value
                    .chars()
                    .map(|ch| ch.to_char().unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect(),
            )
        },
        Cow::Borrowed,
    )
}

/// Returns the Rust text of an optional JavaScript string value (see [`js_str_text`]).
pub(in crate::lowering) fn opt_js_str_text(value: Option<JSStr<'_>>) -> Option<Cow<'_, str>> {
    value.map(js_str_text)
}

/// Returns the Rust text of a statically known property name (see [`js_str_text`]).
///
/// Borrowed AST names keep their lifetime; numeric keys, which oxc already owns as text, become
/// an owned `String`.
pub(in crate::lowering) fn static_name_text(name: StaticPropertyName<'_>) -> Cow<'_, str> {
    match name {
        StaticPropertyName::Borrowed(value) => js_str_text(value),
        StaticPropertyName::Owned(value) => Cow::Owned(value.into_string()),
    }
}

/// Returns the text of a template literal quasi: its cooked value (see [`js_str_text`]), or the
/// raw source text when the quasi has no cooked value (an invalid escape in a tagged template).
pub(in crate::lowering) fn template_element_text<'a>(
    value: &TemplateElementValue<'a>,
) -> Cow<'a, str> {
    value
        .cooked
        .map_or_else(|| Cow::Borrowed(value.raw.as_str()), js_str_text)
}

#[cfg(test)]
mod tests {
    //! The UTF-8 and lone-surrogate paths of [`js_str_text`].

    use std::borrow::Cow;

    use oxc::allocator::Allocator;
    use oxc::str::{JSStr, JSStrBuilder};

    use super::js_str_text;

    #[test]
    fn utf8_values_are_borrowed_unchanged() {
        let text = js_str_text(JSStr::from("héllo 🦀"));
        assert!(matches!(text, Cow::Borrowed("héllo 🦀")), "got {text:?}");
    }

    #[test]
    fn lone_surrogates_become_replacement_characters() {
        let allocator = Allocator::new();
        let mut builder = JSStrBuilder::new_in(&&allocator);
        builder.push('a');
        builder.push_code_unit(0xD800);
        builder.push('b');
        builder.push_code_unit(0xDC00);
        let value = builder.into_js_str();
        let expected = String::from_utf16_lossy(&value.encode_utf16().collect::<Vec<_>>());
        assert_eq!(js_str_text(value), expected, "lossy text must match from_utf16_lossy");
        assert_eq!(expected, "a\u{FFFD}b\u{FFFD}", "each lone surrogate is one U+FFFD");
    }
}
