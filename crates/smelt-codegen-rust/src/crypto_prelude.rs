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

use crate::rust::CodeWriter;

/// How the digest helper reports an unrecognized algorithm name.
///
/// The spec's `NotSupportedError` is branded so a source `catch` can read
/// `error.name`, which needs the erased carrier to brand it ON. A crate without
/// one has no erased value for a `catch` to inspect anyway, so the message is
/// all there is to carry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DigestThrow {
    /// Brand the error record, so `error.name` reads `"NotSupportedError"`.
    Branded,
    /// Carry the message alone.
    Message,
}

/// Which `WebCrypto` helpers a generated crate needs emitted.
///
/// Each field names the helper it causes rather than the source feature that
/// implied it, so the AND-ing of "the program calls this" with "the crate has
/// that carrier" happens once, at the call site where both are known.
#[derive(Clone, Copy, Debug)]
pub struct CryptoDemand {
    /// Emit the fill helper over the concrete `SmeltUint8Array`.
    pub fill_concrete_view: bool,
    /// Emit the fill helper over the erased byte-backed host record.
    pub fill_erased_view: bool,
    /// Emit the digest helper, reporting a bad name this way.
    pub digest: Option<DigestThrow>,
    /// Emit `SmeltCryptoKey` and the `importKey`/`sign`/`verify` helpers.
    /// Requires the erased carrier (the key's boundary adapters and the
    /// branded throws live beside it).
    pub key: bool,
}

/// Emit the `WebCrypto` helpers a generated crate uses.
///
/// Every helper is gated, so a program that only calls `randomUUID` emits none
/// of them and carries neither hash crate nor `getrandom`.
pub fn emit(writer: &mut CodeWriter, demand: CryptoDemand) {
    if demand.fill_concrete_view {
        emit_random_values(writer);
    }
    if demand.fill_erased_view {
        emit_random_values_erased(writer);
    }
    if let Some(throw) = demand.digest {
        emit_digest(writer, throw);
    }
    if demand.key {
        emit_crypto_key(writer);
    }
}

/// The SHA hashers an HMAC key can name, as `(spec spelling, sha crate path)`.
const HMAC_HASHES: [(&str, &str); 4] = [
    ("SHA-1", "sha1::Sha1"),
    ("SHA-256", "sha2::Sha256"),
    ("SHA-384", "sha2::Sha384"),
    ("SHA-512", "sha2::Sha512"),
];

