//! Runtime prelude for the byte-backed host objects.
//!
//! JavaScript's binary-data host objects — `ArrayBuffer`, `SharedArrayBuffer`,
//! the eleven `TypedArray` views, Node's `Buffer`, and `DataView` — are modeled as
//! marker-bearing `SmeltUnknown::Object` records that carry their storage in a
//! `bytes` list (see `smelt_stdlib::host_object::ByteBufferRole`). Before this
//! module existed the records were identity-only: `slice(0)` handed the *same*
//! object back, `byteLength` and index reads answered `undefined`, and
//! `new Uint8Array(buf)` panicked. Every clone/equality path in a library that
//! touches binary data (es-toolkit's `clone`, `cloneDeepWith`, `isEqualWith`)
//! walks exactly those operations.
//!
//! ## Element typing
//!
//! A typed array is *bytes plus an element type*, and the element type is not
//! decoration: the same eight bytes are two `Float32Array` elements or one
//! `Float64Array` element, and the same byte `0xff` reads as `255` through a
//! `Uint8Array` and `-1` through an `Int8Array`. So each byte-backed marker that
//! names a view carries a `TypedArrayElement` in the shared registry, and the
//! helpers below derive one little-endian decode/encode pair per element type from
//! it. That single fact drives everything observable:
//!
//! * `length` is `byteLength / BYTES_PER_ELEMENT`, so a `Float64Array` over eight
//!   bytes has one element where a `Uint8Array` has eight;
//! * an indexed read decodes at the element's width and signedness, and an
//!   indexed write encodes back into exactly those bytes;
//! * `slice`/`subarray` bounds are *element* indices for a view and *byte*
//!   indices for byte-addressed storage, which is the same code once the stride
//!   comes from the registry;
//! * `new Float32Array(arrayBuffer)` re-*views* the bytes while
//!   `new Float32Array(uint8View)` *converts* the elements — the two JavaScript
//!   constructor behaviors that a shapeless byte copy cannot tell apart.
//!
//! The emitted helpers here are the *single* place that knows the record layout.
//! Their names live in `smelt_stdlib::runtime_symbols::byte_buffer` so the
//! definitions below and the emitter call sites cannot drift, and both the set of
//! byte-backed markers and their element types are read from the shared registry
//! rather than restated.
//!
//! Layout of a byte-backed host record:
//!
//! ```text
//! { "<marker>": true, "bytes": [b, b, ...], "byteLength": N,
//!   "length": N / width, "byteOffset": off?, "buffer": <storage record>? }
//! ```
//!
//! `byteOffset` and `buffer` are present for the element-typed views, which in
//! JavaScript are always a window onto an `ArrayBuffer`. `length` and `byteLength`
//! are recomputed whenever the helpers mint a new record, so a slice never reports
//! its source's size.
//!
//! ## Deliberate limit: no write-through aliasing
//!
//! Two views over one `ArrayBuffer` share the buffer *record* (so
//! `view.buffer === buffer` holds and clone specs that assert it pass), but each
//! view keeps its own byte window, so a write through one view is not observed
//! through the other. Smelt models these records by value; making the window a
//! borrow of the storage record is a separate change to record identity, not to
//! element typing, and nothing in the measured corpora reads a byte written
//! through an aliasing view.

