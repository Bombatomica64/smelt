// Runtime prelude for the `WebCrypto` members Smelt models.
//
// Three keyless members of the `crypto` global, and each one is a thin call
// into a crate a hand-writing Rust team would pick for exactly this job:
//
// * `randomUUID()` — `uuid`'s v4 generator. Emitted INLINE at its call site
//   rather than through a helper here, because the whole lowering is one
//   expression (`uuid::Uuid::new_v4().to_string()`) and the spec's lowercase
//   hyphenated form is already `Uuid`'s `Display`.
// * `getRandomValues(view)` — `getrandom`. Needs a helper: the spec fills the
//   argument IN PLACE and answers that same object, so the generated code has
//   to reach the view's shared byte storage and then hand the SAME reference
//   identity back. See [`emit_random_values`].
// * `subtle.digest(algorithm, data)` — `sha1` and `sha2`. Needs a helper: the
//   algorithm is a run-time string, so one call site has to be able to answer
//   any of the four widths. See [`emit_digest`].
//
// ## Why no `SmeltCrypto` type
//
// Every other surface in this directory emits a struct because the JS value
// has state: a `Headers` list, a `Blob`'s bytes, a `Response`'s consumed-body
// flag. `crypto` has none — it is a namespace whose members are pure functions
// of their arguments — so there is no value to model and nothing for a
// `SmeltCrypto` to hold. Emitting one would mean `crypto.randomUUID()` built
// an object to immediately discard.
//
// * `subtle.importKey`/`sign`/`verify` — the key-taking half, for `raw` HMAC
//   keys (what signed cookies and HMAC JWTs use), over `hmac`. A `CryptoKey`
//   is the concrete `SmeltCryptoKey`. See [`emit_crypto_key`].
//
// ## What is not here
//
// The other key formats (`jwk`, `spki`, `pkcs8`) and the asymmetric and
// cipher algorithms. They throw the spec's `NotSupportedError` at run time,
// which a source `catch` can read, rather than computing a wrong answer.

// Emit `crypto.getRandomValues(view)`.
//
// The spec's return value is the ARGUMENT, not a copy: `crypto
// .getRandomValues(buffer) === buffer` is `true`, and a program that fills a
// view it already holds a reference to expects to see the bytes through that
// reference. `SmeltUint8Array` is an `Rc<RefCell<Vec<u8>>>` behind a JS
// reference id, so cloning the struct after the fill answers a value that
// shares both the bytes and the identity — which is what makes the source's
// `===` and the aliased read both come out right.
//
// `getrandom::fill` fails only when the platform has no random source at all.
// That is not a condition the spec gives a JS-visible error for (the DOM
// `QuotaExceededError` is about a view longer than 65536 bytes, which is a
// different check), so it is an `expect`: a host with no entropy is not a
// program error to be caught, and inventing a catchable throw for it would let
// generated code "handle" a state it cannot recover from.
// @smelt:item smelt_crypto_random_values when crypto_random_values
/// `crypto.getRandomValues(view)`: fill in place, answer the same view.
#[allow(dead_code)]
fn smelt_crypto_random_values(view: &SmeltTypedArray) -> SmeltTypedArray {
    //~ The view's OWN byte window, not the whole storage: a view over
    //~ part of an `ArrayBuffer` must not scribble on its siblings.
    let mut bytes = vec![0_u8; view.byte_length() as usize];
    getrandom::fill(&mut bytes).expect("the platform random source is unavailable");
    view.write_bytes(&bytes);
    view.clone()
}

// @smelt:item-end

