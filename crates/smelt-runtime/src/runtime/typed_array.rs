// The concrete typed-array family: byte storage and element views.
//
// Two runtime types, both with a JavaScript reference identity, plus the
// `SmeltDataView` record, the `SmeltTypedArrayKind` enum and the `SmeltUint8Array`
// alias:
//
// * `SmeltArrayBuffer`, byte storage that owns its bytes and interprets none
//   of them (`ArrayBuffer`);
// * `SmeltTypedArray`, an ELEMENT VIEW over storage: a kind (element type), a
//   byte offset into the buffer, and an element count.
//
// Making the offset and the buffer real is the whole point of the shape. A
// typed array is not a `Vec<u8>` with a name: `view.subarray(1)` is a second
// view over the SAME storage, so a write through one is visible through the
// other, and `view.byteOffset`/`view.buffer` are what the spec's own members
// answer. A design that copied bytes per view would pass every test that does
// not share a buffer and silently diverge on every test that does.
//
// The kind table is derived from the host-object registry
// (`smelt_stdlib::TYPED_ARRAY_CLASS_NAMES` plus `typed_array_element`), so the
// eleven views' names, element widths and `[object X]` tags cannot drift from
// the erased face's answers for the same eleven classes.

// Emit the element-kind enum: width, spec tag, and the element codec.
//
// One enum per element type rather than a generic parameter, because the kind
// is a RUNTIME property of the value: `Object.prototype.toString.call(view)`
// reports it, two views of different kinds over the same buffer are different
// values, and reflective construction reads it back off an erased record. A
// `SmeltTypedArray<T>` would have to erase that answer at every boundary.
// @smelt:item SmeltTypedArrayKind when byte_array
/// The element type of a typed-array view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum SmeltTypedArrayKind {
    //~ `Int8Array`: 1-byte element.
    Int8,
    //~ `Uint8Array`: 1-byte element.
    Uint8,
    //~ `Uint8ClampedArray`: 1-byte element.
    Uint8Clamped,
    //~ `Int16Array`: 2-byte elements.
    Int16,
    //~ `Uint16Array`: 2-byte elements.
    Uint16,
    //~ `Int32Array`: 4-byte elements.
    Int32,
    //~ `Uint32Array`: 4-byte elements.
    Uint32,
    //~ `Float32Array`: 4-byte elements.
    Float32,
    //~ `Float64Array`: 8-byte elements.
    Float64,
    //~ `BigInt64Array`: 8-byte elements.
    BigInt64,
    //~ `BigUint64Array`: 8-byte elements.
    BigUint64,
}

#[allow(dead_code)]
impl SmeltTypedArrayKind {
    //~ The constructor name, which is also the `[object X]` tag.
    pub fn class_name(self) -> &'static str {
        match self {
            Self::Int8 => "Int8Array",
            Self::Uint8 => "Uint8Array",
            Self::Uint8Clamped => "Uint8ClampedArray",
            Self::Int16 => "Int16Array",
            Self::Uint16 => "Uint16Array",
            Self::Int32 => "Int32Array",
            Self::Uint32 => "Uint32Array",
            Self::Float32 => "Float32Array",
            Self::Float64 => "Float64Array",
            Self::BigInt64 => "BigInt64Array",
            Self::BigUint64 => "BigUint64Array",
        }
    }
    //~ The identity marker the erased face stamps for this kind.
    pub fn marker(self) -> &'static str {
        match self {
            Self::Int8 => "__smelt_int8array",
            Self::Uint8 => "__smelt_uint8array",
            Self::Uint8Clamped => "__smelt_uint8clampedarray",
            Self::Int16 => "__smelt_int16array",
            Self::Uint16 => "__smelt_uint16array",
            Self::Int32 => "__smelt_int32array",
            Self::Uint32 => "__smelt_uint32array",
            Self::Float32 => "__smelt_float32array",
            Self::Float64 => "__smelt_float64array",
            Self::BigInt64 => "__smelt_bigint64array",
            Self::BigUint64 => "__smelt_biguint64array",
        }
    }
    //~ `BYTES_PER_ELEMENT`: the stride between elements.
    pub fn byte_width(self) -> usize {
        match self {
            Self::Int8 => 1,
            Self::Uint8 => 1,
            Self::Uint8Clamped => 1,
            Self::Int16 => 2,
            Self::Uint16 => 2,
            Self::Int32 => 4,
            Self::Uint32 => 4,
            Self::Float32 => 4,
            Self::Float64 => 8,
            Self::BigInt64 => 8,
            Self::BigUint64 => 8,
        }
    }
    //~ The kind whose marker a byte-backed record carries, if any.
    pub fn from_marker(marker: &str) -> Option<Self> {
        if marker == "__smelt_int8array" { return Some(Self::Int8); }
        if marker == "__smelt_uint8array" { return Some(Self::Uint8); }
        if marker == "__smelt_uint8clampedarray" { return Some(Self::Uint8Clamped); }
        if marker == "__smelt_int16array" { return Some(Self::Int16); }
        if marker == "__smelt_uint16array" { return Some(Self::Uint16); }
        if marker == "__smelt_int32array" { return Some(Self::Int32); }
        if marker == "__smelt_uint32array" { return Some(Self::Uint32); }
        if marker == "__smelt_float32array" { return Some(Self::Float32); }
        if marker == "__smelt_float64array" { return Some(Self::Float64); }
        if marker == "__smelt_bigint64array" { return Some(Self::BigInt64); }
        if marker == "__smelt_biguint64array" { return Some(Self::BigUint64); }
        None
    }
    //~ Decoding and encoding are the only places the element's width and
    //~ SIGNEDNESS live. `Uint8Clamped` differs from `Uint8` on the write
    //~ side only: it saturates where the others wrap, which is the whole
    //~ reason the spec has a separate view for it.
    //~ Read one element out of `bytes` at a byte offset.
    pub fn decode(self, bytes: &[u8], at: usize) -> f64 {
        let width = self.byte_width();
        if at + width > bytes.len() { return 0.0; }
        let window = &bytes[at..at + width];
        match self {
            Self::Int8 => f64::from(window[0] as i8),
            Self::Uint8 | Self::Uint8Clamped => f64::from(window[0]),
            Self::Int16 => f64::from(i16::from_le_bytes([window[0], window[1]])),
            Self::Uint16 => f64::from(u16::from_le_bytes([window[0], window[1]])),
            Self::Int32 => f64::from(i32::from_le_bytes([window[0], window[1], window[2], window[3]])),
            Self::Uint32 => f64::from(u32::from_le_bytes([window[0], window[1], window[2], window[3]])),
            Self::Float32 => f64::from(f32::from_le_bytes([window[0], window[1], window[2], window[3]])),
            Self::Float64 => f64::from_le_bytes([window[0], window[1], window[2], window[3], window[4], window[5], window[6], window[7]]),
            //~ A `BigInt` is modeled as a number, so a 64-bit integer
            //~ view decodes through `f64` like every other kind; the
            //~ registry's comment says the same thing.
            Self::BigInt64 => i64::from_le_bytes([window[0], window[1], window[2], window[3], window[4], window[5], window[6], window[7]]) as f64,
            Self::BigUint64 => u64::from_le_bytes([window[0], window[1], window[2], window[3], window[4], window[5], window[6], window[7]]) as f64,
        }
    }
    //~ Write one element into `bytes` at a byte offset.
    pub fn encode(self, value: f64, bytes: &mut [u8], at: usize) {
        let width = self.byte_width();
        if at + width > bytes.len() { return; }
        let encoded = self.encode_bytes(value);
        bytes[at..at + width].copy_from_slice(&encoded[..width]);
    }
    //~ The encoding itself, as a tail expression: the widths differ, so each
    //~ kind fills the low bytes of one eight-byte buffer and the caller
    //~ copies as many as the element needs.
    //~ One element's little-endian bytes, low-order first.
    fn encode_bytes(self, value: f64) -> [u8; 8] {
        match self {
            Self::Int8 => [(value as i64 as i8) as u8, 0, 0, 0, 0, 0, 0, 0],
            Self::Uint8 => [(value as i64 as u8), 0, 0, 0, 0, 0, 0, 0],
            //~ The saturating write: `new Uint8ClampedArray([300])[0]`
            //~ is 255, not 44.
            Self::Uint8Clamped => [(if value.is_nan() { 0.0 } else { value.round_ties_even().clamp(0.0, 255.0) }) as u8, 0, 0, 0, 0, 0, 0, 0],
            Self::Int16 => { let mut out = [0_u8; 8]; out[..2].copy_from_slice(&(value as i64 as i16).to_le_bytes()); out },
            Self::Uint16 => { let mut out = [0_u8; 8]; out[..2].copy_from_slice(&(value as i64 as u16).to_le_bytes()); out },
            Self::Int32 => { let mut out = [0_u8; 8]; out[..4].copy_from_slice(&(value as i64 as i32).to_le_bytes()); out },
            Self::Uint32 => { let mut out = [0_u8; 8]; out[..4].copy_from_slice(&(value as i64 as u32).to_le_bytes()); out },
            Self::Float32 => { let mut out = [0_u8; 8]; out[..4].copy_from_slice(&(value as f32).to_le_bytes()); out },
            Self::Float64 => value.to_le_bytes(),
            Self::BigInt64 => (value as i64).to_le_bytes(),
            Self::BigUint64 => (value as i64 as u64).to_le_bytes(),
        }
    }
}

