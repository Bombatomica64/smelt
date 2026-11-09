// Runtime prelude for the WHATWG text codecs and the concrete byte view.
//
// Three generated types, each a real Rust value with typed methods rather than
// a marker-bearing `SmeltUnknown` record:
//
// * `SmeltUint8Array` — an alias of `SmeltTypedArray` (kind, byte offset, shared
//   buffer). This is what `TextEncoder.encode` answers, so the bytes a program
//   encodes keep a concrete byte-element type all the way to runtime, instead of
//   a tagged record whose elements have to be re-narrowed one at a time.
// * `SmeltTextEncoder` — identity plus the `encoding` data property. The spec
//   fixes a `TextEncoder` at UTF-8 and gives it no other state, so `encode` is
//   `str::as_bytes`.
// * `SmeltTextDecoder` — identity plus its normalized encoding label. `decode`
//   is `String::from_utf8_lossy`, which is *exactly* the spec's non-`fatal`
//   behaviour: an ill-formed sequence becomes U+FFFD REPLACEMENT CHARACTER.
//   (`fatal: true` would throw instead; the frontend refuses that option
//   rather than pretending, so the two never disagree.)
//
// ## Why UTF-8 needs no crate
//
// Rust's `String`/`str` are UTF-8, so both directions of the codec are already
// in `core`: `as_bytes` cannot fail, and `from_utf8_lossy` performs the spec's
// substitution. An `encoding_rs` dependency would only start paying for itself
// at the first non-UTF-8 label, which the frontend currently refuses outright.
//
// ## Identity
//
// All three are JavaScript reference objects, so each carries a
// `smelt_next_object_id` identity and the byte view keeps its bytes behind an
// `Rc<RefCell<..>>` — the same shape as `SmeltList`, `SmeltHeaders` and
// `SmeltRegExp`. Two variables holding one view therefore observe each other,
// as they do in JavaScript.
//
// ## The erased form
//
// A byte view that crosses a dynamic boundary becomes the byte-backed host
// record the typed-array views already use
// (`{ "__smelt_uint8array": true, "bytes": [..], "byteLength": N, "length": N }`,
// see `smelt-runtime`'s `byte_buffer.rs`), so an erased concrete view and an erased
// `new Uint8Array(..)` are indistinguishable: `instanceof Uint8Array`, the
// `[object Uint8Array]` tag, and the element reads all keep working on either.
// That adapter is a DYNAMIC BOUNDARY adapter only — the internal
// representation stays concrete.

// Emit the `SmeltTextEncoder` runtime type.
//
// `needs_unknown` gates the erasure adapters, as it does for the byte view.
// @smelt:item SmeltTextEncoder when text_encoder
/// A WHATWG `TextEncoder`: a UTF-8 encoder with a JS reference identity.
#[derive(Clone)]
pub struct SmeltTextEncoder {
    id: usize,
}

//~ Every `TextEncoder` is interchangeable — the spec gives it no state — so
//~ two of them are equal, which is what `expect(new TextEncoder()).toEqual(
//~ new TextEncoder())` observes.
impl PartialEq for SmeltTextEncoder { fn eq(&self, _other: &Self) -> bool { true } }
impl ::std::fmt::Debug for SmeltTextEncoder { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str("TextEncoder { encoding: 'utf-8' }") } }
impl Default for SmeltTextEncoder { fn default() -> Self { Self::new() } }

#[allow(dead_code)]
impl SmeltTextEncoder {
    //~ A fresh encoder with a fresh JS reference identity.
    pub fn new() -> Self { Self { id: smelt_next_object_id() } }
    //~ JS reference identity of this encoder.
    pub fn id(&self) -> usize { self.id }
    //~ `encoding`: always `"utf-8"` per the spec.
    pub fn encoding(&self) -> String { "utf-8".to_owned() }
    //~ `encode(input)`: the input's UTF-8 bytes.
    pub fn encode(&self, input: &(impl SmeltHostText + ?Sized)) -> SmeltUint8Array { let input = input.host_text(); let input: &str = &input; SmeltUint8Array::from_bytes(input.as_bytes().to_vec()) }
}

// @smelt:item-end

