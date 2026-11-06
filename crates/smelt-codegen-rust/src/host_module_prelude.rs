//! Runtime prelude for the modeled Node host-module functions.
//!
//! Two groups, each emitted only when a program calls into it:
//!
//! * **`node:path` (POSIX).** `join`, `resolve`, `normalize`, `dirname`,
//!   `basename`, `extname`, `relative` and `isAbsolute`, each a line-for-line
//!   port of Node's own `lib/path.js` POSIX implementation (its
//!   `normalizeString` in particular). `std::path` was considered and
//!   rejected: `Path::components` drops a trailing slash, treats `..` as an
//!   opaque component rather than collapsing it, and keeps `//` — every one of
//!   which Node specifies differently (`join('a/', '')` is `a/`,
//!   `normalize('a/../..')` is `..`). The functions work on UTF-8 BYTES: every
//!   decision Node makes is about the two ASCII code units `/` and `.`, so byte
//!   positions are code-unit positions for every character the algorithm looks
//!   at, and slicing at them never splits a multi-byte character.
//! * **`node:crypto` `createHash`.** The stateful, chainable `Hash`
//!   (`SmeltHash`): open (an algorithm and the bytes fed so far) or finalized,
//!   behind a shared cell so `h.update(a); h.update(b)` feeds one hasher and
//!   `update` can answer the SAME hash for chaining. The hashing itself is the
//!   RustCrypto crates a Rust team would pick (`md-5`, `sha1`, `sha2`, which
//!   share one `digest::Digest` trait).
//!
//! The hasher has four catchable failure modes: `Digest method not supported`,
//! `Digest already called`, an odd-length `hex` input, and a `digest` encoding
//! with no text form. The first three carry Node's own messages; the fourth is
//! a Smelt-written `TypeError`. All four are raised through the same error
//! channel a source `throw` uses.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]


/// Name of the generated `path.join` helper, `fn(&[String]) -> String`.
pub(crate) const PATH_JOIN_FN: &str = "smelt_path_join";
/// Name of the generated `path.resolve` helper, `fn(&[String]) -> String`.
pub(crate) const PATH_RESOLVE_FN: &str = "smelt_path_resolve";
/// Name of the generated `path.normalize` helper, `fn(&str) -> String`.
pub(crate) const PATH_NORMALIZE_FN: &str = "smelt_path_normalize";
/// Name of the generated `path.dirname` helper, `fn(&str) -> String`.
pub(crate) const PATH_DIRNAME_FN: &str = "smelt_path_dirname";
/// Name of the generated `path.basename` helper, `fn(&str, Option<&str>) -> String`.
pub(crate) const PATH_BASENAME_FN: &str = "smelt_path_basename";
/// Name of the generated `path.extname` helper, `fn(&str) -> String`.
pub(crate) const PATH_EXTNAME_FN: &str = "smelt_path_extname";
/// Name of the generated `path.relative` helper, `fn(&str, &str) -> String`.
pub(crate) const PATH_RELATIVE_FN: &str = "smelt_path_relative";
/// Name of the generated `path.isAbsolute` helper, `fn(&str) -> bool`.
pub(crate) const PATH_IS_ABSOLUTE_FN: &str = "smelt_path_is_absolute";

/// Name of the generated `createHash` helper.
pub(crate) const HASH_CREATE_FN: &str = "smelt_hash_create";
/// Name of the generated `hash.update(bytes)` helper.
pub(crate) const HASH_UPDATE_BYTES_FN: &str = "smelt_hash_update_bytes";
/// Name of the generated `hash.update(text, encoding)` helper.
pub(crate) const HASH_UPDATE_TEXT_FN: &str = "smelt_hash_update_text";
/// Name of the generated `hash.digest(encoding)` helper.
pub(crate) const HASH_DIGEST_TEXT_FN: &str = "smelt_hash_digest_text";
/// Name of the generated `hash.digest()` helper.
pub(crate) const HASH_DIGEST_BYTES_FN: &str = "smelt_hash_digest_bytes";
