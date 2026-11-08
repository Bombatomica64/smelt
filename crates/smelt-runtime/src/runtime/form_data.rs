// Runtime prelude for the WHATWG `FormData` value and its body parsers.
//
// `FormData` is the third ordered name/value pair list Smelt models, after
// `Headers` and `URLSearchParams`, and it is deliberately the same shape: an
// `Rc<RefCell<Vec<(String, value)>>>` behind a JS reference identity. Two
// things separate it, and both are in the value type rather than in the list:
//
// * names are **case-sensitive** — a form's `"file"` and `"File"` are two
//   different names, where a header list folds them together;
// * an entry's value is **`string | File`**, which is why this type belongs
//   after the `Blob` work. Both arms are modeled Rust types, so the entry
//   value is the concrete two-arm `SmeltFormDataValue` and nothing about a
//   form crosses the dynamic boundary. Before `SmeltBlob` existed the file arm
//   would have had to be a `SmeltUnknown`, and every `get` would have been an
//   erasure.
//
// ## Reading a body as a form
//
// `Request.formData()` / `Response.formData()` accept the two encodings the
// spec's "extract a body" step produces:
//
// * `application/x-www-form-urlencoded`, decoded by `url::form_urlencoded` —
//   the same decoder `SmeltUrlSearchParams` uses, so the two cannot disagree
//   about `+`, `%XX` or a pair with no `=`;
// * `multipart/form-data` (RFC 7578), parsed by
//   [`emit_multipart_helpers`]'s generated helpers.
//
// **Why the multipart parser is written here rather than taken from a crate.**
// The obvious candidates are stream parsers: `multer` is built on
// `futures`/`bytes` and wants a `Stream<Item = Result<Bytes, E>>`, and the
// server-side `multipart` crates want a framework's request type. A
// `SmeltBody` is already a fully-buffered `Vec<u8>` by the time `formData()`
// runs, so none of that machinery applies: what is left is splitting on the
// boundary delimiter and reading each part's header block, which is the ~40
// generated lines below. Pulling in an async stream stack (and its runtime
// bound) to reach a `&[u8]` split would be the larger dependency AND the
// larger amount of code. `url` is already a dependency of these types, so the
// urlencoded half is delegated rather than hand-written.

// Emit `SmeltFormDataValue`, the `string | File` entry value.
// @smelt:item SmeltFormDataValue when form_data
/// A WHATWG form entry value: `string | File`.
///
/// Both arms are modeled concrete types, so a form entry never needs a
/// tagged runtime value. The file arm is a `SmeltBlob` whose name is
/// present, which is what a `File` is (see `StdlibClass::File`).
#[derive(Clone, PartialEq, Debug)]
pub enum SmeltFormDataValue {
    /// A text entry.
    Text(String),
    /// A file entry.
    File(SmeltBlob),
}

#[allow(dead_code)]
impl SmeltFormDataValue {
    /// The entry read as text: a text entry verbatim, a file's bytes
    /// decoded as UTF-8. This is what the urlencoded serialization and
    /// `String(value)` observe.
    pub fn to_text(&self) -> String { match self { Self::Text(text) => text.clone(), Self::File(file) => file.to_text() } }
    /// Whether this entry is a file.
    pub fn is_file(&self) -> bool { matches!(self, Self::File(_)) }
}

/// Build a form entry value from a blob, applying the spec's naming rule.
///
/// A `Blob` that is not a `File` becomes a `File` named `blob`; an
/// explicit `filename` argument replaces whatever name the value has.
/// That falls out of modeling a file as a blob whose name is present.
#[allow(dead_code)]
fn smelt_form_data_file_value(blob: SmeltBlob, filename: Option<impl SmeltHostText>) -> SmeltFormDataValue { let filename = filename.map(SmeltHostText::into_host_string);
    let existing = if blob.is_file() { Some(blob.file_name()) } else { None };
    let name = filename.or(existing).unwrap_or_else(|| "blob".to_owned());
    SmeltFormDataValue::File(SmeltBlob::with_metadata(blob.to_bytes(), blob.blob_type(), Some(name), Some(blob.last_modified())))
}

