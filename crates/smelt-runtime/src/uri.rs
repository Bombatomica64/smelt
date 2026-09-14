//! URI encoding helpers (`encodeURI` and friends).
//!
//! The emitted names live in `smelt_stdlib::runtime_symbols::strings`, and the
//! `runtime_manifest_symbol_names_match` test in `smelt-codegen-rust` pins the
//! item text below to those constants so the definition and the call sites
//! cannot drift.

// @smelt:item smelt_encode_uri
/// JavaScript `encodeURI`: percent-encode `value` as a full URI.
///
/// Leaves the ECMA-262 `encodeURI` unescaped set intact — ASCII alphanumerics,
/// the unreserved marks `- _ . ! ~ * ' ( )`, the URI reserved separators
/// `; / ? : @ & = + $ ,`, and `#` — and percent-encodes every other character's
/// UTF-8 bytes as uppercase `%XX` triplets. Rust `&str` is always valid UTF-8,
/// so ECMA-262's lone-surrogate `URIError` case cannot occur here.
fn smelt_encode_uri(value: &str) -> String {
    use ::std::fmt::Write as _;
    let mut encoded = String::with_capacity(value.len());
    for ch in value.chars() {
        let unescaped = ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')'
                | ';' | '/' | '?' | ':' | '@' | '&' | '=' | '+' | '$' | ',' | '#');
        if unescaped {
            encoded.push(ch);
        } else {
            let mut buffer = [0u8; 4];
            for byte in ch.encode_utf8(&mut buffer).as_bytes() {
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}
// @smelt:item-end

// @smelt:item smelt_encode_uri_component
/// JavaScript `encodeURIComponent`: percent-encode `value` as one URI component.
///
/// Differs from `encodeURI` by exactly the URI reserved separators: ECMA-262
/// leaves only ASCII alphanumerics and the unreserved marks `- _ . ! ~ * ' ( )`
/// literal, and percent-encodes `; / ? : @ & = + $ , #` so the result can sit
/// inside one path segment or query value without being reparsed as structure.
/// Everything else becomes uppercase `%XX` UTF-8 triplets. Rust `&str` is
/// always valid UTF-8, so ECMA-262's lone-surrogate `URIError` cannot occur.
fn smelt_encode_uri_component(value: &str) -> String {
    use ::std::fmt::Write as _;
    let mut encoded = String::with_capacity(value.len());
    for ch in value.chars() {
        let unescaped = ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')');
        if unescaped {
            encoded.push(ch);
        } else {
            let mut buffer = [0u8; 4];
            for byte in ch.encode_utf8(&mut buffer).as_bytes() {
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}
// @smelt:item-end

// @smelt:item smelt_decode_uri_octets
/// Shared percent-decoder for `decodeURI` and `decodeURIComponent`.
///
/// ECMA-262 gives both one algorithm (`Decode`) parameterized by a *preserve
/// set*: an escape whose decoded character is in that set is left in its
/// ESCAPED form rather than decoded, which is how `decodeURI` avoids
/// disturbing the URI structure it must not touch. Everything else is decoded.
///
/// Returns `None` for exactly the input ECMA-262 rejects with a `URIError`: a
/// `%` not followed by two hex digits, a UTF-8 continuation byte where a
/// leading byte is required (or a truncated multi-byte run), and octets that
/// are not valid UTF-8. Decoding runs over BYTES because one character can be
/// several consecutive escapes.
fn smelt_decode_uri_octets(value: &str, preserve: &str) -> ::std::option::Option<String> {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = *bytes.get(index)?;
        if byte != b'%' {
            // Not an escape: copy the character whole. `value` is valid UTF-8,
            // so a non-`%` byte begins a character of known length.
            let ch = value.get(index..)?.chars().next()?;
            out.push(ch);
            index = index.saturating_add(ch.len_utf8());
            continue;
        }
        // One escape run: `%XX`, plus further `%XX` for the remaining bytes of
        // a multi-byte character.
        let start = index;
        let mut octets: Vec<u8> = Vec::new();
        loop {
            if index >= bytes.len() || *bytes.get(index)? != b'%' {
                // The run ended before the leading byte's announced length.
                return None;
            }
            let hex = value.get(index.saturating_add(1)..index.saturating_add(3))?;
            if !hex.bytes().all(|digit| digit.is_ascii_hexdigit()) {
                return None;
            }
            let octet = u8::from_str_radix(hex, 16).ok()?;
            octets.push(octet);
            index = index.saturating_add(3);
            let first = *octets.first()?;
            let expected = if first < 0x80 {
                1usize
            } else if first & 0xE0 == 0xC0 {
                2
            } else if first & 0xF0 == 0xE0 {
                3
            } else if first & 0xF8 == 0xF0 {
                4
            } else {
                // A continuation byte in leading position is a `URIError`.
                return None;
            };
            if octets.len() >= expected {
                break;
            }
        }
        let decoded = ::std::str::from_utf8(&octets).ok()?;
        if decoded.chars().count() != 1 {
            return None;
        }
        let ch = decoded.chars().next()?;
        if preserve.contains(ch) {
            // Reserved for this variant: keep the escaped text verbatim.
            out.push_str(value.get(start..index)?);
        } else {
            out.push(ch);
        }
    }
    Some(out)
}
// @smelt:item-end

// @smelt:item smelt_decode_uri
/// JavaScript `decodeURI`: percent-decode a full URI.
///
/// The URI reserved separators `; / ? : @ & = + $ , #` stay ESCAPED, because
/// decoding one would change the URI's structure — a `%2F` inside a path
/// segment is a literal slash in a name, not a separator. Returns `None` where
/// ECMA-262 throws a `URIError`.
fn smelt_decode_uri(value: &str) -> ::std::option::Option<String> {
    smelt_decode_uri_octets(value, ";/?:@&=+$,#")
}
// @smelt:item-end

// @smelt:item smelt_decode_uri_component
/// JavaScript `decodeURIComponent`: percent-decode one URI component.
///
/// Preserves nothing: a component is already a single value, so every escape
/// is decoded, separators included. Returns `None` where ECMA-262 throws a
/// `URIError`.
fn smelt_decode_uri_component(value: &str) -> ::std::option::Option<String> {
    smelt_decode_uri_octets(value, "")
}
// @smelt:item-end

#[cfg(test)]
mod tests {
    use super::{
        smelt_decode_uri, smelt_decode_uri_component, smelt_encode_uri,
        smelt_encode_uri_component,
    };

    /// The ECMA-262 unescaped set survives verbatim.
    #[test]
    fn the_unescaped_set_is_left_alone() {
        let unescaped = "azAZ09-_.!~*'();/?:@&=+$,#";
        assert_eq!(
            smelt_encode_uri(unescaped),
            unescaped,
            "encodeURI must not touch its unescaped set"
        );
    }

    /// Everything else becomes uppercase `%XX` triplets over UTF-8 bytes.
    #[test]
    fn other_characters_become_uppercase_triplets() {
        assert_eq!(
            smelt_encode_uri("a b"),
            "a%20b",
            "a space is one triplet"
        );
        assert_eq!(
            smelt_encode_uri("<>\"{}|\\^`"),
            "%3C%3E%22%7B%7D%7C%5C%5E%60",
            "the delimiters outside the unescaped set are encoded"
        );
    }

    /// Multi-byte characters encode one triplet per UTF-8 byte.
    #[test]
    fn multibyte_characters_encode_every_byte() {
        assert_eq!(
            smelt_encode_uri("é"),
            "%C3%A9",
            "a two-byte character is two triplets"
        );
        assert_eq!(
            smelt_encode_uri("€"),
            "%E2%82%AC",
            "a three-byte character is three triplets"
        );
        assert_eq!(
            smelt_encode_uri("😀"),
            "%F0%9F%98%80",
            "an astral character is four triplets"
        );
    }

    /// The empty string encodes to the empty string.
    #[test]
    fn the_empty_string_round_trips() {
        assert_eq!(smelt_encode_uri(""), "", "nothing to encode");
    }

    /// `encodeURIComponent` differs from `encodeURI` by exactly the URI
    /// reserved separators — the whole reason both exist.
    #[test]
    fn the_component_encoder_escapes_the_reserved_separators() {
        let reserved = ";/?:@&=+$,#";
        assert_eq!(
            smelt_encode_uri(reserved),
            reserved,
            "encodeURI leaves URI structure alone"
        );
        assert_eq!(
            smelt_encode_uri_component(reserved),
            "%3B%2F%3F%3A%40%26%3D%2B%24%2C%23",
            "encodeURIComponent escapes every separator"
        );
        // The unreserved set is shared.
        let unreserved = "azAZ09-_.!~*'()";
        assert_eq!(smelt_encode_uri_component(unreserved), unreserved);
        assert_eq!(
            smelt_encode_uri_component("a b"),
            "a%20b",
            "a space is one triplet in both"
        );
        assert_eq!(
            smelt_encode_uri_component("é"),
            "%C3%A9",
            "multi-byte characters encode per UTF-8 byte in both"
        );
    }

    /// `decodeURIComponent` decodes everything; `decodeURI` keeps the reserved
    /// separators escaped, which is the same asymmetry seen from the other side.
    #[test]
    fn decoding_preserves_reserved_separators_only_for_decode_uri() {
        assert_eq!(
            smelt_decode_uri_component("%3B%2F%3F%3A%40%26%3D%2B%24%2C%23").as_deref(),
            Some(";/?:@&=+$,#")
        );
        assert_eq!(
            smelt_decode_uri("%3B%2F%3F%3A%40%26%3D%2B%24%2C%23").as_deref(),
            Some("%3B%2F%3F%3A%40%26%3D%2B%24%2C%23"),
            "decodeURI must not turn an escaped separator into structure"
        );
        // Everything outside the preserve set decodes in both.
        assert_eq!(smelt_decode_uri("Hello%20World").as_deref(), Some("Hello World"));
        assert_eq!(
            smelt_decode_uri_component("Hello%20World").as_deref(),
            Some("Hello World")
        );
        // Unescaped text passes through untouched, multi-byte included.
        assert_eq!(smelt_decode_uri("a/é?b").as_deref(), Some("a/é?b"));
    }

    /// A multi-byte character is several consecutive escapes and decodes as one.
    #[test]
    fn a_multibyte_escape_run_decodes_to_one_character() {
        assert_eq!(smelt_decode_uri_component("%C3%A9").as_deref(), Some("é"));
        assert_eq!(smelt_decode_uri_component("%E2%82%AC").as_deref(), Some("€"));
        assert_eq!(
            smelt_decode_uri_component("%F0%9F%98%80").as_deref(),
            Some("😀")
        );
        assert_eq!(
            smelt_decode_uri_component("a%C3%A9b").as_deref(),
            Some("aéb"),
            "an escape run in the middle of literal text"
        );
    }

    /// Exactly the inputs ECMA-262 answers with a `URIError` return `None`.
    ///
    /// This is the set Hono's `tryDecode` catches, so the boundary matters:
    /// anything reported as decodable here would silently skip its fallback.
    #[test]
    fn malformed_input_is_rejected() {
        // `%` without two hex digits.
        assert_eq!(smelt_decode_uri_component("%").as_deref(), None);
        assert_eq!(smelt_decode_uri_component("%A").as_deref(), None);
        assert_eq!(smelt_decode_uri_component("%zz").as_deref(), None);
        assert_eq!(smelt_decode_uri_component("a%2").as_deref(), None);
        // A continuation byte where a leading byte is required.
        assert_eq!(smelt_decode_uri_component("%A2%A2").as_deref(), None);
        // A truncated multi-byte run.
        assert_eq!(smelt_decode_uri_component("%C3").as_deref(), None);
        assert_eq!(smelt_decode_uri_component("%E2%82").as_deref(), None);
        // Octets that are not valid UTF-8 together.
        assert_eq!(smelt_decode_uri_component("%C3%28").as_deref(), None);
        // The same rejections apply to `decodeURI`.
        assert_eq!(smelt_decode_uri("%").as_deref(), None);
        assert_eq!(smelt_decode_uri("%A2%A2").as_deref(), None);
        // The empty string is not malformed.
        assert_eq!(smelt_decode_uri_component("").as_deref(), Some(""));
    }
}
