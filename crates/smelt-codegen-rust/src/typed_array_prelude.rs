//! The concrete typed-array family: byte storage and element views.
//!
//! Increment 1 of `blocker-logs/standards-typed-array-views-plan.md`. Two
//! runtime types, both with a JavaScript reference identity:
//!
//! * [`emit_array_buffer`] — `SmeltArrayBuffer`, byte storage that owns its
//!   bytes and interprets none of them (`ArrayBuffer`);
//! * [`emit_typed_array`] — `SmeltTypedArray`, an ELEMENT VIEW over storage: a
//!   kind (element type), a byte offset into the buffer, and an element count.
//!
//! Making the offset and the buffer real is the whole point of the shape. A
//! typed array is not a `Vec<u8>` with a name: `view.subarray(1)` is a second
//! view over the SAME storage, so a write through one is visible through the
//! other, and `view.byteOffset`/`view.buffer` are what the spec's own members
//! answer. A design that copied bytes per view would pass every test that does
//! not share a buffer and silently diverge on every test that does.
//!
//! The kind table is derived from the host-object registry
//! (`smelt_stdlib::TYPED_ARRAY_CLASS_NAMES` plus `typed_array_element`), so the
//! eleven views' names, element widths and `[object X]` tags cannot drift from
//! the erased face's answers for the same eleven classes.
//!
//! ## What is reachable from source today
//!
//! Only the `Uint8` face, under its existing name: `SmeltUint8Array` is emitted
//! as an alias of `SmeltTypedArray`, so `TextEncoder.encode`,
//! `TextDecoder.decode` and `crypto.getRandomValues` keep the value they always
//! had, with the same `length`/`byteLength`/`to_bytes`/`Debug`/`PartialEq`
//! answers. The remaining members exist for increment 3, which is where a
//! source `Uint8Array` annotation stops being erased; until then a
//! `new Uint8Array(..)` is still the erased host record
//! (`byte_buffer_prelude`), and both faces answer through the same registry.

use crate::rust::CodeWriter;

/// The `SmeltTypedArrayKind` variant expression for a source constructor name.
///
/// `None` for a name that is not one of the eleven views — including
/// `ArrayBuffer`, which is the storage half and has no element kind. Derived
/// from the shared host-object registry, so the emitter kind selection and the
/// emitted enum cannot name different variants for the same class.
#[must_use]
pub fn kind_expression(class_name: &str) -> Option<String> {
    let element = smelt_stdlib::typed_array_element(class_name)?;
    Some(format!("SmeltTypedArrayKind::{}", variant_name(element)))
}

/// Emit the whole concrete typed-array family.
///
/// Order matters: the kind table is referenced by both types, and the view
/// names the buffer.
pub fn emit(writer: &mut CodeWriter, needs_unknown: bool) {
    emit_kind(writer);
    emit_array_buffer(writer, needs_unknown);
    emit_typed_array(writer, needs_unknown);
    if needs_unknown {
        emit_origin_write_through(writer);
    }
}

/// Emit the bridge from an erased record's index write to its live value.
///
/// The boundary is two-way for READS by construction — the record carries the
/// bytes — but a WRITE through an erased alias (`(view as any)[0] = 9`) lands
/// in the record, and the concrete value the record was made from is still the
/// thing every other reference reads. Without this bridge such a write is
/// silently lost the moment one of the two aliases is concrete, which is a
/// class of divergence that only appears in a mixed program and never in a
/// wholly erased or wholly concrete one.
///
/// The record's own id and its storage record's id are both tried, because a
/// view's window and its buffer are two live values with two identities and an
/// element write is a write to both.
fn emit_origin_write_through(writer: &mut CodeWriter) {
    writer.line("/// Write an erased record's encoded bytes through to its live value.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_typed_array_write_origin(id: usize, offset: usize, encoded: &[SmeltUnknown])",
        |fn_writer| {
            fn_writer.line("let bytes: Vec<u8> = encoded.iter().map(|byte| match byte { SmeltUnknown::Number(value) => *value as i64 as u8, _ => 0 }).collect();");
            fn_writer.line("if let Some(view) = smelt_restore_host_origin_by_id::<SmeltTypedArray>(id) { view.write_bytes_at(offset, &bytes); return; }");
            fn_writer.line("if let Some(storage) = smelt_restore_host_origin_by_id::<SmeltArrayBuffer>(id) { storage.write_bytes_at(offset, &bytes); }");
        },
    );
    writer.blank_line();
}

