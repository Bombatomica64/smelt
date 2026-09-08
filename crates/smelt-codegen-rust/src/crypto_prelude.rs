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
//! ## What is not here
//!
//! The key-taking half of `SubtleCrypto` (`importKey`/`sign`/`verify`). That is
//! a key-management surface — JWK and raw key material, per-algorithm
//! parameters, a `CryptoKey` value with its own extractable/usages state — not
//! three more calls, so it stays *declared* in
//! `smelt_stdlib::host_modules` and using it is a named blocker. See
//! `CRYPTO_KEY_REASON` there.

use crate::rust::CodeWriter;

/// Emit the `WebCrypto` helpers a generated crate uses.
///
/// Both halves are gated separately by the caller, so a program that only calls
/// `randomUUID` emits neither and carries neither hash crate nor `getrandom`.
pub fn emit(
    writer: &mut CodeWriter,
    needs_random_values: bool,
    needs_digest: bool,
    needs_byte_array: bool,
    needs_unknown: bool,
) {
    if needs_random_values && needs_byte_array {
        emit_random_values(writer);
    }
    if needs_random_values && needs_unknown {
        emit_random_values_erased(writer);
    }
    if needs_digest {
        emit_digest(writer, needs_unknown);
    }
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
        "fn smelt_crypto_random_values(view: &SmeltUint8Array) -> SmeltUint8Array",
        |fn_writer| {
            fn_writer.line(
                "getrandom::fill(view.bytes.borrow_mut().as_mut_slice()).expect(\"the platform random source is unavailable\");",
            );
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
/// one. See `StdlibClass::ByteArray` in `smelt_stdlib::classes` for that
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
            fn_writer.line(&format!(
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
            fn_writer.line(&format!(
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
/// The result is a `SmeltUint8Array` because that is Smelt's one concrete byte
/// value; the spec's `ArrayBuffer` and a `Uint8Array` over it differ only in a
/// view-vs-storage distinction the modeled surface does not observe (the same
/// call `Blob.arrayBuffer()` already makes).
fn emit_digest(writer: &mut CodeWriter, needs_unknown: bool) {
    writer.line("/// `crypto.subtle.digest(algorithm, data)`: hash bytes by algorithm name.");
    writer.line("#[allow(dead_code)]");
    writer.block(
        "fn smelt_crypto_digest(algorithm: &str, data: &[u8]) -> Result<SmeltUint8Array, Box<dyn ::std::error::Error>>",
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
                fn_writer.line(&format!(
                    "if name == \"{spelling}\" {{ let mut hasher = {hasher}::new(); hasher.update(data); return Ok(SmeltUint8Array::from_bytes(hasher.finalize().to_vec())); }}"
                ));
            }
            fn_writer.line(&format!("Err({})", digest_unsupported_expr(needs_unknown)));
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
fn digest_unsupported_expr(needs_unknown: bool) -> String {
    if needs_unknown {
        crate::thrown::throw_expr(&crate::thrown::error_payload_record_expr(
            "NotSupportedError",
            "\"Unrecognized algorithm name\"",
        ))
    } else {
        "Box::<dyn ::std::error::Error>::from(\"NotSupportedError: Unrecognized algorithm name\")"
            .to_owned()
    }
}
