//! The runtime's own source text, and how it is rendered into a generated crate.
//!
//! The emitter needs the *text* of the runtime, not the compiled symbols:
//! generated crates inline it rather than linking this crate (see the crate-level
//! docs). This module embeds each family file with [`include_str!`] and renders
//! its marked regions.
//!
//! # The marker language
//!
//! ```text
//! // @smelt:item NAME when EXPR    opens a region; `when EXPR` is optional
//! // @smelt:item-end               closes it
//! // @smelt:if EXPR                keeps the lines up to `else`/`end` when EXPR holds
//! // @smelt:else
//! // @smelt:end
//! // @smelt:slot NAME              the emitter supplies these lines
//! //= TEXT                         emits TEXT; the `else` arm the crate cannot compile
//! //~ NOTE                         for readers here, never emitted
//! /*@if EXPR*/TEXT/*@end*/         keeps part of one line when EXPR holds
//! ```
//!
//! An `EXPR` is `flag`, `!flag` or several joined by ` & `. Every flag is a name
//! the emitter answers. The compiled crate sees the all-flags-on text, because
//! each directive is a comment.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Marker prefix that opens a named item region.
const ITEM_START: &str = "// @smelt:item ";

/// Marker line that closes an item region.
const ITEM_END: &str = "// @smelt:item-end";

/// Directive opening a conditional run of lines.
const IF: &str = "// @smelt:if ";

/// Directive switching a conditional run to its other arm.
const ELSE: &str = "// @smelt:else";

/// Directive closing a conditional run of lines.
const END: &str = "// @smelt:end";

/// Directive naming lines the emitter supplies.
const SLOT: &str = "// @smelt:slot ";

/// Prefix of a note that is never emitted.
const NOTE: &str = "//~";

/// Prefix of emitted text the compiled crate does not see.
const ALTERNATE: &str = "//= ";

/// Opens an inline span.
const SPAN_IF: &str = "/*@if ";