/// Emit the element-kind enum: width, spec tag, and the element codec.
///
/// One enum per element type rather than a generic parameter, because the kind
/// is a RUNTIME property of the value: `Object.prototype.toString.call(view)`
/// reports it, two views of different kinds over the same buffer are different
/// values, and reflective construction reads it back off an erased record. A
/// `SmeltTypedArray<T>` would have to erase that answer at every boundary.
fn emit_kind(writer: &mut CodeWriter) {
    let kinds = kind_table();
    writer.line("/// The element type of a typed-array view.");
    writer.line("#[derive(Clone, Copy, PartialEq, Eq, Debug)]");
    writer.line("#[allow(dead_code)]");
    writer.block("pub enum SmeltTypedArrayKind", |enum_writer| {
        for (variant, class_name, _, width) in &kinds {
            enum_writer.line(format!(
                "/// `{class_name}`: {width}-byte element{plural}.",
                plural = if *width == 1 { "" } else { "s" }
            ));
            enum_writer.line(format!("{variant},"));
        }
    });
    writer.blank_line();
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltTypedArrayKind", |impl_writer| {
        impl_writer.line("/// The constructor name, which is also the `[object X]` tag.");
        impl_writer.block("pub fn class_name(self) -> &'static str", |fn_writer| {
            fn_writer.block("match self", |match_writer| {
                for (variant, class_name, _, _) in &kinds {
                    match_writer.line(format!("Self::{variant} => {class_name:?},"));
                }
            });
        });
        impl_writer.line("/// The identity marker the erased face stamps for this kind.");
        impl_writer.block("pub fn marker(self) -> &'static str", |fn_writer| {
            fn_writer.block("match self", |match_writer| {
                for (variant, _, marker, _) in &kinds {
                    match_writer.line(format!("Self::{variant} => {marker:?},"));
                }
            });
        });
        impl_writer.line("/// `BYTES_PER_ELEMENT`: the stride between elements.");
        impl_writer.block("pub fn byte_width(self) -> usize", |fn_writer| {
            fn_writer.block("match self", |match_writer| {
                for (variant, _, _, width) in &kinds {
                    match_writer.line(format!("Self::{variant} => {width},"));
                }
            });
        });
        impl_writer.line("/// The kind whose marker a byte-backed record carries, if any.");
        impl_writer.block(
            "pub fn from_marker(marker: &str) -> Option<Self>",
            |fn_writer| {
                for (variant, _, marker, _) in &kinds {
                    fn_writer.line(format!(
                        "if marker == {marker:?} {{ return Some(Self::{variant}); }}"
                    ));
                }
                fn_writer.line("None");
            },
        );
        // Decoding and encoding are the only places the element's width and
        // SIGNEDNESS live. `Uint8Clamped` differs from `Uint8` on the write
        // side only: it saturates where the others wrap, which is the whole
        // reason the spec has a separate view for it.
        impl_writer.line("/// Read one element out of `bytes` at a byte offset.");
        impl_writer.block(
            "pub fn decode(self, bytes: &[u8], at: usize) -> f64",
            |fn_writer| {
                fn_writer.line("let width = self.byte_width();");
                fn_writer
                    .line("if at + width > bytes.len() { return 0.0; }");
                fn_writer.line("let window = &bytes[at..at + width];");
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::Int8 => f64::from(window[0] as i8),");
                    match_writer.line("Self::Uint8 | Self::Uint8Clamped => f64::from(window[0]),");
                    match_writer.line("Self::Int16 => f64::from(i16::from_le_bytes([window[0], window[1]])),");
                    match_writer.line("Self::Uint16 => f64::from(u16::from_le_bytes([window[0], window[1]])),");
                    match_writer.line("Self::Int32 => f64::from(i32::from_le_bytes([window[0], window[1], window[2], window[3]])),");
                    match_writer.line("Self::Uint32 => f64::from(u32::from_le_bytes([window[0], window[1], window[2], window[3]])),");
                    match_writer.line("Self::Float32 => f64::from(f32::from_le_bytes([window[0], window[1], window[2], window[3]])),");
                    match_writer.line("Self::Float64 => f64::from_le_bytes([window[0], window[1], window[2], window[3], window[4], window[5], window[6], window[7]]),");
                    // A `BigInt` is modeled as a number, so a 64-bit integer
                    // view decodes through `f64` like every other kind; the
                    // registry's comment says the same thing.
                    match_writer.line("Self::BigInt64 => i64::from_le_bytes([window[0], window[1], window[2], window[3], window[4], window[5], window[6], window[7]]) as f64,");
                    match_writer.line("Self::BigUint64 => u64::from_le_bytes([window[0], window[1], window[2], window[3], window[4], window[5], window[6], window[7]]) as f64,");
                });
            },
        );
        impl_writer.line("/// Write one element into `bytes` at a byte offset.");
        impl_writer.block(
            "pub fn encode(self, value: f64, bytes: &mut [u8], at: usize)",
            |fn_writer| {
                fn_writer.line("let width = self.byte_width();");
                fn_writer.line("if at + width > bytes.len() { return; }");
                fn_writer.line("let encoded = self.encode_bytes(value);");
                fn_writer.line("bytes[at..at + width].copy_from_slice(&encoded[..width]);");
            },
        );
        // The encoding itself, as a tail expression: the widths differ, so each
        // kind fills the low bytes of one eight-byte buffer and the caller
        // copies as many as the element needs.
        impl_writer.line("/// One element's little-endian bytes, low-order first.");
        impl_writer.block(
            "fn encode_bytes(self, value: f64) -> [u8; 8]",
            |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::Int8 => [(value as i64 as i8) as u8, 0, 0, 0, 0, 0, 0, 0],");
                    match_writer.line("Self::Uint8 => [(value as i64 as u8), 0, 0, 0, 0, 0, 0, 0],");
                    // The saturating write: `new Uint8ClampedArray([300])[0]`
                    // is 255, not 44.
                    match_writer.line("Self::Uint8Clamped => [(if value.is_nan() { 0.0 } else { value.round_ties_even().clamp(0.0, 255.0) }) as u8, 0, 0, 0, 0, 0, 0, 0],");
                    match_writer.line("Self::Int16 => { let mut out = [0_u8; 8]; out[..2].copy_from_slice(&(value as i64 as i16).to_le_bytes()); out },");
                    match_writer.line("Self::Uint16 => { let mut out = [0_u8; 8]; out[..2].copy_from_slice(&(value as i64 as u16).to_le_bytes()); out },");
                    match_writer.line("Self::Int32 => { let mut out = [0_u8; 8]; out[..4].copy_from_slice(&(value as i64 as i32).to_le_bytes()); out },");
                    match_writer.line("Self::Uint32 => { let mut out = [0_u8; 8]; out[..4].copy_from_slice(&(value as i64 as u32).to_le_bytes()); out },");
                    match_writer.line("Self::Float32 => { let mut out = [0_u8; 8]; out[..4].copy_from_slice(&(value as f32).to_le_bytes()); out },");
                    match_writer.line("Self::Float64 => value.to_le_bytes(),");
                    match_writer.line("Self::BigInt64 => (value as i64).to_le_bytes(),");
                    match_writer.line("Self::BigUint64 => (value as i64 as u64).to_le_bytes(),");
                });
            },
        );
    });
    writer.blank_line();
}

