// `btoa` / `atob` as concrete surfaces, with the spec's branded
// `InvalidCharacterError` on the input each direction cannot represent.
//
// Both directions are FALLIBLE, unlike the URI transcoders where only the
// decoders throw, so both lower to a call terminator with an unwind edge: a
// `try` the source wrote around either one reaches its handler. That also
// closed a general gap — the throwing pass named `JSON.parse` alone, so every
// fallible builtin added after it (the URI decoders included) failed to mark
// its enclosing function, and `decodeURI` outside a `try` emitted a `?` in a
// signature that returns no `Result`. The question is now asked of the builtin
// (`BuiltinFn::is_fallible`).
//
// `atob` implements WHATWG *forgiving*-base64, not canonical base64, and the
// difference is observable: unpadded input decodes, non-canonical trailing bits
// decode, embedded whitespace is stripped — while a length of `4n + 1` and a
// stray `=` throw. Every line below is diffed against Node.

function encode(value: string): string {
  return btoa(value);
}

function decode(value: string): string {
  return atob(value);
}

console.log(encode("hello"));
console.log(encode(""));
console.log(encode("a"), encode("ab"), encode("abc"));
// One byte per code point, up to U+00FF, and the output is padded.
console.log(encode("ÿ "));

console.log(decode("aGVsbG8="));
console.log(decode(""));
console.log(decode("YQ=="), decode("YWI="), decode("YWJj"));
// Forgiving: no padding, non-canonical trailing bits, embedded whitespace.
console.log(decode("aGVsbG8"));
console.log(decode("YR=="));
console.log(decode("aGVs bG8="));
// A byte string round-trips through both directions unchanged, which is why
// `atob` answers one code point per byte rather than UTF-8 text.
console.log(decode(encode("round trip é")));

// The throws are catchable and carry the spec's brand.
function safeEncode(value: string): string {
  try {
    return btoa(value);
  } catch (error) {
    const failure = error as DOMException;
    return `E:${failure.name}:${failure.message}`;
  }
}

function safeDecode(value: string): string {
  try {
    return atob(value);
  } catch (error) {
    const failure = error as DOMException;
    return `E:${failure.name}:${failure.message}`;
  }
}

// A code point above U+00FF has no byte.
console.log(safeEncode("Ā"));
console.log(safeEncode("ok"));
// A length of `4n + 1`, and a character outside the alphabet.
console.log(safeDecode("a"));
console.log(safeDecode("!!!!"));
// A stray `=` that padding removal cannot reach, because the length is not a
// multiple of four.
console.log(safeDecode("YR="));
console.log(safeDecode("YWJj"));

// The brand is a real `DOMException`, so an erased `catch` binding can identify
// it the way any other host error is identified.
function decodedBrand(value: string): boolean {
  try {
    atob(value);
    return false;
  } catch (error) {
    return error instanceof DOMException;
  }
}

console.log(decodedBrand("****"));
console.log(decodedBrand("YWJj"));