//~ Emit `SmeltArrayBuffer`: byte storage with a reference identity.
//~
//~ The bytes are shared (`Rc<RefCell<..>>`) because an `ArrayBuffer` IS shared:
//~ every view over it observes writes through any other view, and passing the
//~ buffer around does not copy it. `slice` is the one member that copies,
//~ exactly as the spec says.
/// Byte storage with a JavaScript reference identity (`ArrayBuffer`).
#[derive(Clone)]
pub struct SmeltArrayBuffer {
    id: usize,
    //~ The storage, shared by every view over it.
    bytes: ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>>,
    //~ `SharedArrayBuffer` is this same type with the flag set. The two
    //~ constructors differ in their `[object X]` tag, their `instanceof`
    //~ answer and the growth members - and in nothing about the bytes,
    //~ because storage every view writes through is what this already is.
    //~ The one behaviour a hand-written Rust pair would differ on is
    //~ cross-thread visibility, which a program with no workers cannot
    //~ observe. Answering "ArrayBuffer" for a `SharedArrayBuffer` WOULD be
    //~ observable, so the flag is not optional.
    //~ Whether this storage was constructed as a `SharedArrayBuffer`.
    shared: bool,
}

//~ Structural equality over the bytes, the same rule the byte view uses:
//~ `toEqual` compares contents.
impl PartialEq for SmeltArrayBuffer { fn eq(&self, other: &Self) -> bool { *self.bytes.borrow() == *other.bytes.borrow() } }
impl ::std::fmt::Debug for SmeltArrayBuffer { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { write!(formatter, "{} {{ byteLength: {} }}", self.class_name(), self.bytes.borrow().len()) } }
#[allow(dead_code)]
impl SmeltArrayBuffer {
    //~ Zeroed storage of `byte_length` bytes.
    pub fn new(byte_length: usize) -> Self { Self::from_bytes(vec![0_u8; byte_length]) }
    //~ Storage over owned bytes, with a fresh identity.
    pub fn from_bytes(bytes: Vec<u8>) -> Self { Self { id: smelt_next_object_id(), bytes: ::std::rc::Rc::new(::std::cell::RefCell::new(bytes)), shared: false } }
    //~ Zeroed SHARED storage: `new SharedArrayBuffer(n)`.
    pub fn new_shared(byte_length: usize) -> Self { Self { id: smelt_next_object_id(), bytes: ::std::rc::Rc::new(::std::cell::RefCell::new(vec![0_u8; byte_length])), shared: true } }
    //~ Whether this storage is a `SharedArrayBuffer`.
    pub fn is_shared(&self) -> bool { self.shared }
    //~ The constructor name, which is also the `[object X]` tag.
    pub fn class_name(&self) -> &'static str { if self.shared { "SharedArrayBuffer" } else { "ArrayBuffer" } }
    //~ JS reference identity of this storage.
    pub fn id(&self) -> usize { self.id }
    //~ `byteLength`: the storage size in bytes.
    pub fn byte_length(&self) -> f64 { self.bytes.borrow().len() as f64 }
    //~ A COPY of the storage's bytes.
    pub fn to_bytes(&self) -> Vec<u8> { self.bytes.borrow().clone() }
    //~ The shared storage handle, for a view over it.
    pub fn storage(&self) -> ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>> { ::std::rc::Rc::clone(&self.bytes) }
    //~ Storage sharing this buffer's bytes handle and identity.
    pub fn from_storage(id: usize, bytes: ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>>) -> Self { Self { id, bytes, shared: false } }
    //~ Storage sharing a handle and identity, keeping the shared flag.
    pub fn from_shared_storage(id: usize, bytes: ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>>, shared: bool) -> Self { Self { id, bytes, shared } }
    //~ `ArrayBuffer.prototype.slice` COPIES, and both bounds clamp and count
    //~ back from the end when negative — the same rule the erased face's
    //~ slice helper applies.
    //~ The write side an erased record's index write reaches (see
    //~ `smelt_typed_array_write_origin`): absolute byte offsets, because
    //~ storage interprets nothing.
    //~ Overwrite bytes at an absolute offset; out-of-range bytes are dropped.
    pub fn write_bytes_at(&self, offset: usize, source: &[u8]) {
        let mut bytes = self.bytes.borrow_mut();
        for (step, byte) in source.iter().enumerate() { if let Some(slot) = bytes.get_mut(offset + step) { *slot = *byte; } }
    }
    //~ `slice(start, end)`: a COPY of a byte range in fresh storage.
    //~ Emitted before the closing brace below so `slice` stays last in the
    //~ inherent impl; see the `Default` impl after it for why storage has one.
    pub fn slice(&self, start: i64, end: Option<i64>) -> Self {
        let bytes = self.bytes.borrow();
        let len = bytes.len() as i64;
        let from = (if start < 0 { len + start } else { start }).clamp(0, len) as usize;
        let to = end.map_or(len, |end| if end < 0 { len + end } else { end }).clamp(0, len) as usize;
        //~ `SharedArrayBuffer.prototype.slice` answers a
        //~ `SharedArrayBuffer`, so the copy keeps the species.
        let copy = Self::from_bytes(bytes[from..to.max(from)].to_vec());
        Self { shared: self.shared, ..copy }
    }
}
//~ Zero-length storage is the empty value the type can hold, and a
//~ monomorphized generic (`clone<T>(x: T): T` instantiated at
//~ `ArrayBuffer`) asks for it by its `Default` bound. The view half already
//~ has one for the same reason.
impl Default for SmeltArrayBuffer { fn default() -> Self { Self::new(0) } }

