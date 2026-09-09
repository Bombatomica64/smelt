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

/// Emit the concrete byte view, which is the typed-array family's `Uint8` face.
///
/// The view used to be its own `Rc<RefCell<Vec<u8>>>` struct with `length` and
/// `byteLength` and nothing else. It is now `SmeltTypedArray` — a kind, a byte
/// offset and a shared buffer — with `SmeltUint8Array` emitted as an alias, so
/// every reachable use (`TextEncoder.encode`, `TextDecoder.decode`,
/// `crypto.getRandomValues`) keeps the same type spelling and the same answers
/// while the members a source `Uint8Array` will need already exist. See
/// `crate::typed_array_prelude` and
/// `blocker-logs/standards-typed-array-views-plan.md`.
///
/// `needs_unknown` gates the erasure adapters (`IntoSmeltUnknown` /
/// `SmeltFromUnknown`): a program that never crosses the dynamic boundary does
/// not emit the carrier type, so the impls must not be emitted either.
pub fn emit_byte_array(writer: &mut CodeWriter, needs_unknown: bool) {
    crate::typed_array_prelude::emit(writer, needs_unknown);
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