// @smelt:item smelt_text_encoder_host_method when text_encoder & unknown
/// The modeled member of an erased `TextEncoder` record, resolved at run time.
//~
//~ Same dynamic boundary as the sibling host resolvers: the receiver is a
//~ marker-bearing record, so the member is decided by the marker and the
//~ member NAME at run time. A program reaches this only by erasing the codec
//~ on purpose; answering `undefined`, which is what a plain property read
//~ does, made `(encoder as any).encode('ab')` a null rather than the bytes.
fn smelt_text_encoder_host_method(object: &SmeltObject, name: &str) -> Option<SmeltUnknown> { if !object.contains_key("__smelt_textencoder") || name != "encode" { return None; } let encoder = <SmeltTextEncoder as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(object.clone())); Some(SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let input = args.first().cloned().map_or_else(String::new, smelt_property_key); Ok(encoder.encode(&input).into_smelt_unknown()) }))) }

// @smelt:item-end

// Emit the `SmeltTextDecoder` runtime type.
//
// `needs_unknown` gates the erasure adapters, as it does for the byte view.
// @smelt:item SmeltTextDecoder when text_decoder
/// A WHATWG `TextDecoder`: a UTF-8 decoder with a JS reference identity.
#[derive(Clone)]
pub struct SmeltTextDecoder {
    id: usize,
    //~ The decoder's normalized encoding label.
    encoding: String,
}

impl PartialEq for SmeltTextDecoder { fn eq(&self, other: &Self) -> bool { self.encoding == other.encoding } }
impl ::std::fmt::Debug for SmeltTextDecoder { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { write!(formatter, "TextDecoder {{ encoding: '{}', fatal: false, ignoreBOM: false }}", self.encoding) } }
impl Default for SmeltTextDecoder { fn default() -> Self { Self::new() } }

#[allow(dead_code)]
impl SmeltTextDecoder {
    //~ A fresh UTF-8 decoder with a fresh JS reference identity.
    pub fn new() -> Self { Self { id: smelt_next_object_id(), encoding: "utf-8".to_owned() } }
    //~ The label is normalized the way the encoding standard's label table
    //~ is looked up (trimmed, lower-cased) and then reported in the spec's
    //~ own canonical spelling: `new TextDecoder("UTF8").encoding` is
    //~ `"utf-8"`, not `"utf8"`. The frontend has already refused every label
    //~ that is not a UTF-8 row, so the canonical name is known here.
    //~ A decoder for an encoding label, which is always a UTF-8 label.
    pub fn from_label(_label: &(impl SmeltHostText + ?Sized)) -> Self { Self::new() }
    //~ JS reference identity of this decoder.
    pub fn id(&self) -> usize { self.id }
    //~ `encoding`: the normalized encoding label.
    pub fn encoding(&self) -> String { self.encoding.clone() }
    //~ `decode(input)`: the bytes read back as a string, substituting
    //~ U+FFFD for ill-formed sequences as the non-`fatal` spec does.
    pub fn decode(&self, input: &SmeltUint8Array) -> String { String::from_utf8_lossy(&input.to_bytes()).into_owned() }
}

// @smelt:item-end

// @smelt:item smelt_text_decoder_host_method when text_decoder & unknown
/// The modeled member of an erased `TextDecoder` record, resolved at run time.
//~
//~ Same dynamic boundary as the sibling host resolvers: the receiver is a
//~ marker-bearing record, so the member is decided by the marker and the
//~ member NAME at run time. A program reaches this only by erasing the codec
//~ on purpose; answering `undefined`, which is what a plain property read
//~ does, made `(encoder as any).encode('ab')` a null rather than the bytes.
fn smelt_text_decoder_host_method(object: &SmeltObject, name: &str) -> Option<SmeltUnknown> { if !object.contains_key("__smelt_textdecoder") || name != "decode" { return None; } let decoder = <SmeltTextDecoder as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(object.clone())); Some(SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let bytes = args.first().cloned().map_or_else(SmeltUint8Array::new, <SmeltUint8Array as SmeltFromUnknown>::smelt_from_unknown); Ok(SmeltUnknown::String(decoder.decode(&bytes).into())) }))) }

// @smelt:item-end