/// Emit `SmeltCryptoKey` and `crypto.subtle.importKey`/`sign`/`verify`.
///
/// A `CryptoKey` is a value with state (its algorithm, hash, key bytes and
/// the extractable/usages fixed at import), so unlike the keyless members it
/// is a real type. The key bytes are `Rc`-shared: a key is immutable, and a
/// clone of it is the same key.
///
/// Only `raw` HMAC keys are modeled. Every other format, algorithm or hash
/// answers the spec's `NotSupportedError`, branded so a source `catch` reads
/// `error.name`; `sign`/`verify` with an algorithm the key was not imported
/// for answer `InvalidAccessError`. `verify` compares in constant time, as the
/// spec requires of a MAC check.
fn emit_crypto_key(writer: &mut CodeWriter) {
    writer.line("/// A WebCrypto `CryptoKey`: an imported `raw` HMAC key.");
    writer.line("#[derive(Clone, Debug, Default)]");
    writer.line("#[allow(dead_code)]");
    writer.block("pub struct SmeltCryptoKey", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("algorithm: String,");
        struct_writer.line("hash: String,");
        struct_writer.line("key: ::std::rc::Rc<Vec<u8>>,");
        struct_writer.line("extractable: bool,");
        struct_writer.line("usages: Vec<String>,");
    });
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltCryptoKey", |impl_writer| {
        impl_writer.line("/// JS reference identity of this key.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// `key.type`: a symmetric key is `\"secret\"`.");
        impl_writer.line("pub fn key_type(&self) -> String { \"secret\".to_owned() }");
        impl_writer.line("/// `key.extractable`.");
        impl_writer.line("pub fn extractable(&self) -> bool { self.extractable }");
        impl_writer.line("/// `key.usages`.");
        impl_writer.line("pub fn usages(&self) -> SmeltList<String> { SmeltList::new(self.usages.clone()) }");
    });
    writer.blank_line();
    writer.line("/// Erase a key for a dynamic boundary: the spec's visible members, with the");
    writer.line("/// live key riding in the origin slot so narrowing it back recovers it.");
    writer.block("impl IntoSmeltUnknown for SmeltCryptoKey", |impl_writer| {
        impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
            fn_writer.line("let smelt_host_origin = self.clone();");
            fn_writer.line("let algorithm = SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"name\".to_owned(), SmeltUnknown::String(self.algorithm.clone().into())), (\"hash\".to_owned(), SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"name\".to_owned(), SmeltUnknown::String(self.hash.clone().into()))]))))])));");
            fn_writer.line("let usages = SmeltUnknown::Array(self.usages.iter().map(|usage| SmeltUnknown::String(usage.clone().into())).collect::<Vec<_>>().into());");
            fn_writer.line("smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([(\"__smelt_crypto_key\".to_owned(), SmeltUnknown::Bool(true)), (\"type\".to_owned(), SmeltUnknown::String(\"secret\".into())), (\"extractable\".to_owned(), SmeltUnknown::Bool(self.extractable)), (\"algorithm\".to_owned(), algorithm), (\"usages\".to_owned(), usages)]))), smelt_host_origin)");
        });
    });
    writer.blank_line();
    writer.line("/// Recover a key from an erased value: only a key that was erased in this");
    writer.line("/// thread carries its material, so anything else is a broken boundary.");
    writer.block("impl SmeltFromUnknown for SmeltCryptoKey", |impl_writer| {
        impl_writer.block("fn smelt_from_unknown(value: SmeltUnknown) -> Self", |fn_writer| {
            fn_writer.line("smelt_restore_host_origin::<Self>(&value).unwrap_or_else(|| panic!(\"erased value is not a CryptoKey\"))");
        });
    });
    writer.blank_line();
    let not_supported = crate::thrown::throw_expr(&crate::thrown::error_payload_record_expr(
        "NotSupportedError",
        "\"Algorithm: Unrecognized name\"",
    ));
    let invalid_access = crate::thrown::throw_expr(&crate::thrown::error_payload_record_expr(
        "InvalidAccessError",
        "\"The requested operation is not valid for the provided key\"",
    ));
    writer.line("/// The canonical spelling of a supported HMAC hash name, matched");
    writer.line("/// case-insensitively as the spec's algorithm normalization does.");
    writer.line("#[allow(dead_code)]");
    writer.block("fn smelt_crypto_hmac_hash(hash: &str) -> Option<&'static str>", |fn_writer| {
        fn_writer.line("let hash = hash.trim().to_ascii_uppercase();");
        for (spelling, _) in HMAC_HASHES {
            fn_writer.line(format!("if hash == \"{spelling}\" {{ return Some(\"{spelling}\"); }}"));
        }
        fn_writer.line("None");
    });
    writer.blank_line();
    writer.line("/// The HMAC tag of `data` under `key` with the named hash.");
    writer.line("#[allow(dead_code)]");
    writer.block("fn smelt_crypto_hmac_tag(hash: &str, key: &[u8], data: &[u8]) -> Vec<u8>", |fn_writer| {
        fn_writer.line("use hmac::Mac as _;");
        for (spelling, hasher) in HMAC_HASHES {
            fn_writer.line(format!(
                "if hash == \"{spelling}\" {{ let mut mac = hmac::Hmac::<{hasher}>::new_from_slice(key).expect(\"HMAC accepts a key of any length\"); mac.update(data); return mac.finalize().into_bytes().to_vec(); }}"
            ));
        }
        fn_writer.line("Vec::new()");
    });
    writer.blank_line();
    writer.line("/// `crypto.subtle.importKey(format, keyData, algorithm, extractable, usages)`.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_crypto_import_key(format: &str, key: Vec<u8>, algorithm: &str, hash: &str, extractable: bool, usages: Vec<String>) -> Result<SmeltCryptoKey, Box<dyn ::std::error::Error>>",
        |fn_writer| {
            fn_writer.line(format!(
                "if format != \"raw\" || !algorithm.trim().eq_ignore_ascii_case(\"HMAC\") {{ return Err({not_supported}); }}"
            ));
            fn_writer.line(format!(
                "let Some(hash) = smelt_crypto_hmac_hash(hash) else {{ return Err({not_supported}); }};"
            ));
            fn_writer.line("Ok(SmeltCryptoKey { id: smelt_next_object_id(), algorithm: \"HMAC\".to_owned(), hash: hash.to_owned(), key: ::std::rc::Rc::new(key), extractable, usages })");
        },
    );
    writer.blank_line();
    writer.line("/// `crypto.subtle.sign(algorithm, key, data)`: the MAC as an `ArrayBuffer`.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_crypto_sign(algorithm: &str, key: &SmeltCryptoKey, data: &[u8]) -> Result<SmeltArrayBuffer, Box<dyn ::std::error::Error>>",
        |fn_writer| {
            fn_writer.line(format!(
                "if !algorithm.trim().eq_ignore_ascii_case(&key.algorithm) {{ return Err({invalid_access}); }}"
            ));
            fn_writer.line("Ok(SmeltArrayBuffer::from_bytes(smelt_crypto_hmac_tag(&key.hash, &key.key, data)))");
        },
    );
    writer.blank_line();
    writer.line("/// `crypto.subtle.verify(algorithm, key, signature, data)`, comparing the");
    writer.line("/// tags in constant time.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_crypto_verify(algorithm: &str, key: &SmeltCryptoKey, signature: &[u8], data: &[u8]) -> Result<bool, Box<dyn ::std::error::Error>>",
        |fn_writer| {
            fn_writer.line(format!(
                "if !algorithm.trim().eq_ignore_ascii_case(&key.algorithm) {{ return Err({invalid_access}); }}"
            ));
            fn_writer.line("let expected = smelt_crypto_hmac_tag(&key.hash, &key.key, data);");
            fn_writer.line("Ok(expected.len() == signature.len() && expected.iter().zip(signature).fold(0_u8, |acc, (left, right)| acc | (left ^ right)) == 0)");
        },
    );
    writer.blank_line();
}

