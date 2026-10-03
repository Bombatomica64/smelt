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
//!   (`SmeltHash`): an algorithm, the bytes fed so far and a finalized flag,
//!   behind a shared cell so `h.update(a); h.update(b)` feeds one hasher and
//!   `update` can answer the SAME hash for chaining. The hashing itself is the
//!   RustCrypto crates a Rust team would pick (`md-5`, `sha1`, `sha2`, which
//!   share one `digest::Digest` trait).
//!
//! The hasher's three failure modes are Node's own catchable errors
//! (`Digest method not supported`, `Digest already called`, and an odd-length
//! `hex` input), raised through the same error channel a source `throw` uses.

use crate::rust::CodeWriter;

/// Name of the generated `path.join` helper, `fn(&[String]) -> String`.
pub const PATH_JOIN_FN: &str = "smelt_path_join";
/// Name of the generated `path.resolve` helper, `fn(&[String]) -> String`.
pub const PATH_RESOLVE_FN: &str = "smelt_path_resolve";
/// Name of the generated `path.normalize` helper, `fn(&str) -> String`.
pub const PATH_NORMALIZE_FN: &str = "smelt_path_normalize";
/// Name of the generated `path.dirname` helper, `fn(&str) -> String`.
pub const PATH_DIRNAME_FN: &str = "smelt_path_dirname";
/// Name of the generated `path.basename` helper, `fn(&str, Option<&str>) -> String`.
pub const PATH_BASENAME_FN: &str = "smelt_path_basename";
/// Name of the generated `path.extname` helper, `fn(&str) -> String`.
pub const PATH_EXTNAME_FN: &str = "smelt_path_extname";
/// Name of the generated `path.relative` helper, `fn(&str, &str) -> String`.
pub const PATH_RELATIVE_FN: &str = "smelt_path_relative";
/// Name of the generated `path.isAbsolute` helper, `fn(&str) -> bool`.
pub const PATH_IS_ABSOLUTE_FN: &str = "smelt_path_is_absolute";

/// Node's POSIX `lib/path.js`, ported function for function.
///
/// Kept as one verbatim block rather than assembled line by line: it is a port
/// of a reference implementation, and reading it next to Node's source is how
/// it is reviewed. Each function's doc names the Node function it ports.
const PATH_HELPERS: &str = r#"/// Node's `normalizeString` for POSIX separators: resolve `.` and `..`
/// segments and collapse repeated slashes. `allow_above_root` keeps leading
/// `..` segments (a relative path), where an absolute path drops them.
#[allow(dead_code)]
fn smelt_path_normalize_string(path: &str, allow_above_root: bool) -> String {
    let bytes = path.as_bytes();
    let mut res = String::new();
    let mut last_segment_length: isize = 0;
    let mut last_slash: isize = -1;
    let mut dots: i32 = 0;
    let mut code: u8 = 0;
    let mut i: usize = 0;
    while i <= bytes.len() {
        if i < bytes.len() {
            code = bytes[i];
        } else if code == b'/' {
            break;
        } else {
            code = b'/';
        }
        if code == b'/' {
            if last_slash == i as isize - 1 || dots == 1 {
                // A repeated slash or a `.` segment: nothing to append.
            } else if dots == 2 {
                let rb = res.as_bytes();
                if res.len() < 2 || last_segment_length != 2 || rb[rb.len() - 1] != b'.' || rb[rb.len() - 2] != b'.' {
                    if res.len() > 2 {
                        match res.rfind('/') {
                            None => {
                                res.clear();
                                last_segment_length = 0;
                            }
                            Some(index) => {
                                res.truncate(index);
                                last_segment_length = res.len() as isize - 1 - res.rfind('/').map_or(-1, |value| value as isize);
                            }
                        }
                        last_slash = i as isize;
                        dots = 0;
                        i += 1;
                        continue;
                    } else if !res.is_empty() {
                        res.clear();
                        last_segment_length = 0;
                        last_slash = i as isize;
                        dots = 0;
                        i += 1;
                        continue;
                    }
                }
                if allow_above_root {
                    res.push_str(if res.is_empty() { ".." } else { "/.." });
                    last_segment_length = 2;
                }
            } else {
                let segment = &path[(last_slash + 1) as usize..i];
                if !res.is_empty() {
                    res.push('/');
                }
                res.push_str(segment);
                last_segment_length = i as isize - last_slash - 1;
            }
            last_slash = i as isize;
            dots = 0;
        } else if code == b'.' && dots != -1 {
            dots += 1;
        } else {
            dots = -1;
        }
        i += 1;
    }
    res
}

