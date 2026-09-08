//! Runtime prelude for the WHATWG text codecs and the concrete byte view.
//!
//! Three generated types, each a real Rust value with typed methods rather than
//! a marker-bearing `SmeltUnknown` record:
//!
//! * `SmeltUint8Array` — a shared `Vec<u8>` with a JS reference identity. This
//!   is what `TextEncoder.encode` answers, so the bytes a program encodes have
//!   the type a hand-writing Rust team would give them (`Vec<u8>`), carried all
//!   the way to runtime, instead of a tagged record whose elements have to be
//!   re-narrowed one at a time.
//! * `SmeltTextEncoder` — identity plus the `encoding` data property. The spec
//!   fixes a `TextEncoder` at UTF-8 and gives it no other state, so `encode` is
//!   `str::as_bytes`.
//! * `SmeltTextDecoder` — identity plus its normalized encoding label. `decode`
//!   is `String::from_utf8_lossy`, which is *exactly* the spec's non-`fatal`
//!   behaviour: an ill-formed sequence becomes U+FFFD REPLACEMENT CHARACTER.
//!   (`fatal: true` would throw instead; the frontend refuses that option
//!   rather than pretending, so the two never disagree.)
//!
//! ## Why UTF-8 needs no crate
//!
//! Rust's `String`/`str` are UTF-8, so both directions of the codec are already
//! in `core`: `as_bytes` cannot fail, and `from_utf8_lossy` performs the spec's
//! substitution. An `encoding_rs` dependency would only start paying for itself
//! at the first non-UTF-8 label, which the frontend currently refuses outright.
//!
//! ## Identity
//!
//! All three are JavaScript reference objects, so each carries a
//! `smelt_next_object_id` identity and the byte view keeps its bytes behind an
//! `Rc<RefCell<..>>` — the same shape as `SmeltList`, `SmeltHeaders` and
//! `SmeltRegExp`. Two variables holding one view therefore observe each other,
//! as they do in JavaScript.
//!
//! ## The erased form
//!
//! A byte view that crosses a dynamic boundary becomes the byte-backed host
//! record the typed-array views already use
//! (`{ "__smelt_uint8array": true, "bytes": [..], "byteLength": N, "length": N }`,
//! see `byte_buffer_prelude`), so an erased concrete view and an erased
//! `new Uint8Array(..)` are indistinguishable: `instanceof Uint8Array`, the
//! `[object Uint8Array]` tag, and the element reads all keep working on either.
//! That adapter is a DYNAMIC BOUNDARY adapter only — the internal
//! representation stays concrete.

use crate::rust::CodeWriter;

/// Emit the `SmeltUint8Array` runtime type.
///
/// `needs_unknown` gates the erasure adapters (`IntoSmeltUnknown` /
/// `SmeltFromUnknown`): a program that never crosses the dynamic boundary does
/// not emit the carrier type, so the impls must not be emitted either.
pub fn emit_byte_array(writer: &mut CodeWriter, needs_unknown: bool) {
    emit_byte_array_struct(writer);
    emit_byte_array_inherent_impl(writer);
    emit_byte_array_traits(writer, needs_unknown);
}


