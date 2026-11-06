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

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