/// `path.posix.normalize(path)`.
#[allow(dead_code)]
fn smelt_path_normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".to_owned();
    }
    let is_absolute = path.starts_with('/');
    let trailing_separator = path.ends_with('/');
    let mut normalized = smelt_path_normalize_string(path, !is_absolute);
    if normalized.is_empty() {
        if is_absolute {
            return "/".to_owned();
        }
        return if trailing_separator { "./".to_owned() } else { ".".to_owned() };
    }
    if trailing_separator {
        normalized.push('/');
    }
    if is_absolute { format!("/{normalized}") } else { normalized }
}

/// `path.posix.isAbsolute(path)`.
#[allow(dead_code)]
fn smelt_path_is_absolute(path: &str) -> bool {
    path.starts_with('/')
}

/// `path.posix.join(...segments)`: the non-empty segments joined by `/`, then
/// normalized; no segment at all is `.`.
#[allow(dead_code)]
fn smelt_path_join(segments: &[String]) -> String {
    let joined = segments.iter().filter(|segment| !segment.is_empty()).map(String::as_str).collect::<Vec<_>>().join("/");
    if joined.is_empty() {
        return ".".to_owned();
    }
    smelt_path_normalize(&joined)
}

/// `path.posix.resolve(...segments)`: right to left until a segment is
/// absolute, then the process working directory.
#[allow(dead_code)]
fn smelt_path_resolve(segments: &[String]) -> String {
    let mut resolved = String::new();
    let mut resolved_absolute = false;
    let cwd = ::std::env::current_dir().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_else(|_| "/".to_owned());
    for segment in segments.iter().map(String::as_str).rev().chain(::std::iter::once(cwd.as_str())) {
        if resolved_absolute {
            break;
        }
        if segment.is_empty() {
            continue;
        }
        resolved = format!("{segment}/{resolved}");
        resolved_absolute = segment.starts_with('/');
    }
    let normalized = smelt_path_normalize_string(&resolved, !resolved_absolute);
    if resolved_absolute {
        return format!("/{normalized}");
    }
    if normalized.is_empty() { ".".to_owned() } else { normalized }
}

/// `path.posix.relative(from, to)`.
#[allow(dead_code)]
fn smelt_path_relative(from: &str, to: &str) -> String {
    if from == to {
        return String::new();
    }
    let from = smelt_path_resolve(&[from.to_owned()]);
    let to = smelt_path_resolve(&[to.to_owned()]);
    if from == to {
        return String::new();
    }
    let (from_bytes, to_bytes) = (from.as_bytes(), to.as_bytes());
    let from_start = 1usize;
    let from_end = from_bytes.len();
    let from_len = from_end - from_start;
    let to_start = 1usize;
    let to_len = to_bytes.len() - to_start;
    let length = from_len.min(to_len);
    let mut last_common_sep: isize = -1;
    let mut i = 0usize;
    while i < length {
        let from_code = from_bytes[from_start + i];
        if from_code != to_bytes[to_start + i] {
            break;
        } else if from_code == b'/' {
            last_common_sep = i as isize;
        }
        i += 1;
    }
    if i == length {
        if to_len > length {
            if to_bytes[to_start + i] == b'/' {
                return to[to_start + i + 1..].to_owned();
            }
            if i == 0 {
                return to[to_start + i..].to_owned();
            }
        } else if from_len > length {
            if from_bytes[from_start + i] == b'/' {
                last_common_sep = i as isize;
            } else if i == 0 {
                last_common_sep = 0;
            }
        }
    }
    let mut out = String::new();
    let mut index = (from_start as isize + last_common_sep + 1) as usize;
    while index <= from_end {
        if index == from_end || from_bytes[index] == b'/' {
            out.push_str(if out.is_empty() { ".." } else { "/.." });
        }
        index += 1;
    }
    format!("{out}{}", &to[(to_start as isize + last_common_sep) as usize..])
}