/// Emit `crypto.getRandomValues(view)`.
///
/// The spec's return value is the ARGUMENT, not a copy: `crypto
/// .getRandomValues(buffer) === buffer` is `true`, and a program that fills a
/// view it already holds a reference to expects to see the bytes through that
/// reference. `SmeltUint8Array` is an `Rc<RefCell<Vec<u8>>>` behind a JS
/// reference id, so cloning the struct after the fill answers a value that
/// shares both the bytes and the identity — which is what makes the source's
/// `===` and the aliased read both come out right.
///
/// `getrandom::fill` fails only when the platform has no random source at all.
/// That is not a condition the spec gives a JS-visible error for (the DOM
/// `QuotaExceededError` is about a view longer than 65536 bytes, which is a
/// different check), so it is an `expect`: a host with no entropy is not a
/// program error to be caught, and inventing a catchable throw for it would let
/// generated code "handle" a state it cannot recover from.
fn emit_random_values(writer: &mut CodeWriter) {
    writer.line("/// `crypto.getRandomValues(view)`: fill in place, answer the same view.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_crypto_random_values(view: &SmeltTypedArray) -> SmeltTypedArray",
        |fn_writer| {
            // The view's OWN byte window, not the whole storage: a view over
            // part of an `ArrayBuffer` must not scribble on its siblings.
            fn_writer.line("let mut bytes = vec![0_u8; view.byte_length() as usize];");
            fn_writer.line(
                "getrandom::fill(&mut bytes).expect(\"the platform random source is unavailable\");",
            );
            fn_writer.line("view.write_bytes(&bytes);");
            fn_writer.line("view.clone()");
        },
    );
    writer.blank_line();
}

/// Emit `crypto.getRandomValues(view)` for a view that is still a host record.
///
/// **The genuine dynamic boundary.** The eleven typed-array VIEWS are byte-backed
/// host records by an explicit design decision, not by omission: a view's
/// identity carries an element type, a byte offset and a shared `ArrayBuffer`
/// that reflective construction reads back, so `new Uint8Array(16)` has the
/// erased type and only `TextEncoder.encode`-shaped values have the concrete
/// one. See `StdlibClass::TypedArray` in `smelt_stdlib::classes` for that
/// decision and the recorded demand to change it.
///
/// `getRandomValues`'s argument is exactly that value in real code — a program
/// allocates the view it wants filled — so a lowering that refused the erased
/// record would refuse the whole API. The concrete arm above is taken whenever
/// the argument's type IS concrete; this arm exists for the source shape the
/// view family has not caught up with yet, and it disappears with that family.
///
/// What it must not become is what was here before: the previous lowering
/// answered its own ARGUMENT unchanged, so `crypto.getRandomValues(output)`
/// type-checked, reported no blocker, and left `output` full of zeros. A
/// generated program that silently returns zeros where it asked for randomness
/// is the worst kind of false green, and it is why filling the record is worth a
/// boundary rather than a refusal.
///
/// The fill goes through the byte-buffer record's own storage helpers so a view
/// over an `ArrayBuffer` writes THROUGH to the buffer, exactly as an indexed
/// element write does; `SmeltObject` is `Rc`-shared, so every alias of the view
/// observes the fill and the returned value keeps its reference identity.
fn emit_random_values_erased(writer: &mut CodeWriter) {
    writer
        .line("/// `crypto.getRandomValues(view)` over a byte-backed host record: fill in place.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_crypto_random_values_erased(view: SmeltUnknown) -> SmeltUnknown",
        |fn_writer| {
            fn_writer.line("let SmeltUnknown::Object(map) = &view else { return view };");
            fn_writer.line(format!(
                "let Some(SmeltUnknown::Array(values)) = map.get(\"{key}\") else {{ return view }};",
                key = smelt_stdlib::runtime_symbols::byte_buffer::BYTES_KEY,
            ));
            fn_writer.line("let count = values.into_vec().len();");
            fn_writer.line("let mut buffer = vec![0u8; count];");
            fn_writer.line(
                "getrandom::fill(&mut buffer).expect(\"the platform random source is unavailable\");",
            );
            fn_writer.line(
                "let encoded: Vec<SmeltUnknown> = buffer.iter().map(|byte| SmeltUnknown::Number(f64::from(*byte))).collect();",
            );
            fn_writer.line(format!(
                "map.insert(\"{key}\".to_owned(), SmeltUnknown::Array(SmeltArray::new(encoded.clone())));",
                key = smelt_stdlib::runtime_symbols::byte_buffer::BYTES_KEY,
            ));
            fn_writer.line("smelt_host_buffer_write_through(map, 0, &encoded);");
            fn_writer.line("view");
        },
    );
    writer.blank_line();
}

