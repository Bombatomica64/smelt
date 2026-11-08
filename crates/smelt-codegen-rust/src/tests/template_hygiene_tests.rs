//! Emitter templates bind only reserved names around spliced code.
//!
//! A template such as `{list}.iter().map(|item| {body})` splices program code
//! (`{body}`) inside a scope the template opens. If the template's own binder
//! were a name a program can spell (`item`, `value`, `key`), a program local of
//! that name referenced from `{body}` would silently read the template's
//! binder instead. Program locals are claimed from the body's name scope,
//! which moves every source name out of the emitter's prefixes
//! ([`crate::names`]: `smelt_value` in source renders `src_smelt_value`), so a
//! template temporary spelled with one of those prefixes can never capture a
//! program local. This test holds every emitter format template to that rule.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "a lexical scan whose every offset is a cursor bounded by the text it walks"
)]

use std::path::Path;

/// Files whose templates never splice program code into a scope they open.
///
/// * `json_decode.rs` builds standalone `fn smelt_decode_N(value, path)`
///   items; their splices are calls of other decoders the same module renders,
///   never program expressions, so no source local is ever in scope.
/// * `*_prelude.rs` files are fixed runtime text with no program splices.
const EXEMPT_FILES: &[&str] = &["json_decode.rs"];

/// Whether a name a template binds is one no program local can render to.
fn is_reserved(name: &str) -> bool {
    name == "_"
        || name == "self"
        || name.starts_with("smelt_")
        || name.starts_with("_smelt")
        || name.starts_with("__smelt")
        || name.starts_with("closure_arg_")
}

/// The byte ranges of the format placeholders (`{name}`, `{}`) in `body`.
fn placeholders(body: &str) -> Vec<(usize, usize)> {
    let bytes = body.as_bytes();
    let mut spans = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if body[at..].starts_with("{{") || body[at..].starts_with("}}") {
            at += 2;
            continue;
        }
        if bytes[at] == b'{' {
            let Some(end) = body[at..].find('}') else { break };
            spans.push((at, at + end));
            at += end + 1;
            continue;
        }
        at += 1;
    }
    spans
}

/// The identifiers at the start of `text`, split on `,` and parentheses.
fn identifiers(text: &str) -> Vec<&str> {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|word| word.chars().next().is_some_and(|first| first.is_ascii_lowercase() || first == '_'))
        .filter(|word| !matches!(*word, "mut" | "ref"))
        .collect()
}

/// The names `body` (one template, placeholders blanked) binds with a closure
/// parameter list, a `let`, a `for` or a single-binding pattern, each with
/// the offset where its scope starts.
fn binders(body: &str) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    let mut rest = body;
    let mut offset = 0;
    while let Some(bar) = rest.find('|') {
        let after = &rest[bar + 1..];
        if let Some(close) = after.find('|') {
            let params = &after[..close];
            let before = rest[..bar].trim_end();
            // `a || b` and `a | b` are operators, not parameter lists.
            let opens_closure = !params.contains('|')
                && !params.is_empty()
                && (before.is_empty() || before.ends_with(['(', ',', '=', '{']) || before.ends_with("move"));
            if opens_closure {
                let names = params.split(',').map(|param| param.split(':').next().unwrap_or(""));
                for name in names.flat_map(identifiers) {
                    found.push((name.to_owned(), offset + bar + close + 2));
                }
            }
            rest = &after[close + 1..];
            offset += bar + close + 2;
        } else {
            break;
        }
    }
    for keyword in ["let ", "for "] {
        let mut start = 0;
        while let Some(offset_in_rest) = body[start..].find(keyword) {
            let at = start + offset_in_rest;
            let preceded = body[..at].chars().next_back().is_none_or(|ch| !(ch.is_ascii_alphanumeric() || ch == '_'));
            let tail = &body[at + keyword.len()..];
            let end = if keyword == "for " { tail.find(" in ") } else { tail.find(['=', ';']) };
            if preceded && let Some(end) = end {
                let annotated = tail[..end].split(':').next().unwrap_or("");
                let pattern = annotated.trim_start_matches("mut ");
                if !pattern.contains("::") && !pattern.chars().next().is_some_and(char::is_uppercase) {
                    for name in identifiers(pattern) {
                        found.push((name.to_owned(), at));
                    }
                }
            }
            start = at + keyword.len();
        }
    }
    // A one-binding variant pattern: `Some(x) =>`, `SmeltUnknown::String(x) if`,
    // `if let Ok(x) = ..`, `Self::M0(x) =>`.
    for (at, _) in body.match_indices('(') {
        let path_start = body[..at]
            .rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == ':'))
            .map_or(0, |index| index + 1);
        let path = &body[path_start..at];
        let variant = path.rsplit("::").next().unwrap_or("");
        if !variant.chars().next().is_some_and(char::is_uppercase) {
            continue;
        }
        let tail = &body[at + 1..];
        let Some(close) = tail.find(')') else { continue };
        let inner = tail[..close].trim().trim_start_matches("ref ").trim_start_matches("mut ");
        let after = tail[close + 1..].trim_start();
        let binds = after.starts_with("=>") || after.starts_with("if ") || (after.starts_with('=') && !after.starts_with("=="));
        if binds && identifiers(inner).len() == 1 && identifiers(inner)[0] == inner {
            found.push((inner.to_owned(), at));
        }
    }
    found
}