/// Emit the `SmeltTextEncoder` runtime type.
///
/// `needs_unknown` gates the erasure adapters, as it does for the byte view.
pub fn emit_encoder(writer: &mut CodeWriter, needs_unknown: bool) {
    writer.line("/// A WHATWG `TextEncoder`: a UTF-8 encoder with a JS reference identity.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltTextEncoder", |struct_writer| {
        struct_writer.line("id: usize,");
    });
    writer.blank_line();
    // Every `TextEncoder` is interchangeable — the spec gives it no state — so
    // two of them are equal, which is what `expect(new TextEncoder()).toEqual(
    // new TextEncoder())` observes.
    writer.line(
        "impl PartialEq for SmeltTextEncoder { fn eq(&self, _other: &Self) -> bool { true } }",
    );
    writer.line(
        "impl ::std::fmt::Debug for SmeltTextEncoder { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(\"TextEncoder { encoding: 'utf-8' }\") } }",
    );
    writer.line("impl Default for SmeltTextEncoder { fn default() -> Self { Self::new() } }");
    writer.blank_line();
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltTextEncoder", |impl_writer| {
        impl_writer.line("/// A fresh encoder with a fresh JS reference identity.");
        impl_writer
            .line("pub fn new() -> Self { Self { id: smelt_next_object_id() } }");
        impl_writer.line("/// JS reference identity of this encoder.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// `encoding`: always `\"utf-8\"` per the spec.");
        impl_writer.line("pub fn encoding(&self) -> String { \"utf-8\".to_owned() }");
        impl_writer.line("/// `encode(input)`: the input's UTF-8 bytes.");
        impl_writer.line(
            "pub fn encode(&self, input: &str) -> SmeltUint8Array { SmeltUint8Array::from_bytes(input.as_bytes().to_vec()) }",
        );
    });
    writer.blank_line();
    emit_codec_erasure(writer, needs_unknown, "SmeltTextEncoder", "__smelt_textencoder");
    if needs_unknown {
        writer.line("/// The modeled member of an erased `TextEncoder` record, resolved at run time.");
        writer.line("///");
        writer.line("/// Same dynamic boundary as the sibling host resolvers: the receiver is a");
        writer.line("/// marker-bearing record, so the member is decided by the marker and the");
        writer.line("/// member NAME at run time. A program reaches this only by erasing the codec");
        writer.line("/// on purpose; answering `undefined`, which is what a plain property read");
        writer.line("/// does, made `(encoder as any).encode('ab')` a null rather than the bytes.");
        writer.line("fn smelt_text_encoder_host_method(object: &SmeltObject, name: &str) -> Option<SmeltUnknown> { if !object.contains_key(\"__smelt_textencoder\") || name != \"encode\" { return None; } let encoder = <SmeltTextEncoder as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(object.clone())); Some(SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let input = args.first().cloned().map_or_else(String::new, smelt_property_key); Ok(encoder.encode(&input).into_smelt_unknown()) }))) }");
        writer.blank_line();
    }

}

/// Emit the `SmeltTextDecoder` runtime type.
///
/// `needs_unknown` gates the erasure adapters, as it does for the byte view.
pub fn emit_decoder(writer: &mut CodeWriter, needs_unknown: bool) {
    writer.line("/// A WHATWG `TextDecoder`: a UTF-8 decoder with a JS reference identity.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltTextDecoder", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("/// The decoder's normalized encoding label.");
        struct_writer.line("encoding: String,");
    });
    writer.blank_line();
    writer.line(
        "impl PartialEq for SmeltTextDecoder { fn eq(&self, other: &Self) -> bool { self.encoding == other.encoding } }",
    );
    writer.line(
        "impl ::std::fmt::Debug for SmeltTextDecoder { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { write!(formatter, \"TextDecoder {{ encoding: '{}', fatal: false, ignoreBOM: false }}\", self.encoding) } }",
    );
    writer.line("impl Default for SmeltTextDecoder { fn default() -> Self { Self::new() } }");
    writer.blank_line();
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltTextDecoder", |impl_writer| {
        impl_writer.line("/// A fresh UTF-8 decoder with a fresh JS reference identity.");
        impl_writer.line(
            "pub fn new() -> Self { Self { id: smelt_next_object_id(), encoding: \"utf-8\".to_owned() } }",
        );
        // The label is normalized the way the encoding standard's label table
        // is looked up (trimmed, lower-cased) and then reported in the spec's
        // own canonical spelling: `new TextDecoder("UTF8").encoding` is
        // `"utf-8"`, not `"utf8"`. The frontend has already refused every label
        // that is not a UTF-8 row, so the canonical name is known here.
        impl_writer.line("/// A decoder for an encoding label, which is always a UTF-8 label.");
        impl_writer.line(
            "pub fn from_label(_label: &str) -> Self { Self::new() }",
        );
        impl_writer.line("/// JS reference identity of this decoder.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// `encoding`: the normalized encoding label.");
        impl_writer.line("pub fn encoding(&self) -> String { self.encoding.clone() }");
        impl_writer.line(
            "/// `decode(input)`: the bytes read back as a string, substituting",
        );
        impl_writer.line("/// U+FFFD for ill-formed sequences as the non-`fatal` spec does.");
        impl_writer.line(
            "pub fn decode(&self, input: &SmeltUint8Array) -> String { String::from_utf8_lossy(&input.to_bytes()).into_owned() }",
        );
    });
    writer.blank_line();
    emit_codec_erasure(writer, needs_unknown, "SmeltTextDecoder", "__smelt_textdecoder");
    if needs_unknown {
        writer.line("/// The modeled member of an erased `TextDecoder` record, resolved at run time.");
        writer.line("///");
        writer.line("/// Same dynamic boundary as the sibling host resolvers: the receiver is a");
        writer.line("/// marker-bearing record, so the member is decided by the marker and the");
        writer.line("/// member NAME at run time. A program reaches this only by erasing the codec");
        writer.line("/// on purpose; answering `undefined`, which is what a plain property read");
        writer.line("/// does, made `(encoder as any).encode('ab')` a null rather than the bytes.");
        writer.line("fn smelt_text_decoder_host_method(object: &SmeltObject, name: &str) -> Option<SmeltUnknown> { if !object.contains_key(\"__smelt_textdecoder\") || name != \"decode\" { return None; } let decoder = <SmeltTextDecoder as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(object.clone())); Some(SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let bytes = args.first().cloned().map_or_else(SmeltUint8Array::new, <SmeltUint8Array as SmeltFromUnknown>::smelt_from_unknown); Ok(SmeltUnknown::String(decoder.decode(&bytes).into())) }))) }");
        writer.blank_line();
    }

}