/// Emit `SmeltArrayBuffer`: byte storage with a reference identity.
///
/// The bytes are shared (`Rc<RefCell<..>>`) because an `ArrayBuffer` IS shared:
/// every view over it observes writes through any other view, and passing the
/// buffer around does not copy it. `slice` is the one member that copies,
/// exactly as the spec says.
fn emit_array_buffer(writer: &mut CodeWriter, needs_unknown: bool) {
    writer.line("/// Byte storage with a JavaScript reference identity (`ArrayBuffer`).");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltArrayBuffer", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("/// The storage, shared by every view over it.");
        struct_writer.line("bytes: ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>>,");
    });
    writer.blank_line();
    // Structural equality over the bytes, the same rule the byte view uses:
    // `toEqual` compares contents.
    writer.line(
        "impl PartialEq for SmeltArrayBuffer { fn eq(&self, other: &Self) -> bool { *self.bytes.borrow() == *other.bytes.borrow() } }",
    );
    writer.line(
        "impl ::std::fmt::Debug for SmeltArrayBuffer { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { write!(formatter, \"ArrayBuffer {{ byteLength: {} }}\", self.bytes.borrow().len()) } }",
    );
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltArrayBuffer", |impl_writer| {
        impl_writer.line("/// Zeroed storage of `byte_length` bytes.");
        impl_writer.line(
            "pub fn new(byte_length: usize) -> Self { Self::from_bytes(vec![0_u8; byte_length]) }",
        );
        impl_writer.line("/// Storage over owned bytes, with a fresh identity.");
        impl_writer.line(
            "pub fn from_bytes(bytes: Vec<u8>) -> Self { Self { id: smelt_next_object_id(), bytes: ::std::rc::Rc::new(::std::cell::RefCell::new(bytes)) } }",
        );
        impl_writer.line("/// JS reference identity of this storage.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// `byteLength`: the storage size in bytes.");
        impl_writer.line("pub fn byte_length(&self) -> f64 { self.bytes.borrow().len() as f64 }");
        impl_writer.line("/// A COPY of the storage's bytes.");
        impl_writer.line("pub fn to_bytes(&self) -> Vec<u8> { self.bytes.borrow().clone() }");
        impl_writer.line("/// The shared storage handle, for a view over it.");
        impl_writer.line(
            "pub fn storage(&self) -> ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>> { ::std::rc::Rc::clone(&self.bytes) }",
        );
        impl_writer.line("/// Storage sharing this buffer's bytes handle and identity.");
        impl_writer.line(
            "pub fn from_storage(id: usize, bytes: ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>>) -> Self { Self { id, bytes } }",
        );
        // `ArrayBuffer.prototype.slice` COPIES, and both bounds clamp and count
        // back from the end when negative — the same rule the erased face's
        // slice helper applies.
        // The write side an erased record's index write reaches (see
        // `smelt_typed_array_write_origin`): absolute byte offsets, because
        // storage interprets nothing.
        impl_writer.line("/// Overwrite bytes at an absolute offset; out-of-range bytes are dropped.");
        impl_writer.block("pub fn write_bytes_at(&self, offset: usize, source: &[u8])", |fn_writer| {
            fn_writer.line("let mut bytes = self.bytes.borrow_mut();");
            fn_writer.line("for (step, byte) in source.iter().enumerate() { if let Some(slot) = bytes.get_mut(offset + step) { *slot = *byte; } }");
        });
        impl_writer.line("/// `slice(start, end)`: a COPY of a byte range in fresh storage.");
        // Emitted before the closing brace below so `slice` stays last in the
        // inherent impl; see the `Default` impl after it for why storage has one.
        impl_writer.block(
            "pub fn slice(&self, start: i64, end: Option<i64>) -> Self",
            |fn_writer| {
                fn_writer.line("let bytes = self.bytes.borrow();");
                fn_writer.line("let len = bytes.len() as i64;");
                fn_writer.line("let from = (if start < 0 { len + start } else { start }).clamp(0, len) as usize;");
                fn_writer.line("let to = end.map_or(len, |end| if end < 0 { len + end } else { end }).clamp(0, len) as usize;");
                fn_writer.line("Self::from_bytes(bytes[from..to.max(from)].to_vec())");
            },
        );
    });
    // Zero-length storage is the empty value the type can hold, and a
    // monomorphized generic (`clone<T>(x: T): T` instantiated at
    // `ArrayBuffer`) asks for it by its `Default` bound. The view half already
    // has one for the same reason.
    writer.line("impl Default for SmeltArrayBuffer { fn default() -> Self { Self::new(0) } }");
    writer.blank_line();
    if needs_unknown {
        emit_array_buffer_adapters(writer);
    }
}

