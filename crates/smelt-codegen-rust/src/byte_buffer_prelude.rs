//! Runtime prelude for the byte-backed host objects.
//!
//! JavaScript's binary-data host objects — `ArrayBuffer`, `SharedArrayBuffer`,
//! Node's `Buffer`, and `DataView` — are modeled as marker-bearing
//! `SmeltUnknown::Object` records that carry their storage in a `bytes` list (see
//! `smelt_stdlib::host_object::ByteBufferRole`). Before this module existed the
//! records were identity-only: `slice(0)` handed the *same* object back,
//! `byteLength` and index reads answered `undefined`, and `new Uint8Array(buf)`
//! panicked. Every clone/equality path in a library that touches binary data
//! (es-toolkit's `clone`, `cloneDeepWith`, `isEqualWith`) walks exactly those
//! operations.
//!
//! The emitted helpers here are the *single* place that knows the record layout.
//! Their names live in `smelt_stdlib::runtime_symbols::byte_buffer` so the
//! definitions below and the emitter call sites cannot drift, and the set of
//! byte-backed markers is read from the shared registry rather than restated.
//!
//! Layout of a byte-backed host record:
//!
//! ```text
//! { "<marker>": true, "bytes": [n, n, ...], "byteLength": N, "length": N? }
//! ```
//!
//! `length` is present only for the view kinds that expose it (Node `Buffer`);
//! `byteLength` is present for every kind. Both are recomputed whenever the
//! helpers mint a new record, so a slice never reports its source's length.

use smelt_stdlib::ByteBufferRole;
use smelt_stdlib::runtime_symbols::byte_buffer as symbols;

use crate::rust::CodeWriter;

