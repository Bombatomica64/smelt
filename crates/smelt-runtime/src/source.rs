//! The runtime's own source text, indexed by item name.
//!
//! The emitter needs the *text* of a runtime helper, not the compiled symbol:
//! generated crates inline the helpers rather than linking this crate (see the
//! crate-level docs). This module embeds each runtime module's source with
//! [`include_str!`] and exposes the region between a
//! `// @smelt:item <name>` marker and the following `// @smelt:item-end` marker.
//!
//! Marker lines and everything outside a marked region — module docs, `use`
//! statements, `#[cfg(test)] mod tests` — are never part of an item's text, so
//! the runtime modules are free to carry whatever scaffolding they need to
//! compile and to be tested here.

/// Marker prefix that opens a named item region.
const ITEM_START: &str = "// @smelt:item ";

/// Marker line that closes an item region.
const ITEM_END: &str = "// @smelt:item-end";

/// One runtime module's embedded source text.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct RuntimeModule {
    /// Path-like module name, for diagnostics (`value`, `value::list`, ...).
    pub name: &'static str,
    /// The module's full source text.
    pub source: &'static str,
}

/// Every runtime module whose source may contribute emitted items.
///
/// Order is the order items are indexed in; it does not constrain emission
/// order, which the emitter's gate manifest fixes.
const MODULES: &[RuntimeModule] = &[
    RuntimeModule {
        name: "value",
        source: include_str!("value.rs"),
    },
    RuntimeModule {
        name: "value::list",
        source: include_str!("value/list.rs"),
    },
    RuntimeModule {
        name: "clock",
        source: include_str!("clock.rs"),
    },
    RuntimeModule {
        name: "captures",
        source: include_str!("captures.rs"),
    },
    RuntimeModule {
        name: "uri",
        source: include_str!("uri.rs"),
    },
];

/// Returns every runtime module whose source is indexed for emission.
#[must_use]
pub fn modules() -> &'static [RuntimeModule] {
    MODULES
}

/// Returns the verbatim source text of the runtime item called `name`.
///
/// The returned text excludes the marker lines and ends with a newline. `None`
/// means no module declares an item region with that name — which the emitter
/// treats as a hard error rather than emitting nothing.
#[must_use]
pub fn item_source(name: &str) -> Option<&'static str> {
    MODULES
        .iter()
        .find_map(|module| module_item_source(module.source, name))
}

/// Returns the names of every item region, in module then source order.
#[must_use]
pub fn item_names() -> Vec<&'static str> {
    MODULES
        .iter()
        .flat_map(|module| module_item_names(module.source))
        .collect()
}

/// Returns the item region named `name` inside one module's `source`.
///
/// Implemented as a byte-offset slice of the embedded text so the emitted bytes
/// are exactly the bytes on disk, with no re-joining of lines.
fn module_item_source(source: &'static str, name: &str) -> Option<&'static str> {
    let mut offset = 0usize;
    let mut region_start: Option<usize> = None;
    for line in source.split_inclusive('\n') {
        let line_start = offset;
        offset = offset.saturating_add(line.len());
        let trimmed = line.trim();
        if let Some(found) = trimmed.strip_prefix(ITEM_START) {
            region_start = if found.trim() == name {
                Some(offset)
            } else {
                None
            };
            continue;
        }
        if trimmed == ITEM_END {
            if let Some(start) = region_start {
                return source.get(start..line_start);
            }
        }
    }
    None
}

/// Returns the item names declared by one module's `source`, in source order.
fn module_item_names(source: &'static str) -> Vec<&'static str> {
    source
        .lines()
        .filter_map(|line| line.trim().strip_prefix(ITEM_START))
        .map(str::trim)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ITEM_END, ITEM_START, item_names, item_source, modules};

    /// Every marker opens exactly one region and every region is closed.
    #[test]
    fn every_item_marker_is_balanced() {
        for module in modules() {
            let starts = module
                .source
                .lines()
                .filter(|line| line.trim().starts_with(ITEM_START))
                .count();
            let ends = module
                .source
                .lines()
                .filter(|line| line.trim() == ITEM_END)
                .count();
            assert_eq!(
                starts, ends,
                "module {} has {starts} item markers and {ends} end markers",
                module.name
            );
        }
    }

    /// Item names are unique across modules, so lookup is unambiguous.
    #[test]
    fn item_names_are_unique() {
        let names = item_names();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            names.len(),
            "duplicate runtime item names in {names:?}"
        );
    }

    /// An item's name is the name of a Rust item its own text declares.
    ///
    /// This is what keeps a region's name honest: without it a marker could name
    /// `smelt_typo` while defining `smelt_real`, and the emitter would happily
    /// emit text under a name no call site matches.
    #[test]
    fn item_names_match_a_declaration_in_their_text() {
        for name in item_names() {
            let text = item_source(name).expect("indexed item must resolve");
            let declarations = [
                format!("fn {name}("),
                format!("fn {name}<"),
                format!("struct {name}"),
                format!("enum {name}"),
                format!("static {name}:"),
                format!("trait {name}"),
                format!("type {name}"),
            ];
            assert!(
                declarations
                    .iter()
                    .any(|declaration| text.contains(declaration.as_str())),
                "item {name} declares none of {declarations:?}"
            );
        }
    }

    /// A name with no region resolves to `None` instead of empty text.
    #[test]
    fn unknown_item_does_not_resolve() {
        assert!(
            item_source("smelt_not_a_runtime_item").is_none(),
            "an unknown item name must not resolve"
        );
    }

    /// Item text excludes its marker lines and ends with a newline.
    #[test]
    fn item_text_excludes_markers() {
        let text = item_source("smelt_next_object_id").expect("object id helper is indexed");
        assert!(
            !text.contains("@smelt:item"),
            "marker lines leaked into item text: {text}"
        );
        assert!(text.ends_with('\n'), "item text must end with a newline");
    }

    /// Emitted regions must not reference module-relative paths.
    ///
    /// The emitted crate has one flat module, so `crate::` / `super::` inside a
    /// region would compile here and break every generated crate.
    #[test]
    fn item_text_has_no_module_relative_paths() {
        for name in item_names() {
            let text = item_source(name).expect("indexed item must resolve");
            for forbidden in ["crate::", "super::", "self::"] {
                assert!(
                    !text.contains(forbidden),
                    "item {name} references `{forbidden}`, which does not exist in a generated crate"
                );
            }
        }
    }
}
