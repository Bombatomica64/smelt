//! Runtime prelude for the byte-backed host objects.
//!
//! JavaScript's binary-data host objects — `ArrayBuffer`, `SharedArrayBuffer`,
//! the eleven `TypedArray` views, Node's `Buffer`, and `DataView` — are modeled as
//! marker-bearing `SmeltUnknown::Object` records that carry their storage in a
//! `bytes` list (see `smelt_stdlib::host_object::ByteBufferRole`). Before this
//! module existed the records were identity-only: `slice(0)` handed the *same*
//! object back, `byteLength` and index reads answered `undefined`, and
//! `new Uint8Array(buf)` panicked. Every clone/equality path in a library that
//! touches binary data (es-toolkit's `clone`, `cloneDeepWith`, `isEqualWith`)
//! walks exactly those operations.
//!
//! ## Element typing
//!
//! A typed array is *bytes plus an element type*, and the element type is not
//! decoration: the same eight bytes are two `Float32Array` elements or one
//! `Float64Array` element, and the same byte `0xff` reads as `255` through a
//! `Uint8Array` and `-1` through an `Int8Array`. So each byte-backed marker that
//! names a view carries a `TypedArrayElement` in the shared registry, and the
//! helpers below derive one little-endian decode/encode pair per element type from
//! it. That single fact drives everything observable:
//!
//! * `length` is `byteLength / BYTES_PER_ELEMENT`, so a `Float64Array` over eight
//!   bytes has one element where a `Uint8Array` has eight;
//! * an indexed read decodes at the element's width and signedness, and an
//!   indexed write encodes back into exactly those bytes;
//! * `slice`/`subarray` bounds are *element* indices for a view and *byte*
//!   indices for byte-addressed storage, which is the same code once the stride
//!   comes from the registry;
//! * `new Float32Array(arrayBuffer)` re-*views* the bytes while
//!   `new Float32Array(uint8View)` *converts* the elements — the two JavaScript
//!   constructor behaviors that a shapeless byte copy cannot tell apart.
//!
//! The emitted helpers here are the *single* place that knows the record layout.
//! Their names live in `smelt_stdlib::runtime_symbols::byte_buffer` so the
//! definitions below and the emitter call sites cannot drift, and both the set of
//! byte-backed markers and their element types are read from the shared registry
//! rather than restated.
//!
//! Layout of a byte-backed host record:
//!
//! ```text
//! { "<marker>": true, "bytes": [b, b, ...], "byteLength": N,
//!   "length": N / width, "byteOffset": off?, "buffer": <storage record>? }
//! ```
//!
//! `byteOffset` and `buffer` are present for the element-typed views, which in
//! JavaScript are always a window onto an `ArrayBuffer`. `length` and `byteLength`
//! are recomputed whenever the helpers mint a new record, so a slice never reports
//! its source's size.
//!
//! ## Deliberate limit: sibling views do not share a byte window
//!
//! Two views over one `ArrayBuffer` share the buffer *record* (so
//! `view.buffer === buffer` holds and clone specs that assert it pass), but each
//! view keeps its own byte window, so a write through one view is not observed
//! through the other. Smelt models these records by value; making the window a
//! borrow of the storage record is a separate change to record identity, not to
//! element typing, and nothing in the measured corpora reads a byte written
//! through an aliasing view.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]


