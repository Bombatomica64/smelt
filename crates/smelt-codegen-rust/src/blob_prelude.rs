//! Runtime prelude for the WHATWG `Blob`/`File` value.
//!
//! One generated type, `SmeltBlob`, backing both spellings.
//!
//! ## Why one type
//!
//! The spec's `File` is a `Blob` plus exactly two data properties (`name`,
//! `lastModified`) and no behaviour of its own. Rust has no inheritance, so the
//! choices were a `SmeltFile` that wraps a `SmeltBlob` — which makes every
//! `Blob`-typed slot need a widening conversion the type system would have to
//! carry — or one type whose file metadata is optional. The second is what the
//! ERASED record already did (`__smelt_file` stamped on top of `__smelt_blob`,
//! one shape), so it is also the choice that keeps the concrete value and its
//! erased form describing the same object.
//!
//! ## Immutability, and why there is no `RefCell`
//!
//! A `Blob`'s bytes and type are immutable in the spec: `slice` answers a NEW
//! blob and there is no mutator anywhere on the interface. So the bytes sit
//! behind a plain `Rc<Vec<u8>>` — shared, never borrowed mutably — rather than
//! the `Rc<RefCell<..>>` the mutable reference types (`SmeltHeaders`,
//! `SmeltList`) need. Two variables holding one blob still observe one object,
//! because there is nothing to observe changing. The `Rc` is there for cheap
//! cloning of what can be a large byte buffer, not for interior mutability.
//!
//! ## `size` is a BYTE count
//!
//! The old erased record stored its content as a `String` and reported
//! `content.len()`. That happened to be right for `size` (Rust's `String::len`
//! is a byte count) and wrong for everything else: a blob's content is
//! arbitrary bytes, not necessarily UTF-8, and `arrayBuffer()`/`bytes()` have
//! to answer those bytes exactly. This type stores `Vec<u8>` and decodes only
//! at `text()`, which is the direction the spec defines (`text()` is "UTF-8
//! decode", replacement characters and all).
//!
//! ## The erased form
//!
//! Unchanged from the record the runtime helper used to build:
//! `{ "__smelt_blob": true, "type": .., "size": .., "content": ..,
//! ("__smelt_file": true, "name": .., "lastModified": ..) }`. Keeping it
//! byte-identical is what leaves `instanceof Blob` on an erased value, the
//! `clone`/`cloneDeepWith` paths that rebuild a blob from those fields, and
//! `Object.prototype.toString` all working exactly as before. `content` stays a
//! STRING there because that is what the existing consumers read; the concrete
//! type is the one that carries the bytes losslessly.

use crate::rust::CodeWriter;

/// Emit the `SmeltBlob` runtime type.
///
/// `needs_unknown` gates the erasure adapters (`IntoSmeltUnknown` /
/// `SmeltFromUnknown`): a program that never crosses the dynamic boundary does
/// not emit the carrier type, so the impls must not be emitted either.
pub fn emit(writer: &mut CodeWriter, needs_unknown: bool) {
    emit_struct(writer);
    emit_inherent_impl(writer);
    emit_traits(writer, needs_unknown);
}

/// Emit the struct definition and its comparisons.
fn emit_struct(writer: &mut CodeWriter) {
    writer.line("/// A WHATWG `Blob` (or `File`): immutable bytes, a MIME type, and the");
    writer.line("/// two optional `File` data properties.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltBlob", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("/// The blob's bytes. Immutable, so shared without a cell.");
        struct_writer.line("bytes: ::std::rc::Rc<Vec<u8>>,");
        struct_writer.line("/// The `type` data property, `\"\"` when none was supplied.");
        struct_writer.line("blob_type: String,");
        struct_writer.line("/// `Some` exactly when this blob is a `File`.");
        struct_writer.line("file_name: Option<String>,");
        struct_writer.line("/// `lastModified` in epoch milliseconds; meaningless unless a file.");
        struct_writer.line("last_modified: f64,");
    });
    writer.blank_line();
    // Structural equality over everything observable. `toEqual` on two blobs
    // built from the same parts and options must hold, and a file must not
    // equal a same-bytes blob (their `name` differs, and one has none).
    writer.line(
        "impl PartialEq for SmeltBlob { fn eq(&self, other: &Self) -> bool { self.bytes == other.bytes && self.blob_type == other.blob_type && self.file_name == other.file_name && self.last_modified == other.last_modified } }",
    );
    // Node prints `Blob { size: 3, type: 'text/plain' }` and
    // `File { size: 3, type: '', name: 'a.txt', lastModified: 0 }`.
    writer.block("impl ::std::fmt::Debug for SmeltBlob", |impl_writer| {
        impl_writer.block(
            "fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result",
            |fn_writer| {
                fn_writer.line("match &self.file_name {");
                fn_writer.line(
                    "    Some(name) => write!(formatter, \"File {{ size: {}, type: '{}', name: '{name}', lastModified: {} }}\", self.bytes.len(), self.blob_type, self.last_modified),",
                );
                fn_writer.line(
                    "    None => write!(formatter, \"Blob {{ size: {}, type: '{}' }}\", self.bytes.len(), self.blob_type),",
                );
                fn_writer.line("}");
            },
        );
    });
    writer.line("impl Default for SmeltBlob { fn default() -> Self { Self::new() } }");
    writer.blank_line();
}