/// Emit the byte-view struct and its comparisons.
fn emit_byte_array_struct(writer: &mut CodeWriter) {
    writer.line("/// A concrete byte view: shared bytes with a JS reference identity.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltUint8Array", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("/// The view's bytes, shared like every JS reference object's state.");
        struct_writer.line("bytes: ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>>,");
    });
    writer.blank_line();
    // Structural equality over the bytes: `expect(encoder.encode("a")).toEqual(
    // new Uint8Array([97]))` compares contents, not identity.
    writer.line(
        "impl PartialEq for SmeltUint8Array { fn eq(&self, other: &Self) -> bool { *self.bytes.borrow() == *other.bytes.borrow() } }",
    );
    // Node prints a typed array as `Uint8Array(3) [ 1, 2, 3 ]`, and an empty one
    // as `Uint8Array(0) []`. Matching that here is what keeps a `console.log` of
    // an encoded value byte-identical to Node's.
    writer.block(
        "impl ::std::fmt::Debug for SmeltUint8Array",
        |impl_writer| {
            impl_writer.block(
                "fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result",
                |fn_writer| {
                    fn_writer.line("let bytes = self.bytes.borrow();");
                    fn_writer.line("if bytes.is_empty() { return write!(formatter, \"Uint8Array(0) []\"); }");
                    fn_writer.line(
                        "let items = bytes.iter().map(|byte| byte.to_string()).collect::<Vec<String>>().join(\", \");",
                    );
                    fn_writer
                        .line("write!(formatter, \"Uint8Array({}) [ {items} ]\", bytes.len())");
                },
            );
        },
    );
    writer.line("impl Default for SmeltUint8Array { fn default() -> Self { Self::new() } }");
    writer.blank_line();
}

