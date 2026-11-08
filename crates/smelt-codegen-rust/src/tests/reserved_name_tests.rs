//! A source item never takes a name the generated crate already spells.
//!
//! The runtime text and the emitter's own output name Rust prelude items
//! unqualified (`Box<dyn Fn(..)>`, `impl Clone for`, `.into_iter()` through
//! `IntoIterator`) and declare runtime items at the crate root (`JsString`). A
//! source type of one of those names (`type Fn`, `class Iterator`) declared at
//! the crate root would shadow it for the whole crate, so
//! the name allocator ([`crate::names`]) renames it. The required set is derived
//! here from what is actually emitted: the fully enabled runtime and every
//! committed example crate.

use std::collections::BTreeSet;

use crate::runtime_prelude::rendered_text;
use crate::names::{NameScope, NameSpace, Spelling};

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
        let after = text[at.saturating_add(name.len())..].chars().next();
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
    let unreserved: Vec<&String> = required
        .iter()
        .filter(|name| NameScope::new(NameSpace::Types).claim(Spelling::Source(name)).as_str() == name.as_str())
        .collect();
    assert!(unreserved.is_empty(), "names a source item could shadow: {unreserved:?}");
}

/// The trait of every `impl <Trait> for` a committed example crate writes after
/// its prelude, with its generic arguments and path stripped: the last token
/// before ` for `, which an impl's own generic list never reaches.
fn implemented_traits(texts: &[String]) -> BTreeSet<String> {
    let mut traits = BTreeSet::new();
    for text in texts {
        let Some(program) = text.split(crate::PRELUDE_END_MARKER).nth(1) else {
            continue;
        };
        for line in program.lines().filter(|line| line.starts_with("impl")) {
            let Some((head, _)) = line.split_once(" for ") else { continue };
            let Some(token) = head.rsplit(' ').next() else { continue };
            let name = token.split('<').next().unwrap_or(token);
            if !name.is_empty() && !name.starts_with("impl") {
                traits.insert(name.to_owned());
            }
        }
    }
    traits
}

/// Every trait the emitter implements for a generated type is in
/// [`crate::derive_impls::EMITTED_TRAITS`], so a source method can never take
/// the name of a method the emitter calls through it (a user `clone()` that
/// shadowed `Clone::clone` made every internal handle copy a fresh object).
/// The generated `<Bound>Like` traits are exempt: their members ARE the
/// source methods, by their planned names.
#[test]
fn every_trait_the_emitter_implements_reserves_its_methods() {
    let texts = emitted_texts();
    let known: BTreeSet<&str> = crate::derive_impls::EMITTED_TRAITS.iter().map(|(name, _)| *name).collect();
    let unknown: Vec<String> = implemented_traits(texts.get(1..).unwrap_or_default())
        .into_iter()
        .filter(|name| !known.contains(name.as_str()) && !name.ends_with("Like"))
        .collect();
    assert!(unknown.is_empty(), "traits implemented for user types without reserved methods: {unknown:?}");
    for (_, methods) in crate::derive_impls::EMITTED_TRAITS {
        for method in *methods {
            let claimed = NameScope::new(NameSpace::Members).claim(Spelling::Source(method));
            assert_ne!(claimed.as_str(), *method, "a source method may still take `{method}`");
        }
    }
}