// @smelt:item-end

// Emit the storage's dynamic-boundary adapters.
//
// The erased form is the byte-backed host record the erased face uses, so a
// concrete buffer and an erased one are the same value once erased, and every
// answer the erased helpers give (`ArrayBuffer.isView` false, no own index
// keys, `{}` under `JSON.stringify`) holds for a concrete buffer that crossed
// the boundary.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltArrayBuffer when byte_array & unknown
/// Erase byte storage for a dynamic boundary.
impl IntoSmeltUnknown for SmeltArrayBuffer {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        let smelt_host_origin = self.clone();
        let bytes = self.bytes.borrow();
        let elements: Vec<SmeltUnknown> = bytes.iter().map(|byte| SmeltUnknown::Number(f64::from(*byte))).collect();
        //~ The erased face's OWN record builder, at this value's identity:
        //~ storage carries no `buffer` of its own and no offset.
        //~ The marker is the erased face's IDENTITY, so it has to follow
        //~ the flag: a `SharedArrayBuffer` that crossed the boundary and
        //~ came back must still answer `instanceof SharedArrayBuffer`.
        let marker = if self.shared { "__smelt_sharedarraybuffer" } else { "__smelt_arraybuffer" };
        smelt_with_host_origin(smelt_host_buffer_view_record_with_id(self.id, marker, elements, None, 0), smelt_host_origin)
    }
}

/// Rebuild byte storage from an erased value.
impl SmeltFromUnknown for SmeltArrayBuffer {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        //~ Any byte-backed record answers here, not only a buffer's:
        //~ every view and every buffer carries its storage under
        //~ `bytes`, so a view crossing into a buffer parameter reads the
        //~ bytes it views rather than becoming empty.
        if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }
        let SmeltUnknown::Object(map) = value else { return Self::new(0) };
        let Some(SmeltUnknown::Array(items)) = map.get("bytes") else { return Self::new(0) };
        let bytes = items.into_vec().into_iter().map(|item| match item { SmeltUnknown::Number(value) => value as i64 as u8, _ => 0 }).collect::<Vec<u8>>();
        let shared = map.contains_key("__smelt_sharedarraybuffer");
        Self::from_shared_storage(map.id, ::std::rc::Rc::new(::std::cell::RefCell::new(bytes)), shared)
    }
}

// @smelt:item-end

// Emit the view struct, its equality and its `Debug`.
// @smelt:item SmeltTypedArray when byte_array
/// An element view over byte storage: kind, offset, length.
#[derive(Clone)]
pub struct SmeltTypedArray {
    id: usize,
    //~ The element type this view reads and writes.
    kind: SmeltTypedArrayKind,
    //~ The storage, SHARED with every other view over it.
    bytes: ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>>,
    //~ Identity of the `ArrayBuffer` this view reports.
    buffer_id: usize,
    //~ `byteOffset`: where this view starts in the storage.
    byte_offset: usize,
    //~ `length`: how many elements this view spans.
    length: usize,
    //~ A view over a `SharedArrayBuffer` must report that buffer back:
    //~ `new Uint8Array(sab).buffer` is the `SharedArrayBuffer`, so the
    //~ species travels with the view rather than being re-derived from
    //~ storage it cannot ask.
    //~ Whether the storage behind this view is a `SharedArrayBuffer`.
    buffer_shared: bool,
}

