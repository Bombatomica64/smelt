//! Runtime prelude for the WHATWG `FormData` value and its body parsers.
//!
//! `FormData` is the third ordered name/value pair list Smelt models, after
//! `Headers` and `URLSearchParams`, and it is deliberately the same shape: an
//! `Rc<RefCell<Vec<(String, value)>>>` behind a JS reference identity. Two
//! things separate it, and both are in the value type rather than in the list:
//!
//! * names are **case-sensitive** — a form's `"file"` and `"File"` are two
//!   different names, where a header list folds them together;
//! * an entry's value is **`string | File`**, which is why this type belongs
//!   after the `Blob` work. Both arms are modeled Rust types, so the entry
//!   value is the concrete two-arm `SmeltFormDataValue` and nothing about a
//!   form crosses the dynamic boundary. Before `SmeltBlob` existed the file arm
//!   would have had to be a `SmeltUnknown`, and every `get` would have been an
//!   erasure.
//!
//! ## Reading a body as a form
//!
//! `Request.formData()` / `Response.formData()` accept the two encodings the
//! spec's "extract a body" step produces:
//!
//! * `application/x-www-form-urlencoded`, decoded by `url::form_urlencoded` —
//!   the same decoder `SmeltUrlSearchParams` uses, so the two cannot disagree
//!   about `+`, `%XX` or a pair with no `=`;
//! * `multipart/form-data` (RFC 7578), parsed by
//!   [`emit_multipart_helpers`]'s generated helpers.
//!
//! **Why the multipart parser is written here rather than taken from a crate.**
//! The obvious candidates are stream parsers: `multer` is built on
//! `futures`/`bytes` and wants a `Stream<Item = Result<Bytes, E>>`, and the
//! server-side `multipart` crates want a framework's request type. A
//! `SmeltBody` is already a fully-buffered `Vec<u8>` by the time `formData()`
//! runs, so none of that machinery applies: what is left is splitting on the
//! boundary delimiter and reading each part's header block, which is the ~40
//! generated lines below. Pulling in an async stream stack (and its runtime
//! bound) to reach a `&[u8]` split would be the larger dependency AND the
//! larger amount of code. `url` is already a dependency of these types, so the
//! urlencoded half is delegated rather than hand-written.

use crate::rust::CodeWriter;

/// Emit the `SmeltFormData` runtime type and its parsers.
///
/// `needs_unknown` gates the erasure adapters, exactly as the other fetch
/// types' preludes do: a program that never crosses the dynamic boundary does
/// not emit the `SmeltUnknown` carrier, so the impls must not be emitted.
pub fn emit(writer: &mut CodeWriter, needs_unknown: bool) {
    emit_value_enum(writer);
    emit_struct(writer);
    emit_inherent_impl(writer);
    emit_multipart_helpers(writer);
    emit_body_reader(writer, needs_unknown);
    emit_traits(writer, needs_unknown);
}

/// Emit the `Content-Type`-driven body reader `formData()` lowers to.
///
/// A free function rather than a `SmeltBody` method: the encoding is decided by
/// the `Content-Type` HEADER, which belongs to the `Request`/`Response` and not
/// to the body, so the caller passes both. Keeping it here also keeps
/// `SmeltBody` free of a type it would otherwise have to be emitted alongside.
///
/// The `Content-Type` that names neither encoding is Node's own `TypeError`,
/// message included; it is branded exactly like the double-read throw, and for
/// the same reason (see `fetch_types_prelude`'s `take_bytes`): with the erased
/// carrier a source `catch` sees `error.name === "TypeError"`, and without it
/// there are no erased values to inspect anyway.
fn emit_body_reader(writer: &mut CodeWriter, needs_unknown: bool) {
    let unsupported_type = if needs_unknown {
        format!(
            "return Err({});",
            crate::thrown::throw_expr(&crate::thrown::error_payload_record_expr(
                "TypeError",
                "\"Content-Type was not one of \\\"multipart/form-data\\\" or \\\"application/x-www-form-urlencoded\\\".\""
            ))
        )
    } else {
        "return Err(Box::<dyn ::std::error::Error>::from(\"TypeError: Content-Type was not one of \\\"multipart/form-data\\\" or \\\"application/x-www-form-urlencoded\\\".\"));"
            .to_owned()
    };
    writer.line("/// Parse body bytes as a form, choosing the parser by `Content-Type`.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_form_data_from_body(content_type: Option<String>, bytes: Vec<u8>) -> Result<SmeltFormData, Box<dyn ::std::error::Error>>",
        |fn_writer| {
            fn_writer.line("let content_type = content_type.unwrap_or_default();");
            fn_writer.line("let essence = content_type.split(';').next().unwrap_or(\"\").trim().to_ascii_lowercase();");
            fn_writer.block("if essence == \"multipart/form-data\"", |arm_writer| {
                arm_writer.line(
                    "let boundary = smelt_multipart_parameter(&content_type, \"boundary\").unwrap_or_default();",
                );
                arm_writer.block("if boundary.is_empty()", |inner_writer| {
                    inner_writer.line(&unsupported_type);
                });
                arm_writer.line("return Ok(SmeltFormData::from_multipart(&bytes, &boundary));");
            });
            fn_writer.block(
                "if essence == \"application/x-www-form-urlencoded\"",
                |arm_writer| {
                    arm_writer.line("return Ok(SmeltFormData::from_urlencoded(&bytes));");
                },
            );
            fn_writer.line(&unsupported_type);
        },
    );
    writer.blank_line();
}

