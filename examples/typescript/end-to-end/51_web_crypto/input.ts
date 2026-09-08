// WebCrypto: `crypto.randomUUID()` and `crypto.subtle.digest(..)`.
//
// The byte-level assertions for this surface — that `getRandomValues` actually
// FILLS its view, and that each digest matches its published test vector — live
// in the `web_crypto_runtime` tier rather than here, and deliberately. Reading
// individual bytes needs `new Uint8Array(buffer)`, and a typed-array VIEW is
// still a byte-backed host record by an explicit design decision (see
// `StdlibClass::ByteArray`), so every such line would add erasure to a corpus
// whose whole job is to hold zero of it. What is asserted here is the part that
// is concrete all the way down: a `string` UUID and a byte view's own width.
//
// The unrecognized-algorithm throw is in that tier for a different reason: a
// `try` whose body AWAITS, at module top level, currently emits a tail that
// declares its temporaries in one match arm and assigns them in the other, so
// the crate does not compile. Inside a function body — which is where the tier
// puts it — the same source is fine. See
// `blocker-logs/top-level-try-await-tail.md`.
//
// A `/regex/.test(id)` assertion is absent for a third reason: a regex LITERAL
// receiver lowers `test` to `exec(..)` and then erases the match to null-check
// it, where the recognized rule is a plain `is_match`. That is one avoidable
// erasure per call in a corpus that must hold zero, and it is a regex-lowering
// bug rather than anything about this surface. See
// `blocker-logs/regex-literal-test-erases-match.md`.

// `randomUUID()` answers a v4 UUID: 36 characters, dashes at the four fixed
// offsets, version nibble `4`, and variant nibble one of 8/9/a/b. The value is
// random, so the fixture prints the SHAPE and that two calls differ.
const id = crypto.randomUUID();
console.log(id.length);
console.log(id[8], id[13], id[18], id[23]);
console.log(id[14]);
console.log("89ab".includes(id[19]));
console.log(id === crypto.randomUUID());
console.log(id.toLowerCase() === id);

// Every hyphen-separated group has the spec's width.
const groups = id.split("-");
console.log(groups.length);
console.log(groups[0].length, groups[1].length, groups[2].length);
console.log(groups[3].length, groups[4].length);

// `subtle.digest` answers a buffer whose width is the algorithm's, and the
// algorithm name is case-insensitive.
const message = new TextEncoder().encode("abc");
console.log((await crypto.subtle.digest("SHA-1", message)).byteLength);
console.log((await crypto.subtle.digest("SHA-256", message)).byteLength);
console.log((await crypto.subtle.digest("SHA-384", message)).byteLength);
console.log((await crypto.subtle.digest("SHA-512", message)).byteLength);
console.log((await crypto.subtle.digest("sha-256", message)).byteLength);

// The input's length does not change the digest's.
const empty = new TextEncoder().encode("");
console.log((await crypto.subtle.digest("SHA-256", empty)).byteLength);
