// Erased `Blob` records.

// Unconditional: the shared host constructor
// (`smelt_reflected_construct`, always emitted) builds `Blob`/`File`
// through this helper, so it cannot be gated on the crate spelling a
// `new Blob(...)` itself.
// @smelt:item smelt_blob_parts_bytes when unknown
/// Concatenate an erased `BlobPart` array into the bytes it contributes.
///
/// A `BlobPart` is `Blob | BufferSource | string` — a heterogeneous host
/// union whose arm is a runtime fact — which is why the parts array is
/// erased and walked here. Strings contribute their UTF-8 bytes; a nested
/// `Blob`/`File` record contributes its exact bytes (`smelt_blob_record_bytes`,
/// the same bytes a concrete `SmeltBlob` part contributes); a byte-backed
/// host record (`Uint8Array`, `ArrayBuffer`, `DataView`, ...) contributes
/// its raw `bytes`, which the pre-byte-backed version stringified and so
/// corrupted; anything else stringifies the way JavaScript does.
///
/// Shared by `new Blob(..)` (through `SmeltBlob::from_parts_unknown`) and by
/// the reflected host construction below, so a directly-constructed blob and
/// a reflectively-constructed one cannot disagree about their bytes.
fn smelt_blob_parts_bytes(parts: SmeltUnknown) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    if let SmeltUnknown::Array(items) = parts {
        for item in items.iter() {
            match item {
                SmeltUnknown::String(text) => bytes.extend_from_slice(text.as_bytes()),
                SmeltUnknown::Object(map) if map.contains_key("__smelt_blob") => {
                    bytes.extend_from_slice(&smelt_blob_record_bytes(&map));
                }
                SmeltUnknown::Object(map) if matches!(map.get("bytes"), Some(SmeltUnknown::Array(_))) => {
                    if let Some(SmeltUnknown::Array(elements)) = map.get("bytes") {
                        for element in elements.iter() { if let SmeltUnknown::Number(value) = element { bytes.push(value as u8); } }
                    }
                }
                other => bytes.extend_from_slice(other.to_string().as_bytes()),
            }
        }
    }
    bytes
}

/// The ERASED form of a blob: the one definition of the record shape.
///
/// Called both by the reflected constructor below (through
/// `smelt_blob_record_from_parts`) and by `impl IntoSmeltUnknown for
/// SmeltBlob`, so a concrete blob crossing a dynamic boundary and a
/// reflectively-constructed one are the same record. `content` is the
/// UTF-8 decoding of the bytes, which is what consumers of an erased blob's
/// text read. When the bytes are not valid UTF-8 that decoding is lossy, so
/// the record also carries them exactly in the hidden `__smelt_blob_bytes`
/// list (a host-marked record shows no keys to enumeration); a UTF-8 blob's
/// record is unchanged. `smelt_blob_record_bytes` reads them back.
///
/// A file name stamps `__smelt_file` on top of `__smelt_blob`, so
/// `file instanceof Blob` observes the host subtype relationship.
fn smelt_blob_record(id: usize, bytes: &[u8], blob_type: &str, file_name: Option<String>, last_modified: f64) -> SmeltUnknown {
    let content = String::from_utf8_lossy(bytes).into_owned();
    let record = Vec::from([
        ("__smelt_blob".to_owned(), SmeltUnknown::Bool(true)),
        ("type".to_owned(), SmeltUnknown::String(blob_type.to_owned().into())),
        ("size".to_owned(), SmeltUnknown::Number(bytes.len() as f64)),
        ("content".to_owned(), SmeltUnknown::String(content.into())),
    ]);
    let record = SmeltObject::with_id(id, record);
    if ::std::str::from_utf8(bytes).is_err() {
        let exact = bytes.iter().map(|byte| SmeltUnknown::Number(f64::from(*byte))).collect::<Vec<_>>();
        record.insert("__smelt_blob_bytes".to_owned(), SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), exact)));
    }
    if let Some(name) = file_name {
        record.insert("__smelt_file".to_owned(), SmeltUnknown::Bool(true));
        record.insert("name".to_owned(), SmeltUnknown::String(name.into()));
        record.insert("lastModified".to_owned(), SmeltUnknown::Number(last_modified));
    }
    SmeltUnknown::Object(record)
}

/// The exact bytes of an erased `Blob`/`File` record: the hidden byte list
/// `smelt_blob_record` stores when `content` is a lossy decoding, else the
/// UTF-8 bytes of `content`.
///
/// The one reader of an erased blob's bytes, shared by the `BlobPart` walk
/// and by `SmeltFromUnknown for SmeltBlob`, so a blob that crossed a dynamic
/// boundary contributes the same bytes as the concrete blob it came from.
fn smelt_blob_record_bytes(map: &SmeltObject) -> Vec<u8> {
    if let Some(SmeltUnknown::Array(elements)) = map.get("__smelt_blob_bytes") {
        return elements.iter().filter_map(|element| match element { SmeltUnknown::Number(value) => Some(value as u8), _ => None }).collect();
    }
    match map.get("content") { Some(SmeltUnknown::String(text)) => text.as_bytes().to_vec(), _ => Vec::new() }
}

/// Build the erased `Blob`/`File` record for the reflected constructor.
///
/// Kept separate from the concrete `SmeltBlob` so a crate that reflects over
/// host constructors but never names a blob does not carry the blob type.
/// `lastModified` defaults to `0.0` for determinism instead of the wall clock.
fn smelt_blob_record_from_parts(parts: SmeltUnknown, blob_type: String, file_name: Option<String>, last_modified: Option<f64>) -> SmeltUnknown {
    let bytes = smelt_blob_parts_bytes(parts);
    smelt_blob_record(smelt_next_object_id(), &bytes, &blob_type, file_name, last_modified.unwrap_or(0.0))
}

// @smelt:item-end