/// Emit the storage's dynamic-boundary adapters.
///
/// Increment 2 of the plan: the erased form is the byte-backed host record the
/// erased face already uses, so a concrete buffer and an erased one are the
/// same value once erased, and every answer the erased helpers give
/// (`ArrayBuffer.isView` false, no own index keys, `{}` under
/// `JSON.stringify`) holds for a concrete buffer that crossed the boundary.
fn emit_array_buffer_adapters(writer: &mut CodeWriter) {
    let marker = smelt_stdlib::host_object_marker("ArrayBuffer").unwrap_or("__smelt_arraybuffer");
    writer.line("/// Erase byte storage for a dynamic boundary.");
    writer.block("impl IntoSmeltUnknown for SmeltArrayBuffer", |impl_writer| {
        impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
            fn_writer.line("smelt_register_host_origin(self.id, self.clone());");
            fn_writer.line("let bytes = self.bytes.borrow();");
            fn_writer.line(
                "let elements: Vec<SmeltUnknown> = bytes.iter().map(|byte| SmeltUnknown::Number(f64::from(*byte))).collect();",
            );
            // The erased face's OWN record builder, at this value's identity:
            // storage carries no `buffer` of its own and no offset.
            fn_writer.line(format!(
                "{with_id}(self.id, {marker:?}, elements, None, 0)",
                with_id = smelt_stdlib::runtime_symbols::byte_buffer::VIEW_RECORD_WITH_ID,
            ));
        });
    });
    writer.blank_line();
    writer.line("/// Rebuild byte storage from an erased value.");
    writer.block("impl SmeltFromUnknown for SmeltArrayBuffer", |impl_writer| {
        impl_writer.block(
            "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
            |fn_writer| {
                // Any byte-backed record answers here, not only a buffer's:
                // every view and every buffer carries its storage under
                // `bytes`, so a view crossing into a buffer parameter reads the
                // bytes it views rather than becoming empty.
                fn_writer.line("if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }");
                fn_writer.line("let SmeltUnknown::Object(map) = value else { return Self::new(0) };");
                fn_writer.line("let Some(SmeltUnknown::Array(items)) = map.get(\"bytes\") else { return Self::new(0) };");
                fn_writer.line("let bytes = items.into_vec().into_iter().map(|item| match item { SmeltUnknown::Number(value) => value as i64 as u8, _ => 0 }).collect::<Vec<u8>>();");
                fn_writer.line("Self::from_storage(map.id, ::std::rc::Rc::new(::std::cell::RefCell::new(bytes)))");
            },
        );
    });
    writer.blank_line();
}

/// Emit `SmeltTypedArray`: an element view over shared storage.
fn emit_typed_array(writer: &mut CodeWriter, needs_unknown: bool) {
    emit_typed_array_struct(writer);
    emit_typed_array_inherent_impl(writer);
    if needs_unknown {
        emit_typed_array_adapters(writer);
    }
    // The byte view keeps its existing name: every reachable use of the family
    // today is the `Uint8` face (`TextEncoder.encode`, `TextDecoder.decode`,
    // `crypto.getRandomValues`), and its emitted type spelling, members and
    // answers are unchanged. Increment 3 is where a source `Uint8Array`
    // annotation resolves to the family and the alias stops being the only door.
    writer.line("/// The `Uint8` face of the family: Smelt's concrete byte view.");
    writer.line("pub type SmeltUint8Array = SmeltTypedArray;");
    writer.blank_line();
}

/// Emit the view struct, its equality and its `Debug`.
fn emit_typed_array_struct(writer: &mut CodeWriter) {
    writer.line("/// An element view over byte storage: kind, offset, length.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltTypedArray", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("/// The element type this view reads and writes.");
        struct_writer.line("kind: SmeltTypedArrayKind,");
        struct_writer.line("/// The storage, SHARED with every other view over it.");
        struct_writer.line("bytes: ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>>,");
        struct_writer.line("/// Identity of the `ArrayBuffer` this view reports.");
        struct_writer.line("buffer_id: usize,");
        struct_writer.line("/// `byteOffset`: where this view starts in the storage.");
        struct_writer.line("byte_offset: usize,");
        struct_writer.line("/// `length`: how many elements this view spans.");
        struct_writer.line("length: usize,");
    });
    writer.blank_line();
    // Structural equality over the ELEMENTS at this view's kind, which for two
    // `Uint8` views is byte equality — the answer the byte view has always
    // given (`expect(encoder.encode("a")).toEqual(new Uint8Array([97]))`).
    // Comparing the kind too is what keeps a `Uint8Array` from equalling an
    // `Int8Array` over the same bytes, which Node's deep equality also refuses.
    writer.line(
        "impl PartialEq for SmeltTypedArray { fn eq(&self, other: &Self) -> bool { self.kind == other.kind && self.to_elements() == other.to_elements() } }",
    );
    // Node prints `Uint8Array(3) [ 1, 2, 3 ]` and an empty one as
    // `Uint8Array(0) []`; the kind supplies the name so every view prints as
    // its own constructor.
    writer.block("impl ::std::fmt::Debug for SmeltTypedArray", |impl_writer| {
        impl_writer.block(
            "fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result",
            |fn_writer| {
                fn_writer.line("let elements = self.to_elements();");
                fn_writer.line("let name = self.kind.class_name();");
                fn_writer.line("if elements.is_empty() { return write!(formatter, \"{name}(0) []\"); }");
                fn_writer.line(
                    "let items = elements.iter().map(|element| element.to_string()).collect::<Vec<String>>().join(\", \");",
                );
                fn_writer.line("write!(formatter, \"{name}({}) [ {items} ]\", elements.len())");
            },
        );
    });
    writer.line("impl Default for SmeltTypedArray { fn default() -> Self { Self::new() } }");
    writer.blank_line();
}