// Emit `crypto.getRandomValues(view)` for a view that is still a host record.
//
// **The genuine dynamic boundary.** The eleven typed-array VIEWS are byte-backed
// host records by an explicit design decision, not by omission: a view's
// identity carries an element type, a byte offset and a shared `ArrayBuffer`
// that reflective construction reads back, so `new Uint8Array(16)` has the
// erased type and only `TextEncoder.encode`-shaped values have the concrete
// one. See `StdlibClass::TypedArray` in `smelt_stdlib::classes` for that
// decision and the recorded demand to change it.
//
// `getRandomValues`'s argument is exactly that value in real code — a program
// allocates the view it wants filled — so a lowering that refused the erased
// record would refuse the whole API. The concrete arm above is taken whenever
// the argument's type IS concrete; this arm exists for the source shape the
// view family has not caught up with yet, and it disappears with that family.
//
// What it must not become is what was here before: the previous lowering
// answered its own ARGUMENT unchanged, so `crypto.getRandomValues(output)`
// type-checked, reported no blocker, and left `output` full of zeros. A
// generated program that silently returns zeros where it asked for randomness
// is the worst kind of false green, and it is why filling the record is worth a
// boundary rather than a refusal.
//
// The fill goes through the byte-buffer record's own storage helpers so a view
// over an `ArrayBuffer` writes THROUGH to the buffer, exactly as an indexed
// element write does; `SmeltObject` is `Rc`-shared, so every alias of the view
// observes the fill and the returned value keeps its reference identity.
// @smelt:item smelt_crypto_random_values_erased when crypto_random_values & unknown
/// `crypto.getRandomValues(view)` over a byte-backed host record: fill in place.
#[allow(dead_code)]
fn smelt_crypto_random_values_erased(view: SmeltUnknown) -> SmeltUnknown {
    let SmeltUnknown::Object(map) = &view else { return view };
    let Some(SmeltUnknown::Array(values)) = map.get("bytes") else { return view };
    let count = values.into_vec().len();
    let mut buffer = vec![0u8; count];
    getrandom::fill(&mut buffer).expect("the platform random source is unavailable");
    let encoded: Vec<SmeltUnknown> = buffer.iter().map(|byte| SmeltUnknown::Number(f64::from(*byte))).collect();
    map.insert("bytes".to_owned(), SmeltUnknown::Array(SmeltArray::new(encoded.clone())));
    smelt_host_buffer_write_through(map, 0, &encoded);
    view
}

// @smelt:item-end

// Emit `crypto.subtle.digest(algorithm, data)`.
//
// The algorithm is a string at RUN time, so the match is in the generated code
// rather than in the compiler: one `digest` call site can name any width, and
// which one it named is not knowable from the source in general.
//
// Three spec details are in the generated body rather than at the call site,
// because they are properties of the operation and not of any one caller:
//
// * the name is matched **case-insensitively** (`"sha-256"` is `"SHA-256"`),
//   which is the spec's algorithm-normalization step;
// * only the four hyphenated spellings are recognized. Node rejects `"SHA256"`
//   as readily as `"MD5"`, so the match is exact-after-lowercasing rather than
//   a hyphen-insensitive fold;
// * an unrecognized name is the spec's `NotSupportedError`, message included,
//   branded so a source `catch` reads `error.name === "NotSupportedError"`.
//
// The result is the spec's `ArrayBuffer` — byte STORAGE, not a view. It used
// to be a byte view, because a view was Smelt's only concrete byte value and
// the view-vs-storage distinction was unobservable; with the typed-array
// family concrete it IS observable (`ArrayBuffer.isView(digest)` is `false`,
// and `new Uint8Array(digest)` re-views the same storage rather than copying
// elements), so the honest type is the one the spec names.
// @smelt:item smelt_crypto_digest when crypto_digest
/// `crypto.subtle.digest(algorithm, data)`: hash bytes by algorithm name.
#[allow(dead_code)]
fn smelt_crypto_digest(algorithm: &(impl SmeltHostText + ?Sized), data: &[u8]) -> Result<SmeltArrayBuffer, Box<dyn ::std::error::Error>> {
    //~ One `Digest` import covers both crates: `sha1` and `sha2` are two
    //~ faces of the same `digest` traits, so `sha1::Digest` IS
    //~ `sha2::Digest` and importing it twice would not compile.
    use sha1::Digest as _;
    let algorithm = algorithm.host_text(); let algorithm: &str = &algorithm;
    let name = algorithm.trim().to_ascii_lowercase();
    if name == "sha-1" { let mut hasher = sha1::Sha1::new(); hasher.update(data); return Ok(SmeltArrayBuffer::from_bytes(hasher.finalize().to_vec())); }
    if name == "sha-256" { let mut hasher = sha2::Sha256::new(); hasher.update(data); return Ok(SmeltArrayBuffer::from_bytes(hasher.finalize().to_vec())); }
    if name == "sha-384" { let mut hasher = sha2::Sha384::new(); hasher.update(data); return Ok(SmeltArrayBuffer::from_bytes(hasher.finalize().to_vec())); }
    if name == "sha-512" { let mut hasher = sha2::Sha512::new(); hasher.update(data); return Ok(SmeltArrayBuffer::from_bytes(hasher.finalize().to_vec())); }
// @smelt:if unknown
    Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("NotSupportedError".into())), ("message".to_owned(), SmeltUnknown::String("Unrecognized algorithm name".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)])))))