//~ Emit the `SmeltFormData` struct and its comparisons.
/// A WHATWG `FormData` entry list: ordered, case-sensitive
/// name/value pairs whose values are `string | File`.
#[derive(Clone)]
pub struct SmeltFormData {
    id: usize,
    /// Name/value pairs in insertion order.
    entries: ::std::rc::Rc<::std::cell::RefCell<Vec<(String, SmeltFormDataValue)>>>,
}

//~ Structural equality over the ordered pairs, like `SmeltUrlSearchParams`:
//~ two forms are the same entries when they carry the same pairs in order.
impl PartialEq for SmeltFormData { fn eq(&self, other: &Self) -> bool { *self.entries.borrow() == *other.entries.borrow() } }
//~ Node prints a form as `FormData { [Symbol(state)]: [ .. ] }`; the entry
//~ list is the part a program can actually observe, so that is what is
//~ shown.
impl ::std::fmt::Debug for SmeltFormData { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str("FormData ")?; formatter.debug_list().entries(self.entries.borrow().iter()).finish() } }
impl Default for SmeltFormData { fn default() -> Self { Self::new() } }

//~ Emit the WHATWG `FormData` operations as inherent methods.
#[allow(dead_code)]
impl SmeltFormData {
    /// An empty form with a fresh JS reference identity.
    pub fn new() -> Self { Self { id: smelt_next_object_id(), entries: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())) } }
    /// JS reference identity of this form.
    pub fn id(&self) -> usize { self.id }
    /// Build a form from name/value pairs, in order.
    pub fn from_pairs(pairs: Vec<(String, SmeltFormDataValue)>) -> Self { let form = Self::new(); form.entries.borrow_mut().extend(pairs); form }
    /// `get(name)`: the FIRST value for a name, or `null`.
    pub fn get(&self, name: &(impl SmeltHostText + ?Sized)) -> Option<SmeltFormDataValue> { let name = name.host_text(); let name: &str = &name; self.entries.borrow().iter().find(|(entry_name, _)| entry_name == name).map(|(_, value)| value.clone()) }
    /// `getAll(name)`: every value for a name, in order.
    pub fn get_all(&self, name: &(impl SmeltHostText + ?Sized)) -> Vec<SmeltFormDataValue> { let name = name.host_text(); let name: &str = &name; self.entries.borrow().iter().filter(|(entry_name, _)| entry_name == name).map(|(_, value)| value.clone()).collect() }
    /// `has(name)`.
    pub fn has(&self, name: &(impl SmeltHostText + ?Sized)) -> bool { let name = name.host_text(); let name: &str = &name; self.entries.borrow().iter().any(|(entry_name, _)| entry_name == name) }
    /// `append(name, value)`.
    pub fn append(&self, name: &(impl SmeltHostText + ?Sized), value: SmeltFormDataValue) { let name = name.host_text(); let name: &str = &name; self.entries.borrow_mut().push((name.to_owned(), value)); }
    /// `set(name, value)`: replace the first entry, drop the rest.
    ///
    /// Position-preserving, like the params list: the surviving entry
    /// stays where the first one was rather than moving to the end.
    pub fn set(&self, name: &(impl SmeltHostText + ?Sized), value: SmeltFormDataValue) { let name = name.host_text(); let name: &str = &name;
        let mut entries = self.entries.borrow_mut();
        let position = entries.iter().position(|(entry_name, _)| entry_name == name);
        let Some(index) = position else { entries.push((name.to_owned(), value)); return; };
        entries[index] = (name.to_owned(), value);
        let mut kept = false;
        entries.retain(|(entry_name, _)| { if entry_name != name { return true; } let first = !kept; kept = true; first });
    }
    /// `delete(name)`: remove every entry with the name.
    pub fn delete(&self, name: &(impl SmeltHostText + ?Sized)) { let name = name.host_text(); let name: &str = &name; self.entries.borrow_mut().retain(|(entry_name, _)| entry_name != name); }
    /// `entries()`: the pairs in insertion order.
    pub fn entries_in_order(&self) -> Vec<(String, SmeltFormDataValue)> { self.entries.borrow().clone() }
    /// `keys()`: one name per ENTRY, in insertion order.
    ///
    /// A duplicated name appears once per entry, because the spec's
    /// iterator walks the entry list rather than a set of names.
    pub fn keys(&self) -> Vec<String> { self.entries.borrow().iter().map(|(name, _)| name.clone()).collect() }
    /// `values()`: entry values in insertion order.
    pub fn values(&self) -> Vec<SmeltFormDataValue> { self.entries.borrow().iter().map(|(_, value)| value.clone()).collect() }
    /// The number of entries.
    pub fn size(&self) -> f64 { self.entries.borrow().len() as f64 }
    /// Parse `application/x-www-form-urlencoded` bytes into a form.
    ///
    /// Delegated to `url::form_urlencoded`, the same decoder
    /// `SmeltUrlSearchParams::from_query` uses. Every entry is text:
    /// this encoding cannot carry a file.
    pub fn from_urlencoded(bytes: &[u8]) -> Self {
        let pairs = url::form_urlencoded::parse(bytes).map(|(name, value): (::std::borrow::Cow<'_, str>, ::std::borrow::Cow<'_, str>)| (name.into_owned(), SmeltFormDataValue::Text(value.into_owned()))).collect::<Vec<(String, SmeltFormDataValue)>>();
        Self::from_pairs(pairs)
    }
    //~ The inverse of `from_multipart`, and it lives beside it so the two
    //~ cannot drift on the details that matter: CRLF line endings, the
    //~ `--boundary` / `--boundary--` framing, a `filename` parameter on a
    //~ file part and none on a text part, and a `Content-Type` header only
    //~ where the spec puts one.
    //~
    //~ The boundary is a PARAMETER rather than minted here, because it also
    //~ has to appear in the body's `Content-Type` header — the framing bytes
    //~ and the header have to agree, and only the caller holds both.
    /// Serialize this form as `multipart/form-data` bytes (RFC 7578).
    pub fn to_multipart(&self, boundary: &str) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::new();
        for (name, value) in self.entries_in_order() {
            bytes.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            match value {
                //~ A text part carries only its name. Node emits no
                //~ `Content-Type` for one, and the parser reads its
                //~ absence as text, so adding one here would make a
                //~ round trip through `formData()` answer a `File`.
                SmeltFormDataValue::Text(text) => {
                    bytes.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
                    bytes.extend_from_slice(text.as_bytes());
                }
                //~ A file part carries its `filename` and its type.
                //~ An empty type becomes `application/octet-stream` rather
                //~ than an empty header; the parser's missing-type default is `text/plain`.
                SmeltFormDataValue::File(file) => {
                    let file_name = file.file_name();
                    let content_type = if file.blob_type().is_empty() { "application/octet-stream".to_owned() } else { file.blob_type() };
                    bytes.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\n").as_bytes());
                    bytes.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
                    bytes.extend_from_slice(&file.to_bytes());
                }
            }
            bytes.extend_from_slice(b"\r\n");
        }
        bytes.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        bytes
    }
    /// Parse `multipart/form-data` bytes (RFC 7578) into a form.
    ///
    /// A part with a `filename` parameter on its `Content-Disposition`
    /// is a FILE entry, carrying the part's `Content-Type` (the RFC's
    /// default is `text/plain`); any other part is a text entry decoded
    /// as UTF-8. A part with no `name` parameter is skipped, as it names
    /// no form field.
    pub fn from_multipart(bytes: &[u8], boundary: &str) -> Self {
        let form = Self::new();
        for (headers, content) in smelt_multipart_parts(bytes, boundary) {
            let disposition = smelt_multipart_header(&headers, "content-disposition").unwrap_or_default();
            let Some(name) = smelt_multipart_parameter(&disposition, "name") else { continue };
            match smelt_multipart_parameter(&disposition, "filename") {
                Some(file_name) => { let content_type = smelt_multipart_header(&headers, "content-type").unwrap_or_else(|| "text/plain".to_owned()); form.append(&name, SmeltFormDataValue::File(SmeltBlob::with_metadata(content, content_type, Some(file_name), None))); }
                None => form.append(&name, SmeltFormDataValue::Text(String::from_utf8_lossy(&content).into_owned())),
            }
        }
        form
    }
}