/// Every format template under `dir`, as `(file, line, body)`.
fn format_templates(dir: &Path, out: &mut Vec<(String, usize, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "tests") {
                format_templates(&path, out);
            }
            continue;
        }
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_owned();
        if path.extension().is_none_or(|extension| extension != "rs") || name.ends_with("_prelude.rs") || EXEMPT_FILES.contains(&name.as_str()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for macro_name in ["format!(", "format_args!("] {
            let mut start = 0;
            while let Some(offset_in_rest) = text[start..].find(macro_name) {
                let at = start + offset_in_rest;
                start = at + macro_name.len();
                let tail = text[start..].trim_start();
                let line = text[..at].matches('\n').count() + 1;
                // A raw template (`r"..."`, `r#"..."#`) ends at its own closer.
                if let Some(raw) = tail.strip_prefix('r') {
                    let hashes = raw.chars().take_while(|ch| *ch == '#').count();
                    let Some(body) = raw[hashes..].strip_prefix('"') else { continue };
                    let closer = format!("\"{}", "#".repeat(hashes));
                    let end = body.find(&closer).unwrap_or(body.len());
                    out.push((path.display().to_string(), line, body[..end].to_owned()));
                    continue;
                }
                let Some(literal) = tail.strip_prefix('"') else { continue };
                let mut end = 0;
                let bytes = literal.as_bytes();
                while end < bytes.len() && bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                out.push((path.display().to_string(), line, literal[..end.min(literal.len())].to_owned()));
            }
        }
    }
}

/// No emitter template binds a name a program local can spell around code it
/// splices: every such binder carries a reserved prefix.
#[test]
fn template_binders_around_spliced_code_are_reserved() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut templates = Vec::new();
    format_templates(&root, &mut templates);
    assert!(templates.len() > 500, "the scan must see the emitter's templates, saw {}", templates.len());
    let mut offenders = Vec::new();
    for (file, line, body) in &templates {
        let holes = placeholders(body);
        let mut blanked = body.clone();
        for (start, end) in holes.iter().rev() {
            blanked.replace_range(*start..=*end, &"#".repeat(end - start + 1));
        }
        for (name, scope) in binders(&blanked) {
            let splices_in_scope = holes.iter().any(|(start, _)| *start >= scope);
            if splices_in_scope && !is_reserved(&name) {
                offenders.push(format!("{file}:{line}: `{name}`"));
            }
        }
    }
    offenders.sort();
    offenders.dedup();
    assert!(offenders.is_empty(), "template temporaries a program local can shadow:\n{}", offenders.join("\n"));
}

/// A template that binds a reserved name never still interpolates the name it
/// had before the `smelt_` prefix: `{{value:?}}` in a template whose binder
/// became `smelt_value` named nothing in the generated program (E0425, Hono's
/// fetch body-arm panic message).
#[test]
fn renamed_binders_leave_no_stale_inline_argument() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut templates = Vec::new();
    format_templates(&root, &mut templates);
    let mut offenders = Vec::new();
    for (file, line, body) in &templates {
        let mut blanked = body.clone();
        for (start, end) in placeholders(body).iter().rev() {
            blanked.replace_range(*start..=*end, &"#".repeat(end - start + 1));
        }
        for (name, _) in binders(&blanked) {
            let Some(bare) = name.strip_prefix("smelt_") else { continue };
            if body.contains(&format!("{{{{{bare}}}}}")) || body.contains(&format!("{{{{{bare}:")) {
                offenders.push(format!("{file}:{line}: `{{{{{bare}}}}}` after binding `{name}`"));
            }
        }
    }
    offenders.sort();
    offenders.dedup();
    assert!(offenders.is_empty(), "stale inline arguments:\n{}", offenders.join("\n"));
}

/// The scanner sees each binder shape, and passes reserved names.
#[test]
fn the_binder_scan_sees_each_shape() {
    let names = |body: &str| binders(body).into_iter().map(|(name, _)| name).collect::<Vec<_>>();
    assert_eq!(names("xs.iter().map(|item| #)"), ["item"]);
    assert_eq!(names("m.iter().all(|(key, value)| #)"), ["key", "value"]);
    assert_eq!(names("{{ let needle = #; # }}"), ["needle"]);
    assert_eq!(names("for (index, item) in # {{ # }}"), ["index", "item"]);
    assert_eq!(names("match # {{ Some(value) => #, None => # }}"), ["value"]);
    assert_eq!(names("a || b"), Vec::<String>::new());
    assert_eq!(names("Some(SmeltUnknown::Null) => #"), Vec::<String>::new());
    assert_eq!(names("match # {{ SmeltUnknown::String(value) => {{ # }} }}"), ["value"]);
    assert!(is_reserved("smelt_value") && !is_reserved("value"));
}
