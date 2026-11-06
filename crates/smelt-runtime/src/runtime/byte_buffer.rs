// Runtime prelude for the byte-backed host objects.
//
// JavaScript's binary-data host objects — `ArrayBuffer`, `SharedArrayBuffer`,
// the eleven `TypedArray` views, Node's `Buffer`, and `DataView` — are modeled as
// marker-bearing `SmeltUnknown::Object` records that carry their storage in a
// `bytes` list (see `smelt_stdlib::host_object::ByteBufferRole`). Before this
// module existed the records were identity-only: `slice(0)` handed the *same*
// object back, `byteLength` and index reads answered `undefined`, and
// `new Uint8Array(buf)` panicked. Every clone/equality path in a library that
// touches binary data (es-toolkit's `clone`, `cloneDeepWith`, `isEqualWith`)
// walks exactly those operations.
//
// ## Element typing
//
// A typed array is *bytes plus an element type*, and the element type is not
// decoration: the same eight bytes are two `Float32Array` elements or one
// `Float64Array` element, and the same byte `0xff` reads as `255` through a
// `Uint8Array` and `-1` through an `Int8Array`. So each byte-backed marker that
// names a view carries a `TypedArrayElement` in the shared registry, and the
// helpers below derive one little-endian decode/encode pair per element type from
// it. That single fact drives everything observable:
//
// * `length` is `byteLength / BYTES_PER_ELEMENT`, so a `Float64Array` over eight
//   bytes has one element where a `Uint8Array` has eight;
// * an indexed read decodes at the element's width and signedness, and an
//   indexed write encodes back into exactly those bytes;
// * `slice`/`subarray` bounds are *element* indices for a view and *byte*
//   indices for byte-addressed storage, which is the same code once the stride
//   comes from the registry;
// * `new Float32Array(arrayBuffer)` re-*views* the bytes while
//   `new Float32Array(uint8View)` *converts* the elements — the two JavaScript
//   constructor behaviors that a shapeless byte copy cannot tell apart.
//
// The emitted helpers here are the *single* place that knows the record layout.
// Their names live in `smelt_stdlib::runtime_symbols::byte_buffer` so the
// definitions below and the emitter call sites cannot drift, and both the set of
// byte-backed markers and their element types are read from the shared registry
// rather than restated.
//
// Layout of a byte-backed host record:
//
// ```text
// { "<marker>": true, "bytes": [b, b, ...], "byteLength": N,
//   "length": N / width, "byteOffset": off?, "buffer": <storage record>? }
// ```
//
// `byteOffset` and `buffer` are present for the element-typed views, which in
// JavaScript are always a window onto an `ArrayBuffer`. `length` and `byteLength`
// are recomputed whenever the helpers mint a new record, so a slice never reports
// its source's size.
//
// ## Deliberate limit: sibling views do not share a byte window
//
// Two views over one `ArrayBuffer` share the buffer *record* (so
// `view.buffer === buffer` holds and clone specs that assert it pass), but each
// view keeps its own byte window, so a write through one view is not observed
// through the other. Smelt models these records by value; making the window a
// borrow of the storage record is a separate change to record identity, not to
// element typing, and nothing in the measured corpora reads a byte written
// through an aliasing view.

