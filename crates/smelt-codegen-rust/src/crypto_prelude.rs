//! Runtime prelude for the `WebCrypto` members Smelt models.
//!
//! Three keyless members of the `crypto` global, and each one is a thin call
//! into a crate a hand-writing Rust team would pick for exactly this job:
//!
//! * `randomUUID()` — `uuid`'s v4 generator. Emitted INLINE at its call site
//!   rather than through a helper here, because the whole lowering is one
//!   expression (`uuid::Uuid::new_v4().to_string()`) and the spec's lowercase
//!   hyphenated form is already `Uuid`'s `Display`.
//! * `getRandomValues(view)` — `getrandom`. Needs a helper: the spec fills the
//!   argument IN PLACE and answers that same object, so the generated code has
//!   to reach the view's shared byte storage and then hand the SAME reference
//!   identity back. See [`emit_random_values`].
//! * `subtle.digest(algorithm, data)` — `sha1` and `sha2`. Needs a helper: the
//!   algorithm is a run-time string, so one call site has to be able to answer
//!   any of the four widths. See [`emit_digest`].
//!
//! ## Why no `SmeltCrypto` type
//!
//! Every other surface in this directory emits a struct because the JS value
//! has state: a `Headers` list, a `Blob`'s bytes, a `Response`'s consumed-body
//! flag. `crypto` has none — it is a namespace whose members are pure functions
//! of their arguments — so there is no value to model and nothing for a
//! `SmeltCrypto` to hold. Emitting one would mean `crypto.randomUUID()` built
//! an object to immediately discard.
//!
//! * `subtle.importKey`/`sign`/`verify` — the key-taking half, for `raw` HMAC
//!   keys (what signed cookies and HMAC JWTs use), over `hmac`. A `CryptoKey`
//!   is the concrete `SmeltCryptoKey`. See [`emit_crypto_key`].
//!
//! ## What is not here
//!
//! The other key formats (`jwk`, `spki`, `pkcs8`) and the asymmetric and
//! cipher algorithms. They throw the spec's `NotSupportedError` at run time,
//! which a source `catch` can read, rather than computing a wrong answer.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

