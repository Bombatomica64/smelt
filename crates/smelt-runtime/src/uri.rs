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

#[cfg(test)]
mod tests {
    use super::smelt_encode_uri;

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
}