/// Emit the `Blob`/`File` operations as inherent methods.
fn emit_inherent_impl(writer: &mut CodeWriter) {
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltBlob", |impl_writer| {
        impl_writer.line("/// An empty, untyped blob with a fresh JS reference identity.");
        impl_writer.line("pub fn new() -> Self { Self::from_bytes(Vec::new(), String::new()) }");
        impl_writer.line("/// A blob over owned bytes, with a fresh JS reference identity.");
        impl_writer.line(
            "pub fn from_bytes(bytes: Vec<u8>, blob_type: String) -> Self { Self { id: smelt_next_object_id(), bytes: ::std::rc::Rc::new(bytes), blob_type, file_name: None, last_modified: 0.0 } }",
        );
        // `lastModified` defaults to 0 rather than the wall clock, keeping a
        // generated program deterministic; the erased record already did this.
        impl_writer.line("/// The same, as a `File`: the two extra data properties are present.");
        impl_writer.line(
            "pub fn file_from_bytes(bytes: Vec<u8>, blob_type: String, name: String, last_modified: f64) -> Self { Self { id: smelt_next_object_id(), bytes: ::std::rc::Rc::new(bytes), blob_type, file_name: Some(name), last_modified } }",
        );
        // The two concrete `BlobPart` arms. A string part contributes its UTF-8
        // bytes and a blob part its own bytes — exactly what the erased walk
        // (`smelt_blob_parts_bytes`) does for the same values, so a concretely
        // typed parts array and an erased one build the same blob.
        impl_writer.line("/// A blob from string parts, with optional `File` metadata.");
        impl_writer.line(
            "pub fn from_string_parts(parts: Vec<String>, blob_type: String, file_name: Option<String>, last_modified: Option<f64>) -> Self { let mut bytes: Vec<u8> = Vec::new(); for part in &parts { bytes.extend_from_slice(part.as_bytes()); } Self::with_metadata(bytes, blob_type, file_name, last_modified) }",
        );
        impl_writer.line("/// A blob from blob parts, with optional `File` metadata.");
        impl_writer.line(
            "pub fn from_blob_parts(parts: Vec<SmeltBlob>, blob_type: String, file_name: Option<String>, last_modified: Option<f64>) -> Self { let mut bytes: Vec<u8> = Vec::new(); for part in &parts { bytes.extend_from_slice(&part.bytes); } Self::with_metadata(bytes, blob_type, file_name, last_modified) }",
        );
        impl_writer.line("/// Pick the plain-blob or file constructor from the optional metadata.");
        impl_writer.line(
            "pub fn with_metadata(bytes: Vec<u8>, blob_type: String, file_name: Option<String>, last_modified: Option<f64>) -> Self { match file_name { Some(name) => Self::file_from_bytes(bytes, blob_type, name, last_modified.unwrap_or(0.0)), None => Self::from_bytes(bytes, blob_type) } }",
        );
        impl_writer.line("/// JS reference identity of this blob.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// Whether this blob is a `File`.");
        impl_writer.line("pub fn is_file(&self) -> bool { self.file_name.is_some() }");
        impl_writer.line("/// `size`: the byte count.");
        impl_writer.line("pub fn size(&self) -> f64 { self.bytes.len() as f64 }");
        impl_writer.line("/// `type`: the MIME type.");
        impl_writer.line("pub fn blob_type(&self) -> String { self.blob_type.clone() }");
        impl_writer.line("/// `name`: the file name, empty for a plain blob.");
        impl_writer.line(
            "pub fn file_name(&self) -> String { self.file_name.clone().unwrap_or_default() }",
        );
        impl_writer.line("/// `lastModified`: epoch milliseconds.");
        impl_writer.line("pub fn last_modified(&self) -> f64 { self.last_modified }");
        impl_writer.line("/// A copy of the blob's bytes.");
        impl_writer.line("pub fn to_bytes(&self) -> Vec<u8> { self.bytes.as_ref().clone() }");
        impl_writer.line("/// `text()`: the bytes UTF-8 decoded, U+FFFD for ill-formed input.");
        impl_writer.line(
            "pub fn to_text(&self) -> String { String::from_utf8_lossy(&self.bytes).into_owned() }",
        );
        // The spec's `slice` clamps a negative index from the end and an
        // out-of-range one to the bounds, then answers an EMPTY blob when the
        // range is inverted; and it never carries the source's file identity,
        // so slicing a `File` gives a `Blob`.
        impl_writer.line("/// `slice(start?, end?, contentType?)`: a new blob over a byte range.");
        impl_writer.block(
            "pub fn slice(&self, start: Option<f64>, end: Option<f64>, content_type: Option<String>) -> Self",
            |fn_writer| {
                fn_writer.line("let len = self.bytes.len() as f64;");
                fn_writer.line("let clamp = |value: f64| -> usize { let resolved = if value < 0.0 { len + value } else { value }; resolved.clamp(0.0, len) as usize };");
                fn_writer.line("let from = start.map_or(0, clamp);");
                fn_writer.line("let to = end.map_or(self.bytes.len(), clamp);");
                fn_writer.line("let range = if to > from { self.bytes[from..to].to_vec() } else { Vec::new() };");
                fn_writer.line("Self::from_bytes(range, content_type.unwrap_or_default())");
            },
        );
    });
    writer.blank_line();
}