/// Closes an inline span.
const SPAN_END: &str = "/*@end*/";

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
        name: "abort",
        source: include_str!("runtime/abort.rs"),
    },
    RuntimeModule {
        name: "accessor",
        source: include_str!("runtime/accessor.rs"),
    },
    RuntimeModule {
        name: "array",
        source: include_str!("runtime/array.rs"),
    },
    RuntimeModule {
        name: "bigint",
        source: include_str!("runtime/bigint.rs"),
    },
    RuntimeModule {
        name: "blob",
        source: include_str!("runtime/blob.rs"),
    },
    RuntimeModule {
        name: "blob_record",
        source: include_str!("runtime/blob_record.rs"),
    },
    RuntimeModule {
        name: "boxing",
        source: include_str!("runtime/boxing.rs"),
    },
    RuntimeModule {
        name: "builtin_member",
        source: include_str!("runtime/builtin_member.rs"),
    },
    RuntimeModule {
        name: "byte_buffer",
        source: include_str!("runtime/byte_buffer.rs"),
    },
    RuntimeModule {
        name: "callable_registry",
        source: include_str!("runtime/callable_registry.rs"),
    },
    RuntimeModule {
        name: "captures",
        source: include_str!("runtime/captures.rs"),
    },
    RuntimeModule {
        name: "clock",
        source: include_str!("runtime/clock.rs"),
    },
    RuntimeModule {
        name: "console_inspect",
        source: include_str!("runtime/console_inspect.rs"),
    },
    RuntimeModule {
        name: "conversions",
        source: include_str!("runtime/conversions.rs"),
    },
    RuntimeModule {
        name: "crypto",
        source: include_str!("runtime/crypto.rs"),
    },
    RuntimeModule {
        name: "date",
        source: include_str!("runtime/date.rs"),
    },
    RuntimeModule {
        name: "erased_function",
        source: include_str!("runtime/erased_function.rs"),
    },
    RuntimeModule {
        name: "erased_method",
        source: include_str!("runtime/erased_method.rs"),
    },
    RuntimeModule {
        name: "event_emitter",
        source: include_str!("runtime/event_emitter.rs"),
    },
    RuntimeModule {
        name: "fetch_types",
        source: include_str!("runtime/fetch_types.rs"),
    },
    RuntimeModule {
        name: "field_store",
        source: include_str!("runtime/field_store.rs"),
    },
    RuntimeModule {
        name: "for_in",
        source: include_str!("runtime/for_in.rs"),
    },
    RuntimeModule {
        name: "form_data",
        source: include_str!("runtime/form_data.rs"),
    },
    RuntimeModule {
        name: "function_object",
        source: include_str!("runtime/function_object.rs"),
    },
    RuntimeModule {
        name: "generator",
        source: include_str!("runtime/generator.rs"),
    },
    RuntimeModule {
        name: "host_method",
        source: include_str!("runtime/host_method.rs"),
    },
    RuntimeModule {
        name: "host_module",
        source: include_str!("runtime/host_module.rs"),
    },
    RuntimeModule {
        name: "host_override",
        source: include_str!("runtime/host_override.rs"),
    },
    RuntimeModule {
        name: "host_value_erasure",
        source: include_str!("runtime/host_value_erasure.rs"),
    },
    RuntimeModule {
        name: "http_server",
        source: include_str!("runtime/http_server.rs"),
    },
    RuntimeModule {
        name: "identity",
        source: include_str!("runtime/identity.rs"),
    },
    RuntimeModule {
        name: "js_map",
        source: include_str!("runtime/js_map.rs"),
    },
    RuntimeModule {
        name: "js_set",
        source: include_str!("runtime/js_set.rs"),
    },
    RuntimeModule {
        name: "js_string",
        source: include_str!("runtime/js_string.rs"),
    },
    RuntimeModule {
        name: "json_indent",
        source: include_str!("runtime/json_indent.rs"),
    },
    RuntimeModule {
        name: "json_serde",
        source: include_str!("runtime/json_serde.rs"),
    },
    RuntimeModule {
        name: "key_eq",
        source: include_str!("runtime/key_eq.rs"),
    },
    RuntimeModule {
        name: "list",
        source: include_str!("runtime/list.rs"),
    },
    RuntimeModule {
        name: "math",
        source: include_str!("runtime/math.rs"),
    },
    RuntimeModule {
        name: "number_format",
        source: include_str!("runtime/number_format.rs"),
    },
    RuntimeModule {
        name: "object",
        source: include_str!("runtime/object.rs"),
    },
    RuntimeModule {
        name: "object_model",
        source: include_str!("runtime/object_model.rs"),
    },
    RuntimeModule {
        name: "prim_set",
        source: include_str!("runtime/prim_set.rs"),
    },
    RuntimeModule {
        name: "promise",
        source: include_str!("runtime/promise.rs"),
    },
    RuntimeModule {
        name: "property_access",
        source: include_str!("runtime/property_access.rs"),
    },
    RuntimeModule {
        name: "py_repr",
        source: include_str!("runtime/py_repr.rs"),
    },
    RuntimeModule {
        name: "record",
        source: include_str!("runtime/record.rs"),
    },
    RuntimeModule {
        name: "reflection",
        source: include_str!("runtime/reflection.rs"),
    },
    RuntimeModule {
        name: "regexp",
        source: include_str!("runtime/regexp.rs"),
    },
    RuntimeModule {
        name: "regexp_match",
        source: include_str!("runtime/regexp_match.rs"),
    },
    RuntimeModule {
        name: "sqlite",
        source: include_str!("runtime/sqlite.rs"),
    },
    RuntimeModule {
        name: "string_to_number",
        source: include_str!("runtime/string_to_number.rs"),
    },
    RuntimeModule {
        name: "string_units",
        source: include_str!("runtime/string_units.rs"),
    },
    RuntimeModule {
        name: "strings",
        source: include_str!("runtime/strings.rs"),
    },
    RuntimeModule {
        name: "structural_eq",
        source: include_str!("runtime/structural_eq.rs"),
    },
    RuntimeModule {
        name: "symbol_keys",
        source: include_str!("runtime/symbol_keys.rs"),
    },
    RuntimeModule {
        name: "text_codec",
        source: include_str!("runtime/text_codec.rs"),
    },
    RuntimeModule {
        name: "this_channel",
        source: include_str!("runtime/this_channel.rs"),
    },
    RuntimeModule {
        name: "thrown",
        source: include_str!("runtime/thrown.rs"),
    },
    RuntimeModule {
        name: "timers",
        source: include_str!("runtime/timers.rs"),
    },
    RuntimeModule {
        name: "to_string_tag",
        source: include_str!("runtime/to_string_tag.rs"),
    },
    RuntimeModule {
        name: "typed_array",
        source: include_str!("runtime/typed_array.rs"),
    },
    RuntimeModule {
        name: "unknown",
        source: include_str!("runtime/unknown.rs"),
    },
    RuntimeModule {
        name: "uri",
        source: include_str!("runtime/uri.rs"),
    },
    RuntimeModule {
        name: "url",
        source: include_str!("runtime/url.rs"),
    },
    RuntimeModule {
        name: "value",
        source: include_str!("runtime/value.rs"),
    },
    RuntimeModule {
        name: "view_write_through",
        source: include_str!("runtime/view_write_through.rs"),
    },
    RuntimeModule {
        name: "vitest_mock",
        source: include_str!("runtime/vitest_mock.rs"),
    },
];

