//! Tests for the runtime prelude manifest (gate → runtime item names).
//!
//! The manifest is the seam between the emitter's `needs_*` gates and the
//! runtime source of truth in `smelt-runtime`. These tests are the "fail loudly"
//! half of that seam: a gate that names an item the runtime does not define, or
//! an item whose emitted name drifts from the shared `runtime_symbols` constant,
//! is caught here rather than by a generated crate that silently lacks a helper.

use super::*;
use crate::runtime_prelude::{flag, rendered_text};
use smelt_runtime::order::{CONSOLE_INSPECT, Entry, ORDER};
use smelt_runtime::source::{flag_names, item_names, item_source};

/// Every item the order names resolves, and every item is in the order.
///
/// This walks the whole manifest, not just what one program turns on, and reports
/// every problem at once. An item left out of the order would be compiled and
/// tested here and silently never reach a generated crate.
#[test]
fn runtime_order_and_items_agree() {
    let ordered: Vec<&str> = ORDER
        .iter()
        .chain(CONSOLE_INSPECT)
        .filter_map(|entry| match entry {
            Entry::Item(name) => Some(*name),
            _ => None,
        })
        .collect();
    let missing: Vec<_> = ordered
        .iter()
        .filter(|name| item_source(name).is_none())
        .collect();
    assert!(missing.is_empty(), "the order names undefined items: {missing:?}");
    let unordered: Vec<_> = item_names()
        .into_iter()
        .filter(|name| !ordered.contains(name))
        .collect();
    assert!(unordered.is_empty(), "items no order reaches: {unordered:?}");
}

/// Every flag a marker names is one the emitter can answer.
///
/// An unanswered flag would read as off, and the text behind it would vanish
/// from every generated crate.
#[test]
fn every_runtime_flag_is_answered() {
    let mut ctx = HirCtx::new();
    assert!(to_hir("", FileId(0), &mut ctx).is_ok(), "HIR");
    let Ok(mir) = smelt_mir::lower_hir(&ctx.krate) else {
        panic!("MIR lowering failed");
    };
    let context = EmitContext::new(&mir).expect("an empty program has a context");
    let unanswered: Vec<_> = flag_names()
        .into_iter()
        .filter(|name| flag(&context, name).is_none())
        .collect();
    assert!(unanswered.is_empty(), "flags nothing answers: {unanswered:?}");
}

/// A name with no region is not an item.
#[test]
fn a_missing_runtime_item_is_absent() {
    assert!(
        item_source("smelt_definitely_not_an_item").is_none(),
        "the runtime must not resolve an invented item name"
    );
}

/// The identity counter comes before the list that mints from it, and no marker
/// reaches generated text.
#[test]
fn rendered_text_keeps_order_and_drops_markers() {
    let source = rendered_text(&|name| matches!(name, "object_ids" | "smelt_list"));
    let identity = source
        .find("fn smelt_next_object_id() -> usize {")
        .expect("identity helper is emitted");
    let list = source
        .find("pub struct SmeltList<T> {")
        .expect("list type is emitted");
    assert!(
        identity < list,
        "SmeltList::new calls smelt_next_object_id, so the helper must come first"
    );
    let everything = rendered_text(&|_| true);
    for marker in ["@smelt:", "//~", "/*@if", "/*@end"] {
        assert!(!everything.contains(marker), "`{marker}` reached generated text");
    }
}

/// Emitted runtime item names agree with the shared `runtime_symbols` constants.
///
/// The runtime source spells helper names literally (it is real Rust), while call
/// sites use the constants in `smelt_stdlib::runtime_symbols`. This test is what
/// keeps the two from drifting.
#[test]
fn runtime_manifest_symbol_names_match() {
    let encode_uri = smelt_stdlib::runtime_symbols::strings::ENCODE_URI;
    let source = smelt_runtime::source::item_source(encode_uri)
        .expect("encodeURI helper is named after its runtime_symbols constant");
    assert!(
        source.contains(&format!("fn {encode_uri}(value: &str) -> String {{")),
        "the emitted encodeURI helper must define `{encode_uri}`"
    );
}

/// Every `__smelt_*` key of the runtime text is on the one list, and every entry
/// of the list is used.
///
/// The runtime spells keys literally because it is real Rust. This is what keeps
/// the literal spelling and the emitter's constants from drifting apart.
#[test]
fn every_runtime_marker_is_registered() {
    use smelt_stdlib::runtime_symbols::markers::ALL;
    let mut seen = std::collections::BTreeSet::new();
    for module in smelt_runtime::source::modules() {
        let mut rest = module.source;
        while let Some(at) = rest.find("__smelt_") {
            let tail = rest.get(at..).unwrap_or_default();
            let end = tail
                .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                .unwrap_or(tail.len());
            let name = tail.get(..end).unwrap_or_default();
            if !name.ends_with('_') {
                seen.insert(name);
            }
            rest = tail.get(end..).unwrap_or_default();
        }
    }
    let unregistered: Vec<_> = seen.iter().filter(|name| !ALL.contains(name)).collect();
    assert!(unregistered.is_empty(), "markers with no constant: {unregistered:?}");
    let unused: Vec<_> = ALL.iter().filter(|name| !seen.contains(*name)).collect();
    assert!(unused.is_empty(), "constants no runtime text uses: {unused:?}");
}

/// The host registry's markers are real keys of the runtime.
#[test]
fn every_host_object_marker_is_a_runtime_marker() {
    use smelt_stdlib::runtime_symbols::markers::ALL;
    let missing: Vec<_> = smelt_stdlib::host_object_markers()
        .filter(|marker| !ALL.contains(marker))
        .collect();
    assert!(missing.is_empty(), "registry markers the runtime never spells: {missing:?}");
}