//~ Structural equality over the ELEMENTS at this view's kind, which for two
//~ `Uint8` views is byte equality — the answer the byte view has always
//~ given (`expect(encoder.encode("a")).toEqual(new Uint8Array([97]))`).
//~ Comparing the kind too is what keeps a `Uint8Array` from equalling an
//~ `Int8Array` over the same bytes, which Node's deep equality also refuses.
impl PartialEq for SmeltTypedArray { fn eq(&self, other: &Self) -> bool { self.kind == other.kind && self.to_elements() == other.to_elements() } }
//~ Node prints `Uint8Array(3) [ 1, 2, 3 ]` and an empty one as
//~ `Uint8Array(0) []`; the kind supplies the name so every view prints as
//~ its own constructor.
impl ::std::fmt::Debug for SmeltTypedArray {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        let elements = self.to_elements();
        let name = self.kind.class_name();
        if elements.is_empty() { return write!(formatter, "{name}(0) []"); }
        let items = elements.iter().map(|element| element.to_string()).collect::<Vec<String>>().join(", ");
        write!(formatter, "{name}({}) [ {items} ]", elements.len())
    }
}
impl Default for SmeltTypedArray { fn default() -> Self { Self::new() } }

//~ Emit the view's members.
#[allow(dead_code)]
impl SmeltTypedArray {
    //~ An empty `Uint8` view with a fresh reference identity.
    pub fn new() -> Self { Self::from_bytes(Vec::new()) }
    //~ A `Uint8` view over owned bytes, with a fresh identity.
    pub fn from_bytes(bytes: Vec<u8>) -> Self { Self::with_bytes(SmeltTypedArrayKind::Uint8, bytes) }
    //~ A view of `kind` over owned bytes, in fresh storage.
    pub fn with_bytes(kind: SmeltTypedArrayKind, bytes: Vec<u8>) -> Self {
        let length = bytes.len() / kind.byte_width();
        Self { id: smelt_next_object_id(), kind, bytes: ::std::rc::Rc::new(::std::cell::RefCell::new(bytes)), buffer_id: smelt_next_object_id(), byte_offset: 0, length, buffer_shared: false }
    }
    //~ `new Uint8Array(8)` is a LENGTH, not a byte count: for a wider view
    //~ the storage is `length * BYTES_PER_ELEMENT` bytes, all zero.
    //~ A zero-filled view of `kind` holding `length` elements.
    pub fn with_length(kind: SmeltTypedArrayKind, length: usize) -> Self { Self::with_bytes(kind, vec![0_u8; length * kind.byte_width()]) }
    //~ A view of `kind` over the given elements, in fresh storage.
    pub fn with_elements(kind: SmeltTypedArrayKind, elements: &[f64]) -> Self {
        let width = kind.byte_width();
        let mut bytes = vec![0_u8; elements.len() * width];
        for (index, element) in elements.iter().enumerate() { kind.encode(*element, &mut bytes, index * width); }
        Self::with_bytes(kind, bytes)
    }
    //~ The shared-buffer constructor. This is what makes `new
    //~ Uint8Array(buffer, 8, 2)` and `subarray` views of the SAME storage:
    //~ writes through one are visible through the other.
    //~ A view of `kind` over an existing buffer's storage.
    pub fn over_buffer(kind: SmeltTypedArrayKind, buffer: &SmeltArrayBuffer, byte_offset: usize, length: Option<usize>) -> Self {
        let storage = buffer.storage();
        let available = storage.borrow().len().saturating_sub(byte_offset) / kind.byte_width();
        let length = length.map_or(available, |length| length.min(available));
        Self { id: smelt_next_object_id(), kind, bytes: storage, buffer_id: buffer.id(), byte_offset, length, buffer_shared: buffer.is_shared() }
    }
    //~ JS reference identity of this view.
    pub fn id(&self) -> usize { self.id }
    //~ The element type this view reads.
    pub fn kind(&self) -> SmeltTypedArrayKind { self.kind }
    //~ The constructor name, which is also the `[object X]` tag.
    pub fn class_name(&self) -> &'static str { self.kind.class_name() }
    //~ `length`: the element count.
    pub fn length(&self) -> f64 { self.length as f64 }
    //~ `byteLength`: the byte count this view spans.
    pub fn byte_length(&self) -> f64 { (self.length * self.kind.byte_width()) as f64 }
    //~ `byteOffset`: where this view starts in its buffer.
    pub fn byte_offset(&self) -> f64 { self.byte_offset as f64 }
    //~ `buffer`: the storage this view reads, shared not copied.
    pub fn buffer(&self) -> SmeltArrayBuffer { SmeltArrayBuffer::from_shared_storage(self.buffer_id, ::std::rc::Rc::clone(&self.bytes), self.buffer_shared) }
    //~ A COPY of the bytes this view spans.
    pub fn to_bytes(&self) -> Vec<u8> {
        let bytes = self.bytes.borrow();
        let start = self.byte_offset.min(bytes.len());
        let end = (start + self.length * self.kind.byte_width()).min(bytes.len());
        bytes[start..end].to_vec()
    }
    //~ The view's decoded elements, at its own width and signedness.
    pub fn to_elements(&self) -> Vec<f64> {
        let bytes = self.bytes.borrow();
        let width = self.kind.byte_width();
        (0..self.length).map(|index| self.kind.decode(&bytes, self.byte_offset + index * width)).collect()
    }
    //~ An out-of-range index READ is `undefined` in JavaScript, which is
    //~ `None` here; an out-of-range WRITE is dropped, not an error.
    //~ `TypedArray.prototype.toString` IS `Array.prototype.toString`, so a
    //~ view stringifies as its joined elements — `String(view)` and
    //~ `${view}` are "1,2,3", not "[object Uint8Array]". (The `[object X]`
    //~ tag is a different question, answered by
    //~ `Object.prototype.toString.call`.)
    //~ `toString()`: the elements joined by commas.
    pub fn to_js_string(&self) -> String { self.to_elements().into_iter().map(|element| element.to_string()).collect::<Vec<_>>().join(",") }
    //~ An indexed element read; `None` past the end.
    pub fn get(&self, index: f64) -> Option<f64> {
        if index < 0.0 || index.fract() != 0.0 { return None; }
        let index = index as usize;
        if index >= self.length { return None; }
        let bytes = self.bytes.borrow();
        Some(self.kind.decode(&bytes, self.byte_offset + index * self.kind.byte_width()))
    }
    //~ Writing a whole byte window is what an in-place FILL needs
    //~ (`crypto.getRandomValues`): the bytes are already the right width, so
    //~ going through `set_index` per element would decode and re-encode
    //~ them. It writes only this view's window, so a view over part of a
    //~ buffer leaves the rest of the storage alone.
    //~ Overwrite this view's byte window; extra source bytes are ignored.
    pub fn write_bytes(&self, source: &[u8]) {
        let mut bytes = self.bytes.borrow_mut();
        let start = self.byte_offset.min(bytes.len());
        let end = (start + self.length * self.kind.byte_width()).min(bytes.len());
        let count = (end - start).min(source.len());
        bytes[start..start + count].copy_from_slice(&source[..count]);
    }
    //~ Overwrite bytes at an offset WITHIN this view's window.
    pub fn write_bytes_at(&self, offset: usize, source: &[u8]) {
        let mut bytes = self.bytes.borrow_mut();
        let end = self.byte_offset + self.length * self.kind.byte_width();
        for (step, byte) in source.iter().enumerate() { let at = self.byte_offset + offset + step; if at < end { if let Some(slot) = bytes.get_mut(at) { *slot = *byte; } } }
    }
    //~ An indexed element write; dropped past the end.
    pub fn set_index(&self, index: f64, value: f64) {
        if index < 0.0 || index.fract() != 0.0 { return; }
        let index = index as usize;
        if index >= self.length { return; }
        let mut bytes = self.bytes.borrow_mut();
        let at = self.byte_offset + index * self.kind.byte_width();
        self.kind.encode(value, &mut bytes, at);
    }
    //~ `subarray` SHARES the storage; `slice` copies. Both take element
    //~ indices, both clamp, and both count back from the end for a negative
    //~ bound.
    //~ `subarray(start, end)`: another view over the SAME storage.
    pub fn subarray(&self, start: i64, end: Option<i64>) -> Self {
        let (from, to) = self.element_range(start, end);
        Self { id: smelt_next_object_id(), kind: self.kind, bytes: ::std::rc::Rc::clone(&self.bytes), buffer_id: self.buffer_id, byte_offset: self.byte_offset + from * self.kind.byte_width(), length: to.saturating_sub(from), buffer_shared: self.buffer_shared }
    }
    //~ `slice(start, end)`: a COPY of an element range.
    pub fn slice(&self, start: i64, end: Option<i64>) -> Self {
        let (from, to) = self.element_range(start, end);
        let width = self.kind.byte_width();
        let bytes = self.bytes.borrow();
        let at = self.byte_offset + from * width;
        let until = (self.byte_offset + to * width).min(bytes.len());
        Self::with_bytes(self.kind, bytes[at.min(until)..until].to_vec())
    }
    //~ `set(source, offset)`: copy elements in, converting per element.
    pub fn set_from(&self, source: &Self, offset: usize) {
        //~ Element-by-element, not byte-by-byte: `new
        //~ Uint8Array(4).set(new Float64Array([1]))` writes the NUMBER
        //~ 1, not the eight bytes of a double.
        for (index, element) in source.to_elements().into_iter().enumerate() { self.set_index((offset + index) as f64, element); }
    }
    //~ `fill(value, start, end)`: write one value across a range.
    pub fn fill(&self, value: f64, start: i64, end: Option<i64>) -> Self {
        let (from, to) = self.element_range(start, end);
        for index in from..to { self.set_index(index as f64, value); }
        self.clone()
    }
    //~ Clamped element bounds, counting back from the end when negative.
    fn element_range(&self, start: i64, end: Option<i64>) -> (usize, usize) {
        let len = self.length as i64;
        let from = (if start < 0 { len + start } else { start }).clamp(0, len);
        let to = end.map_or(len, |end| if end < 0 { len + end } else { end }).clamp(0, len);
        (from as usize, to.max(from) as usize)
    }
}