// Emit the marker lookup and the `ArrayBuffer.isView` predicate.
// @smelt:item smelt_host_buffer_marker when unknown
/// Return the byte-backed host marker a record carries, if any.
///
/// The marker doubles as the record's identity, so a slice can rebuild a fresh
/// record of the *same* host kind rather than degrading to a plain object, and
/// the element codec can find the record's element type without being told.
fn smelt_host_buffer_marker(map: &SmeltObject) -> Option<&'static str> { if !map.store.borrow().has_internal_keys() { return None; } ["__smelt_arraybuffer", "__smelt_sharedarraybuffer", "__smelt_int8array", "__smelt_uint8array", "__smelt_uint8clampedarray", "__smelt_int16array", "__smelt_uint16array", "__smelt_int32array", "__smelt_uint32array", "__smelt_float32array", "__smelt_float64array", "__smelt_bigint64array", "__smelt_biguint64array", "__smelt_buffer", "__smelt_dataview"].into_iter().find(|marker| map.contains_key(marker)) }
/// Whether an erased value is a *view* over byte storage (`ArrayBuffer.isView`).
///
/// `true` for the view kinds (the typed arrays, `DataView`, Node `Buffer`) and
/// `false` for the storage kinds (`ArrayBuffer`, `SharedArrayBuffer`), exactly
/// as the platform predicate answers.
fn smelt_host_buffer_is_view(value: &SmeltUnknown) -> bool { let SmeltUnknown::Object(map) = value else { return false; }; ["__smelt_int8array", "__smelt_uint8array", "__smelt_uint8clampedarray", "__smelt_int16array", "__smelt_uint16array", "__smelt_int32array", "__smelt_uint32array", "__smelt_float32array", "__smelt_float64array", "__smelt_bigint64array", "__smelt_biguint64array", "__smelt_buffer", "__smelt_dataview"].into_iter().any(|marker| map.contains_key(marker)) }
/// Whether a byte-backed marker names a *view* rather than byte storage.
///
/// A view in JavaScript is always a window onto an `ArrayBuffer`, so this is
/// what decides whether a freshly built record also gets `buffer`/`byteOffset`.
fn smelt_host_buffer_is_view_marker(marker: &str) -> bool { ["__smelt_int8array", "__smelt_uint8array", "__smelt_uint8clampedarray", "__smelt_int16array", "__smelt_uint16array", "__smelt_int32array", "__smelt_uint32array", "__smelt_float32array", "__smelt_float64array", "__smelt_bigint64array", "__smelt_biguint64array", "__smelt_buffer", "__smelt_dataview"].into_iter().any(|entry| entry == marker) }
//~ Emit the element-type table and its little-endian decode/encode pair.
/// The element tag and byte stride of a byte-backed marker's view.
///
/// `None` for the byte-addressed kinds (`ArrayBuffer`, `SharedArrayBuffer`,
/// `DataView`), whose bytes carry no single element type; callers then treat
/// the storage as a stride-one byte sequence.
fn smelt_host_buffer_element_kind(marker: &str) -> Option<(&'static str, usize)> { [("__smelt_int8array", "int8", 1), ("__smelt_uint8array", "uint8", 1), ("__smelt_uint8clampedarray", "uint8clamped", 1), ("__smelt_int16array", "int16", 2), ("__smelt_uint16array", "uint16", 2), ("__smelt_int32array", "int32", 4), ("__smelt_uint32array", "uint32", 4), ("__smelt_float32array", "float32", 4), ("__smelt_float64array", "float64", 8), ("__smelt_bigint64array", "bigint64", 8), ("__smelt_biguint64array", "biguint64", 8), ("__smelt_buffer", "uint8", 1)].into_iter().find(|(entry, _, _)| *entry == marker).map(|(_, tag, width)| (tag, width)) }
/// The byte stride of a byte-backed marker: its `BYTES_PER_ELEMENT`, or 1.
fn smelt_host_buffer_stride(marker: &str) -> usize { smelt_host_buffer_element_kind(marker).map_or(1, |(_, width)| width) }
/// Read one raw byte out of a `bytes` storage list, zero past the end.
fn smelt_host_buffer_raw_byte(bytes: &[SmeltUnknown], index: usize) -> u8 { match bytes.get(index) { Some(SmeltUnknown::Number(value)) if value.is_finite() => *value as i64 as u8, _ => 0 } }
/// Coerce a JavaScript number to the integer an integer view stores.
///
/// Truncates toward zero and maps the non-finite values to `0`, matching
/// ECMAScript `ToIntegerOrInfinity` followed by the modulo-2^n wrap that the
/// caller's `as i8`/`as u16`/... cast performs.
fn smelt_host_buffer_to_integer(value: f64) -> i64 { if value.is_finite() { value.trunc() as i64 } else { 0 } }
/// Decode the element of type `kind` starting at byte `offset`.
///
/// Little-endian, and signed for the signed views: byte `0xff` decodes to
/// `255` through `uint8` and `-1` through `int8`. Bytes past the end read as
/// zero, so a truncated final element decodes rather than panicking.
fn smelt_host_buffer_decode_element(kind: &str, bytes: &[SmeltUnknown], offset: usize) -> SmeltUnknown { let mut raw = [0u8; 8]; let width = smelt_host_buffer_kind_width(kind); for index in 0..width { raw[index] = smelt_host_buffer_raw_byte(bytes, offset + index); } SmeltUnknown::Number(match kind { "int8" => i8::from_le_bytes([raw[0]]) as f64, "uint8" => raw[0] as f64, "uint8clamped" => raw[0] as f64, "int16" => i16::from_le_bytes([raw[0], raw[1]]) as f64, "uint16" => u16::from_le_bytes([raw[0], raw[1]]) as f64, "int32" => i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64, "uint32" => u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64, "float32" => f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64, "float64" => f64::from_le_bytes(raw), "bigint64" => i64::from_le_bytes(raw) as f64, "biguint64" => u64::from_le_bytes(raw) as f64, _ => raw[0] as f64 }) }
/// The byte width of an element tag, for the decode buffer above.
fn smelt_host_buffer_kind_width(kind: &str) -> usize { match kind { "int8" => 1, "uint8" => 1, "uint8clamped" => 1, "int16" => 2, "uint16" => 2, "int32" => 4, "uint32" => 4, "float32" => 4, "float64" => 8, "bigint64" => 8, "biguint64" => 8, _ => 1 } }
/// Encode one JavaScript value as the bytes of an element of type `kind`.
///
/// Integer views wrap modulo their width (ECMAScript `ToInt8`/`ToUint16`/...),
/// which is why an `Int8Array` holding `-1` and a `Uint8Array` holding `255`
/// have byte-identical buffers. `Uint8ClampedArray` saturates instead.
fn smelt_host_buffer_encode_element(kind: &str, value: &SmeltUnknown) -> Vec<SmeltUnknown> { let number = match value { SmeltUnknown::Number(value) => *value, SmeltUnknown::Bool(value) => if *value { 1.0 } else { 0.0 }, SmeltUnknown::String(text) => smelt_string_to_number(text), SmeltUnknown::Null => 0.0, _ => f64::NAN }; let raw: Vec<u8> = match kind { "int8" => vec![smelt_host_buffer_to_integer(number) as i8 as u8], "uint8" => vec![smelt_host_buffer_to_integer(number) as u8], "uint8clamped" => vec![if number.is_nan() { 0u8 } else { number.round_ties_even().clamp(0.0, 255.0) as u8 }], "int16" => (smelt_host_buffer_to_integer(number) as i16).to_le_bytes().to_vec(), "uint16" => (smelt_host_buffer_to_integer(number) as u16).to_le_bytes().to_vec(), "int32" => (smelt_host_buffer_to_integer(number) as i32).to_le_bytes().to_vec(), "uint32" => (smelt_host_buffer_to_integer(number) as u32).to_le_bytes().to_vec(), "float32" => (number as f32).to_le_bytes().to_vec(), "float64" => number.to_le_bytes().to_vec(), "bigint64" => smelt_host_buffer_to_integer(number).to_le_bytes().to_vec(), "biguint64" => (smelt_host_buffer_to_integer(number) as u64).to_le_bytes().to_vec(), _ => vec![smelt_host_buffer_to_integer(number) as u8] }; raw.into_iter().map(|byte| SmeltUnknown::Number(f64::from(byte))).collect() }
//~ Emit the record constructor and the element/byte extraction helpers.
/// Build a byte-backed host record of `marker` identity over `bytes`.
///
/// `byteLength` is the byte count and `length` is the *element* count, so a
/// `Float64Array` over eight bytes reports length 1 while a `Uint8Array` over
/// the same bytes reports 8. An element-typed view is always a window onto an
/// `ArrayBuffer` in JavaScript, so it also gets `byteOffset` and a `buffer`
/// storage record. The record gets a fresh object id, which is what
/// `clone(buf) !== buf` depends on.
fn smelt_host_buffer_record(marker: &'static str, bytes: Vec<SmeltUnknown>) -> SmeltUnknown { let buffer = smelt_host_buffer_is_view_marker(marker).then(|| smelt_host_buffer_storage_record(bytes.clone())); smelt_host_buffer_view_record(marker, bytes, buffer, 0) }
/// Build a fresh `ArrayBuffer` record over `bytes`.
///
/// Backs `typedArray.buffer` for a view that was not built from an existing
/// buffer (`new Uint8Array([1, 2, 3]).buffer`). `ArrayBuffer` is byte-addressed,
/// so this never recurses into a nested buffer of its own.
fn smelt_host_buffer_storage_record(bytes: Vec<SmeltUnknown>) -> SmeltUnknown { smelt_host_buffer_view_record("__smelt_arraybuffer", bytes, None, 0) }
/// Build a byte-backed host record, optionally sharing an existing `buffer`.
///
/// Passing the *same* storage record two views were built from is what makes
/// `view.buffer === buffer` hold, which lodash-compatible clone specs assert on
/// a cloned view.
fn smelt_host_buffer_view_record(marker: &'static str, bytes: Vec<SmeltUnknown>, buffer: Option<SmeltUnknown>, byte_offset: usize) -> SmeltUnknown { smelt_host_buffer_view_record_with_id(smelt_next_object_id(), marker, bytes, buffer, byte_offset) }
//~ The same record at a CALLER-CHOSEN identity. The concrete family's
//~ erasure adapters build their record here so a concrete value and an
//~ erased one are byte-identical once erased, and so erasing the same
//~ concrete value twice yields two records that are `===`.
/// Build a byte-backed host record at a given JS reference id.
fn smelt_host_buffer_view_record_with_id(id: usize, marker: &'static str, bytes: Vec<SmeltUnknown>, buffer: Option<SmeltUnknown>, byte_offset: usize) -> SmeltUnknown { let byte_length = bytes.len(); let stride = smelt_host_buffer_stride(marker); let mut fields = Vec::from([(marker.to_owned(), SmeltUnknown::Bool(true)), ("bytes".to_owned(), SmeltUnknown::Array(SmeltArray::new(bytes))), ("byteLength".to_owned(), SmeltUnknown::Number(byte_length as f64)), ("length".to_owned(), SmeltUnknown::Number((byte_length / stride) as f64))]); if let Some(buffer) = buffer { fields.push(("buffer".to_owned(), buffer)); fields.push(("byteOffset".to_owned(), SmeltUnknown::Number(byte_offset as f64))); } SmeltUnknown::Object(SmeltObject::with_id(id, fields)) }
/// Return a byte-backed host record's raw storage bytes, or `None`.
///
/// This is the *byte* view of the record, which is what
/// `new Float32Array(arrayBuffer)` re-interprets and what `DataView` windows.
fn smelt_host_buffer_raw_bytes(value: &SmeltUnknown) -> Option<Vec<SmeltUnknown>> { let SmeltUnknown::Object(map) = value else { return None; }; smelt_host_buffer_marker(map)?; Some(match map.get("bytes") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }) }
/// Return a byte-backed host record's storage decoded as its *elements*.
///
/// `None` for any other value, so callers can fall through to their existing
/// array/string/iterator handling. This is the array-like face of a typed
/// array: iteration, spread and `Array.from` all see decoded elements at the
/// view's own width, not its raw bytes. Byte-addressed storage decodes as its
/// bytes, which is what `new Uint8Array(dataView)` wants.
fn smelt_host_buffer_elements(value: &SmeltUnknown) -> Option<Vec<SmeltUnknown>> { let SmeltUnknown::Object(map) = value else { return None; }; let marker = smelt_host_buffer_marker(map)?; let bytes = smelt_host_buffer_raw_bytes(value)?; let Some((kind, width)) = smelt_host_buffer_element_kind(marker) else { return Some(bytes); }; Some((0..bytes.len() / width).map(|index| smelt_host_buffer_decode_element(kind, &bytes, index * width)).collect()) }
/// A byte-backed host record's OWN ENUMERABLE property values.
///
/// Own indexed properties belong to an ELEMENT-TYPED view and to nothing
/// else: byte storage (`ArrayBuffer`) and a `DataView` address their bytes
/// through accessors, so `Object.keys` / `for...in` / `JSON.stringify` see no
/// properties on them at all, exactly as Node does. The array-like face
/// above (`ELEMENTS`) answers a different question and must keep decoding
/// storage as bytes for iteration and `new Uint8Array(dataView)`.
fn smelt_host_buffer_own_elements(value: &SmeltUnknown) -> Option<Vec<SmeltUnknown>> { let SmeltUnknown::Object(map) = value else { return None; }; let marker = smelt_host_buffer_marker(map)?; if smelt_host_buffer_element_kind(marker).is_none() { return Some(Vec::new()); } smelt_host_buffer_elements(value) }
/// A byte-backed host record's own enumerable keys: its element indices.
///
/// `None` for values that are not byte-backed. A typed array's own properties
/// are exactly its indexed elements — `length`, `byteLength`, `byteOffset` and
/// `buffer` are prototype accessors and never enumerate — so
/// `Object.keys(new Uint8Array(1))` is `['0']` and a deep-equality walk over
/// two views compares elements rather than internal storage keys.
fn smelt_host_buffer_index_keys(value: &SmeltUnknown) -> Option<Vec<String>> { let count = smelt_host_buffer_own_elements(value)?.len(); Some((0..count).map(|index| index.to_string()).collect()) }
/// The same own-key set, for a byte-backed record reached through the
/// structural `SmeltRecord` ABI rather than as a tagged `SmeltUnknown`.
///
/// An erased object crossing into a `Record<string, unknown>` parameter is
/// re-materialized as a `SmeltRecord`, so `Object.keys` on that side needs the
/// same index-key answer — remeda's `isShallowEqual` reads exactly that path.
fn smelt_host_buffer_record_index_keys(record: &SmeltRecord<String, SmeltUnknown>) -> Option<Vec<String>> { smelt_host_buffer_index_keys(&SmeltUnknown::Object(SmeltObject::from_unknown_record(record.clone()))) }
/// The decoded elements of a byte-backed record reached through the
/// structural `SmeltRecord` ABI. Backs `Object.values`/`Object.entries` over a
/// view, which pair with the index keys above.
fn smelt_host_buffer_record_elements(record: &SmeltRecord<String, SmeltUnknown>) -> Option<Vec<SmeltUnknown>> { smelt_host_buffer_own_elements(&SmeltUnknown::Object(SmeltObject::from_unknown_record(record.clone()))) }
//~ Emit slice and indexed element read/write.
/// Slice a byte-backed host record into a fresh record of the same host kind.
///
/// `None` for values that are not byte-backed, so `.slice()`/`.subarray()` on
/// an erased receiver keeps its array/string behavior. Bounds are *element*
/// indices for an element-typed view and byte indices for byte-addressed
/// storage — one code path once the stride comes from the registry. Negative
/// bounds count back from the end and both bounds clamp, matching
/// `ArrayBuffer.prototype.slice` and `TypedArray.prototype.subarray`.
fn smelt_host_buffer_slice(value: &SmeltUnknown, start: i64, end: Option<i64>) -> Option<SmeltUnknown> { let SmeltUnknown::Object(map) = value else { return None; }; let marker = smelt_host_buffer_marker(map)?; let bytes = smelt_host_buffer_raw_bytes(value)?; let stride = smelt_host_buffer_stride(marker); let len = (bytes.len() / stride) as i64; let from = (if start < 0 { len + start } else { start }).clamp(0, len); let to = end.map_or(len, |end| if end < 0 { len + end } else { end }).clamp(0, len); let take = to.saturating_sub(from) as usize; Some(smelt_host_buffer_record(marker, bytes.into_iter().skip(from as usize * stride).take(take * stride).collect())) }
/// Read one indexed element (`view[1]`) of a byte-backed host record.
///
/// `None` when the receiver is not byte-backed or the key is not an array
/// index, so ordinary erased field/index reads are untouched. An in-range
/// index of a view decodes at the element's own width and signedness; past the
/// end it answers `undefined`, as a typed array does.
fn smelt_host_buffer_element(map: &SmeltObject, key: &str) -> Option<SmeltUnknown> { let marker = smelt_host_buffer_marker(map)?; let index = key.parse::<usize>().ok()?; let bytes = match map.get("bytes") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }; let (kind, width) = smelt_host_buffer_element_kind(marker).unwrap_or(("uint8", 1)); if (index + 1) * width > bytes.len() { return None; } Some(smelt_host_buffer_decode_element(kind, &bytes, index * width)) }
/// Write one indexed element (`view[i] = value`) of a byte-backed host record.
///
/// Returns whether the write was absorbed by the byte storage; `false` leaves
/// the caller's ordinary record insert in charge. The value is encoded into
/// exactly the element's bytes, so `int8View[0] = -1` stores the byte `255`.
/// Writes past the end are absorbed and dropped, matching a typed array's
/// fixed-length storage. The write also lands in the view's backing `buffer`
/// record *in place*, so `view[0] = 1` is visible through `view.buffer` and the
/// buffer keeps its object identity (`view.buffer === buffer` still holds).
fn smelt_host_buffer_set_element(map: &SmeltObject, key: &str, value: SmeltUnknown) -> bool { let Some(marker) = smelt_host_buffer_marker(map) else { return false; }; let Ok(index) = key.parse::<usize>() else { return false; }; let Some(SmeltUnknown::Array(values)) = map.get("bytes") else { return false; }; let mut bytes = values.into_vec(); let (kind, width) = smelt_host_buffer_element_kind(marker).unwrap_or(("uint8", 1)); let offset = index * width; if offset + width <= bytes.len() { let encoded = smelt_host_buffer_encode_element(kind, &value); for (step, byte) in encoded.iter().enumerate() { bytes[offset + step] = byte.clone(); } map.insert("bytes".to_owned(), SmeltUnknown::Array(SmeltArray::new(bytes))); smelt_host_buffer_write_through(map, offset, &encoded); /*@if byte_array*/smelt_typed_array_write_origin(map, offset, &encoded);/*@end*/ } true }
/// Mirror an element write into the view's backing `buffer` storage record.
///
/// A typed array is a window onto an `ArrayBuffer`, so a write through the view
/// is a write to the buffer — `new Uint8Array(buf)[0] = 1` then shows up in
/// `buf`. `SmeltObject` has interior mutability, so the storage record is updated
/// in place rather than replaced: replacing it would mint a new object id and
/// break `view.buffer === buf`. `byteOffset` places the window inside the
/// buffer. A no-op for the byte-addressed kinds, which have no backing buffer.
fn smelt_host_buffer_write_through(map: &SmeltObject, offset: usize, encoded: &[SmeltUnknown]) { let Some(SmeltUnknown::Object(storage)) = map.get("buffer") else { return; }; let base = match map.get("byteOffset") { Some(SmeltUnknown::Number(value)) if value >= 0.0 => value as usize, _ => 0 }; let Some(SmeltUnknown::Array(values)) = storage.get("bytes") else { return; }; let mut bytes = values.into_vec(); for (step, byte) in encoded.iter().enumerate() { let at = base + offset + step; if at < bytes.len() { bytes[at] = byte.clone(); } } storage.insert("bytes".to_owned(), SmeltUnknown::Array(SmeltArray::new(bytes))); /*@if byte_array*/smelt_typed_array_write_origin(&storage, base + offset, encoded);/*@end*/ }
//~ Emit the shared byte-buffer constructor.
/// Whether an erased value is byte *storage* (an `ArrayBuffer`), not a view.
///
/// This is the distinction that decides what `new Float32Array(source)` means:
/// over storage it re-*views* the bytes (eight bytes become two elements), over
/// a view or an array it *converts* the elements one by one.
fn smelt_host_buffer_is_storage(value: &SmeltUnknown) -> bool { let SmeltUnknown::Object(map) = value else { return false; }; ["__smelt_arraybuffer", "__smelt_sharedarraybuffer"].into_iter().any(|marker| map.contains_key(marker)) }
/// The six-bit value of one base64 character (either alphabet), if it has one.
fn smelt_host_buffer_base64_value(value: char) -> Option<u8> { match value { 'A'..='Z' => Some(value as u8 - b'A'), 'a'..='z' => Some(value as u8 - b'a' + 26), '0'..='9' => Some(value as u8 - b'0' + 52), '+' | '-' => Some(62), '/' | '_' => Some(63), _ => None } }
/// Encode a JavaScript string into buffer bytes under a Node encoding name.
///
/// `Buffer.from(string[, encoding])` is the one byte-buffer constructor form
/// whose source is *text*: a typed array built from a string has no elements,
/// but a Buffer built from a string holds that string's encoded bytes. The
/// frontend rejects encodings this helper does not implement, so an unknown
/// name can only reach here as a dynamic value and falls back to the
/// JavaScript default of UTF-8.
fn smelt_host_buffer_encode_text(text: &str, encoding: Option<String>) -> Vec<SmeltUnknown> {
    let encoding = encoding.unwrap_or_else(|| "utf8".to_owned()).to_ascii_lowercase();
    let bytes: Vec<u8> = match encoding.as_str() {
        "latin1" | "binary" => text.chars().map(|value| (value as u32 & 0xff) as u8).collect(),
        "ascii" => text.chars().map(|value| (value as u32 & 0x7f) as u8).collect(),
        "hex" => { let digits = text.chars().filter_map(|value| value.to_digit(16)).collect::<Vec<_>>(); digits.chunks_exact(2).map(|pair| (pair[0] * 16 + pair[1]) as u8).collect() }
        //~ base64 / base64url: six-bit groups, padding and unknown characters
        //~ skipped, trailing partial group dropped — Node's permissive decode.
        "base64" | "base64url" => { let mut bytes = Vec::new(); let mut accumulator: u32 = 0; let mut bits = 0u32; for value in text.chars() { let Some(sextet) = smelt_host_buffer_base64_value(value) else { continue; }; accumulator = (accumulator << 6) | u32::from(sextet); bits += 6; if bits >= 8 { bits -= 8; bytes.push(((accumulator >> bits) & 0xff) as u8); } } bytes }
        _ => text.as_bytes().to_vec(),
    };
    bytes.into_iter().map(|byte| SmeltUnknown::Number(f64::from(byte))).collect()
}
/// Read an optional non-negative numeric constructor argument.
fn smelt_host_buffer_count_argument(argument: Option<&SmeltUnknown>) -> Option<usize> { match argument { Some(SmeltUnknown::Number(value)) if *value >= 0.0 => Some(*value as usize), _ => None } }
/// Build a byte-backed host record of `marker` identity from `new X(...)` args.
///
/// The four JavaScript constructor forms, all resolved from the registry rather
/// than per call site:
///
/// * `new X(n)` — `n` *elements* of zeroes (`new Float64Array(2)` is 16 bytes);
/// * `new X(arrayBuffer[, byteOffset[, length]])` — a **view** over that
///   storage's bytes, sharing the buffer record so `view.buffer === buffer`;
/// * `new X(otherView)` / `new X([...])` — an element-by-element **conversion**,
///   so `new Uint8Array(new Int8Array([-1]))` holds `255`;
/// * `new X()` — empty storage.
///
/// Byte-addressed targets (`ArrayBuffer`, `SharedArrayBuffer`) have stride one
/// and no element codec, so the same code gives them their platform behavior.
fn smelt_host_buffer_construct(marker: &'static str, args: Vec<SmeltUnknown>) -> SmeltUnknown {
    let element = smelt_host_buffer_element_kind(marker);
    let stride = element.map_or(1, |(_, width)| width);
    let source = args.first();
    match source {
        //~ `Buffer.from(string[, encoding])`: the source is text, not elements. Only
        //~ the Buffer identity has this constructor form; `new Uint8Array("ab")` is
        //~ empty, so the arm is gated on the registry's Buffer marker.
        Some(SmeltUnknown::String(text)) if marker == "__smelt_buffer" => { let encoding = match args.get(1) { Some(SmeltUnknown::String(value)) => Some(value.to_string()), _ => None }; smelt_host_buffer_record(marker, smelt_host_buffer_encode_text(&text.to_string(), encoding)) }
        //~ `new X(arrayBuffer, byteOffset, length)`: a byte window over shared storage.
        Some(value) if smelt_host_buffer_is_storage(value) => { let storage = smelt_host_buffer_raw_bytes(value).unwrap_or_default(); let offset = smelt_host_buffer_count_argument(args.get(1)).unwrap_or(0).min(storage.len()); let available = storage.len() - offset; let take = smelt_host_buffer_count_argument(args.get(2)).map_or(available, |count| (count * stride).min(available)); let window = storage.into_iter().skip(offset).take(take).collect::<Vec<_>>(); let buffer = smelt_host_buffer_is_view_marker(marker).then(|| value.clone()); smelt_host_buffer_view_record(marker, window, buffer, offset) }
        //~ `new X(otherView)` / `new X([...])`: element-by-element conversion.
        Some(value) => { let source_elements = match value { SmeltUnknown::Array(values) => Some(values.clone().into_vec()), _ => smelt_host_buffer_elements(value) }; match source_elements { Some(source_elements) => match element { Some((kind, _)) => smelt_host_buffer_record(marker, source_elements.iter().flat_map(|item| smelt_host_buffer_encode_element(kind, item)).collect()), None => smelt_host_buffer_record(marker, source_elements) }, None => match smelt_host_buffer_count_argument(Some(value)) { Some(count) => smelt_host_buffer_record(marker, vec![SmeltUnknown::Number(0.0); count * stride]), None => smelt_host_buffer_record(marker, Vec::new()) } } }
        None => smelt_host_buffer_record(marker, Vec::new()),
    }
}
// @smelt:item-end
