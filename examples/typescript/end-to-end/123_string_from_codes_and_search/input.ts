// `String.fromCharCode` and `String.fromCodePoint` build a string from numbers,
// and `string.search` finds a pattern.
//
// Neither was recognized: `String` resolved to its erased host value and the
// call answered `undefined`, so an MDN-style base64 encoder
// (`binary += String.fromCharCode(bytes[i])`, Hono's `encodeBase64`) built the
// text "nullnull..". `fromCharCode` takes each argument through `ToUint16` (so
// 65536 + 65 is "A"); `fromCodePoint` takes whole code points (astral ones
// included).
//
// Every line below is diffed against Node.

const codes = [72, 101, 108, 108, 111];
let text = '';
for (let i = 0; i < codes.length; i++) {
  text += String.fromCharCode(codes[i]);
}
console.log(text);
console.log(String.fromCharCode(0x4e16, 0x754c, 65536 + 65));
console.log(String.fromCodePoint(0x1f525, 0x41));
console.log(String.fromCharCode().length);

// `string.search(pattern)` answers the UTF-16 index of the first match, or
// -1; a string pattern is compiled as a RegExp. It was not modeled and
// answered `undefined`, so Hono's `escapeToBuffer` (`str.search(escapeRe)`)
// never escaped anything.
const escapeRe = /[&<>'"]/
console.log('I <b>'.search(escapeRe), 'ab'.search('b'), 'héllo<'.search(/</), 'x'.search(/y/))