//~ Emit the multipart splitting helpers the parser is built from.
//~
//~ Free `smelt_*` functions rather than methods: they are byte-level utilities
//~ over a `&[u8]`, they carry no form state, and keeping them out of the impl
//~ keeps [`emit_inherent_impl`]'s methods readable.
//~
//~ A fresh boundary per body, which is what the spec requires: it must not
//~ occur in the payload. The JS reference implementations mint a random one;
//~ this uses the same monotonic object-id counter the runtime already has,
//~ so it is unique within the program without pulling in a random source a
//~ program might not otherwise pay for. The leading `----` is the shape
//~ runtimes commonly use; `SmeltFormBoundary` is Smelt's own token. Together
//~ they keep the boundary out of ordinary text.
/// A fresh `multipart/form-data` boundary, unique in this program.
#[allow(dead_code)]
fn smelt_multipart_boundary() -> String { format!("----SmeltFormBoundary{:016x}", smelt_next_object_id()) }

/// Find `needle` in `haystack` at or after `from`.
#[allow(dead_code)]
fn smelt_find_bytes(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() { return None; }
    (from..=haystack.len().saturating_sub(needle.len())).find(|&start| &haystack[start..start + needle.len()] == needle)
}

/// Split multipart bytes into each part's (header lines, content).
///
/// The delimiter is `--<boundary>`; a part ends at the CRLF before the
/// next delimiter, and the header block ends at the first empty line.
/// The closing `--<boundary>--` delimiter has no part after it, and the
/// preamble before the first delimiter is discarded, both per RFC 7578.
#[allow(dead_code)]
fn smelt_multipart_parts(bytes: &[u8], boundary: &str) -> Vec<(Vec<String>, Vec<u8>)> {
    let delimiter = format!("--{boundary}").into_bytes();
    let mut parts: Vec<(Vec<String>, Vec<u8>)> = Vec::new();
    let Some(mut cursor) = smelt_find_bytes(bytes, &delimiter, 0) else { return parts; };
    loop {
        cursor += delimiter.len();
        //~ `--` right after the delimiter is the closing one.
        if bytes.get(cursor..cursor + 2) == Some(b"--") { return parts; }
        let Some(header_end) = smelt_find_bytes(bytes, b"\r\n\r\n", cursor) else { return parts; };
        let headers = String::from_utf8_lossy(&bytes[cursor..header_end]).lines().map(|line| line.trim().to_owned()).filter(|line| !line.is_empty()).collect::<Vec<String>>();
        let content_start = header_end + 4;
        let next = smelt_find_bytes(bytes, &delimiter, content_start);
        let content_end = next.map_or(bytes.len(), |start| start.saturating_sub(2));
        parts.push((headers, bytes[content_start..content_end.max(content_start)].to_vec()));
        let Some(next) = next else { return parts; };
        cursor = next;
    }
}