/// `path.posix.dirname(path)`.
#[allow(dead_code)]
fn smelt_path_dirname(path: &str) -> String {
    if path.is_empty() {
        return ".".to_owned();
    }
    let bytes = path.as_bytes();
    let has_root = bytes[0] == b'/';
    let mut end: isize = -1;
    let mut matched_slash = true;
    let mut i = bytes.len() - 1;
    while i >= 1 {
        if bytes[i] == b'/' {
            if !matched_slash {
                end = i as isize;
                break;
            }
        } else {
            matched_slash = false;
        }
        i -= 1;
    }
    if end == -1 {
        return if has_root { "/".to_owned() } else { ".".to_owned() };
    }
    if has_root && end == 1 {
        return "//".to_owned();
    }
    path[..end as usize].to_owned()
}

/// `path.posix.basename(path, suffix?)`.
#[allow(dead_code)]
fn smelt_path_basename(path: &str, suffix: Option<&str>) -> String {
    let bytes = path.as_bytes();
    let mut start = 0usize;
    let mut end: isize = -1;
    let mut matched_slash = true;
    if let Some(suffix) = suffix.filter(|suffix| !suffix.is_empty() && suffix.len() <= path.len()) {
        if suffix == path {
            return String::new();
        }
        let suffix_bytes = suffix.as_bytes();
        let mut ext_index = suffix_bytes.len() as isize - 1;
        let mut first_non_slash_end: isize = -1;
        for i in (0..bytes.len()).rev() {
            let code = bytes[i];
            if code == b'/' {
                if !matched_slash {
                    start = i + 1;
                    break;
                }
            } else {
                if first_non_slash_end == -1 {
                    matched_slash = false;
                    first_non_slash_end = i as isize + 1;
                }
                if ext_index >= 0 {
                    if code == suffix_bytes[ext_index as usize] {
                        ext_index -= 1;
                        if ext_index == -1 {
                            end = i as isize;
                        }
                    } else {
                        ext_index = -1;
                        end = first_non_slash_end;
                    }
                }
            }
        }
        if start as isize == end {
            end = first_non_slash_end;
        } else if end == -1 {
            end = bytes.len() as isize;
        }
        return path[start..end as usize].to_owned();
    }
    for i in (0..bytes.len()).rev() {
        if bytes[i] == b'/' {
            if !matched_slash {
                start = i + 1;
                break;
            }
        } else if end == -1 {
            matched_slash = false;
            end = i as isize + 1;
        }
    }
    if end == -1 {
        return String::new();
    }
    path[start..end as usize].to_owned()
}

/// `path.posix.extname(path)`.
#[allow(dead_code)]
fn smelt_path_extname(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut start_dot: isize = -1;
    let mut start_part: isize = 0;
    let mut end: isize = -1;
    let mut matched_slash = true;
    let mut pre_dot_state = 0;
    for i in (0..bytes.len()).rev() {
        let code = bytes[i];
        if code == b'/' {
            if !matched_slash {
                start_part = i as isize + 1;
                break;
            }
            continue;
        }
        if end == -1 {
            matched_slash = false;
            end = i as isize + 1;
        }
        if code == b'.' {
            if start_dot == -1 {
                start_dot = i as isize;
            } else if pre_dot_state != 1 {
                pre_dot_state = 1;
            }
        } else if start_dot != -1 {
            pre_dot_state = -1;
        }
    }
    if start_dot == -1 || end == -1 || pre_dot_state == 0 || (pre_dot_state == 1 && start_dot == end - 1 && start_dot == start_part + 1) {
        return String::new();
    }
    path[start_dot as usize..end as usize].to_owned()
}
"#;

/// Emit the `node:path` helpers.
pub fn emit_path(writer: &mut CodeWriter) {
    writer.blank_line();
    for line in PATH_HELPERS.lines() {
        writer.line(line);
    }
    writer.blank_line();
}