/// A Rust array literal of the byte-backed host markers, in registry order.
///
/// Used by the helpers that must recognize "is this record byte-backed?" without
/// caring which kind it is.
fn byte_backed_marker_array() -> String {
    let markers = smelt_stdlib::byte_buffer_host_objects()
        .map(|(marker, _role)| format!("\"{marker}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{markers}]")
}

/// A Rust array literal of the byte-backed markers whose role is
/// [`ByteBufferRole::View`].
///
/// `ArrayBuffer.isView(x)` is `true` for exactly these, which is what makes
/// es-toolkit's `isTypedArray` (`ArrayBuffer.isView(x) && !(x instanceof
/// DataView)`) answer correctly for a Node `Buffer` while staying `false` for the
/// `ArrayBuffer` that backs it.
fn view_marker_array() -> String {
    let markers = smelt_stdlib::byte_buffer_host_objects()
        .filter(|(_marker, role)| *role == ByteBufferRole::View)
        .map(|(marker, _role)| format!("\"{marker}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{markers}]")
}

/// Emit the byte-buffer host-object runtime helpers into the generated prelude.
///
/// The helpers are mutually dependent (slice and indexed access both resolve the
/// record's marker through the same lookup), so they are emitted as one block
/// rather than gated individually. They are small and `dead_code`-allowed like
/// the rest of the prelude.
pub fn emit(writer: &mut CodeWriter) {
    let byte_backed = byte_backed_marker_array();
    let views = view_marker_array();

    writer.line("/// Return the byte-backed host marker a record carries, if any.");
    writer.line("///");
    writer.line("/// The marker doubles as the record's identity, so a slice can rebuild a fresh");
    writer.line("/// record of the *same* host kind rather than degrading to a plain object.");
    writer.line(format!(
        "fn smelt_host_buffer_marker(map: &SmeltObject) -> Option<&'static str> {{ {byte_backed}.into_iter().find(|marker| map.contains_key(marker)) }}"
    ));

    writer.line("/// Return a byte-backed host record's storage as an element vector.");
    writer.line("///");
    writer.line("/// `None` for any other value, so callers can fall through to their existing");
    writer.line("/// array/string/iterator handling. Backs `new Uint8Array(arrayBuffer)`: a typed");
    writer.line("/// array over byte storage sees exactly those bytes.");
    writer.line(format!(
        "fn {elements}(value: &SmeltUnknown) -> Option<Vec<SmeltUnknown>> {{ let SmeltUnknown::Object(map) = value else {{ return None; }}; smelt_host_buffer_marker(map)?; match map.get(\"{bytes}\") {{ Some(SmeltUnknown::Array(values)) => Some(values.into_vec()), _ => Some(Vec::new()) }} }}",
        elements = symbols::ELEMENTS,
        bytes = symbols::BYTES_KEY,
    ));

    writer.line("/// Build a byte-backed host record of `marker` identity over `bytes`.");
    writer.line("///");
    writer.line("/// `byteLength` and `length` are derived from the byte count so a sliced");
    writer.line("/// buffer never reports its source's size. The record gets a fresh object id,");
    writer.line("/// which is what `clone(buf) !== buf` depends on.");
    writer.line(format!(
        "fn smelt_host_buffer_record(marker: &'static str, bytes: Vec<SmeltUnknown>) -> SmeltUnknown {{ let count = bytes.len() as f64; let mut fields = ::std::collections::HashMap::new(); fields.insert(marker.to_owned(), SmeltUnknown::Bool(true)); fields.insert(\"{bytes_key}\".to_owned(), SmeltUnknown::Array(SmeltArray::new(bytes))); fields.insert(\"byteLength\".to_owned(), SmeltUnknown::Number(count)); fields.insert(\"length\".to_owned(), SmeltUnknown::Number(count)); SmeltUnknown::Object(SmeltObject::new(fields)) }}",
        bytes_key = symbols::BYTES_KEY,
    ));

    writer.line("/// Slice a byte-backed host record into a fresh record of the same host kind.");
    writer.line("///");
    writer.line("/// `None` for values that are not byte-backed, so `.slice()`/`.subarray()` on");
    writer.line("/// an erased receiver keeps its array/string behavior. Negative bounds count");
    writer.line("/// back from the end and both bounds clamp, matching");
    writer.line("/// `ArrayBuffer.prototype.slice` and `TypedArray.prototype.subarray`.");
    writer.line(format!(
        "fn {slice}(value: &SmeltUnknown, start: i64, end: Option<i64>) -> Option<SmeltUnknown> {{ let SmeltUnknown::Object(map) = value else {{ return None; }}; let marker = smelt_host_buffer_marker(map)?; let bytes = match map.get(\"{bytes_key}\") {{ Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }}; let len = bytes.len() as i64; let from = (if start < 0 {{ len + start }} else {{ start }}).clamp(0, len); let to = end.map_or(len, |end| if end < 0 {{ len + end }} else {{ end }}).clamp(0, len); let take = to.saturating_sub(from) as usize; Some(smelt_host_buffer_record(marker, bytes.into_iter().skip(from as usize).take(take).collect())) }}",
        slice = symbols::SLICE,
        bytes_key = symbols::BYTES_KEY,
    ));

    writer.line("/// Read one indexed element (`buffer[1]`) of a byte-backed host record.");
    writer.line("///");
    writer.line("/// `None` when the receiver is not byte-backed or the key is not an array");
    writer.line("/// index, so ordinary erased field/index reads are untouched. An in-range");
    writer.line("/// index of a byte buffer is a byte, never `undefined`.");
    writer.line(format!(
        "fn {element}(map: &SmeltObject, key: &str) -> Option<SmeltUnknown> {{ smelt_host_buffer_marker(map)?; let index = key.parse::<usize>().ok()?; match map.get(\"{bytes_key}\") {{ Some(SmeltUnknown::Array(values)) => values.into_vec().get(index).cloned(), _ => None }} }}",
        element = symbols::ELEMENT,
        bytes_key = symbols::BYTES_KEY,
    ));

    writer.line("/// Write one indexed element (`view[i] = byte`) of a byte-backed host record.");
    writer.line("///");
    writer.line("/// Returns whether the write was absorbed by the byte storage; `false` leaves");
    writer.line("/// the caller's ordinary record insert in charge. Writes past the end are");
    writer.line("/// absorbed and dropped, matching a typed array's fixed-length storage.");
    writer.line(format!(
        "fn {set_element}(map: &SmeltObject, key: &str, value: SmeltUnknown) -> bool {{ if smelt_host_buffer_marker(map).is_none() {{ return false; }} let Ok(index) = key.parse::<usize>() else {{ return false; }}; let Some(SmeltUnknown::Array(values)) = map.get(\"{bytes_key}\") else {{ return false; }}; let mut bytes = values.into_vec(); if index < bytes.len() {{ bytes[index] = value; map.insert(\"{bytes_key}\".to_owned(), SmeltUnknown::Array(SmeltArray::new(bytes))); }} true }}",
        set_element = symbols::SET_ELEMENT,
        bytes_key = symbols::BYTES_KEY,
    ));

    writer.line("/// Whether an erased value is a *view* over byte storage (`ArrayBuffer.isView`).");
    writer.line("///");
    writer.line("/// `true` for the view kinds (`DataView`, Node `Buffer`) and `false` for the");
    writer.line("/// storage kinds (`ArrayBuffer`, `SharedArrayBuffer`), exactly as the platform");
    writer.line("/// predicate answers.");
    writer.line(format!(
        "fn {is_view}(value: &SmeltUnknown) -> bool {{ let SmeltUnknown::Object(map) = value else {{ return false; }}; {views}.into_iter().any(|marker| map.contains_key(marker)) }}",
        is_view = symbols::IS_VIEW,
    ));
}