// @smelt:item-end

// Emit the view's dynamic-boundary adapters.
//
// The erased form is the byte-backed host record the erased face uses, so a
// concrete view and a `new Uint8Array(..)` are the same value once erased —
// every erased answer (the `[object X]` tag, index keys, element decoding,
// `ArrayBuffer.isView`) is produced by the helpers in this family from this
// record, and the kind's own marker is what picks the right ones.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltTypedArray when byte_array & unknown
/// Erase a typed-array view for a dynamic boundary.
impl IntoSmeltUnknown for SmeltTypedArray {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        //~ The record's `bytes` are the view's OWN bytes and its `length`
        //~ the element count at its own width, so an erased `Float64Array`
        //~ enumerates two elements over sixteen bytes rather than sixteen.
        //~ Retaining the live value is what makes the boundary two-way: the
        //~ record's id is the key, so `SmeltFromUnknown` hands back the SAME
        //~ view (sharing its storage) and an index write through the record
        //~ reaches the storage the concrete value is still holding.
        let smelt_host_origin = self.clone();
        let bytes = self.to_bytes();
        let elements: Vec<SmeltUnknown> = bytes.iter().map(|byte| SmeltUnknown::Number(f64::from(*byte))).collect();
        //~ A view's record carries its STORAGE under `buffer`, plus the
        //~ offset into it, which is what `view.buffer` and
        //~ `view.byteOffset` answer once erased and what a clone that
        //~ reconstructs through `Object.getPrototypeOf(x).constructor`
        //~ reads back. The storage record is the whole buffer, not the
        //~ window: `new Uint8Array(buffer, 2, 4).buffer.byteLength` is the
        //~ buffer's length.
        let storage = self.buffer().into_smelt_unknown();
        smelt_with_host_origin(smelt_host_buffer_view_record_with_id(self.id, self.kind.marker(), elements, Some(storage), self.byte_offset), smelt_host_origin)
    }
}

