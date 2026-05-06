//! Identifier normalization helpers for TypeScript source names.

/// Converts a TypeScript-style identifier into a Rust-style snake_case identifier.
pub fn camel_to_snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len());

    for (idx, ch) in chars.iter().copied().enumerate() {
        if ch == '_' {
            out.push(ch);
            continue;
        }

        if ch.is_ascii_uppercase() {
            let prev = idx.checked_sub(1).and_then(|prev| chars.get(prev)).copied();
            let next = chars.get(idx + 1).copied();
            let prev_is_word =
                prev.is_some_and(|prev| prev.is_ascii_lowercase() || prev.is_ascii_digit());
            let acronym_boundary = prev.is_some_and(|prev| prev.is_ascii_uppercase())
                && next.is_some_and(|next| next.is_ascii_lowercase());

            if (prev_is_word || acronym_boundary) && !out.ends_with('_') {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }

    out
}
