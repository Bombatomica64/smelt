//! Runtime prelude for the WHATWG text codecs and the concrete byte view.
//!
//! Three generated types, each a real Rust value with typed methods rather than
//! a marker-bearing `SmeltUnknown` record:
//!
//! * `SmeltUint8Array` — an alias of `SmeltTypedArray` (kind, byte offset, shared
//!   buffer). This is what `TextEncoder.encode` answers, so the bytes a program
//!   encodes keep a concrete byte-element type all the way to runtime, instead of
//!   a tagged record whose elements have to be re-narrowed one at a time.
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

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]