//~ The one genuine dynamic boundary in construction. `new Uint8Array(x)`
//~ where `x` is `unknown` (or a generic `T`) is a spelling JavaScript
//~ resolves by INSPECTING the argument: a number is a length, an array-like
//~ is elements, another view or a buffer re-views the bytes. There is no
//~ static type to carry that decision, so it is made at run time here —
//~ once, in the prelude — rather than in each generated call site.
#[allow(dead_code)]
impl SmeltTypedArray {
    //~ Construct a view of `kind` from an argument of unknown shape.
    pub fn from_erased(kind: SmeltTypedArrayKind, value: &SmeltUnknown) -> Self {
        match value {
            SmeltUnknown::Number(length) => Self::with_length(kind, length.max(0.0) as usize),
            SmeltUnknown::Array(items) => { let elements = items.iter().map(|item| match item { SmeltUnknown::Number(value) => value, _ => 0.0 }).collect::<Vec<f64>>(); Self::with_elements(kind, &elements) }
            SmeltUnknown::Object(_) => { let source = Self::smelt_from_unknown(value.clone()); if smelt_host_buffer_is_view(value) { Self::with_elements(kind, &source.to_elements()) } else { Self::with_bytes(kind, source.to_bytes()) } }
            _ => Self::with_length(kind, 0),
        }
    }
}

/// Rebuild a typed-array view from an erased value.
impl SmeltFromUnknown for SmeltTypedArray {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        //~ Any byte-backed record answers here, not only a view's: every
        //~ view and every buffer carries its storage under `bytes`, so a
        //~ `DataView` or an `ArrayBuffer` crossing into a byte view
        //~ reads its bytes rather than silently becoming empty. The KIND
        //~ comes from whichever marker the record carries, falling back
        //~ to `Uint8` — which is what a buffer's bytes are.
        if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }
        let SmeltUnknown::Object(map) = value else { return Self::new() };
        let Some(SmeltUnknown::Array(items)) = map.get("bytes") else { return Self::new() };
        let bytes = items.into_vec().into_iter().map(|item| match item { SmeltUnknown::Number(value) => value as i64 as u8, _ => 0 }).collect::<Vec<u8>>();
        let kind = map.iter().find_map(|(key, _)| SmeltTypedArrayKind::from_marker(&key.to_string_lossy())).unwrap_or(SmeltTypedArrayKind::Uint8);
        let width = kind.byte_width();
        let length = bytes.len() / width;
        //~ The record's own marker says whether the storage it came from
        //~ was shared, so a `SharedArrayBuffer`-backed view keeps
        //~ reporting a `SharedArrayBuffer` after a boundary round-trip.
        let buffer_shared = map.contains_key("__smelt_sharedarraybuffer");
        Self { id: map.id, kind, bytes: ::std::rc::Rc::new(::std::cell::RefCell::new(bytes)), buffer_id: smelt_next_object_id(), byte_offset: 0, length, buffer_shared }
    }
}

// @smelt:item-end

// Emit `SmeltTypedArray`: an element view over shared storage.
//
// The byte view keeps its own name, the spelling `TextEncoder.encode`,
// `TextDecoder.decode` and `crypto.getRandomValues` answer.
// @smelt:item SmeltUint8Array when byte_array
/// The `Uint8` face of the family: Smelt's concrete byte view.
pub type SmeltUint8Array = SmeltTypedArray;

// @smelt:item-end

// Emit `SmeltDataView`: a window over byte storage with per-CALL element
// widths.
//
// The difference from `SmeltTypedArray` is the whole reason this is a
// second type rather than a twelfth kind: a typed array's element type is a
// property of the VALUE, so it lives in the struct, while a `DataView`'s is an
// argument of every accessor — the same view answers `getInt16(0)` and
// `getFloat64(0)`. Byte ORDER is a parameter for the same reason, and its
// default is the opposite of a typed array's: `DataView` reads BIG-endian
// unless the call says otherwise, where every typed array is little-endian.
//
// The decode and encode themselves are the kind table's, reached by reversing
// the window for a big-endian call, so the widths and signednesses have one
// definition rather than two that can drift.
// @smelt:item SmeltDataView when byte_array & data_view
/// A `DataView`: a window over byte storage, read at a width per call.
#[derive(Clone)]
pub struct SmeltDataView {
    id: usize,
    //~ The storage, SHARED with every other view over it.
    bytes: ::std::rc::Rc<::std::cell::RefCell<Vec<u8>>>,
    //~ Identity of the buffer this view reports.
    buffer_id: usize,
    //~ `byteOffset`: where this view starts in the storage.
    byte_offset: usize,
    //~ `byteLength`: how many bytes this view spans.
    byte_length: usize,
    //~ Whether the storage behind this view is a `SharedArrayBuffer`.
    buffer_shared: bool,
}

