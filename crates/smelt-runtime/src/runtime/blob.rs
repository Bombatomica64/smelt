// Runtime prelude for the WHATWG `Blob`/`File` value.
//
// One generated type, `SmeltBlob`, backing both spellings.
//
// ## Why one type
//
// The spec's `File` is a `Blob` plus exactly two data properties (`name`,
// `lastModified`) and no behaviour of its own. Rust has no inheritance, so the
// choices were a `SmeltFile` that wraps a `SmeltBlob` — which makes every
// `Blob`-typed slot need a widening conversion the type system would have to
// carry — or one type whose file metadata is optional. The second is what the
// ERASED record already did (`__smelt_file` stamped on top of `__smelt_blob`,
// one shape), so it is also the choice that keeps the concrete value and its
// erased form describing the same object.
//
// ## Immutability, and why there is no `RefCell`
//
// A `Blob`'s bytes and type are immutable in the spec: `slice` answers a NEW
// blob and there is no mutator anywhere on the interface. So the bytes sit
// behind a plain `Rc<Vec<u8>>` — shared, never borrowed mutably — rather than
// the `Rc<RefCell<..>>` the mutable reference types (`SmeltHeaders`,
// `SmeltList`) need. Two variables holding one blob still observe one object,
// because there is nothing to observe changing. The `Rc` is there for cheap
// cloning of what can be a large byte buffer, not for interior mutability.
//
// ## `size` is a BYTE count
//
// The old erased record stored its content as a `String` and reported
// `content.len()`. That happened to be right for `size` (Rust's `String::len`
// is a byte count) and wrong for everything else: a blob's content is
// arbitrary bytes, not necessarily UTF-8, and `arrayBuffer()`/`bytes()` have
// to answer those bytes exactly. This type stores `Vec<u8>` and decodes only
// at `text()`, which is the direction the spec defines (`text()` is "UTF-8
// decode", replacement characters and all).
//
// ## The erased form
//
// The record the runtime helper used to build:
// `{ "__smelt_blob": true, "type": .., "size": .., "content": ..,
// ("__smelt_file": true, "name": .., "lastModified": ..) }`. Keeping that
// shape is what leaves `instanceof Blob` on an erased value, the
// `clone`/`cloneDeepWith` paths that rebuild a blob from those fields, and
// `Object.prototype.toString` all working exactly as before. `content` stays a
// STRING because that is what text consumers read. It is a lossy decoding
// when the bytes are not UTF-8, so only then the record also carries the exact
// bytes in a hidden `__smelt_blob_bytes` list, and every reader of an erased
// blob's bytes goes through `smelt_blob_record_bytes`. Without it a blob
// erased and rebuilt (`new Blob([u])` over an `unknown`, or a mixed
// `[blob, "x"]` parts array) turned a `0xFF` byte into the three bytes of
// U+FFFD.

// Emit the struct definition and its comparisons.
// @smelt:item SmeltBlob when blob
/// A WHATWG `Blob` (or `File`): immutable bytes, a MIME type, and the
/// two optional `File` data properties.
#[derive(Clone)]
pub struct SmeltBlob {
    id: usize,
    /// The blob's bytes. Immutable, so shared without a cell.
    bytes: ::std::rc::Rc<Vec<u8>>,
    /// The `type` data property, `""` when none was supplied.
    blob_type: String,
    /// `Some` exactly when this blob is a `File`.
    file_name: Option<String>,
    /// `lastModified` in epoch milliseconds; meaningless unless a file.
    last_modified: f64,
    /// The source class of an instance decoded into this type, when a program
    /// subclasses `Blob`: `instanceof` reads it back through `__smelt_class`.
    instance_class: Option<String>,
}