use smelt_stdlib::runtime_symbols::byte_buffer as symbols;
use smelt_stdlib::{ByteBufferRole, TypedArrayElement};

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
/// DataView)`) answer correctly for a typed array or a Node `Buffer` while staying
/// `false` for the `ArrayBuffer` that backs them.
fn view_marker_array() -> String {
    let markers = smelt_stdlib::byte_buffer_host_objects()
        .filter(|(_marker, role)| *role == ByteBufferRole::View)
        .map(|(marker, _role)| format!("\"{marker}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{markers}]")
}

/// A Rust array literal of `(marker, element_tag, byte_width)` for every
/// element-typed view.
///
/// This is the whole element-typing table the generated codec dispatches on:
/// given a record's own marker it yields the element tag that selects the
/// decode/encode arm and the stride that separates `length` from `byteLength`.
fn element_kind_table() -> String {
    let entries = smelt_stdlib::typed_array_host_objects()
        .map(|(marker, element)| {
            format!(
                "(\"{marker}\", \"{tag}\", {width})",
                tag = element.tag(),
                width = element.byte_width(),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{entries}]")
}

/// The Rust expression decoding one element of `element` out of `raw: [u8; 8]`.
///
/// Little-endian, matching every platform Smelt targets and the byte order every
/// `TypedArray` uses on those platforms. `BigInt64Array`/`BigUint64Array` widen
/// to `f64` because Smelt models a JavaScript `BigInt` as a number; that loses
/// precision above 2^53 exactly as a `Number(bigint)` conversion would.
fn decode_expression(element: TypedArrayElement) -> &'static str {
    match element {
        TypedArrayElement::Int8 => "i8::from_le_bytes([raw[0]]) as f64",
        TypedArrayElement::Uint8 | TypedArrayElement::Uint8Clamped => "raw[0] as f64",
        TypedArrayElement::Int16 => "i16::from_le_bytes([raw[0], raw[1]]) as f64",
        TypedArrayElement::Uint16 => "u16::from_le_bytes([raw[0], raw[1]]) as f64",
        TypedArrayElement::Int32 => "i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64",
        TypedArrayElement::Uint32 => "u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64",
        TypedArrayElement::Float32 => "f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64",
        TypedArrayElement::Float64 => "f64::from_le_bytes(raw)",
        TypedArrayElement::BigInt64 => "i64::from_le_bytes(raw) as f64",
        TypedArrayElement::BigUint64 => "u64::from_le_bytes(raw) as f64",
    }
}

/// The Rust expression encoding `number: f64` into a `Vec<u8>` of `element`.
///
/// Integer views wrap modulo their width, which is what ECMAScript's
/// `ToInt8`/`ToUint16`/... conversions do and why `new Int8Array([-1]).buffer`
/// holds the byte `255` — the fact es-toolkit's "should compare array buffers"
/// spec asserts by comparing that buffer against `new Uint8Array([255]).buffer`.
/// `Uint8ClampedArray` is the one exception: it *saturates* and rounds half to
/// even.
fn encode_expression(element: TypedArrayElement) -> &'static str {
    match element {
        TypedArrayElement::Int8 => "vec![smelt_host_buffer_to_integer(number) as i8 as u8]",
        TypedArrayElement::Uint8 => "vec![smelt_host_buffer_to_integer(number) as u8]",
        TypedArrayElement::Uint8Clamped => {
            "vec![if number.is_nan() { 0u8 } else { number.round_ties_even().clamp(0.0, 255.0) as u8 }]"
        }
        TypedArrayElement::Int16 => {
            "(smelt_host_buffer_to_integer(number) as i16).to_le_bytes().to_vec()"
        }
        TypedArrayElement::Uint16 => {
            "(smelt_host_buffer_to_integer(number) as u16).to_le_bytes().to_vec()"
        }
        TypedArrayElement::Int32 => {
            "(smelt_host_buffer_to_integer(number) as i32).to_le_bytes().to_vec()"
        }
        TypedArrayElement::Uint32 => {
            "(smelt_host_buffer_to_integer(number) as u32).to_le_bytes().to_vec()"
        }
        TypedArrayElement::Float32 => "(number as f32).to_le_bytes().to_vec()",
        TypedArrayElement::Float64 => "number.to_le_bytes().to_vec()",
        TypedArrayElement::BigInt64 => {
            "smelt_host_buffer_to_integer(number).to_le_bytes().to_vec()"
        }
        TypedArrayElement::BigUint64 => {
            "(smelt_host_buffer_to_integer(number) as u64).to_le_bytes().to_vec()"
        }
    }
}

/// Render a `match` over element tags, one arm per element type in the registry.
///
/// `arm` maps an element type to the arm body. Element tags are deduplicated, so
/// Node `Buffer` sharing `Uint8`'s tag emits one arm rather than two. The
/// fallback arm keeps byte-addressed storage (`ArrayBuffer`, `DataView`) working
/// through the same helper with a stride of one.
fn element_match(arm: impl Fn(TypedArrayElement) -> &'static str, fallback: &str) -> String {
    let mut seen = Vec::new();
    let mut arms = Vec::new();
    for (_marker, element) in smelt_stdlib::typed_array_host_objects() {
        if seen.contains(&element.tag()) {
            continue;
        }
        seen.push(element.tag());
        arms.push(format!("\"{tag}\" => {body}", tag = element.tag(), body = arm(element)));
    }
    arms.push(format!("_ => {fallback}"));
    arms.join(", ")
}

/// Emit the byte-buffer host-object runtime helpers into the generated prelude.
///
/// The helpers are mutually dependent (slice, indexed access and construction all
/// resolve the record's marker and element stride through the same lookups), so
/// they are emitted as one block rather than gated individually. They are small
/// and `dead_code`-allowed like the rest of the prelude.
pub fn emit(writer: &mut CodeWriter) {
    emit_identity(writer);
    emit_element_codec(writer);
    emit_record(writer);
    emit_access(writer);
    emit_construct(writer);
}

/// Emit the marker lookup and the `ArrayBuffer.isView` predicate.
fn emit_identity(writer: &mut CodeWriter) {
    let byte_backed = byte_backed_marker_array();
    let views = view_marker_array();

    writer.line("/// Return the byte-backed host marker a record carries, if any.");
    writer.line("///");
    writer.line("/// The marker doubles as the record's identity, so a slice can rebuild a fresh");
    writer.line("/// record of the *same* host kind rather than degrading to a plain object, and");
    writer.line("/// the element codec can find the record's element type without being told.");
    writer.line(format!(
        "fn smelt_host_buffer_marker(map: &SmeltObject) -> Option<&'static str> {{ {byte_backed}.into_iter().find(|marker| map.contains_key(marker)) }}"
    ));

    writer.line("/// Whether an erased value is a *view* over byte storage (`ArrayBuffer.isView`).");
    writer.line("///");
    writer.line("/// `true` for the view kinds (the typed arrays, `DataView`, Node `Buffer`) and");
    writer.line("/// `false` for the storage kinds (`ArrayBuffer`, `SharedArrayBuffer`), exactly");
    writer.line("/// as the platform predicate answers.");
    writer.line(format!(
        "fn {is_view}(value: &SmeltUnknown) -> bool {{ let SmeltUnknown::Object(map) = value else {{ return false; }}; {views}.into_iter().any(|marker| map.contains_key(marker)) }}",
        is_view = symbols::IS_VIEW,
    ));

    writer.line("/// Whether a byte-backed marker names a *view* rather than byte storage.");
    writer.line("///");
    writer.line("/// A view in JavaScript is always a window onto an `ArrayBuffer`, so this is");
    writer.line("/// what decides whether a freshly built record also gets `buffer`/`byteOffset`.");
    writer.line(format!(
        "fn smelt_host_buffer_is_view_marker(marker: &str) -> bool {{ {views}.into_iter().any(|entry| entry == marker) }}"
    ));
}

/// Emit the element-type table and its little-endian decode/encode pair.
fn emit_element_codec(writer: &mut CodeWriter) {
    let table = element_kind_table();
    let decode_arms = element_match(decode_expression, "raw[0] as f64");
    let encode_arms = element_match(
        encode_expression,
        "vec![smelt_host_buffer_to_integer(number) as u8]",
    );

    writer.line("/// The element tag and byte stride of a byte-backed marker's view.");
    writer.line("///");
    writer.line("/// `None` for the byte-addressed kinds (`ArrayBuffer`, `SharedArrayBuffer`,");
    writer.line("/// `DataView`), whose bytes carry no single element type; callers then treat");
    writer.line("/// the storage as a stride-one byte sequence.");
    writer.line(format!(
        "fn smelt_host_buffer_element_kind(marker: &str) -> Option<(&'static str, usize)> {{ {table}.into_iter().find(|(entry, _, _)| *entry == marker).map(|(_, tag, width)| (tag, width)) }}"
    ));

    writer.line("/// The byte stride of a byte-backed marker: its `BYTES_PER_ELEMENT`, or 1.");
    writer.line("fn smelt_host_buffer_stride(marker: &str) -> usize { smelt_host_buffer_element_kind(marker).map_or(1, |(_, width)| width) }");

    writer.line("/// Read one raw byte out of a `bytes` storage list, zero past the end.");
    writer.line("fn smelt_host_buffer_raw_byte(bytes: &[SmeltUnknown], index: usize) -> u8 { match bytes.get(index) { Some(SmeltUnknown::Number(value)) if value.is_finite() => *value as i64 as u8, _ => 0 } }");

    writer.line("/// Coerce a JavaScript number to the integer an integer view stores.");
    writer.line("///");
    writer.line("/// Truncates toward zero and maps the non-finite values to `0`, matching");
    writer.line("/// ECMAScript `ToIntegerOrInfinity` followed by the modulo-2^n wrap that the");
    writer.line("/// caller's `as i8`/`as u16`/... cast performs.");
    writer.line("fn smelt_host_buffer_to_integer(value: f64) -> i64 { if value.is_finite() { value.trunc() as i64 } else { 0 } }");

    writer.line("/// Decode the element of type `kind` starting at byte `offset`.");
    writer.line("///");
    writer.line("/// Little-endian, and signed for the signed views: byte `0xff` decodes to");
    writer.line("/// `255` through `uint8` and `-1` through `int8`. Bytes past the end read as");
    writer.line("/// zero, so a truncated final element decodes rather than panicking.");
    writer.line(format!(
        "fn smelt_host_buffer_decode_element(kind: &str, bytes: &[SmeltUnknown], offset: usize) -> SmeltUnknown {{ let mut raw = [0u8; 8]; let width = smelt_host_buffer_kind_width(kind); for index in 0..width {{ raw[index] = smelt_host_buffer_raw_byte(bytes, offset + index); }} SmeltUnknown::Number(match kind {{ {decode_arms} }}) }}"
    ));

    let width_arms = element_match(
        |element| match element.byte_width() {
            1 => "1",
            2 => "2",
            4 => "4",
            _ => "8",
        },
        "1",
    );
    writer.line("/// The byte width of an element tag, for the decode buffer above.");
    writer.line(format!(
        "fn smelt_host_buffer_kind_width(kind: &str) -> usize {{ match kind {{ {width_arms} }} }}"
    ));

    writer.line("/// Encode one JavaScript value as the bytes of an element of type `kind`.");
    writer.line("///");
    writer.line("/// Integer views wrap modulo their width (ECMAScript `ToInt8`/`ToUint16`/...),");
    writer.line("/// which is why an `Int8Array` holding `-1` and a `Uint8Array` holding `255`");
    writer.line("/// have byte-identical buffers. `Uint8ClampedArray` saturates instead.");
    writer.line(format!(
        "fn smelt_host_buffer_encode_element(kind: &str, value: &SmeltUnknown) -> Vec<SmeltUnknown> {{ let number = match value {{ SmeltUnknown::Number(value) => *value, SmeltUnknown::Bool(value) => if *value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::String(text) => text.trim().parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Null => 0.0, _ => f64::NAN }}; let raw: Vec<u8> = match kind {{ {encode_arms} }}; raw.into_iter().map(|byte| SmeltUnknown::Number(f64::from(byte))).collect() }}"
    ));
}

/// Emit the record constructor and the element/byte extraction helpers.
fn emit_record(writer: &mut CodeWriter) {
    writer.line("/// Build a byte-backed host record of `marker` identity over `bytes`.");
    writer.line("///");
    writer.line("/// `byteLength` is the byte count and `length` is the *element* count, so a");
    writer.line("/// `Float64Array` over eight bytes reports length 1 while a `Uint8Array` over");
    writer.line("/// the same bytes reports 8. An element-typed view is always a window onto an");
    writer.line("/// `ArrayBuffer` in JavaScript, so it also gets `byteOffset` and a `buffer`");
    writer.line("/// storage record. The record gets a fresh object id, which is what");
    writer.line("/// `clone(buf) !== buf` depends on.");
    writer.line(
        "fn smelt_host_buffer_record(marker: &'static str, bytes: Vec<SmeltUnknown>) -> SmeltUnknown { let buffer = smelt_host_buffer_is_view_marker(marker).then(|| smelt_host_buffer_storage_record(bytes.clone())); smelt_host_buffer_view_record(marker, bytes, buffer, 0) }",
    );

    writer.line("/// Build a fresh `ArrayBuffer` record over `bytes`.");
    writer.line("///");
    writer.line("/// Backs `typedArray.buffer` for a view that was not built from an existing");
    writer.line("/// buffer (`new Uint8Array([1, 2, 3]).buffer`). `ArrayBuffer` is byte-addressed,");
    writer.line("/// so this never recurses into a nested buffer of its own.");
    writer.line("fn smelt_host_buffer_storage_record(bytes: Vec<SmeltUnknown>) -> SmeltUnknown { smelt_host_buffer_view_record(\"__smelt_arraybuffer\", bytes, None, 0) }");

    writer.line("/// Build a byte-backed host record, optionally sharing an existing `buffer`.");
    writer.line("///");
    writer.line("/// Passing the *same* storage record two views were built from is what makes");
    writer.line("/// `view.buffer === buffer` hold, which lodash-compatible clone specs assert on");
    writer.line("/// a cloned view.");
    writer.line(format!(
        "fn smelt_host_buffer_view_record(marker: &'static str, bytes: Vec<SmeltUnknown>, buffer: Option<SmeltUnknown>, byte_offset: usize) -> SmeltUnknown {{ let byte_length = bytes.len(); let stride = smelt_host_buffer_stride(marker); let mut fields = ::std::collections::HashMap::new(); fields.insert(marker.to_owned(), SmeltUnknown::Bool(true)); fields.insert(\"{bytes_key}\".to_owned(), SmeltUnknown::Array(SmeltArray::new(bytes))); fields.insert(\"byteLength\".to_owned(), SmeltUnknown::Number(byte_length as f64)); fields.insert(\"length\".to_owned(), SmeltUnknown::Number((byte_length / stride) as f64)); if let Some(buffer) = buffer {{ fields.insert(\"buffer\".to_owned(), buffer); fields.insert(\"byteOffset\".to_owned(), SmeltUnknown::Number(byte_offset as f64)); }} SmeltUnknown::Object(SmeltObject::new(fields)) }}",
        bytes_key = symbols::BYTES_KEY,
    ));

    writer.line("/// Return a byte-backed host record's raw storage bytes, or `None`.");
    writer.line("///");
    writer.line("/// This is the *byte* view of the record, which is what");
    writer.line("/// `new Float32Array(arrayBuffer)` re-interprets and what `DataView` windows.");
    writer.line("fn smelt_host_buffer_raw_bytes(value: &SmeltUnknown) -> Option<Vec<SmeltUnknown>> { let SmeltUnknown::Object(map) = value else { return None; }; smelt_host_buffer_marker(map)?; Some(match map.get(\"bytes\") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }) }");

    writer.line("/// Return a byte-backed host record's storage decoded as its *elements*.");
    writer.line("///");
    writer.line("/// `None` for any other value, so callers can fall through to their existing");
    writer.line("/// array/string/iterator handling. This is the array-like face of a typed");
    writer.line("/// array: iteration, spread and `Array.from` all see decoded elements at the");
    writer.line("/// view's own width, not its raw bytes. Byte-addressed storage decodes as its");
    writer.line("/// bytes, which is what `new Uint8Array(dataView)` wants.");
    writer.line(format!(
        "fn {elements}(value: &SmeltUnknown) -> Option<Vec<SmeltUnknown>> {{ let SmeltUnknown::Object(map) = value else {{ return None; }}; let marker = smelt_host_buffer_marker(map)?; let bytes = smelt_host_buffer_raw_bytes(value)?; let Some((kind, width)) = smelt_host_buffer_element_kind(marker) else {{ return Some(bytes); }}; Some((0..bytes.len() / width).map(|index| smelt_host_buffer_decode_element(kind, &bytes, index * width)).collect()) }}",
        elements = symbols::ELEMENTS,
    ));

    writer.line("/// A byte-backed host record's own enumerable keys: its element indices.");
    writer.line("///");
    writer.line("/// `None` for values that are not byte-backed. A typed array's own properties");
    writer.line("/// are exactly its indexed elements — `length`, `byteLength`, `byteOffset` and");
    writer.line("/// `buffer` are prototype accessors and never enumerate — so");
    writer.line("/// `Object.keys(new Uint8Array(1))` is `['0']` and a deep-equality walk over");
    writer.line("/// two views compares elements rather than internal storage keys.");
    writer.line(format!(
        "fn {index_keys}(value: &SmeltUnknown) -> Option<Vec<String>> {{ let count = {elements}(value)?.len(); Some((0..count).map(|index| index.to_string()).collect()) }}",
        index_keys = symbols::INDEX_KEYS,
        elements = symbols::ELEMENTS,
    ));

    writer.line("/// The same own-key set, for a byte-backed record reached through the");
    writer.line("/// structural `SmeltRecord` ABI rather than as a tagged `SmeltUnknown`.");
    writer.line("///");
    writer.line("/// An erased object crossing into a `Record<string, unknown>` parameter is");
    writer.line("/// re-materialized as a `SmeltRecord`, so `Object.keys` on that side needs the");
    writer.line("/// same index-key answer — remeda's `isShallowEqual` reads exactly that path.");
    writer.line(format!(
        "fn {record_index_keys}(record: &SmeltRecord<String, SmeltUnknown>) -> Option<Vec<String>> {{ {index_keys}(&SmeltUnknown::Object(SmeltObject::from_unknown_record(record.clone()))) }}",
        record_index_keys = symbols::RECORD_INDEX_KEYS,
        index_keys = symbols::INDEX_KEYS,
    ));

    writer.line("/// The decoded elements of a byte-backed record reached through the");
    writer.line("/// structural `SmeltRecord` ABI. Backs `Object.values`/`Object.entries` over a");
    writer.line("/// view, which pair with the index keys above.");
    writer.line(format!(
        "fn {record_elements}(record: &SmeltRecord<String, SmeltUnknown>) -> Option<Vec<SmeltUnknown>> {{ {elements}(&SmeltUnknown::Object(SmeltObject::from_unknown_record(record.clone()))) }}",
        record_elements = symbols::RECORD_ELEMENTS,
        elements = symbols::ELEMENTS,
    ));
}

/// Emit slice and indexed element read/write.
fn emit_access(writer: &mut CodeWriter) {
    writer.line("/// Slice a byte-backed host record into a fresh record of the same host kind.");
    writer.line("///");
    writer.line("/// `None` for values that are not byte-backed, so `.slice()`/`.subarray()` on");
    writer.line("/// an erased receiver keeps its array/string behavior. Bounds are *element*");
    writer.line("/// indices for an element-typed view and byte indices for byte-addressed");
    writer.line("/// storage — one code path once the stride comes from the registry. Negative");
    writer.line("/// bounds count back from the end and both bounds clamp, matching");
    writer.line("/// `ArrayBuffer.prototype.slice` and `TypedArray.prototype.subarray`.");
    writer.line(format!(
        "fn {slice}(value: &SmeltUnknown, start: i64, end: Option<i64>) -> Option<SmeltUnknown> {{ let SmeltUnknown::Object(map) = value else {{ return None; }}; let marker = smelt_host_buffer_marker(map)?; let bytes = smelt_host_buffer_raw_bytes(value)?; let stride = smelt_host_buffer_stride(marker); let len = (bytes.len() / stride) as i64; let from = (if start < 0 {{ len + start }} else {{ start }}).clamp(0, len); let to = end.map_or(len, |end| if end < 0 {{ len + end }} else {{ end }}).clamp(0, len); let take = to.saturating_sub(from) as usize; Some(smelt_host_buffer_record(marker, bytes.into_iter().skip(from as usize * stride).take(take * stride).collect())) }}",
        slice = symbols::SLICE,
    ));

    writer.line("/// Read one indexed element (`view[1]`) of a byte-backed host record.");
    writer.line("///");
    writer.line("/// `None` when the receiver is not byte-backed or the key is not an array");
    writer.line("/// index, so ordinary erased field/index reads are untouched. An in-range");
    writer.line("/// index of a view decodes at the element's own width and signedness; past the");
    writer.line("/// end it answers `undefined`, as a typed array does.");
    writer.line(format!(
        "fn {element}(map: &SmeltObject, key: &str) -> Option<SmeltUnknown> {{ let marker = smelt_host_buffer_marker(map)?; let index = key.parse::<usize>().ok()?; let bytes = match map.get(\"{bytes_key}\") {{ Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }}; let (kind, width) = smelt_host_buffer_element_kind(marker).unwrap_or((\"uint8\", 1)); if (index + 1) * width > bytes.len() {{ return None; }} Some(smelt_host_buffer_decode_element(kind, &bytes, index * width)) }}",
        element = symbols::ELEMENT,
        bytes_key = symbols::BYTES_KEY,
    ));

    writer.line("/// Write one indexed element (`view[i] = value`) of a byte-backed host record.");
    writer.line("///");
    writer.line("/// Returns whether the write was absorbed by the byte storage; `false` leaves");
    writer.line("/// the caller's ordinary record insert in charge. The value is encoded into");
    writer.line("/// exactly the element's bytes, so `int8View[0] = -1` stores the byte `255`.");
    writer.line("/// Writes past the end are absorbed and dropped, matching a typed array's");
    writer.line("/// fixed-length storage. The write also lands in the view's backing `buffer`");
    writer.line("/// record *in place*, so `view[0] = 1` is visible through `view.buffer` and the");
    writer.line("/// buffer keeps its object identity (`view.buffer === buffer` still holds).");
    writer.line(format!(
        "fn {set_element}(map: &SmeltObject, key: &str, value: SmeltUnknown) -> bool {{ let Some(marker) = smelt_host_buffer_marker(map) else {{ return false; }}; let Ok(index) = key.parse::<usize>() else {{ return false; }}; let Some(SmeltUnknown::Array(values)) = map.get(\"{bytes_key}\") else {{ return false; }}; let mut bytes = values.into_vec(); let (kind, width) = smelt_host_buffer_element_kind(marker).unwrap_or((\"uint8\", 1)); let offset = index * width; if offset + width <= bytes.len() {{ let encoded = smelt_host_buffer_encode_element(kind, &value); for (step, byte) in encoded.iter().enumerate() {{ bytes[offset + step] = byte.clone(); }} map.insert(\"{bytes_key}\".to_owned(), SmeltUnknown::Array(SmeltArray::new(bytes))); smelt_host_buffer_write_through(map, offset, &encoded); }} true }}",
        set_element = symbols::SET_ELEMENT,
        bytes_key = symbols::BYTES_KEY,
    ));

    writer.line("/// Mirror an element write into the view's backing `buffer` storage record.");
    writer.line("///");
    writer.line("/// A typed array is a window onto an `ArrayBuffer`, so a write through the view");
    writer.line("/// is a write to the buffer — `new Uint8Array(buf)[0] = 1` then shows up in");
    writer.line("/// `buf`. `SmeltObject` has interior mutability, so the storage record is updated");
    writer.line("/// in place rather than replaced: replacing it would mint a new object id and");
    writer.line("/// break `view.buffer === buf`. `byteOffset` places the window inside the");
    writer.line("/// buffer. A no-op for the byte-addressed kinds, which have no backing buffer.");
    writer.line(format!(
        "fn smelt_host_buffer_write_through(map: &SmeltObject, offset: usize, encoded: &[SmeltUnknown]) {{ let Some(SmeltUnknown::Object(storage)) = map.get(\"buffer\") else {{ return; }}; let base = match map.get(\"byteOffset\") {{ Some(SmeltUnknown::Number(value)) if value >= 0.0 => value as usize, _ => 0 }}; let Some(SmeltUnknown::Array(values)) = storage.get(\"{bytes_key}\") else {{ return; }}; let mut bytes = values.into_vec(); for (step, byte) in encoded.iter().enumerate() {{ let at = base + offset + step; if at < bytes.len() {{ bytes[at] = byte.clone(); }} }} storage.insert(\"{bytes_key}\".to_owned(), SmeltUnknown::Array(SmeltArray::new(bytes))); }}",
        bytes_key = symbols::BYTES_KEY,
    ));
}

/// Emit the shared byte-buffer constructor.
fn emit_construct(writer: &mut CodeWriter) {
    let storage_markers = smelt_stdlib::byte_buffer_host_objects()
        .filter(|(_marker, role)| *role == ByteBufferRole::Storage)
        .map(|(marker, _role)| format!("\"{marker}\""))
        .collect::<Vec<_>>()
        .join(", ");

    writer.line("/// Whether an erased value is byte *storage* (an `ArrayBuffer`), not a view.");
    writer.line("///");
    writer.line("/// This is the distinction that decides what `new Float32Array(source)` means:");
    writer.line("/// over storage it re-*views* the bytes (eight bytes become two elements), over");
    writer.line("/// a view or an array it *converts* the elements one by one.");
    writer.line(format!(
        "fn smelt_host_buffer_is_storage(value: &SmeltUnknown) -> bool {{ let SmeltUnknown::Object(map) = value else {{ return false; }}; [{storage_markers}].into_iter().any(|marker| map.contains_key(marker)) }}"
    ));

    writer.line("/// Read an optional non-negative numeric constructor argument.");
    writer.line("fn smelt_host_buffer_count_argument(argument: Option<&SmeltUnknown>) -> Option<usize> { match argument { Some(SmeltUnknown::Number(value)) if *value >= 0.0 => Some(*value as usize), _ => None } }");

    writer.line("/// Build a byte-backed host record of `marker` identity from `new X(...)` args.");
    writer.line("///");
    writer.line("/// The four JavaScript constructor forms, all resolved from the registry rather");
    writer.line("/// than per call site:");
    writer.line("///");
    writer.line("/// * `new X(n)` — `n` *elements* of zeroes (`new Float64Array(2)` is 16 bytes);");
    writer.line("/// * `new X(arrayBuffer[, byteOffset[, length]])` — a **view** over that");
    writer.line("///   storage's bytes, sharing the buffer record so `view.buffer === buffer`;");
    writer.line("/// * `new X(otherView)` / `new X([...])` — an element-by-element **conversion**,");
    writer.line("///   so `new Uint8Array(new Int8Array([-1]))` holds `255`;");
    writer.line("/// * `new X()` — empty storage.");
    writer.line("///");
    writer.line("/// Byte-addressed targets (`ArrayBuffer`, `SharedArrayBuffer`) have stride one");
    writer.line("/// and no element codec, so the same code gives them their platform behavior.");
    writer.line(format!(
        "fn {construct}(marker: &'static str, args: Vec<SmeltUnknown>) -> SmeltUnknown {{",
        construct = symbols::CONSTRUCT,
    ));
    writer.line("    let element = smelt_host_buffer_element_kind(marker);");
    writer.line("    let stride = element.map_or(1, |(_, width)| width);");
    writer.line("    let source = args.first();");
    writer.line("    match source {");
    // `new X(arrayBuffer, byteOffset, length)`: a byte window over shared storage.
    writer.line("        Some(value) if smelt_host_buffer_is_storage(value) => { let storage = smelt_host_buffer_raw_bytes(value).unwrap_or_default(); let offset = smelt_host_buffer_count_argument(args.get(1)).unwrap_or(0).min(storage.len()); let available = storage.len() - offset; let take = smelt_host_buffer_count_argument(args.get(2)).map_or(available, |count| (count * stride).min(available)); let window = storage.into_iter().skip(offset).take(take).collect::<Vec<_>>(); let buffer = smelt_host_buffer_is_view_marker(marker).then(|| value.clone()); smelt_host_buffer_view_record(marker, window, buffer, offset) }");
    // `new X(otherView)` / `new X([...])`: element-by-element conversion.
    writer.line(format!(
        "        Some(value) => {{ let source_elements = match value {{ SmeltUnknown::Array(values) => Some(values.clone().into_vec()), _ => {elements}(value) }}; match source_elements {{ Some(source_elements) => match element {{ Some((kind, _)) => smelt_host_buffer_record(marker, source_elements.iter().flat_map(|item| smelt_host_buffer_encode_element(kind, item)).collect()), None => smelt_host_buffer_record(marker, source_elements) }}, None => match smelt_host_buffer_count_argument(Some(value)) {{ Some(count) => smelt_host_buffer_record(marker, vec![SmeltUnknown::Number(0.0); count * stride]), None => smelt_host_buffer_record(marker, Vec::new()) }} }} }}",
        elements = symbols::ELEMENTS,
    ));
    writer.line("        None => smelt_host_buffer_record(marker, Vec::new()),");
    writer.line("    }");
    writer.line("}");
}