//~ Structural equality over the WINDOW's bytes: two views of the same range
//~ compare equal, which is the answer `toEqual` gives for two `DataView`s.
impl PartialEq for SmeltDataView { fn eq(&self, other: &Self) -> bool { self.to_bytes() == other.to_bytes() } }
impl ::std::fmt::Debug for SmeltDataView { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { write!(formatter, "DataView {{ byteLength: {} }}", self.byte_length) } }
impl Default for SmeltDataView { fn default() -> Self { Self::new() } }
#[allow(dead_code)]
impl SmeltDataView {
    //~ An empty view over its own empty storage.
    pub fn new() -> Self { Self { id: smelt_next_object_id(), bytes: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), buffer_id: smelt_next_object_id(), byte_offset: 0, byte_length: 0, buffer_shared: false } }
    //~ `new DataView(buffer, byteOffset?, byteLength?)`.
    pub fn over_buffer(buffer: &SmeltArrayBuffer, byte_offset: usize, byte_length: Option<usize>) -> Self {
        let storage = buffer.storage();
        let available = storage.borrow().len().saturating_sub(byte_offset);
        let byte_length = byte_length.map_or(available, |length| length.min(available));
        Self { id: smelt_next_object_id(), bytes: storage, buffer_id: buffer.id(), byte_offset, byte_length, buffer_shared: buffer.is_shared() }
    }
    //~ JS reference identity of this view.
    pub fn id(&self) -> usize { self.id }
    //~ The constructor name, which is also the `[object X]` tag.
    pub fn class_name(&self) -> &'static str { "DataView" }
    //~ `byteOffset`: where this view starts in its buffer.
    pub fn byte_offset(&self) -> f64 { self.byte_offset as f64 }
    //~ `byteLength`: the window size in bytes.
    pub fn byte_length(&self) -> f64 { self.byte_length as f64 }
    //~ `buffer`: the storage this view reads, at its own identity.
    pub fn buffer(&self) -> SmeltArrayBuffer { SmeltArrayBuffer::from_shared_storage(self.buffer_id, ::std::rc::Rc::clone(&self.bytes), self.buffer_shared) }
    //~ A COPY of the bytes in this view's window.
    pub fn to_bytes(&self) -> Vec<u8> {
        let bytes = self.bytes.borrow();
        let from = self.byte_offset.min(bytes.len());
        let to = (self.byte_offset + self.byte_length).min(bytes.len());
        bytes[from..to].to_vec()
    }
    //~ `little_endian` is the accessor's third argument, defaulting FALSE —
    //~ which is why the window is reversed for the default: the kind table
    //~ decodes little-endian, so a big-endian read is the same decode over
    //~ reversed bytes. Reversing here rather than duplicating eleven arms is
    //~ what keeps one definition of each width and signedness.
    //~ The CHECKED pair is what the fallible accessor adapters call: an
    //~ offset outside the view is a `RangeError` in JavaScript, and `None` /
    //~ `false` is how that reaches the adapter, which owns the throw (see
    //~ the data-view access support). The bound is one expression
    //~ stated once, so the checked and unchecked halves cannot disagree
    //~ about where the window ends.
    //~ Whether `kind` at `offset` lies wholly inside this view.
    fn in_bounds(&self, kind: SmeltTypedArrayKind, offset: usize) -> bool {
        let width = kind.byte_width();
        offset + width <= self.byte_length && self.byte_offset + offset + width <= self.bytes.borrow().len()
    }
    //~ `getX`, answering `None` when the offset is out of range.
    pub fn get_checked(&self, kind: SmeltTypedArrayKind, offset: usize, little_endian: bool) -> Option<f64> { if self.in_bounds(kind, offset) { Some(self.get(kind, offset, little_endian)) } else { None } }
    //~ `setX`, answering `false` when the offset is out of range.
    pub fn set_checked(&self, kind: SmeltTypedArrayKind, offset: usize, value: f64, little_endian: bool) -> bool { if self.in_bounds(kind, offset) { self.set(kind, offset, value, little_endian); true } else { false } }
    //~ `getX(byteOffset, littleEndian?)`: read one element at `kind`'s width.
    pub fn get(&self, kind: SmeltTypedArrayKind, offset: usize, little_endian: bool) -> f64 {
        let width = kind.byte_width();
        let bytes = self.bytes.borrow();
        let at = self.byte_offset + offset;
        //~ Out of range answers zero HERE, and no caller reaches this
        //~ without the check: a source accessor lowers to the fallible
        //~ adapter, which asks `get_checked` and throws the spec's
        //~ `RangeError`. The guard stays because this half is also the
        //~ one the checked half calls, and a bounds test in two places
        //~ is cheaper than a panic in one.
        if offset + width > self.byte_length || at + width > bytes.len() { return 0.0; }
        let mut window = bytes[at..at + width].to_vec();
        if !little_endian { window.reverse(); }
        kind.decode(&window, 0)
    }
    //~ `setX(byteOffset, value, littleEndian?)`: write one element.
    pub fn set(&self, kind: SmeltTypedArrayKind, offset: usize, value: f64, little_endian: bool) {
        let width = kind.byte_width();
        let at = self.byte_offset + offset;
        let mut encoded = vec![0_u8; width];
        kind.encode(value, &mut encoded, 0);
        if !little_endian { encoded.reverse(); }
        let mut bytes = self.bytes.borrow_mut();
        if offset + width > self.byte_length || at + width > bytes.len() { return; }
        bytes[at..at + width].copy_from_slice(&encoded);
    }
}

// @smelt:item-end

// Emit the `DataView`'s dynamic-boundary adapters.
//
// The erased form is the byte-backed host record every member of the family
// crosses through, so a `DataView` that reached `SmeltUnknown` answers
// `ArrayBuffer.isView` true, reports `[object DataView]`, and — unlike a typed
// array — exposes no index keys, which is what the shared record builder
// already produces for a view carrying its own `buffer` and `byteOffset`.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltDataView when byte_array & data_view & unknown
/// Erase a `DataView` for a dynamic boundary.
impl IntoSmeltUnknown for SmeltDataView {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        let smelt_host_origin = self.clone();
        let storage = self.buffer().into_smelt_unknown();
        let elements: Vec<SmeltUnknown> = self.to_bytes().into_iter().map(|byte| SmeltUnknown::Number(f64::from(byte))).collect();
        smelt_with_host_origin(smelt_host_buffer_view_record_with_id(self.id, "__smelt_dataview", elements, Some(storage), self.byte_offset), smelt_host_origin)
    }
}

/// Rebuild a `DataView` from an erased value.
impl SmeltFromUnknown for SmeltDataView {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }
        let SmeltUnknown::Object(map) = value else { return Self::new() };
        let Some(SmeltUnknown::Array(items)) = map.get("bytes") else { return Self::new() };
        let bytes = items.into_vec().into_iter().map(|item| match item { SmeltUnknown::Number(value) => value as i64 as u8, _ => 0 }).collect::<Vec<u8>>();
        //~ The record's bytes are the WINDOW, so the rebuilt view owns
        //~ them from offset zero: the storage it came from is gone once
        //~ the origin is, and a window is the most the record carries.
        let byte_length = bytes.len();
        Self { id: map.id, bytes: ::std::rc::Rc::new(::std::cell::RefCell::new(bytes)), buffer_id: smelt_next_object_id(), byte_offset: 0, byte_length, buffer_shared: false }
    }
}