/// Emit the `Blob`/`File` dynamic-boundary adapters.
///
/// The erased form is byte-identical to the record the pre-concrete runtime
/// helper built, so every existing consumer of an erased blob is unaffected.
fn emit_traits(writer: &mut CodeWriter, needs_unknown: bool) {
    if !needs_unknown {
        return;
    }
    writer.line("/// Erase a blob for a dynamic boundary.");
    writer.line("///");
    writer.line("/// The record shape has ONE definition, `smelt_blob_record`, shared with the");
    writer.line("/// reflected host constructor, so a concrete blob and a reflectively-built one");
    writer.line("/// erase to the same object.");
    writer.line(
        "impl IntoSmeltUnknown for SmeltBlob { fn into_smelt_unknown(self) -> SmeltUnknown { smelt_blob_record(self.id, &self.bytes, &self.blob_type, self.file_name.clone(), self.last_modified) } }",
    );
    writer.blank_line();
    // The constructor for `new Blob(parts, ..)` / `new File(parts, name, ..)`.
    // It lives in the erasure section because its ARGUMENT is erased: a
    // `BlobPart` is `Blob | BufferSource | string`, a heterogeneous host union
    // whose arm is a runtime fact, so the parts array is the genuine dynamic
    // boundary here. The byte walk itself is `smelt_blob_parts_bytes`, shared
    // with the reflected-construction path so a directly-constructed blob and a
    // reflectively-constructed one cannot disagree about their bytes.
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltBlob", |impl_writer| {
        impl_writer.line("/// Build a blob from an erased `BlobPart` array.");
        impl_writer.block(
            "pub fn from_parts_unknown(parts: SmeltUnknown, blob_type: String, file_name: Option<String>, last_modified: Option<f64>) -> Self",
            |fn_writer| {
                fn_writer.line("Self::with_metadata(smelt_blob_parts_bytes(parts), blob_type, file_name, last_modified)");
            },
        );
    });
    writer.blank_line();
    writer.line("/// Rebuild a blob from an erased value.");
    writer.block("impl SmeltFromUnknown for SmeltBlob", |impl_writer| {
        impl_writer.block(
            "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
            |fn_writer| {
                fn_writer.line("let SmeltUnknown::Object(map) = value else { return Self::new() };");
                fn_writer.line(
                    "let content = match map.get(\"content\") { Some(SmeltUnknown::String(text)) => text.to_string(), _ => String::new() };",
                );
                fn_writer.line(
                    "let blob_type = match map.get(\"type\") { Some(SmeltUnknown::String(text)) => text.to_string(), _ => String::new() };",
                );
                fn_writer.line(
                    "let name = match map.get(\"name\") { Some(SmeltUnknown::String(text)) => Some(text.to_string()), _ => None };",
                );
                fn_writer.line(
                    "let last_modified = match map.get(\"lastModified\") { Some(SmeltUnknown::Number(value)) => value, _ => 0.0 };",
                );
                fn_writer.line(
                    "match name { Some(name) => Self::file_from_bytes(content.into_bytes(), blob_type, name, last_modified), None => Self::from_bytes(content.into_bytes(), blob_type) }",
                );
            },
        );
    });
    writer.blank_line();
}