/// Emit `SmeltFormDataValue`, the `string | File` entry value.
fn emit_value_enum(writer: &mut CodeWriter) {
    writer.line("/// A WHATWG form entry value: `string | File`.");
    writer.line("///");
    writer.line("/// Both arms are modeled concrete types, so a form entry never needs a");
    writer.line("/// tagged runtime value. The file arm is a `SmeltBlob` whose name is");
    writer.line("/// present, which is what a `File` is (see `StdlibClass::File`).");
    writer.line("#[derive(Clone, PartialEq, Debug)]");
    writer.block("pub enum SmeltFormDataValue", |enum_writer| {
        enum_writer.line("/// A text entry.");
        enum_writer.line("Text(String),");
        enum_writer.line("/// A file entry.");
        enum_writer.line("File(SmeltBlob),");
    });
    writer.blank_line();
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltFormDataValue", |impl_writer| {
        impl_writer.line("/// The entry read as text: a text entry verbatim, a file's bytes");
        impl_writer.line("/// decoded as UTF-8. This is what the urlencoded serialization and");
        impl_writer.line("/// `String(value)` observe.");
        impl_writer.line(
            "pub fn to_text(&self) -> String { match self { Self::Text(text) => text.clone(), Self::File(file) => file.to_text() } }",
        );
        impl_writer.line("/// Whether this entry is a file.");
        impl_writer.line(
            "pub fn is_file(&self) -> bool { matches!(self, Self::File(_)) }",
        );
    });
    writer.blank_line();
    writer.line("/// Build a form entry value from a blob, applying the spec's naming rule.");
    writer.line("///");
    writer.line("/// A `Blob` that is not a `File` becomes a `File` named `blob`; an");
    writer.line("/// explicit `filename` argument replaces whatever name the value has.");
    writer.line("/// That falls out of modeling a file as a blob whose name is present.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_form_data_file_value(blob: SmeltBlob, filename: Option<String>) -> SmeltFormDataValue",
        |fn_writer| {
            fn_writer.line(
                "let existing = if blob.is_file() { Some(blob.file_name()) } else { None };",
            );
            fn_writer.line(
                "let name = filename.or(existing).unwrap_or_else(|| \"blob\".to_owned());",
            );
            fn_writer.line(
                "SmeltFormDataValue::File(SmeltBlob::with_metadata(blob.to_bytes(), blob.blob_type(), Some(name), Some(blob.last_modified())))",
            );
        },
    );
    writer.blank_line();
}

/// Emit the `SmeltFormData` struct and its comparisons.
fn emit_struct(writer: &mut CodeWriter) {
    writer.line("/// A WHATWG `FormData` entry list: ordered, case-sensitive");
    writer.line("/// name/value pairs whose values are `string | File`.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltFormData", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("/// Name/value pairs in insertion order.");
        struct_writer.line(
            "entries: ::std::rc::Rc<::std::cell::RefCell<Vec<(String, SmeltFormDataValue)>>>,",
        );
    });
    writer.blank_line();
    // Structural equality over the ordered pairs, like `SmeltUrlSearchParams`:
    // two forms are the same entries when they carry the same pairs in order.
    writer.line(
        "impl PartialEq for SmeltFormData { fn eq(&self, other: &Self) -> bool { *self.entries.borrow() == *other.entries.borrow() } }",
    );
    // Node prints a form as `FormData { [Symbol(state)]: [ .. ] }`; the entry
    // list is the part a program can actually observe, so that is what is
    // shown.
    writer.line(
        "impl ::std::fmt::Debug for SmeltFormData { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(\"FormData \")?; formatter.debug_list().entries(self.entries.borrow().iter()).finish() } }",
    );
    writer.line("impl Default for SmeltFormData { fn default() -> Self { Self::new() } }");
    writer.blank_line();
}