/// Returns every runtime module whose source is indexed for emission.
#[must_use]
pub const fn modules() -> &'static [RuntimeModule] {
    MODULES
}

/// One marked region of a family file.
#[derive(Debug, Clone, Copy)]
struct Region {
    /// The region's text, excluding its marker lines.
    text: &'static str,
    /// The `when` expression of its opening marker, if any.
    when: Option<&'static str>,
}

/// Every region of every module, by name, in the order modules declare them.
fn regions() -> &'static HashMap<&'static str, Region> {
    static REGIONS: OnceLock<HashMap<&'static str, Region>> = OnceLock::new();
    REGIONS.get_or_init(|| {
        MODULES
            .iter()
            .flat_map(|module| module_regions(module.source))
            .collect()
    })
}

/// Splits one module's `source` into its regions.
///
/// A byte-offset slice of the embedded text, so an item's bytes are exactly the
/// bytes on disk.
fn module_regions(source: &'static str) -> Vec<(&'static str, Region)> {
    let mut regions = Vec::new();
    let mut offset = 0_usize;
    let mut open: Option<(&'static str, Option<&'static str>, usize)> = None;
    for line in source.split_inclusive('\n') {
        let line_start = offset;
        offset = offset.saturating_add(line.len());
        let trimmed = line.trim();
        if let Some(marker) = trimmed.strip_prefix(ITEM_START) {
            let (name, when) = parse_marker(marker);
            open = Some((name, when, offset));
        } else if trimmed == ITEM_END
            && let Some((name, when, start)) = open.take()
            && let Some(text) = source.get(start..line_start)
        {
            regions.push((name, Region { text, when }));
        }
    }
    regions
}

/// Splits the text after `// @smelt:item ` into a name and a `when` expression.
fn parse_marker(marker: &'static str) -> (&'static str, Option<&'static str>) {
    match marker.split_once(" when ") {
        Some((name, when)) => (name.trim(), Some(when.trim())),
        None => (marker.trim(), None),
    }
}

/// Returns the raw marked text of the runtime item called `name`.
///
/// Directives are still in the text; [`render_item`] is what the emitter uses.
/// The result excludes the marker lines. `None` means no module declares a region
/// with that name, which the emitter treats as a hard error.
#[must_use]
pub fn item_source(name: &str) -> Option<&'static str> {
    regions().get(name).map(|region| region.text)
}

/// Returns the names of every item region, in module then source order.
#[must_use]
pub fn item_names() -> Vec<&'static str> {
    MODULES
        .iter()
        .flat_map(|module| module_regions(module.source))
        .map(|(name, _)| name)
        .collect()
}

/// Returns every flag a marker, directive or span of any region names.
#[must_use]
pub fn flag_names() -> Vec<&'static str> {
    let mut names = Vec::new();
    let mut add = |expr: &'static str| {
        names.extend(
            expr.split('&')
                .map(|term| term.trim().trim_start_matches('!')),
        );
    };
    for region in regions().values() {
        if let Some(when) = region.when {
            add(when);
        }
        for line in region.text.lines() {
            if let Some(expr) = line.trim().strip_prefix(IF) {
                add(expr);
            }
            let mut rest = line;
            while let Some((_, after)) = rest.split_once(SPAN_IF) {
                let Some((expr, tail)) = after.split_once("*/") else {
                    break;
                };
                add(expr);
                rest = tail;
            }
        }
    }
    for entry in crate::order::ORDER {
        if let crate::order::Entry::Blank(expr) = entry {
            add(expr);
        }
    }
    names.sort_unstable();
    names.dedup();
    names
}

/// Answers an `EXPR` under `flags`.
#[must_use]
pub fn holds(expr: &str, flags: &dyn Fn(&str) -> bool) -> bool {
    expr.split('&').all(|term| {
        let term = term.trim();
        term.strip_prefix('!')
            .map_or_else(|| flags(term), |flag| !flags(flag))
    })
}

/// Whether the item called `name` is emitted under `flags`.
#[must_use]
pub fn item_enabled(name: &str, flags: &dyn Fn(&str) -> bool) -> bool {
    regions()
        .get(name)
        .is_some_and(|region| region.when.is_none_or(|when| holds(when, flags)))
}

