//! A source item never takes a name the generated crate already spells.
//!
//! The runtime text and the emitter's own output name Rust prelude items
//! unqualified (`Box<dyn Fn(..)>`, `impl Clone for`, `.into_iter()` through
//! `IntoIterator`) and declare runtime items at the crate root (`JsString`). A
//! source type of one of those names (`type Fn`, `class Iterator`) declared at
//! the crate root would shadow it for the whole crate, so
//! [`crate::rust::sanitize_identifier`] renames it. The required set is derived
//! here from what is actually emitted: the fully enabled runtime and every
//! committed example crate.

use std::collections::BTreeSet;

use crate::runtime_prelude::rendered_text;
use crate::rust::sanitize_identifier;

/// Every item of the Rust 2024 prelude (`std::prelude::rust_2024`), the names a
/// crate may spell without a `use`.
const RUST_PRELUDE: &[&str] = &[
    "Copy", "Send", "Sized", "Sync", "Unpin", "Drop", "Fn", "FnMut", "FnOnce", "AsyncFn", "AsyncFnMut",
    "AsyncFnOnce", "drop", "Box", "ToOwned", "Clone", "PartialEq", "PartialOrd", "Eq", "Ord", "AsRef",
    "AsMut", "Into", "From", "Default", "Iterator", "Extend", "IntoIterator", "DoubleEndedIterator",
    "ExactSizeIterator", "Option", "Some", "None", "Result", "Ok", "Err", "String", "ToString", "Vec",
    "TryFrom", "TryInto", "FromIterator", "Future", "IntoFuture",
];

/// The emitted texts: the runtime with every flag on, then every example crate.
fn emitted_texts() -> Vec<String> {
    let mut texts = vec![rendered_text(&|_| true)];
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut stack = vec![examples];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                texts.push(text);
            }
        }
    }
    texts
}

/// Whether `text` spells `name` unqualified: not after `::`, `.` or a word character.
fn spells_unqualified(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        !before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == ':' || ch == '.')
            && !after.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
    })
}

/// Names the runtime declares at the crate root (struct, enum, trait, type,
/// union, fn, const, static, macro), other than its `smelt`-prefixed ones.
fn runtime_root_names(runtime: &str) -> BTreeSet<String> {
    let kinds = ["struct ", "enum ", "trait ", "type ", "union ", "fn ", "const ", "static ", "macro_rules! "];
    let mut names = BTreeSet::new();
    for line in runtime.lines() {
        let declaration = line
            .strip_prefix("pub(crate) ")
            .or_else(|| line.strip_prefix("pub "))
            .unwrap_or(line);
        for kind in kinds {
            if let Some(rest) = declaration.strip_prefix(kind) {
                let name: String = rest.chars().take_while(|ch| ch.is_alphanumeric() || *ch == '_').collect();
                let lower = name.to_ascii_lowercase();
                if !name.is_empty() && !lower.starts_with("smelt") && !name.starts_with('_') {
                    names.insert(name);
                }
            }
        }
    }
    names
}

#[test]
fn every_name_the_generated_crate_spells_is_reserved() {
    let texts = emitted_texts();
    let runtime = texts.first().cloned().unwrap_or_default();
    let mut required: BTreeSet<String> = RUST_PRELUDE
        .iter()
        .filter(|name| texts.iter().any(|text| spells_unqualified(text, name)))
        .map(|name| (*name).to_owned())
        .collect();
    required.extend(runtime_root_names(&runtime));
    let unreserved: Vec<&String> = required.iter().filter(|name| sanitize_identifier(name) == **name).collect();
    assert!(unreserved.is_empty(), "names a source item could shadow: {unreserved:?}");
}