/// Name of the generated `createHash` helper.
pub const HASH_CREATE_FN: &str = "smelt_hash_create";
/// Name of the generated `hash.update(bytes)` helper.
pub const HASH_UPDATE_BYTES_FN: &str = "smelt_hash_update_bytes";
/// Name of the generated `hash.update(text, encoding)` helper.
pub const HASH_UPDATE_TEXT_FN: &str = "smelt_hash_update_text";
/// Name of the generated `hash.digest(encoding)` helper.
pub const HASH_DIGEST_TEXT_FN: &str = "smelt_hash_digest_text";
/// Name of the generated `hash.digest()` helper.
pub const HASH_DIGEST_BYTES_FN: &str = "smelt_hash_digest_bytes";

/// The `SmeltHash` runtime type and its helpers, minus the error payloads.
///
/// `{not_supported}`, `{finalized}` and `{bad_hex}` are replaced by the
/// thrown-value expressions (`thrown::error_payload_record_expr`) so the
/// hasher's errors are the branded `Error` records a source `catch` reads.
const HASH_HELPERS: &str = r#"/// The algorithms `createHash` accepts, by their normalized OpenSSL name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SmeltHashAlgorithm { Md5, Sha1, Sha224, Sha256, Sha384, Sha512, Sha512_224, Sha512_256 }

/// The mutable half of a `Hash`: what it hashes and whether `digest` ran.
#[derive(Debug)]
struct SmeltHashState { algorithm: SmeltHashAlgorithm, data: Vec<u8>, finalized: bool }

/// `node:crypto`'s `Hash`: a shared hasher state with a JS reference identity.
///
/// `Clone` shares the state, so `update` can answer the same hash for chaining
/// and two handles observe one hasher, as two JavaScript references do.
#[derive(Clone, Debug)]
pub struct SmeltHash { id: usize, state: ::std::rc::Rc<::std::cell::RefCell<SmeltHashState>> }

impl PartialEq for SmeltHash { fn eq(&self, other: &Self) -> bool { self.id == other.id } }

/// A placeholder for a slot that is assigned before it is read: an already
/// finalized hash, so a stray use throws `Digest already called` instead of
/// hashing into a value the program never created.
impl Default for SmeltHash { fn default() -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltHashState { algorithm: SmeltHashAlgorithm::Sha256, data: Vec::new(), finalized: true })) } } }

impl SmeltHash {
    /// JS reference identity of this hash.
    #[allow(dead_code)]
    pub fn id(&self) -> usize { self.id }
}

/// Map a `createHash` algorithm name to its hasher, as OpenSSL names them:
/// case-insensitive, with the `sha-256` and `RSA-SHA256` aliases Node accepts.
fn smelt_hash_algorithm(name: &str) -> Option<SmeltHashAlgorithm> {
    let lower = name.to_ascii_lowercase();
    let lower = lower.strip_prefix("rsa-").unwrap_or(&lower);
    let normalized = match lower.strip_prefix("sha-") { Some(rest) => format!("sha{rest}"), None => lower.to_owned() };
    match normalized.as_str() {
        "md5" => Some(SmeltHashAlgorithm::Md5),
        "sha1" => Some(SmeltHashAlgorithm::Sha1),
        "sha224" => Some(SmeltHashAlgorithm::Sha224),
        "sha256" => Some(SmeltHashAlgorithm::Sha256),
        "sha384" => Some(SmeltHashAlgorithm::Sha384),
        "sha512" => Some(SmeltHashAlgorithm::Sha512),
        "sha512-224" | "sha512/224" => Some(SmeltHashAlgorithm::Sha512_224),
        "sha512-256" | "sha512/256" => Some(SmeltHashAlgorithm::Sha512_256),
        _ => None,
    }
}

/// `createHash(algorithm)`: a fresh hash, or Node's `Digest method not supported`.
#[allow(dead_code)]
fn smelt_hash_create(algorithm: &str) -> Result<SmeltHash, Box<dyn ::std::error::Error>> {
    let Some(algorithm) = smelt_hash_algorithm(algorithm) else { return Err({not_supported}) };
    Ok(SmeltHash { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltHashState { algorithm, data: Vec::new(), finalized: false })) })
}