/// Emit the view's members.
fn emit_typed_array_inherent_impl(writer: &mut CodeWriter) {
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltTypedArray", |impl_writer| {
        impl_writer.line("/// An empty `Uint8` view with a fresh reference identity.");
        impl_writer.line("pub fn new() -> Self { Self::from_bytes(Vec::new()) }");
        impl_writer.line("/// A `Uint8` view over owned bytes, with a fresh identity.");
        impl_writer.line(
            "pub fn from_bytes(bytes: Vec<u8>) -> Self { Self::with_bytes(SmeltTypedArrayKind::Uint8, bytes) }",
        );
        impl_writer.line("/// A view of `kind` over owned bytes, in fresh storage.");
        impl_writer.block(
            "pub fn with_bytes(kind: SmeltTypedArrayKind, bytes: Vec<u8>) -> Self",
            |fn_writer| {
                fn_writer.line("let length = bytes.len() / kind.byte_width();");
                fn_writer.line("Self { id: smelt_next_object_id(), kind, bytes: ::std::rc::Rc::new(::std::cell::RefCell::new(bytes)), buffer_id: smelt_next_object_id(), byte_offset: 0, length }");
            },
        );
        // `new Uint8Array(8)` is a LENGTH, not a byte count: for a wider view
        // the storage is `length * BYTES_PER_ELEMENT` bytes, all zero.
        impl_writer.line("/// A zero-filled view of `kind` holding `length` elements.");
        impl_writer.line(
            "pub fn with_length(kind: SmeltTypedArrayKind, length: usize) -> Self { Self::with_bytes(kind, vec![0_u8; length * kind.byte_width()]) }",
        );
        impl_writer.line("/// A view of `kind` over the given elements, in fresh storage.");
        impl_writer.block(
            "pub fn with_elements(kind: SmeltTypedArrayKind, elements: &[f64]) -> Self",
            |fn_writer| {
                fn_writer.line("let width = kind.byte_width();");
                fn_writer.line("let mut bytes = vec![0_u8; elements.len() * width];");
                fn_writer.line("for (index, element) in elements.iter().enumerate() { kind.encode(*element, &mut bytes, index * width); }");
                fn_writer.line("Self::with_bytes(kind, bytes)");
            },
        );
        // The shared-buffer constructor. This is what makes `new
        // Uint8Array(buffer, 8, 2)` and `subarray` views of the SAME storage:
        // writes through one are visible through the other.
        impl_writer.line("/// A view of `kind` over an existing buffer's storage.");
        impl_writer.block(
            "pub fn over_buffer(kind: SmeltTypedArrayKind, buffer: &SmeltArrayBuffer, byte_offset: usize, length: Option<usize>) -> Self",
            |fn_writer| {
                fn_writer.line("let storage = buffer.storage();");
                fn_writer.line("let available = storage.borrow().len().saturating_sub(byte_offset) / kind.byte_width();");
                fn_writer.line("let length = length.map_or(available, |length| length.min(available));");
                fn_writer.line("Self { id: smelt_next_object_id(), kind, bytes: storage, buffer_id: buffer.id(), byte_offset, length }");
            },
        );
        impl_writer.line("/// JS reference identity of this view.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// The element type this view reads.");
        impl_writer.line("pub fn kind(&self) -> SmeltTypedArrayKind { self.kind }");
        impl_writer.line("/// The constructor name, which is also the `[object X]` tag.");
        impl_writer.line("pub fn class_name(&self) -> &'static str { self.kind.class_name() }");
        impl_writer.line("/// `length`: the element count.");
        impl_writer.line("pub fn length(&self) -> f64 { self.length as f64 }");
        impl_writer.line("/// `byteLength`: the byte count this view spans.");
        impl_writer.line("pub fn byte_length(&self) -> f64 { (self.length * self.kind.byte_width()) as f64 }");
        impl_writer.line("/// `byteOffset`: where this view starts in its buffer.");
        impl_writer.line("pub fn byte_offset(&self) -> f64 { self.byte_offset as f64 }");
        impl_writer.line("/// `buffer`: the storage this view reads, shared not copied.");
        impl_writer.line("pub fn buffer(&self) -> SmeltArrayBuffer { SmeltArrayBuffer::from_storage(self.buffer_id, ::std::rc::Rc::clone(&self.bytes)) }");
        impl_writer.line("/// A COPY of the bytes this view spans.");
        impl_writer.block("pub fn to_bytes(&self) -> Vec<u8>", |fn_writer| {
            fn_writer.line("let bytes = self.bytes.borrow();");
            fn_writer.line("let start = self.byte_offset.min(bytes.len());");
            fn_writer.line("let end = (start + self.length * self.kind.byte_width()).min(bytes.len());");
            fn_writer.line("bytes[start..end].to_vec()");
        });
        impl_writer.line("/// The view's decoded elements, at its own width and signedness.");
        impl_writer.block("pub fn to_elements(&self) -> Vec<f64>", |fn_writer| {
            fn_writer.line("let bytes = self.bytes.borrow();");
            fn_writer.line("let width = self.kind.byte_width();");
            fn_writer.line("(0..self.length).map(|index| self.kind.decode(&bytes, self.byte_offset + index * width)).collect()");
        });
        // An out-of-range index READ is `undefined` in JavaScript, which is
        // `None` here; an out-of-range WRITE is dropped, not an error.
        // `TypedArray.prototype.toString` IS `Array.prototype.toString`, so a
        // view stringifies as its joined elements — `String(view)` and
        // `${view}` are "1,2,3", not "[object Uint8Array]". (The `[object X]`
        // tag is a different question, answered by
        // `Object.prototype.toString.call`.)
        impl_writer.line("/// `toString()`: the elements joined by commas.");
        impl_writer.line(
            "pub fn to_js_string(&self) -> String { self.to_elements().into_iter().map(|element| element.to_string()).collect::<Vec<_>>().join(\",\") }",
        );
        impl_writer.line("/// An indexed element read; `None` past the end.");
        impl_writer.block("pub fn get(&self, index: f64) -> Option<f64>", |fn_writer| {
            fn_writer.line("if index < 0.0 || index.fract() != 0.0 { return None; }");
            fn_writer.line("let index = index as usize;");
            fn_writer.line("if index >= self.length { return None; }");
            fn_writer.line("let bytes = self.bytes.borrow();");
            fn_writer.line("Some(self.kind.decode(&bytes, self.byte_offset + index * self.kind.byte_width()))");
        });
        // Writing a whole byte window is what an in-place FILL needs
        // (`crypto.getRandomValues`): the bytes are already the right width, so
        // going through `set_index` per element would decode and re-encode
        // them. It writes only this view's window, so a view over part of a
        // buffer leaves the rest of the storage alone.
        impl_writer.line("/// Overwrite this view's byte window; extra source bytes are ignored.");
        impl_writer.block("pub fn write_bytes(&self, source: &[u8])", |fn_writer| {
            fn_writer.line("let mut bytes = self.bytes.borrow_mut();");
            fn_writer.line("let start = self.byte_offset.min(bytes.len());");
            fn_writer.line("let end = (start + self.length * self.kind.byte_width()).min(bytes.len());");
            fn_writer.line("let count = (end - start).min(source.len());");
            fn_writer.line("bytes[start..start + count].copy_from_slice(&source[..count]);");
        });
        impl_writer.line("/// Overwrite bytes at an offset WITHIN this view's window.");
        impl_writer.block("pub fn write_bytes_at(&self, offset: usize, source: &[u8])", |fn_writer| {
            fn_writer.line("let mut bytes = self.bytes.borrow_mut();");
            fn_writer.line("let end = self.byte_offset + self.length * self.kind.byte_width();");
            fn_writer.line("for (step, byte) in source.iter().enumerate() { let at = self.byte_offset + offset + step; if at < end { if let Some(slot) = bytes.get_mut(at) { *slot = *byte; } } }");
        });
        impl_writer.line("/// An indexed element write; dropped past the end.");
        impl_writer.block("pub fn set_index(&self, index: f64, value: f64)", |fn_writer| {
            fn_writer.line("if index < 0.0 || index.fract() != 0.0 { return; }");
            fn_writer.line("let index = index as usize;");
            fn_writer.line("if index >= self.length { return; }");
            fn_writer.line("let mut bytes = self.bytes.borrow_mut();");
            fn_writer.line("let at = self.byte_offset + index * self.kind.byte_width();");
            fn_writer.line("self.kind.encode(value, &mut bytes, at);");
        });
        // `subarray` SHARES the storage; `slice` copies. Both take element
        // indices, both clamp, and both count back from the end for a negative
        // bound.
        impl_writer.line("/// `subarray(start, end)`: another view over the SAME storage.");
        impl_writer.block(
            "pub fn subarray(&self, start: i64, end: Option<i64>) -> Self",
            |fn_writer| {
                fn_writer.line("let (from, to) = self.element_range(start, end);");
                fn_writer.line("Self { id: smelt_next_object_id(), kind: self.kind, bytes: ::std::rc::Rc::clone(&self.bytes), buffer_id: self.buffer_id, byte_offset: self.byte_offset + from * self.kind.byte_width(), length: to.saturating_sub(from) }");
            },
        );
        impl_writer.line("/// `slice(start, end)`: a COPY of an element range.");
        impl_writer.block(
            "pub fn slice(&self, start: i64, end: Option<i64>) -> Self",
            |fn_writer| {
                fn_writer.line("let (from, to) = self.element_range(start, end);");
                fn_writer.line("let width = self.kind.byte_width();");
                fn_writer.line("let bytes = self.bytes.borrow();");
                fn_writer.line("let at = self.byte_offset + from * width;");
                fn_writer.line("let until = (self.byte_offset + to * width).min(bytes.len());");
                fn_writer.line("Self::with_bytes(self.kind, bytes[at.min(until)..until].to_vec())");
            },
        );
        impl_writer.line("/// `set(source, offset)`: copy elements in, converting per element.");
        impl_writer.block(
            "pub fn set_from(&self, source: &Self, offset: usize)",
            |fn_writer| {
                // Element-by-element, not byte-by-byte: `new
                // Uint8Array(4).set(new Float64Array([1]))` writes the NUMBER
                // 1, not the eight bytes of a double.
                fn_writer.line("for (index, element) in source.to_elements().into_iter().enumerate() { self.set_index((offset + index) as f64, element); }");
            },
        );
        impl_writer.line("/// `fill(value, start, end)`: write one value across a range.");
        impl_writer.block(
            "pub fn fill(&self, value: f64, start: i64, end: Option<i64>) -> Self",
            |fn_writer| {
                fn_writer.line("let (from, to) = self.element_range(start, end);");
                fn_writer.line("for index in from..to { self.set_index(index as f64, value); }");
                fn_writer.line("self.clone()");
            },
        );
        impl_writer.line("/// Clamped element bounds, counting back from the end when negative.");
        impl_writer.block(
            "fn element_range(&self, start: i64, end: Option<i64>) -> (usize, usize)",
            |fn_writer| {
                fn_writer.line("let len = self.length as i64;");
                fn_writer.line("let from = (if start < 0 { len + start } else { start }).clamp(0, len);");
                fn_writer.line("let to = end.map_or(len, |end| if end < 0 { len + end } else { end }).clamp(0, len);");
                fn_writer.line("(from as usize, to.max(from) as usize)");
            },
        );
    });
    writer.blank_line();
}