// @smelt:else
//=     Err(Box::<dyn ::std::error::Error>::from("NotSupportedError: Unrecognized algorithm name"))
// @smelt:end
}

// @smelt:item-end

// Emit `SmeltCryptoKey` and `crypto.subtle.importKey`/`sign`/`verify`.
//
// A `CryptoKey` is a value with state (its algorithm, hash, key bytes and
// the extractable/usages fixed at import), so unlike the keyless members it
// is a real type. The key bytes are `Rc`-shared: a key is immutable, and a
// clone of it is the same key.
//
// Only `raw` HMAC keys are modeled. Every other format, algorithm or hash
// answers the spec's `NotSupportedError`, branded so a source `catch` reads
// `error.name`; `sign`/`verify` with an algorithm the key was not imported
// for answer `InvalidAccessError`. `verify` compares in constant time, as the
// spec requires of a MAC check.
// @smelt:item SmeltCryptoKey when crypto_key
/// A WebCrypto `CryptoKey`: an imported `raw` HMAC key.
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct SmeltCryptoKey {
    id: usize,
    algorithm: String,
    hash: String,
    key: ::std::rc::Rc<Vec<u8>>,
    extractable: bool,
    usages: Vec<String>,
}
#[allow(dead_code)]
impl SmeltCryptoKey {
    /// JS reference identity of this key.
    pub fn id(&self) -> usize { self.id }
    /// `key.type`: a symmetric key is `"secret"`.
    pub fn key_type(&self) -> String { "secret".to_owned() }
    /// `key.extractable`.
    pub fn extractable(&self) -> bool { self.extractable }
    /// `key.usages`.
    pub fn usages(&self) -> SmeltList<String> { SmeltList::new(self.usages.clone()) }
}

/// Erase a key for a dynamic boundary: the spec's visible members, with the
/// live key riding in the origin slot so narrowing it back recovers it.
impl IntoSmeltUnknown for SmeltCryptoKey {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        let smelt_host_origin = self.clone();
        let algorithm = SmeltUnknown::Object(SmeltObject::new(Vec::from([("name".to_owned(), SmeltUnknown::String(self.algorithm.clone().into())), ("hash".to_owned(), SmeltUnknown::Object(SmeltObject::new(Vec::from([("name".to_owned(), SmeltUnknown::String(self.hash.clone().into()))]))))])));
        let usages = SmeltUnknown::Array(self.usages.iter().map(|usage| SmeltUnknown::String(usage.clone().into())).collect::<Vec<_>>().into());
        smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([("__smelt_crypto_key".to_owned(), SmeltUnknown::Bool(true)), ("type".to_owned(), SmeltUnknown::String("secret".into())), ("extractable".to_owned(), SmeltUnknown::Bool(self.extractable)), ("algorithm".to_owned(), algorithm), ("usages".to_owned(), usages)]))), smelt_host_origin)
    }
}

/// Recover a key from an erased value: only a key that was erased in this
/// thread carries its material, so anything else is a broken boundary.
impl SmeltFromUnknown for SmeltCryptoKey {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        smelt_restore_host_origin::<Self>(&value).unwrap_or_else(|| panic!("erased value is not a CryptoKey"))
    }
}

/// The canonical spelling of a supported HMAC hash name, matched
/// case-insensitively as the spec's algorithm normalization does.
#[allow(dead_code)]
fn smelt_crypto_hmac_hash(hash: &str) -> Option<&'static str> {
    let hash = hash.trim().to_ascii_uppercase();
    if hash == "SHA-1" { return Some("SHA-1"); }
    if hash == "SHA-256" { return Some("SHA-256"); }
    if hash == "SHA-384" { return Some("SHA-384"); }
    if hash == "SHA-512" { return Some("SHA-512"); }
    None
}