/// Emit the WHATWG `FormData` operations as inherent methods.
fn emit_inherent_impl(writer: &mut CodeWriter) {
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltFormData", |impl_writer| {
        impl_writer.line("/// An empty form with a fresh JS reference identity.");
        impl_writer.line(
            "pub fn new() -> Self { Self { id: smelt_next_object_id(), entries: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())) } }",
        );
        impl_writer.line("/// JS reference identity of this form.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// Build a form from name/value pairs, in order.");
        impl_writer.line(
            "pub fn from_pairs(pairs: Vec<(String, SmeltFormDataValue)>) -> Self { let form = Self::new(); form.entries.borrow_mut().extend(pairs); form }",
        );
        impl_writer.line("/// `get(name)`: the FIRST value for a name, or `null`.");
        impl_writer.line(
            "pub fn get(&self, name: &str) -> Option<SmeltFormDataValue> { self.entries.borrow().iter().find(|(entry_name, _)| entry_name == name).map(|(_, value)| value.clone()) }",
        );
        impl_writer.line("/// `getAll(name)`: every value for a name, in order.");
        impl_writer.line(
            "pub fn get_all(&self, name: &str) -> Vec<SmeltFormDataValue> { self.entries.borrow().iter().filter(|(entry_name, _)| entry_name == name).map(|(_, value)| value.clone()).collect() }",
        );
        impl_writer.line("/// `has(name)`.");
        impl_writer.line(
            "pub fn has(&self, name: &str) -> bool { self.entries.borrow().iter().any(|(entry_name, _)| entry_name == name) }",
        );
        impl_writer.line("/// `append(name, value)`.");
        impl_writer.line(
            "pub fn append(&self, name: &str, value: SmeltFormDataValue) { self.entries.borrow_mut().push((name.to_owned(), value)); }",
        );
        impl_writer.line("/// `set(name, value)`: replace the first entry, drop the rest.");
        impl_writer.line("///");
        impl_writer.line("/// Position-preserving, like the params list: the surviving entry");
        impl_writer.line("/// stays where the first one was rather than moving to the end.");
        impl_writer.block(
            "pub fn set(&self, name: &str, value: SmeltFormDataValue)",
            |fn_writer| {
                fn_writer.line("let mut entries = self.entries.borrow_mut();");
                fn_writer.line(
                    "let position = entries.iter().position(|(entry_name, _)| entry_name == name);",
                );
                fn_writer.line(
                    "let Some(index) = position else { entries.push((name.to_owned(), value)); return; };",
                );
                fn_writer.line("entries[index] = (name.to_owned(), value);");
                fn_writer.line("let mut kept = false;");
                fn_writer.line(
                    "entries.retain(|(entry_name, _)| { if entry_name != name { return true; } let first = !kept; kept = true; first });",
                );
            },
        );
        impl_writer.line("/// `delete(name)`: remove every entry with the name.");
        impl_writer.line(
            "pub fn delete(&self, name: &str) { self.entries.borrow_mut().retain(|(entry_name, _)| entry_name != name); }",
        );
        impl_writer.line("/// `entries()`: the pairs in insertion order.");
        impl_writer.line(
            "pub fn entries_in_order(&self) -> Vec<(String, SmeltFormDataValue)> { self.entries.borrow().clone() }",
        );
        impl_writer.line("/// `keys()`: one name per ENTRY, in insertion order.");
        impl_writer.line("///");
        impl_writer.line("/// A duplicated name appears once per entry, because the spec's");
        impl_writer.line("/// iterator walks the entry list rather than a set of names.");
        impl_writer.line(
            "pub fn keys(&self) -> Vec<String> { self.entries.borrow().iter().map(|(name, _)| name.clone()).collect() }",
        );
        impl_writer.line("/// `values()`: entry values in insertion order.");
        impl_writer.line(
            "pub fn values(&self) -> Vec<SmeltFormDataValue> { self.entries.borrow().iter().map(|(_, value)| value.clone()).collect() }",
        );
        impl_writer.line("/// The number of entries.");
        impl_writer.line("pub fn size(&self) -> f64 { self.entries.borrow().len() as f64 }");
        impl_writer.line("/// Parse `application/x-www-form-urlencoded` bytes into a form.");
        impl_writer.line("///");
        impl_writer.line("/// Delegated to `url::form_urlencoded`, the same decoder");
        impl_writer.line("/// `SmeltUrlSearchParams::from_query` uses. Every entry is text:");
        impl_writer.line("/// this encoding cannot carry a file.");
        impl_writer.block(
            "pub fn from_urlencoded(bytes: &[u8]) -> Self",
            |fn_writer| {
                fn_writer.line(
                    "let pairs = url::form_urlencoded::parse(bytes).map(|(name, value): (::std::borrow::Cow<'_, str>, ::std::borrow::Cow<'_, str>)| (name.into_owned(), SmeltFormDataValue::Text(value.into_owned()))).collect::<Vec<(String, SmeltFormDataValue)>>();",
                );
                fn_writer.line("Self::from_pairs(pairs)");
            },
        );
        impl_writer.line("/// Parse `multipart/form-data` bytes (RFC 7578) into a form.");
        impl_writer.line("///");
        impl_writer.line("/// A part with a `filename` parameter on its `Content-Disposition`");
        impl_writer.line("/// is a FILE entry, carrying the part's `Content-Type` (the RFC's");
        impl_writer.line("/// default is `text/plain`); any other part is a text entry decoded");
        impl_writer.line("/// as UTF-8. A part with no `name` parameter is skipped, as it names");
        impl_writer.line("/// no form field.");
        impl_writer.block(
            "pub fn from_multipart(bytes: &[u8], boundary: &str) -> Self",
            |fn_writer| {
                fn_writer.line("let form = Self::new();");
                fn_writer.block(
                    "for (headers, content) in smelt_multipart_parts(bytes, boundary)",
                    |loop_writer| {
                        loop_writer.line(
                            "let disposition = smelt_multipart_header(&headers, \"content-disposition\").unwrap_or_default();",
                        );
                        loop_writer.line(
                            "let Some(name) = smelt_multipart_parameter(&disposition, \"name\") else { continue };",
                        );
                        loop_writer.block(
                            "match smelt_multipart_parameter(&disposition, \"filename\")",
                            |match_writer| {
                                match_writer.line(
                                    "Some(file_name) => { let content_type = smelt_multipart_header(&headers, \"content-type\").unwrap_or_else(|| \"text/plain\".to_owned()); form.append(&name, SmeltFormDataValue::File(SmeltBlob::with_metadata(content, content_type, Some(file_name), None))); }",
                                );
                                match_writer.line(
                                    "None => form.append(&name, SmeltFormDataValue::Text(String::from_utf8_lossy(&content).into_owned())),",
                                );
                            },
                        );
                    },
                );
                fn_writer.line("form");
            },
        );
    });
    writer.blank_line();
}