/// Emit the view's dynamic-boundary adapters.
///
/// Increment 2 of the plan. The erased form is the byte-backed host record the
/// erased face uses, so a concrete view and a `new Uint8Array(..)` are the same
/// value once erased — every erased answer (the `[object X]` tag, index keys,
/// element decoding, `ArrayBuffer.isView`) is produced by the helpers in
/// `byte_buffer_prelude` from this record, and the kind's own marker is what
/// picks the right ones.
fn emit_typed_array_adapters(writer: &mut CodeWriter) {
    writer.line("/// Erase a typed-array view for a dynamic boundary.");
    writer.block("impl IntoSmeltUnknown for SmeltTypedArray", |impl_writer| {
        impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
            // The record's `bytes` are the view's OWN bytes and its `length`
            // the element count at its own width, so an erased `Float64Array`
            // enumerates two elements over sixteen bytes rather than sixteen.
            // Retaining the live value is what makes the boundary two-way: the
            // record's id is the key, so `SmeltFromUnknown` hands back the SAME
            // view (sharing its storage) and an index write through the record
            // reaches the storage the concrete value is still holding.
            fn_writer.line("smelt_register_host_origin(self.id, self.clone());");
            fn_writer.line("let bytes = self.to_bytes();");
            fn_writer.line(
                "let elements: Vec<SmeltUnknown> = bytes.iter().map(|byte| SmeltUnknown::Number(f64::from(*byte))).collect();",
            );
            // A view's record carries its STORAGE under `buffer`, plus the
            // offset into it, which is what `view.buffer` and
            // `view.byteOffset` answer once erased and what a clone that
            // reconstructs through `Object.getPrototypeOf(x).constructor`
            // reads back. The storage record is the whole buffer, not the
            // window: `new Uint8Array(buffer, 2, 4).buffer.byteLength` is the
            // buffer's length.
            fn_writer.line("let storage = self.buffer().into_smelt_unknown();");
            fn_writer.line(format!(
                "{with_id}(self.id, self.kind.marker(), elements, Some(storage), self.byte_offset)",
                with_id = smelt_stdlib::runtime_symbols::byte_buffer::VIEW_RECORD_WITH_ID,
            ));
        });
    });
    writer.blank_line();
    // The one genuine dynamic boundary in construction. `new Uint8Array(x)`
    // where `x` is `unknown` (or a generic `T`) is a spelling JavaScript
    // resolves by INSPECTING the argument: a number is a length, an array-like
    // is elements, another view or a buffer re-views the bytes. There is no
    // static type to carry that decision, so it is made at run time here —
    // once, in the prelude — rather than in each generated call site.
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltTypedArray", |impl_writer| {
        impl_writer.line("/// Construct a view of `kind` from an argument of unknown shape.");
        impl_writer.block(
            "pub fn from_erased(kind: SmeltTypedArrayKind, value: &SmeltUnknown) -> Self",
            |fn_writer| {
                fn_writer.line("match value {");
                fn_writer.line("    SmeltUnknown::Number(length) => Self::with_length(kind, length.max(0.0) as usize),");
                fn_writer.line("    SmeltUnknown::Array(items) => { let elements = items.iter().map(|item| match item { SmeltUnknown::Number(value) => value, _ => 0.0 }).collect::<Vec<f64>>(); Self::with_elements(kind, &elements) }");
                fn_writer.line(format!(
                    "    SmeltUnknown::Object(_) => {{ let source = Self::smelt_from_unknown(value.clone()); if {is_view}(value) {{ Self::with_elements(kind, &source.to_elements()) }} else {{ Self::with_bytes(kind, source.to_bytes()) }} }}",
                    is_view = smelt_stdlib::runtime_symbols::byte_buffer::IS_VIEW,
                ));
                fn_writer.line("    _ => Self::with_length(kind, 0),");
                fn_writer.line("}");
            },
        );
    });
    writer.blank_line();
    writer.line("/// Rebuild a typed-array view from an erased value.");
    writer.block("impl SmeltFromUnknown for SmeltTypedArray", |impl_writer| {
        impl_writer.block(
            "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
            |fn_writer| {
                // Any byte-backed record answers here, not only a view's: every
                // view and every buffer carries its storage under `bytes`, so a
                // `DataView` or an `ArrayBuffer` crossing into a byte view
                // reads its bytes rather than silently becoming empty. The KIND
                // comes from whichever marker the record carries, falling back
                // to `Uint8` — which is what a buffer's bytes are.
                fn_writer.line("if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }");
                fn_writer.line("let SmeltUnknown::Object(map) = value else { return Self::new() };");
                fn_writer.line("let Some(SmeltUnknown::Array(items)) = map.get(\"bytes\") else { return Self::new() };");
                fn_writer.line("let bytes = items.into_vec().into_iter().map(|item| match item { SmeltUnknown::Number(value) => value as i64 as u8, _ => 0 }).collect::<Vec<u8>>();");
                fn_writer.line("let kind = map.iter().find_map(|(key, _)| SmeltTypedArrayKind::from_marker(&key)).unwrap_or(SmeltTypedArrayKind::Uint8);");
                fn_writer.line("let width = kind.byte_width();");
                fn_writer.line("let length = bytes.len() / width;");
                fn_writer.line("Self { id: map.id, kind, bytes: ::std::rc::Rc::new(::std::cell::RefCell::new(bytes)), buffer_id: smelt_next_object_id(), byte_offset: 0, length }");
            },
        );
    });
    writer.blank_line();
}