//~ Structural equality over everything observable. `toEqual` on two blobs
//~ built from the same parts and options must hold, and a file must not
//~ equal a same-bytes blob (their `name` differs, and one has none).
impl PartialEq for SmeltBlob { fn eq(&self, other: &Self) -> bool { self.bytes == other.bytes && self.blob_type == other.blob_type && self.file_name == other.file_name && self.last_modified == other.last_modified } }
//~ Node prints `Blob { size: 3, type: 'text/plain' }` and
//~ `File { size: 3, type: '', name: 'a.txt', lastModified: 0 }`.
impl ::std::fmt::Debug for SmeltBlob {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match &self.file_name {
            Some(name) => write!(formatter, "File {{ size: {}, type: '{}', name: '{name}', lastModified: {} }}", self.bytes.len(), self.blob_type, self.last_modified),
            None => write!(formatter, "Blob {{ size: {}, type: '{}' }}", self.bytes.len(), self.blob_type),
        }
    }
}
impl Default for SmeltBlob { fn default() -> Self { Self::new() } }

//~ Emit the `Blob`/`File` operations as inherent methods.
#[allow(dead_code)]
impl SmeltBlob {
    /// An empty, untyped blob with a fresh JS reference identity.
    pub fn new() -> Self { Self::from_bytes(Vec::new(), String::new()) }
    /// A blob over owned bytes, with a fresh JS reference identity.
    pub fn from_bytes(bytes: Vec<u8>, blob_type: String) -> Self { Self { id: smelt_next_object_id(), bytes: ::std::rc::Rc::new(bytes), blob_type, file_name: None, last_modified: 0.0, instance_class: None } }
    //~ `lastModified` defaults to 0 rather than the wall clock, keeping a
    //~ generated program deterministic; the erased record already did this.
    /// The same, as a `File`: the two extra data properties are present.
    pub fn file_from_bytes(bytes: Vec<u8>, blob_type: String, name: String, last_modified: f64) -> Self { Self { id: smelt_next_object_id(), bytes: ::std::rc::Rc::new(bytes), blob_type, file_name: Some(name), last_modified, instance_class: None } }
    //~ The two concrete `BlobPart` arms. A string part contributes its UTF-8
    //~ bytes and a blob part its own bytes — what the erased walk
    //~ (`smelt_blob_parts_bytes`) does for the same values, so a concretely
    //~ typed parts array and an erased one build the same blob. For a blob
    //~ part that holds because the erased record keeps non-UTF-8 bytes
    //~ exactly (`smelt_blob_record_bytes`); it once read the lossy `content`.
    /// A blob from string parts, with optional `File` metadata.
    pub fn from_string_parts(parts: Vec<String>, blob_type: String, file_name: Option<String>, last_modified: Option<f64>) -> Self { let mut bytes: Vec<u8> = Vec::new(); for part in &parts { bytes.extend_from_slice(part.as_bytes()); } Self::with_metadata(bytes, blob_type, file_name, last_modified) }
    /// A blob from blob parts, with optional `File` metadata.
    pub fn from_blob_parts(parts: Vec<SmeltBlob>, blob_type: String, file_name: Option<String>, last_modified: Option<f64>) -> Self { let mut bytes: Vec<u8> = Vec::new(); for part in &parts { bytes.extend_from_slice(&part.bytes); } Self::with_metadata(bytes, blob_type, file_name, last_modified) }
    /// Pick the plain-blob or file constructor from the optional metadata.
    pub fn with_metadata(bytes: Vec<u8>, blob_type: String, file_name: Option<String>, last_modified: Option<f64>) -> Self { match file_name { Some(name) => Self::file_from_bytes(bytes, blob_type, name, last_modified.unwrap_or(0.0)), None => Self::from_bytes(bytes, blob_type) } }
    /// JS reference identity of this blob.
    pub fn id(&self) -> usize { self.id }
    /// Whether this blob is a `File`.
    pub fn is_file(&self) -> bool { self.file_name.is_some() }
    /// `size`: the byte count.
    pub fn size(&self) -> f64 { self.bytes.len() as f64 }
    /// `type`: the MIME type.
    pub fn blob_type(&self) -> String { self.blob_type.clone() }
    /// `name`: the file name, empty for a plain blob.
    pub fn file_name(&self) -> String { self.file_name.clone().unwrap_or_default() }
    /// `lastModified`: epoch milliseconds.
    pub fn last_modified(&self) -> f64 { self.last_modified }
    /// A copy of the blob's bytes.
    pub fn to_bytes(&self) -> Vec<u8> { self.bytes.as_ref().clone() }
    /// `text()`: the bytes UTF-8 decoded, U+FFFD for ill-formed input.
    pub fn to_text(&self) -> String { String::from_utf8_lossy(&self.bytes).into_owned() }
    //~ The spec's `slice` clamps a negative index from the end and an
    //~ out-of-range one to the bounds, then answers an EMPTY blob when the
    //~ range is inverted; and it never carries the source's file identity,
    //~ so slicing a `File` gives a `Blob`.
    /// `slice(start?, end?, contentType?)`: a new blob over a byte range.
    pub fn slice(&self, start: Option<f64>, end: Option<f64>, content_type: Option<String>) -> Self {
        let len = self.bytes.len() as f64;
        let clamp = |value: f64| -> usize { let resolved = if value < 0.0 { len + value } else { value }; resolved.clamp(0.0, len) as usize };
        let from = start.map_or(0, clamp);
        let to = end.map_or(self.bytes.len(), clamp);
        let range = if to > from { self.bytes[from..to].to_vec() } else { Vec::new() };
        Self::from_bytes(range, content_type.unwrap_or_default())
    }
}