/// `hash.update(bytes)`: feed bytes, answering the same hash.
#[allow(dead_code)]
fn smelt_hash_update_bytes(hash: &SmeltHash, bytes: &[u8]) -> Result<SmeltHash, Box<dyn ::std::error::Error>> {
    let mut state = hash.state.borrow_mut();
    if state.finalized { return Err({finalized}) }
    state.data.extend_from_slice(bytes);
    drop(state);
    Ok(hash.clone())
}

/// The bytes a string means under a Node `Buffer` encoding name.
///
/// `utf8` (and any name Node does not decode specially) is the string's UTF-8;
/// `latin1`/`binary`/`ascii` keep the low byte of each UTF-16 code unit;
/// `utf16le`/`ucs2` are the code units little-endian; `hex` reads byte pairs
/// up to the first pair that is not hex; `base64`/`base64url` are Node's
/// lenient decoder (either alphabet, other characters skipped, `=` ends it).
/// `None` is an odd-length `hex` string, which Node rejects.
fn smelt_hash_text_bytes(text: &str, encoding: &str) -> Option<Vec<u8>> {
    match encoding.to_ascii_lowercase().as_str() {
        "latin1" | "binary" | "ascii" => Some(text.encode_utf16().map(|unit| unit as u8).collect()),
        "utf16le" | "utf-16le" | "ucs2" | "ucs-2" => Some(text.encode_utf16().flat_map(u16::to_le_bytes).collect()),
        "hex" => {
            let units: Vec<u16> = text.encode_utf16().collect();
            if units.len() % 2 == 1 { return None; }
            let digit = |unit: u16| char::from_u32(u32::from(unit)).and_then(|ch| ch.to_digit(16));
            let mut bytes = Vec::new();
            for pair in units.chunks(2) {
                match (digit(pair[0]), digit(pair[1])) { (Some(high), Some(low)) => bytes.push((high * 16 + low) as u8), _ => break }
            }
            Some(bytes)
        }
        "base64" | "base64url" => {
            let mut bytes = Vec::new();
            let (mut buffer, mut bits) = (0u32, 0u32);
            for ch in text.chars() {
                let value = match ch { 'A'..='Z' => ch as u32 - 'A' as u32, 'a'..='z' => ch as u32 - 'a' as u32 + 26, '0'..='9' => ch as u32 - '0' as u32 + 52, '+' | '-' => 62, '/' | '_' => 63, '=' => break, _ => continue };
                buffer = (buffer << 6) | value;
                bits += 6;
                if bits >= 8 { bits -= 8; bytes.push((buffer >> bits) as u8); buffer &= (1 << bits) - 1; }
            }
            Some(bytes)
        }
        _ => Some(text.as_bytes().to_vec()),
    }
}

/// `hash.update(text, inputEncoding)`.
#[allow(dead_code)]
fn smelt_hash_update_text(hash: &SmeltHash, text: &str, encoding: &str) -> Result<SmeltHash, Box<dyn ::std::error::Error>> {
    let Some(bytes) = smelt_hash_text_bytes(text, encoding) else { return Err({bad_hex}) };
    smelt_hash_update_bytes(hash, &bytes)
}

/// `hash.digest()`: finalize and answer the digest bytes.
#[allow(dead_code)]
fn smelt_hash_digest_bytes(hash: &SmeltHash) -> Result<Vec<u8>, Box<dyn ::std::error::Error>> {
    use sha2::Digest as _;
    let mut state = hash.state.borrow_mut();
    if state.finalized { return Err({finalized}) }
    state.finalized = true;
    let data = ::std::mem::take(&mut state.data);
    Ok(match state.algorithm {
        SmeltHashAlgorithm::Md5 => md5::Md5::digest(&data).to_vec(),
        SmeltHashAlgorithm::Sha1 => sha1::Sha1::digest(&data).to_vec(),
        SmeltHashAlgorithm::Sha224 => sha2::Sha224::digest(&data).to_vec(),
        SmeltHashAlgorithm::Sha256 => sha2::Sha256::digest(&data).to_vec(),
        SmeltHashAlgorithm::Sha384 => sha2::Sha384::digest(&data).to_vec(),
        SmeltHashAlgorithm::Sha512 => sha2::Sha512::digest(&data).to_vec(),
        SmeltHashAlgorithm::Sha512_224 => sha2::Sha512_224::digest(&data).to_vec(),
        SmeltHashAlgorithm::Sha512_256 => sha2::Sha512_256::digest(&data).to_vec(),
    })
}