/// Read one part header's value, matching the name case-insensitively.
#[allow(dead_code)]
fn smelt_multipart_header(headers: &[String], name: &str) -> Option<String> {
    headers.iter().find_map(|line| { let (header_name, value) = line.split_once(':')?; if header_name.trim().eq_ignore_ascii_case(name) { Some(value.trim().to_owned()) } else { None } })
}

/// Read a `name="value"` parameter out of a header value.
///
/// RFC 7578 sends `name` and `filename` as quoted strings, so an
/// unquoted form is accepted too but the quoted one is what is written.
#[allow(dead_code)]
fn smelt_multipart_parameter(header: &str, parameter: &str) -> Option<String> {
    for piece in header.split(';') {
        let piece = piece.trim();
        let Some((key, value)) = piece.split_once('=') else { continue };
        if !key.trim().eq_ignore_ascii_case(parameter) { continue; }
        let value = value.trim();
        return Some(value.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')).unwrap_or(value).to_owned());
    }
    None
}

//~ Emit the `Content-Type`-driven body reader `formData()` lowers to.
//~
//~ A free function rather than a `SmeltBody` method: the encoding is decided by
//~ the `Content-Type` HEADER, which belongs to the `Request`/`Response` and not
//~ to the body, so the caller passes both. Keeping it here also keeps
//~ `SmeltBody` free of a type it would otherwise have to be emitted alongside.
//~
//~ The `Content-Type` that names neither encoding is Node's own `TypeError`,
//~ message included; it is branded exactly like the double-read throw, and for
//~ the same reason (see `fetch_types_prelude`'s `take_bytes`): with the erased
//~ carrier a source `catch` sees `error.name === "TypeError"`, and without it
//~ there are no erased values to inspect anyway.
/// Parse body bytes as a form, choosing the parser by `Content-Type`.
#[allow(dead_code)]
fn smelt_form_data_from_body(content_type: Option<String>, bytes: Vec<u8>) -> Result<SmeltFormData, Box<dyn ::std::error::Error>> {
    let content_type = content_type.unwrap_or_default();
    let essence = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if essence == "multipart/form-data" {
        let boundary = smelt_multipart_parameter(&content_type, "boundary").unwrap_or_default();
        if boundary.is_empty() {
// @smelt:if unknown
            return Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("TypeError".into())), ("message".to_owned(), SmeltUnknown::String("Content-Type was not one of \"multipart/form-data\" or \"application/x-www-form-urlencoded\".".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)])))));