/// One step of a rendered item.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderedLine<'a> {
    /// A line of text, without its newline. It borrows the item's text unless a
    /// span was resolved in it.
    Text(std::borrow::Cow<'a, str>),
    /// The emitter supplies the lines of this slot here.
    Slot(&'a str),
}

/// Renders the item called `name` under `flags`, resolving every directive.
///
/// Returns `None` when no module declares the item. Whether the item is emitted
/// at all is [`item_enabled`]'s question, not this one's.
#[must_use]
pub fn render_item(name: &str, flags: &dyn Fn(&str) -> bool) -> Option<Vec<RenderedLine<'static>>> {
    let region = regions().get(name)?;
    let mut lines = Vec::new();
    // One entry per open `if`: whether its current arm is the live one.
    let mut arms: Vec<bool> = Vec::new();
    for line in region.text.lines() {
        let trimmed = line.trim_start();
        if let Some(expr) = trimmed.strip_prefix(IF) {
            arms.push(holds(expr, flags));
        } else if trimmed == ELSE {
            if let Some(arm) = arms.last_mut() {
                *arm = !*arm;
            }
        } else if trimmed == END {
            arms.pop();
        } else if !arms.iter().all(|arm| *arm) || trimmed.starts_with(NOTE) {
            // A dead arm, or a note.
        } else if let Some(slot) = trimmed.strip_prefix(SLOT) {
            lines.push(RenderedLine::Slot(slot.trim()));
        } else if let Some(text) = line.strip_prefix(ALTERNATE) {
            lines.push(RenderedLine::Text(resolve_spans(text, flags)));
        } else {
            lines.push(RenderedLine::Text(resolve_spans(line, flags)));
        }
    }
    Some(lines)
}

/// Resolves the inline spans of one line.
fn resolve_spans(line: &'static str, flags: &dyn Fn(&str) -> bool) -> std::borrow::Cow<'static, str> {
    if !line.contains(SPAN_IF) {
        return std::borrow::Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some((before, after)) = rest.split_once(SPAN_IF) {
        out.push_str(before);
        let Some((expr, body)) = after.split_once("*/") else {
            rest = after;
            break;
        };
        let Some((kept, tail)) = body.split_once(SPAN_END) else {
            rest = body;
            break;
        };
        if holds(expr, flags) {
            out.push_str(kept);
        }
        rest = tail;
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::{
        ITEM_END, ITEM_START, RenderedLine, holds, item_enabled, item_names, item_source, modules,
        render_item,
    };

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
            // A region can open with an `impl`, which declares no name of its own.
            if name.starts_with("impl_") || name == "use_hash" {
                continue;
            }
            let text = item_source(name).expect("indexed item must resolve");
            let declarations = [
                format!("fn {name}("),
                format!("fn {name}<"),
                format!("struct {name}"),
                format!("enum {name}"),
                format!("static {name}:"),
                format!("trait {name}"),
                format!("type {name}"),
                format!("const {name}:"),
                format!("static {name}"),
            ];
            assert!(
                declarations
                    .iter()
                    .any(|declaration| text.contains(declaration.as_str())),
                "item {name} declares none of {declarations:?}"
            );
        }
    }

    /// A region's `when` decides whether it is emitted at all.
    #[test]
    fn a_when_expression_gates_its_item() {
        assert!(
            item_enabled("smelt_next_object_id", &|flag| flag == "object_ids"),
            "the id helper is on when its flag is"
        );
        assert!(
            !item_enabled("smelt_next_object_id", &|_| false),
            "the id helper is off when its flag is"
        );
    }

    /// Expressions are conjunctions of flags and negated flags.
    #[test]
    fn expressions_hold_as_conjunctions() {
        let on = |flag: &str| flag == "a";
        assert!(holds("a", &on), "a plain flag");
        assert!(holds("!b", &on), "a negated flag");
        assert!(holds("a & !b", &on), "both terms hold");
        assert!(!holds("a & b", &on), "one term fails");
    }

    /// An inline span drops out without leaving its markers behind.
    #[test]
    fn spans_resolve_within_a_line() {
        let on = super::resolve_spans("x/*@if a*/ y/*@end*/ z", &|_| true);
        let off = super::resolve_spans("x/*@if a*/ y/*@end*/ z", &|_| false);
        assert_eq!(on, "x y z", "a true span keeps its text");
        assert_eq!(off, "x z", "a false span drops its text");
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
            let lines = render_item(name, &|_| true).expect("indexed item must resolve");
            for line in lines {
                let RenderedLine::Text(line) = line else {
                    continue;
                };
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for forbidden in ["crate::", "super::", "self::"] {
                    assert!(
                        !line.contains(forbidden),
                        "item {name} references `{forbidden}`, which does not exist in a generated crate: {line}"
                    );
                }
            }
        }
    }
}