/// Emit the byte view's operations as inherent methods.
fn emit_byte_array_inherent_impl(writer: &mut CodeWriter) {
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltUint8Array", |impl_writer| {
        impl_writer.line("/// An empty view with a fresh JS reference identity.");
        impl_writer.line("pub fn new() -> Self { Self::from_bytes(Vec::new()) }");
        impl_writer.line("/// A view over owned bytes, with a fresh JS reference identity.");
        impl_writer.line(
            "pub fn from_bytes(bytes: Vec<u8>) -> Self { Self { id: smelt_next_object_id(), bytes: ::std::rc::Rc::new(::std::cell::RefCell::new(bytes)) } }",
        );
        impl_writer.line("/// JS reference identity of this view.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// A copy of the view's bytes.");
        impl_writer.line("pub fn to_bytes(&self) -> Vec<u8> { self.bytes.borrow().clone() }");
        // `length` and `byteLength` are separate methods even though they agree
        // for a one-byte element type, because they are separate spec members:
        // aliasing them here would be a lie the moment a wider concrete view
        // exists.
        impl_writer.line("/// `length`: the element count, one element per byte.");
        impl_writer.line("pub fn length(&self) -> f64 { self.bytes.borrow().len() as f64 }");
        impl_writer.line("/// `byteLength`: the byte count.");
        impl_writer.line("pub fn byte_length(&self) -> f64 { self.bytes.borrow().len() as f64 }");
    });
    writer.blank_line();
}

/// Emit the byte view's dynamic-boundary adapters.
///
/// The erased form is the byte-backed host record the typed-array views use, so
/// a concrete view and a `new Uint8Array(..)` are the same value once erased.
fn emit_byte_array_traits(writer: &mut CodeWriter, needs_unknown: bool) {
    if !needs_unknown {
        return;
    }
    writer.line("/// Erase a byte view for a dynamic boundary.");
    writer.block("impl IntoSmeltUnknown for SmeltUint8Array", |impl_writer| {
        impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
            fn_writer.line("let bytes = self.bytes.borrow();");
            fn_writer.line(
                "let elements: Vec<SmeltUnknown> = bytes.iter().map(|byte| SmeltUnknown::Number(f64::from(*byte))).collect();",
            );
            fn_writer.line("let count = elements.len() as f64;");
            fn_writer.line(
                "SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([(\"__smelt_uint8array\".to_owned(), SmeltUnknown::Bool(true)), (\"bytes\".to_owned(), SmeltUnknown::Array(elements.into())), (\"byteLength\".to_owned(), SmeltUnknown::Number(count)), (\"length\".to_owned(), SmeltUnknown::Number(count))])))",
            );
        });
    });
    writer.blank_line();
    writer.line("/// Rebuild a byte view from an erased value.");
    writer.block("impl SmeltFromUnknown for SmeltUint8Array", |impl_writer| {
        impl_writer.block(
            "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
            |fn_writer| {
                // Any byte-backed host record answers here, not only the
                // `__smelt_uint8array` one: every view and every `ArrayBuffer`
                // carries its storage under `bytes`, so a `DataView` or a
                // `Float64Array` crossing into a byte view reads its bytes
                // rather than silently becoming empty.
                fn_writer.line("let SmeltUnknown::Object(map) = value else { return Self::new() };");
                fn_writer.line(
                    "let Some(SmeltUnknown::Array(items)) = map.get(\"bytes\") else { return Self::new() };",
                );
                fn_writer.line(
                    "let bytes = items.into_vec().into_iter().map(|item| match item { SmeltUnknown::Number(number) => number as u8, _ => 0 }).collect::<Vec<u8>>();",
                );
                fn_writer.line("Self::from_bytes(bytes)");
            },
        );
    });
    writer.blank_line();
}

/// Emit one text codec's erasure adapter pair.
///
/// Both codecs erase to the same shape — an identity marker plus the spec's one
/// readable data property, `encoding` — and both recover LOSSLESSLY, because a
/// codec's only state besides its reference identity IS its encoding label. The
/// two therefore share this call rather than each spelling the shape out.
fn emit_codec_erasure(
    writer: &mut CodeWriter,
    needs_unknown: bool,
    type_name: &str,
    marker: &str,
) {
    if !needs_unknown {
        return;
    }
    crate::host_value_erasure::emit_adapters(
        writer,
        type_name,
        marker,
        &[crate::host_value_erasure::DataProperty {
            name: "encoding",
            value_expr: "SmeltUnknown::String(self.encoding().into())",
        }],
        // The spec fixes both codecs at UTF-8 and the label is the only state,
        // so `Self::new()` IS the value the record describes.
        crate::host_value_erasure::Recovery::Structural("Self::new()"),
    );
}