// @smelt:else
//=             return Err(Box::<dyn ::std::error::Error>::from("TypeError: Content-Type was not one of \"multipart/form-data\" or \"application/x-www-form-urlencoded\"."));
// @smelt:end
        }
        return Ok(SmeltFormData::from_multipart(&bytes, &boundary));
    }
    if essence == "application/x-www-form-urlencoded" {
        return Ok(SmeltFormData::from_urlencoded(&bytes));
    }
// @smelt:if unknown
    return Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("TypeError".into())), ("message".to_owned(), SmeltUnknown::String("Content-Type was not one of \"multipart/form-data\" or \"application/x-www-form-urlencoded\".".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)])))));
// @smelt:else
//=     return Err(Box::<dyn ::std::error::Error>::from("TypeError: Content-Type was not one of \"multipart/form-data\" or \"application/x-www-form-urlencoded\"."));
// @smelt:end
}

// @smelt:item-end

// Emit the `FormData` dynamic-boundary adapters.
//
// Same boundary shape as `SmeltHeaders` and `SmeltUrlSearchParams`: an erased
// form is the marker record
// `{ "__smelt_formdata": true, "entries": [[name, value], ..] }`. A DYNAMIC
// BOUNDARY adapter only — the internal representation stays concrete, and a
// file entry crosses as its erased blob so the receiving side can still read
// the name and the bytes.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltFormData when form_data & unknown
/// Erase a form for a dynamic boundary.
impl IntoSmeltUnknown for SmeltFormData {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        //~ Retain the live form under the record's id, so an erased view is
        //~ a view of the SAME form: an `append` through one has to be
        //~ visible on the value the program still holds, and a method
        //~ resolved off the record (`smelt_host_method`) acts on it.
        let smelt_host_origin = self.clone();
        let pairs: Vec<SmeltUnknown> = self.entries_in_order().into_iter().map(|(name, value)| { let erased_value = match value { SmeltFormDataValue::Text(text) => SmeltUnknown::String(text.into()), SmeltFormDataValue::File(file) => file.into_smelt_unknown() }; SmeltUnknown::Array(Vec::from([SmeltUnknown::String(name.into()), erased_value]).into()) }).collect();
        smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([("__smelt_formdata".to_owned(), SmeltUnknown::Bool(true)), ("entries".to_owned(), SmeltUnknown::Array(pairs.into()))]))), smelt_host_origin)
    }
}