/// Emit the multipart splitting helpers the parser is built from.
///
/// Free `smelt_*` functions rather than methods: they are byte-level utilities
/// over a `&[u8]`, they carry no form state, and keeping them out of the impl
/// keeps [`emit_inherent_impl`]'s methods readable.
fn emit_multipart_helpers(writer: &mut CodeWriter) {
    writer.line("/// Find `needle` in `haystack` at or after `from`.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_find_bytes(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize>",
        |fn_writer| {
            fn_writer.line("if needle.is_empty() || haystack.len() < needle.len() { return None; }");
            fn_writer.line(
                "(from..=haystack.len().saturating_sub(needle.len())).find(|&start| &haystack[start..start + needle.len()] == needle)",
            );
        },
    );
    writer.blank_line();
    writer.line("/// Split multipart bytes into each part's (header lines, content).");
    writer.line("///");
    writer.line("/// The delimiter is `--<boundary>`; a part ends at the CRLF before the");
    writer.line("/// next delimiter, and the header block ends at the first empty line.");
    writer.line("/// The closing `--<boundary>--` delimiter has no part after it, and the");
    writer.line("/// preamble before the first delimiter is discarded, both per RFC 7578.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_multipart_parts(bytes: &[u8], boundary: &str) -> Vec<(Vec<String>, Vec<u8>)>",
        |fn_writer| {
            fn_writer.line("let delimiter = format!(\"--{boundary}\").into_bytes();");
            fn_writer.line("let mut parts: Vec<(Vec<String>, Vec<u8>)> = Vec::new();");
            fn_writer
                .line("let Some(mut cursor) = smelt_find_bytes(bytes, &delimiter, 0) else { return parts; };");
            fn_writer.block("loop", |loop_writer| {
                loop_writer.line("cursor += delimiter.len();");
                // `--` right after the delimiter is the closing one.
                loop_writer.line(
                    "if bytes.get(cursor..cursor + 2) == Some(b\"--\") { return parts; }",
                );
                loop_writer.line(
                    "let Some(header_end) = smelt_find_bytes(bytes, b\"\\r\\n\\r\\n\", cursor) else { return parts; };",
                );
                loop_writer.line(
                    "let headers = String::from_utf8_lossy(&bytes[cursor..header_end]).lines().map(|line| line.trim().to_owned()).filter(|line| !line.is_empty()).collect::<Vec<String>>();",
                );
                loop_writer.line("let content_start = header_end + 4;");
                loop_writer.line(
                    "let next = smelt_find_bytes(bytes, &delimiter, content_start);",
                );
                loop_writer.line(
                    "let content_end = next.map_or(bytes.len(), |start| start.saturating_sub(2));",
                );
                loop_writer.line(
                    "parts.push((headers, bytes[content_start..content_end.max(content_start)].to_vec()));",
                );
                loop_writer.line("let Some(next) = next else { return parts; };");
                loop_writer.line("cursor = next;");
            });
        },
    );
    writer.blank_line();
    writer.line("/// Read one part header's value, matching the name case-insensitively.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_multipart_header(headers: &[String], name: &str) -> Option<String>",
        |fn_writer| {
            fn_writer.line(
                "headers.iter().find_map(|line| { let (header_name, value) = line.split_once(':')?; if header_name.trim().eq_ignore_ascii_case(name) { Some(value.trim().to_owned()) } else { None } })",
            );
        },
    );
    writer.blank_line();
    writer.line("/// Read a `name=\"value\"` parameter out of a header value.");
    writer.line("///");
    writer.line("/// RFC 7578 sends `name` and `filename` as quoted strings, so an");
    writer.line("/// unquoted form is accepted too but the quoted one is what is written.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_multipart_parameter(header: &str, parameter: &str) -> Option<String>",
        |fn_writer| {
            fn_writer.block("for piece in header.split(';')", |loop_writer| {
                loop_writer.line("let piece = piece.trim();");
                loop_writer
                    .line("let Some((key, value)) = piece.split_once('=') else { continue };");
                loop_writer.line("if !key.trim().eq_ignore_ascii_case(parameter) { continue; }");
                loop_writer.line("let value = value.trim();");
                loop_writer.line(
                    "return Some(value.strip_prefix('\"').and_then(|rest| rest.strip_suffix('\"')).unwrap_or(value).to_owned());",
                );
            });
            fn_writer.line("None");
        },
    );
    writer.blank_line();
}