/// The eleven views as `(Rust variant, class name, identity marker, width)`.
///
/// Derived from the host-object registry rather than written out here, so the
/// concrete family and the erased face cannot disagree about a view's name,
/// marker or element width — they are the same eleven classes seen twice.
fn kind_table() -> Vec<(String, &'static str, &'static str, usize)> {
    smelt_stdlib::TYPED_ARRAY_CLASS_NAMES
        .iter()
        .filter_map(|class_name| {
            let element = smelt_stdlib::typed_array_element(class_name)?;
            let marker = smelt_stdlib::host_object_marker(class_name)?;
            Some((
                variant_name(element),
                *class_name,
                marker,
                element.byte_width(),
            ))
        })
        .collect()
}

/// The Rust enum variant spelling for one element type.
///
/// The registry's element tag is lowercase (`"uint8clamped"`), and the emitted
/// enum wants `Uint8Clamped`; mapping here keeps the tag as the single source
/// of the set while the generated code stays idiomatic Rust.
fn variant_name(element: smelt_stdlib::TypedArrayElement) -> String {
    match element {
        smelt_stdlib::TypedArrayElement::Int8 => "Int8",
        smelt_stdlib::TypedArrayElement::Uint8 => "Uint8",
        smelt_stdlib::TypedArrayElement::Uint8Clamped => "Uint8Clamped",
        smelt_stdlib::TypedArrayElement::Int16 => "Int16",
        smelt_stdlib::TypedArrayElement::Uint16 => "Uint16",
        smelt_stdlib::TypedArrayElement::Int32 => "Int32",
        smelt_stdlib::TypedArrayElement::Uint32 => "Uint32",
        smelt_stdlib::TypedArrayElement::Float32 => "Float32",
        smelt_stdlib::TypedArrayElement::Float64 => "Float64",
        smelt_stdlib::TypedArrayElement::BigInt64 => "BigInt64",
        smelt_stdlib::TypedArrayElement::BigUint64 => "BigUint64",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kind table is the registry's eleven views, not a second list.
    ///
    /// The concrete family and the erased face answer for the same eleven
    /// classes, so a name, a marker or an element width that differed between
    /// them would be a value that changes shape when it crosses the boundary.
    #[test]
    fn the_kind_table_is_the_registry() {
        let kinds = kind_table();
        assert_eq!(
            kinds.len(),
            smelt_stdlib::TYPED_ARRAY_CLASS_NAMES.len(),
            "one kind per registered view"
        );
        for (variant, class_name, marker, width) in kinds {
            let element = smelt_stdlib::typed_array_element(class_name)
                .expect("a registered view has an element type");
            assert_eq!(width, element.byte_width(), "{class_name} width");
            assert_eq!(
                marker,
                smelt_stdlib::host_object_marker(class_name)
                    .expect("a registered view has a marker"),
                "{class_name} marker"
            );
            assert_eq!(variant, variant_name(element), "{class_name} variant");
        }
    }

    /// The emitted family keeps the byte view's name and its shared storage.
    ///
    /// `SmeltUint8Array` is the door every reachable use comes through today
    /// (`TextEncoder.encode`, `TextDecoder.decode`, `crypto.getRandomValues`),
    /// and `over_buffer`/`subarray` are what make a view a VIEW rather than a
    /// copy — the members increment 3 needs and the ones a refactor would most
    /// easily drop.
    #[test]
    fn the_emitted_family_keeps_the_byte_view_and_its_storage() {
        let mut writer = CodeWriter::new();
        emit(&mut writer, true);
        let text = writer.finish();
        for expected in [
            "pub type SmeltUint8Array = SmeltTypedArray;",
            "pub fn over_buffer(",
            "pub fn subarray(",
            "pub fn byte_offset(&self) -> f64",
            "pub fn buffer(&self) -> SmeltArrayBuffer",
            "impl IntoSmeltUnknown for SmeltTypedArray",
            "impl SmeltFromUnknown for SmeltTypedArray",
            "impl IntoSmeltUnknown for SmeltArrayBuffer",
        ] {
            assert!(text.contains(expected), "missing `{expected}`");
        }
        // The adapters are gated: a program that never crosses the dynamic
        // boundary does not emit the carrier, so the impls must not be emitted
        // either.
        let mut typed_only = CodeWriter::new();
        emit(&mut typed_only, false);
        let typed_only = typed_only.finish();
        assert!(
            !typed_only.contains("impl IntoSmeltUnknown for SmeltTypedArray"),
            "the erasure adapters must stay behind the unknown gate"
        );
    }
}