/// The modeled members of an erased `FormData` record, resolved at run time.
///
/// **Dynamic boundary.** The receiver is a marker-bearing record, so the
/// member it carries is decided by the record's marker and the member NAME,
/// both of which are runtime values here — a program reaches this only by
/// erasing the value on purpose (`as any`, an `any`-typed field), since every
/// ordinary spelling keeps its type through narrowing. Answering `undefined`
/// instead, which is what a plain property read does, was a silent wrong
/// value: `(headers as any).get('a')` gave `null` where Node gives the header.
///
/// The recovered value is the SAME one the record was erased from (the origin
/// registry), so a mutating member is observed by the holder of the concrete
/// value. Only the synchronous members are here; the async body readers are
/// not, and they keep the erased read's `undefined`.
fn smelt_form_data_host_method(object: &SmeltObject, name: &str) -> Option<SmeltUnknown> { if !object.contains_key("__smelt_formdata") { return None; } if !matches!(name, "get" | "getAll" | "has" | "set" | "append" | "delete" | "keys" | "values" | "entries") { return None; } let form = <SmeltFormData as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(object.clone())); let method = name.to_owned(); Some(SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let arg = |index: usize| args.get(index).cloned().map_or_else(String::new, smelt_property_key); let entry_value = |value: SmeltFormDataValue| match value { SmeltFormDataValue::Text(text) => SmeltUnknown::String(text.into()), SmeltFormDataValue::File(file) => file.into_smelt_unknown() }; Ok(match method.as_str() { "get" => form.get(&arg(0)).map_or(SmeltUnknown::Null, entry_value), "getAll" => SmeltUnknown::Array(form.get_all(&arg(0)).into_iter().map(entry_value).collect::<Vec<_>>().into()), "has" => SmeltUnknown::Bool(form.has(&arg(0))), "set" => { form.set(&arg(0), SmeltFormDataValue::Text(arg(1))); SmeltUnknown::Undefined }, "append" => { form.append(&arg(0), SmeltFormDataValue::Text(arg(1))); SmeltUnknown::Undefined }, "delete" => { form.delete(&arg(0)); SmeltUnknown::Undefined }, "keys" => SmeltUnknown::Array(form.keys().into_iter().map(|value| SmeltUnknown::String(value.into())).collect::<Vec<_>>().into()), "values" => SmeltUnknown::Array(form.values().into_iter().map(entry_value).collect::<Vec<_>>().into()), _ => SmeltUnknown::Array(form.entries_in_order().into_iter().map(|(entry_name, value)| SmeltUnknown::Array(Vec::from([SmeltUnknown::String(entry_name.into()), entry_value(value)]).into())).collect::<Vec<_>>().into()), }) }))) }

/// Rebuild a form from an erased value.
impl SmeltFromUnknown for SmeltFormData {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        //~ The retained origin first; the structural rebuild below is
        //~ for a record that did not come from an erasure.
        if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }
        let SmeltUnknown::Object(map) = value else { return Self::new() };
        let Some(SmeltUnknown::Array(pairs)) = map.get("entries") else { return Self::new() };
        let form = Self::new();
        for pair in pairs.into_vec() {
            let SmeltUnknown::Array(pair) = pair else { continue };
            let pair = pair.into_vec();
            let (Some(SmeltUnknown::String(name)), Some(entry_value)) = (pair.first().cloned(), pair.get(1).cloned()) else { continue };
            let value = match entry_value { SmeltUnknown::String(text) => SmeltFormDataValue::Text(text.to_string()), other => SmeltFormDataValue::File(SmeltFromUnknown::smelt_from_unknown(other)) };
            form.append(&name.to_string_lossy(), value);
        }
        form
    }
}

// @smelt:item-end
