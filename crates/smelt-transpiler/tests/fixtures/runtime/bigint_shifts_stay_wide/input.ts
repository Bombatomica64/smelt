// A bigint shift is never JavaScript's 32-bit number shift. Bigint literals
// are typed `bigint` (smelt's `BigInt`), so `result << 8n` keeps the wide shift:
// five bytes build 4328719365n here, where an int32 shift would wrap after
// four (remeda's `randomBigInt` builds its value this way). Number shifts on
// the same page keep ToInt32 semantics.
function asBigInt(bytes: Iterable<number>): bigint {
  let result = 0n;
  for (const byte of bytes) {
    result = (result << 8n) + BigInt(byte);
  }
  return result;
}
console.log(asBigInt([1, 2, 3, 4, 5]).toString());
const wide = 1n << 40n;
console.log(wide.toString());
console.log((wide >> 38n).toString());
const narrow = 1 << 31;
console.log(narrow);
console.log(-8 >> 1);
// A bigint element list builds a bigint typed array element by element.
const big = new BigInt64Array([1n, 2n, 3n]);
console.log(big.length);
console.log(big[2] === 3n);