/// Emit the `FormData` dynamic-boundary adapters.
///
/// Same boundary shape as `SmeltHeaders` and `SmeltUrlSearchParams`: an erased
/// form is the marker record
/// `{ "__smelt_formdata": true, "entries": [[name, value], ..] }`. A DYNAMIC
/// BOUNDARY adapter only — the internal representation stays concrete, and a
/// file entry crosses as its erased blob so the receiving side can still read
/// the name and the bytes.
fn emit_traits(writer: &mut CodeWriter, needs_unknown: bool) {
    if !needs_unknown {
        return;
    }
    writer.line("/// Erase a form for a dynamic boundary.");
    writer.block("impl IntoSmeltUnknown for SmeltFormData", |impl_writer| {
        impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
            // Retain the live form under the record's id, so an erased view is
            // a view of the SAME form: an `append` through one has to be
            // visible on the value the program still holds, and a method
            // resolved off the record (`smelt_host_method`) acts on it.
            fn_writer.line("smelt_register_host_origin(self.id, self.clone());");
            fn_writer.line(
                "let pairs: Vec<SmeltUnknown> = self.entries_in_order().into_iter().map(|(name, value)| { let erased_value = match value { SmeltFormDataValue::Text(text) => SmeltUnknown::String(text.into()), SmeltFormDataValue::File(file) => file.into_smelt_unknown() }; SmeltUnknown::Array(Vec::from([SmeltUnknown::String(name.into()), erased_value]).into()) }).collect();",
            );
            fn_writer.line(
                "SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([(\"__smelt_formdata\".to_owned(), SmeltUnknown::Bool(true)), (\"entries\".to_owned(), SmeltUnknown::Array(pairs.into()))])))",
            );
        });
    });
    writer.blank_line();
    writer.line("/// The modeled members of an erased `FormData` record, resolved at run time.");
    writer.line("///");
    writer.line("/// **Dynamic boundary.** The receiver is a marker-bearing record, so the");
    writer.line("/// member it carries is decided by the record's marker and the member NAME,");
    writer.line("/// both of which are runtime values here — a program reaches this only by");
    writer.line("/// erasing the value on purpose (`as any`, an `any`-typed field), since every");
    writer.line("/// ordinary spelling keeps its type through narrowing. Answering `undefined`");
    writer.line("/// instead, which is what a plain property read does, was a silent wrong");
    writer.line("/// value: `(headers as any).get('a')` gave `null` where Node gives the header.");
    writer.line("///");
    writer.line("/// The recovered value is the SAME one the record was erased from (the origin");
    writer.line("/// registry), so a mutating member is observed by the holder of the concrete");
    writer.line("/// value. Only the synchronous members are here; the async body readers are");
    writer.line("/// not, and they keep the erased read's `undefined`.");
    writer.line("fn smelt_form_data_host_method(object: &SmeltObject, name: &str) -> Option<SmeltUnknown> { if !object.contains_key(\"__smelt_formdata\") { return None; } if !matches!(name, \"get\" | \"getAll\" | \"has\" | \"set\" | \"append\" | \"delete\" | \"keys\" | \"values\" | \"entries\") { return None; } let form = <SmeltFormData as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(object.clone())); let method = name.to_owned(); Some(SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let arg = |index: usize| args.get(index).cloned().map_or_else(String::new, smelt_property_key); let entry_value = |value: SmeltFormDataValue| match value { SmeltFormDataValue::Text(text) => SmeltUnknown::String(text.into()), SmeltFormDataValue::File(file) => file.into_smelt_unknown() }; Ok(match method.as_str() { \"get\" => form.get(&arg(0)).map_or(SmeltUnknown::Null, entry_value), \"getAll\" => SmeltUnknown::Array(form.get_all(&arg(0)).into_iter().map(entry_value).collect::<Vec<_>>().into()), \"has\" => SmeltUnknown::Bool(form.has(&arg(0))), \"set\" => { form.set(&arg(0), SmeltFormDataValue::Text(arg(1))); SmeltUnknown::Undefined }, \"append\" => { form.append(&arg(0), SmeltFormDataValue::Text(arg(1))); SmeltUnknown::Undefined }, \"delete\" => { form.delete(&arg(0)); SmeltUnknown::Undefined }, \"keys\" => SmeltUnknown::Array(form.keys().into_iter().map(|value| SmeltUnknown::String(value.into())).collect::<Vec<_>>().into()), \"values\" => SmeltUnknown::Array(form.values().into_iter().map(entry_value).collect::<Vec<_>>().into()), _ => SmeltUnknown::Array(form.entries_in_order().into_iter().map(|(entry_name, value)| SmeltUnknown::Array(Vec::from([SmeltUnknown::String(entry_name.into()), entry_value(value)]).into())).collect::<Vec<_>>().into()), }) }))) }");
    writer.blank_line();
    writer.line("/// Rebuild a form from an erased value.");
    writer.block("impl SmeltFromUnknown for SmeltFormData", |impl_writer| {
        impl_writer.block(
            "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
            |fn_writer| {
                // The retained origin first; the structural rebuild below is
                // for a record that did not come from an erasure.
                fn_writer.line(
                    "if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }",
                );
                fn_writer
                    .line("let SmeltUnknown::Object(map) = value else { return Self::new() };");
                fn_writer.line(
                    "let Some(SmeltUnknown::Array(pairs)) = map.get(\"entries\") else { return Self::new() };",
                );
                fn_writer.line("let form = Self::new();");
                fn_writer.block("for pair in pairs.into_vec()", |loop_writer| {
                    loop_writer.line("let SmeltUnknown::Array(pair) = pair else { continue };");
                    loop_writer.line("let pair = pair.into_vec();");
                    loop_writer.line(
                        "let (Some(SmeltUnknown::String(name)), Some(entry_value)) = (pair.first().cloned(), pair.get(1).cloned()) else { continue };",
                    );
                    loop_writer.line(
                        "let value = match entry_value { SmeltUnknown::String(text) => SmeltFormDataValue::Text(text.to_string()), other => SmeltFormDataValue::File(SmeltFromUnknown::smelt_from_unknown(other)) };",
                    );
                    loop_writer.line("form.append(&name, value);");
                });
                fn_writer.line("form");
            },
        );
    });
    writer.blank_line();
}