// @smelt:item-end

// Emit the BIGINT element access of `BigInt64Array`, `BigUint64Array` and
// `DataView.getBigInt64`/`getBigUint64` (and their setters).
//
// The `f64` element path every other kind uses cannot carry these: a 64-bit
// element above 2^53 has no exact `f64`. A bigint element is therefore read
// straight from its eight bytes into a `num_bigint::BigInt` at the kind's
// signedness, and written as the low 64 bits of the value's two's complement
// -- `BigInt.asIntN(64, v)` for `BigInt64`, `BigInt.asUintN(64, v)` for
// `BigUint64`, which are the same eight bytes. The storage stays the shared
// `Vec<u8>`; only the boundary converts.
// @smelt:item smelt_bigint_low_u64 when bigint & byte_array
/// The low 64 bits of a bigint's two's complement (`BigInt.asUintN(64, value)`).
fn smelt_bigint_low_u64(value: &::num_bigint::BigInt) -> u64 { let (sign, digits) = value.to_u64_digits(); let low = digits.first().copied().unwrap_or(0); if sign == ::num_bigint::Sign::Minus { low.wrapping_neg() } else { low } }
/// A bigint element from eight little-endian bytes, at `kind`'s signedness.
fn smelt_bigint_from_element(kind: SmeltTypedArrayKind, raw: [u8; 8]) -> ::num_bigint::BigInt { match kind { SmeltTypedArrayKind::BigUint64 => ::num_bigint::BigInt::from(u64::from_le_bytes(raw)), _ => ::num_bigint::BigInt::from(i64::from_le_bytes(raw)) } }
#[allow(dead_code)]
impl SmeltTypedArray {
    //~ A bigint view of `kind` over the given elements, in fresh storage.
    pub fn with_bigint_elements(kind: SmeltTypedArrayKind, elements: &[::num_bigint::BigInt]) -> Self { Self::with_bytes(kind, elements.iter().flat_map(|element| smelt_bigint_low_u64(element).to_le_bytes()).collect()) }
    //~ An indexed bigint element read; `None` past the end.
    pub fn get_bigint(&self, index: f64) -> Option<::num_bigint::BigInt> { if index < 0.0 || index.fract() != 0.0 || index as usize >= self.length { return None; } let at = self.byte_offset + index as usize * 8; let bytes = self.bytes.borrow(); let raw: [u8; 8] = bytes.get(at..at + 8)?.try_into().ok()?; Some(smelt_bigint_from_element(self.kind, raw)) }
    //~ An indexed bigint element write of the value's low 64 bits; dropped past the end.
    pub fn set_bigint(&self, index: f64, value: &::num_bigint::BigInt) { if index < 0.0 || index.fract() != 0.0 || index as usize >= self.length { return; } let at = self.byte_offset + index as usize * 8; if let Some(slot) = self.bytes.borrow_mut().get_mut(at..at + 8) { slot.copy_from_slice(&smelt_bigint_low_u64(value).to_le_bytes()); } }
}

// @smelt:item-end

// @smelt:item impl_SmeltDataView when bigint & byte_array & data_view
#[allow(dead_code)]
impl SmeltDataView {
    //~ `getBigInt64`/`getBigUint64`, answering `None` when the offset is out of range.
    pub fn get_bigint_checked(&self, kind: SmeltTypedArrayKind, offset: usize, little_endian: bool) -> Option<::num_bigint::BigInt> { if !self.in_bounds(kind, offset) { return None; } let at = self.byte_offset + offset; let bytes = self.bytes.borrow(); let mut raw: [u8; 8] = bytes.get(at..at + 8)?.try_into().ok()?; if !little_endian { raw.reverse(); } Some(smelt_bigint_from_element(kind, raw)) }
    //~ `setBigInt64`/`setBigUint64`, answering `false` when the offset is out of range.
    pub fn set_bigint_checked(&self, kind: SmeltTypedArrayKind, offset: usize, value: &::num_bigint::BigInt, little_endian: bool) -> bool { if !self.in_bounds(kind, offset) { return false; } let mut raw = smelt_bigint_low_u64(value).to_le_bytes(); if !little_endian { raw.reverse(); } let at = self.byte_offset + offset; self.bytes.borrow_mut()[at..at + 8].copy_from_slice(&raw); true }
}

// @smelt:item-end

// Emit the bridge from an erased record's index write to its live value.
//
// The boundary is two-way for READS by construction — the record carries the
// bytes — but a WRITE through an erased alias (`(view as any)[0] = 9`) lands
// in the record, and the concrete value the record was made from is still the
// thing every other reference reads. Without this bridge such a write is
// silently lost the moment one of the two aliases is concrete, which is a
// class of divergence that only appears in a mixed program and never in a
// wholly erased or wholly concrete one.
//
// The bridge is called with the view's record and with its storage record,
// because a view's window and its buffer are two live values with two
// identities and an element write is a write to both. Each record carries
// its own live value in its origin slot (`smelt_with_host_origin`).
// @smelt:item smelt_typed_array_write_origin when byte_array & unknown
/// Write an erased record's encoded bytes through to its live value.
#[allow(dead_code)]
fn smelt_typed_array_write_origin(record: &SmeltObject, offset: usize, encoded: &[SmeltUnknown]) {
    let bytes: Vec<u8> = encoded.iter().map(|byte| match byte { SmeltUnknown::Number(value) => *value as i64 as u8, _ => 0 }).collect();
    if let Some(view) = record.smelt_origin::<SmeltTypedArray>() { view.write_bytes_at(offset, &bytes); return; }
    if let Some(storage) = record.smelt_origin::<SmeltArrayBuffer>() { storage.write_bytes_at(offset, &bytes); }
}

// @smelt:item-end