/// Render digest bytes under a Node `Buffer` encoding name (`hex`, `base64`,
/// `base64url`, `latin1`/`binary`, `ascii`, `utf8`, `utf16le`); `None` for a
/// name Node would answer with a `Buffer` rather than a string.
fn smelt_hash_bytes_text(bytes: &[u8], encoding: &str) -> Option<String> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let base64 = |url: bool| {
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let block = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
            for index in 0..=chunk.len() {
                let ch = ALPHABET[((block >> (18 - 6 * index)) & 63) as usize] as char;
                out.push(match (url, ch) { (true, '+') => '-', (true, '/') => '_', _ => ch });
            }
            if !url { for _ in chunk.len()..3 { out.push('='); } }
        }
        out
    };
    match encoding.to_ascii_lowercase().as_str() {
        "hex" => Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect()),
        "base64" => Some(base64(false)),
        "base64url" => Some(base64(true)),
        "latin1" | "binary" => Some(bytes.iter().map(|byte| char::from(*byte)).collect()),
        "ascii" => Some(bytes.iter().map(|byte| char::from(*byte & 0x7f)).collect()),
        "utf8" | "utf-8" => Some(String::from_utf8_lossy(bytes).into_owned()),
        "utf16le" | "utf-16le" | "ucs2" | "ucs-2" => Some(String::from_utf16_lossy(&bytes.chunks(2).filter(|pair| pair.len() == 2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<_>>())),
        _ => None,
    }
}

/// `hash.digest(encoding)`: finalize and answer the encoded digest.
#[allow(dead_code)]
fn smelt_hash_digest_text(hash: &SmeltHash, encoding: &str) -> Result<String, Box<dyn ::std::error::Error>> {
    let bytes = smelt_hash_digest_bytes(hash)?;
    match smelt_hash_bytes_text(&bytes, encoding) { Some(text) => Ok(text), None => Err({bad_encoding}) }
}
"#;

/// Emit the `SmeltHash` runtime type and its helpers.
///
/// Only called where the crate carries the thrown-value ABI (the frontend's
/// hasher calls are fallible, which demands it), since every helper can throw.
pub fn emit_hash(writer: &mut CodeWriter) {
    let error = |message: &str| {
        crate::thrown::throw_expr(&crate::thrown::error_payload_record_expr(
            "Error",
            &format!("{message:?}"),
        ))
    };
    let type_error = |message: &str| {
        crate::thrown::throw_expr(&crate::thrown::error_payload_record_expr(
            "TypeError",
            &format!("{message:?}"),
        ))
    };
    let helpers = HASH_HELPERS
        .replace("{not_supported}", &error("Digest method not supported"))
        .replace("{finalized}", &error("Digest already called"))
        .replace(
            "{bad_hex}",
            &type_error("The argument 'encoding' is invalid for data of odd length. Received 'hex'"),
        )
        .replace(
            "{bad_encoding}",
            &type_error("The digest encoding answers a Buffer, not a string"),
        );
    writer.blank_line();
    for line in helpers.lines() {
        writer.line(line);
    }
    writer.blank_line();
    // Erasing a hash is a dynamic boundary like any host value's: the record
    // carries the identity marker and the live value is retained, so narrowing
    // it back (where that is allowed) reaches the same hasher.
    crate::host_value_erasure::emit_adapters(
        writer,
        "SmeltHash",
        "__smelt_hash",
        &[],
        crate::host_value_erasure::Recovery::None,
    );
}