// @smelt:item-end

// Emit the `Blob`/`File` dynamic-boundary adapters.
//
// The erased form is byte-identical to the record the pre-concrete runtime
// helper built, so every existing consumer of an erased blob is unaffected.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltBlob when blob & unknown
/// Erase a blob for a dynamic boundary.
///
/// The record shape has ONE definition, `smelt_blob_record`, shared with the
/// reflected host constructor, so a concrete blob and a reflectively-built one
/// erase to the same object.
impl IntoSmeltUnknown for SmeltBlob {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        let record = smelt_blob_record(self.id, &self.bytes, &self.blob_type, self.file_name.clone(), self.last_modified);
        if let (SmeltUnknown::Object(object), Some(class)) = (&record, self.instance_class) { object.insert("__smelt_class".to_owned(), SmeltUnknown::String(class.into())); }
        record
    }
}

//~ The constructor for `new Blob(parts, ..)` / `new File(parts, name, ..)`.
//~ It lives in the erasure section because its ARGUMENT is erased: a
//~ `BlobPart` is `Blob | BufferSource | string`, a heterogeneous host union
//~ whose arm is a runtime fact, so the parts array is the genuine dynamic
//~ boundary here. The byte walk itself is `smelt_blob_parts_bytes`, shared
//~ with the reflected-construction path so a directly-constructed blob and a
//~ reflectively-constructed one cannot disagree about their bytes.
#[allow(dead_code)]
impl SmeltBlob {
    /// Build a blob from an erased `BlobPart` array.
    pub fn from_parts_unknown(parts: SmeltUnknown, blob_type: String, file_name: Option<String>, last_modified: Option<f64>) -> Self {
        Self::with_metadata(smelt_blob_parts_bytes(parts), blob_type, file_name, last_modified)
    }
}

/// Rebuild a blob from an erased value.
impl SmeltFromUnknown for SmeltBlob {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        let SmeltUnknown::Object(map) = value else { return Self::new() };
        let bytes = smelt_blob_record_bytes(&map);
        let blob_type = match map.get("type") { Some(SmeltUnknown::String(text)) => text.to_string(), _ => String::new() };
        let name = match map.get("name") { Some(SmeltUnknown::String(text)) => Some(text.to_string()), _ => None };
        let last_modified = match map.get("lastModified") { Some(SmeltUnknown::Number(value)) => value, _ => 0.0 };
        let instance_class = match map.get("__smelt_class") { Some(SmeltUnknown::String(class)) => Some(class.to_string()), _ => None };
        let blob = match name { Some(name) => Self::file_from_bytes(bytes, blob_type, name, last_modified), None => Self::from_bytes(bytes, blob_type) };
        Self { instance_class, ..blob }
    }
}

// @smelt:item-end
