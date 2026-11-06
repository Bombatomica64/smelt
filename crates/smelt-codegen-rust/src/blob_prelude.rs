//! Runtime prelude for the WHATWG `Blob`/`File` value.
//!
//! One generated type, `SmeltBlob`, backing both spellings.
//!
//! ## Why one type
//!
//! The spec's `File` is a `Blob` plus exactly two data properties (`name`,
//! `lastModified`) and no behaviour of its own. Rust has no inheritance, so the
//! choices were a `SmeltFile` that wraps a `SmeltBlob` — which makes every
//! `Blob`-typed slot need a widening conversion the type system would have to
//! carry — or one type whose file metadata is optional. The second is what the
//! ERASED record already did (`__smelt_file` stamped on top of `__smelt_blob`,
//! one shape), so it is also the choice that keeps the concrete value and its
//! erased form describing the same object.
//!
//! ## Immutability, and why there is no `RefCell`
//!
//! A `Blob`'s bytes and type are immutable in the spec: `slice` answers a NEW
//! blob and there is no mutator anywhere on the interface. So the bytes sit
//! behind a plain `Rc<Vec<u8>>` — shared, never borrowed mutably — rather than
//! the `Rc<RefCell<..>>` the mutable reference types (`SmeltHeaders`,
//! `SmeltList`) need. Two variables holding one blob still observe one object,
//! because there is nothing to observe changing. The `Rc` is there for cheap
//! cloning of what can be a large byte buffer, not for interior mutability.
//!
//! ## `size` is a BYTE count
//!
//! The old erased record stored its content as a `String` and reported
//! `content.len()`. That happened to be right for `size` (Rust's `String::len`
//! is a byte count) and wrong for everything else: a blob's content is
//! arbitrary bytes, not necessarily UTF-8, and `arrayBuffer()`/`bytes()` have
//! to answer those bytes exactly. This type stores `Vec<u8>` and decodes only
//! at `text()`, which is the direction the spec defines (`text()` is "UTF-8
//! decode", replacement characters and all).
//!
//! ## The erased form
//!
//! The record the runtime helper used to build:
//! `{ "__smelt_blob": true, "type": .., "size": .., "content": ..,
//! ("__smelt_file": true, "name": .., "lastModified": ..) }`. Keeping that
//! shape is what leaves `instanceof Blob` on an erased value, the
//! `clone`/`cloneDeepWith` paths that rebuild a blob from those fields, and
//! `Object.prototype.toString` all working exactly as before. `content` stays a
//! STRING because that is what text consumers read. It is a lossy decoding
//! when the bytes are not UTF-8, so only then the record also carries the exact
//! bytes in a hidden `__smelt_blob_bytes` list, and every reader of an erased
//! blob's bytes goes through `smelt_blob_record_bytes`. Without it a blob
//! erased and rebuilt (`new Blob([u])` over an `unknown`, or a mixed
//! `[blob, "x"]` parts array) turned a `0xFF` byte into the three bytes of
//! U+FFFD.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

