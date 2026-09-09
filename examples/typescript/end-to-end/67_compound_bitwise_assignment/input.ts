// Every compound assignment whose operator has a binary form lowers as
// `x = x op y`.
//
// The statement path knew `+= -= *= /=` and nothing else, so `out |= x` was
// "assignment operator is not lowered yet: BitwiseOR" while the identical
// `out = out | x` on the next line lowered fine. Hono's constant-time string
// compare (`src/utils/buffer.ts`) is written the first way, and so is most
// hashing code.
//
// There is nothing per-operator to decide: the operator's semantics — JS int32
// truncation for the bitwise and shift forms, `>>>`'s unsigned shift, string
// concatenation for `+=` — come from the same `BinOp` the binary path already
// lowers. `**=` is deliberately still out, because `BinOp` has no
// exponentiation arm.
//
// The expected values are JavaScript's, so the golden is also the int32
// assertion: `-16 >>> 28` is 15 (not 0) because the left operand is read as
// unsigned, and `-16 >> 2` stays negative.

let out = 5 ^ 3;
out |= 8;
console.log(out);

let mask = 0xff;
mask &= 0x0f;
console.log(mask);

let shifted = 1;
shifted <<= 4;
console.log(shifted);

let signed = -16;
signed >>= 2;
console.log(signed);

let unsigned = -16;
unsigned >>>= 28;
console.log(unsigned);

let rest = 17;
rest %= 5;
console.log(rest);

let flipped = 6;
flipped ^= 3;
console.log(flipped);

// The same rule inside a callback, where the accumulator is a CAPTURE rather
// than a plain local: the compact callback IR has its own operator map, and a
// line that lowers outside a closure must not be a blocker inside one.
const values = [1, 2, 3, 4];
let hash = 0;
values.forEach((value) => {
  hash |= value;
});
console.log(hash);

// The shape from Hono: a constant-time comparison that accumulates differences
// with `|=` so it cannot short-circuit.
function constantTimeEqualString(a: string, b: string): boolean {
  const aLen = a.length;
  const bLen = b.length;
  const maxLen = Math.max(aLen, bLen);
  let diff = aLen ^ bLen;
  for (let i = 0; i < maxLen; i++) {
    const aChar = i < aLen ? a.charCodeAt(i) : 0;
    const bChar = i < bLen ? b.charCodeAt(i) : 0;
    diff |= aChar ^ bChar;
  }
  return diff === 0;
}

console.log(constantTimeEqualString("abc", "abc"));
console.log(constantTimeEqualString("abc", "abd"));
console.log(constantTimeEqualString("abc", "abcd"));
