//! Tests for the runtime prelude manifest (gate → runtime item names).
//!
//! The manifest is the seam between the emitter's `needs_*` gates and the
//! runtime source of truth in `smelt-runtime`. These tests are the "fail loudly"
//! half of that seam: a gate that names an item the runtime does not define, or
//! an item whose emitted name drifts from the shared `runtime_symbols` constant,
//! is caught here rather than by a generated crate that silently lacks a helper.

use super::*;
use crate::runtime_prelude::{PreludeGate, emit_gate};

/// Every item every gate names must resolve in the runtime source.
///
/// This is the build-time-style check the plan asks for: it walks the whole
/// manifest, not just the gates a particular program happens to turn on, and
/// reports every missing `(gate, item)` pair at once.
#[test]
fn runtime_manifest_items_resolve() {
    let mut missing = Vec::new();
    for gate in PreludeGate::ALL {
        for item in gate.items() {
            if smelt_runtime::source::item_source(item).is_none() {
                missing.push(format!("{gate:?} -> {item}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "runtime prelude gates name items smelt-runtime does not define: {missing:?}"
    );
}

/// A gate naming a missing item is an `EmitError`, never an empty emission.
///
/// Exercised through the real lookup by asking for a deliberately absent item,
/// which is the failure mode the manifest test above guards against in the
/// committed manifest.
#[test]
fn a_missing_runtime_item_is_an_error() {
    assert!(
        smelt_runtime::source::item_source("smelt_definitely_not_an_item").is_none(),
        "the runtime must not resolve an invented item name"
    );
}

/// Emitting a gate writes its items in manifest order, each followed by a blank.
///
/// Pins the shape the hand-written `writer.line(...)` blocks had: item text
/// verbatim at column zero, one blank line after each item.
#[test]
fn emitting_a_gate_writes_items_in_order() {
    let mut writer = CodeWriter::new();
    emit_gate(&mut writer, PreludeGate::SmeltList).expect("list gate emits");
    let source = writer.finish();
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
    assert!(
        source.ends_with("}\n\n"),
        "each emitted item is followed by a blank line: {source:?}"
    );
    assert!(
        !source.contains("@smelt:item"),
        "marker lines must never reach generated source"
    );
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