/// Emit `crypto.subtle.digest(algorithm, data)`.
///
/// The algorithm is a string at RUN time, so the match is in the generated code
/// rather than in the compiler: one `digest` call site can name any width, and
/// which one it named is not knowable from the source in general.
///
/// Three spec details are in the generated body rather than at the call site,
/// because they are properties of the operation and not of any one caller:
///
/// * the name is matched **case-insensitively** (`"sha-256"` is `"SHA-256"`),
///   which is the spec's algorithm-normalization step;
/// * only the four hyphenated spellings are recognized. Node rejects `"SHA256"`
///   as readily as `"MD5"`, so the match is exact-after-lowercasing rather than
///   a hyphen-insensitive fold;
/// * an unrecognized name is the spec's `NotSupportedError`, message included,
///   branded so a source `catch` reads `error.name === "NotSupportedError"`.
///
/// The result is the spec's `ArrayBuffer` — byte STORAGE, not a view. It used
/// to be a byte view, because a view was Smelt's only concrete byte value and
/// the view-vs-storage distinction was unobservable; with the typed-array
/// family concrete it IS observable (`ArrayBuffer.isView(digest)` is `false`,
/// and `new Uint8Array(digest)` re-views the same storage rather than copying
/// elements), so the honest type is the one the spec names.
fn emit_digest(writer: &mut CodeWriter, throw: DigestThrow) {
    writer.line("/// `crypto.subtle.digest(algorithm, data)`: hash bytes by algorithm name.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_crypto_digest(algorithm: &str, data: &[u8]) -> Result<SmeltArrayBuffer, Box<dyn ::std::error::Error>>",
        |fn_writer| {
            // One `Digest` import covers both crates: `sha1` and `sha2` are two
            // faces of the same `digest` traits, so `sha1::Digest` IS
            // `sha2::Digest` and importing it twice would not compile.
            fn_writer.line("use sha1::Digest as _;");
            fn_writer.line("let name = algorithm.trim().to_ascii_lowercase();");
            for (spelling, hasher) in [
                ("sha-1", "sha1::Sha1"),
                ("sha-256", "sha2::Sha256"),
                ("sha-384", "sha2::Sha384"),
                ("sha-512", "sha2::Sha512"),
            ] {
                fn_writer.line(format!(
                    "if name == \"{spelling}\" {{ let mut hasher = {hasher}::new(); hasher.update(data); return Ok(SmeltArrayBuffer::from_bytes(hasher.finalize().to_vec())); }}"
                ));
            }
            fn_writer.line(format!("Err({})", digest_unsupported_expr(throw)));
        },
    );
    writer.blank_line();
}

/// Return the thrown-value expression for an unrecognized digest algorithm.
///
/// Split out so [`emit_digest`] can stay one shape whether or not the crate
/// carries the erased carrier: with `SmeltUnknown` present the throw is the
/// branded error record a source `catch` can inspect, and without it the
/// message alone is all there is to carry (the same choice
/// `form_data_prelude`'s body reader makes).
fn digest_unsupported_expr(throw: DigestThrow) -> String {
    match throw {
        DigestThrow::Branded => crate::thrown::throw_expr(
            &crate::thrown::error_payload_record_expr(
                "NotSupportedError",
                "\"Unrecognized algorithm name\"",
            ),
        ),
        DigestThrow::Message => {
            "Box::<dyn ::std::error::Error>::from(\"NotSupportedError: Unrecognized algorithm name\")"
                .to_owned()
        }
    }
}