/// The HMAC tag of `data` under `key` with the named hash.
#[allow(dead_code)]
fn smelt_crypto_hmac_tag(hash: &str, key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::Mac as _;
    if hash == "SHA-1" { let mut mac = hmac::Hmac::<sha1::Sha1>::new_from_slice(key).expect("HMAC accepts a key of any length"); mac.update(data); return mac.finalize().into_bytes().to_vec(); }
    if hash == "SHA-256" { let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key).expect("HMAC accepts a key of any length"); mac.update(data); return mac.finalize().into_bytes().to_vec(); }
    if hash == "SHA-384" { let mut mac = hmac::Hmac::<sha2::Sha384>::new_from_slice(key).expect("HMAC accepts a key of any length"); mac.update(data); return mac.finalize().into_bytes().to_vec(); }
    if hash == "SHA-512" { let mut mac = hmac::Hmac::<sha2::Sha512>::new_from_slice(key).expect("HMAC accepts a key of any length"); mac.update(data); return mac.finalize().into_bytes().to_vec(); }
    Vec::new()
}

/// `crypto.subtle.importKey(format, keyData, algorithm, extractable, usages)`.
#[allow(dead_code)]
fn smelt_crypto_import_key(format: &(impl SmeltHostText + ?Sized), key: Vec<u8>, algorithm: &(impl SmeltHostText + ?Sized), hash: &(impl SmeltHostText + ?Sized), extractable: bool, usages: Vec<impl SmeltHostText>) -> Result<SmeltCryptoKey, Box<dyn ::std::error::Error>> { let format = format.host_text(); let format: &str = &format; let algorithm = algorithm.host_text(); let algorithm: &str = &algorithm; let hash = hash.host_text(); let hash: &str = &hash; let usages = usages.into_iter().map(SmeltHostText::into_host_string).collect::<Vec<String>>();
    if format != "raw" || !algorithm.trim().eq_ignore_ascii_case("HMAC") { return Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("NotSupportedError".into())), ("message".to_owned(), SmeltUnknown::String("Algorithm: Unrecognized name".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))); }
    let Some(hash) = smelt_crypto_hmac_hash(hash) else { return Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("NotSupportedError".into())), ("message".to_owned(), SmeltUnknown::String("Algorithm: Unrecognized name".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))); };
    Ok(SmeltCryptoKey { id: smelt_next_object_id(), algorithm: "HMAC".to_owned(), hash: hash.to_owned(), key: ::std::rc::Rc::new(key), extractable, usages })
}

/// `crypto.subtle.sign(algorithm, key, data)`: the MAC as an `ArrayBuffer`.
#[allow(dead_code)]
fn smelt_crypto_sign(algorithm: &(impl SmeltHostText + ?Sized), key: &SmeltCryptoKey, data: &[u8]) -> Result<SmeltArrayBuffer, Box<dyn ::std::error::Error>> { let algorithm = algorithm.host_text(); let algorithm: &str = &algorithm;
    if !algorithm.trim().eq_ignore_ascii_case(&key.algorithm) { return Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("InvalidAccessError".into())), ("message".to_owned(), SmeltUnknown::String("The requested operation is not valid for the provided key".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))); }
    Ok(SmeltArrayBuffer::from_bytes(smelt_crypto_hmac_tag(&key.hash, &key.key, data)))
}

/// `crypto.subtle.verify(algorithm, key, signature, data)`, comparing the
/// tags in constant time.
#[allow(dead_code)]
fn smelt_crypto_verify(algorithm: &(impl SmeltHostText + ?Sized), key: &SmeltCryptoKey, signature: &[u8], data: &[u8]) -> Result<bool, Box<dyn ::std::error::Error>> { let algorithm = algorithm.host_text(); let algorithm: &str = &algorithm;
    if !algorithm.trim().eq_ignore_ascii_case(&key.algorithm) { return Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("InvalidAccessError".into())), ("message".to_owned(), SmeltUnknown::String("The requested operation is not valid for the provided key".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))); }
    let expected = smelt_crypto_hmac_tag(&key.hash, &key.key, data);
    Ok(expected.len() == signature.len() && expected.iter().zip(signature).fold(0_u8, |acc, (left, right)| acc | (left ^ right)) == 0)
}

// @smelt:item-end
