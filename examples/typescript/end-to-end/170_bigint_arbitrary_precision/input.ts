// A `bigint` is an arbitrary-precision integer, and smelt carries it as one.
//
// TypeScript's `bigint` lowers to its own type and to Rust's
// `num_bigint::BigInt`, never to `i64` or `f64`: every value below is exact at
// 2^53, 2^64 and 2^73 (the remeda `randomBigInt` range), and no operation
// rounds through a float. The behaviors are JavaScript's own:
//
// * `/` truncates toward zero and `%` takes the dividend's sign;
// * `>>` is an arithmetic shift that floors (`-5n >> 1n` is `-3n`);
// * `&`, `|`, `^`, `~` treat a negative bigint as infinite two's complement;
// * a zero divisor, a negative exponent and `BigInt(1.5)` throw a catchable
//   `RangeError` (pinned by the `bigint_throws_are_catchable` runtime fixture,
//   whose `catch` bindings are an erasure this corpus does not carry);
// * a bigint compares with a number by mathematical value (`1n < 1.5`,
//   `2n == 2`), but is never `===` to one;
// * `console.log(10n)` prints `10n` while `String(10n)` is `"10"`.

const TWO_53 = 9_007_199_254_740_992n;
const TWO_64 = 18_446_744_073_709_551_616n;
const HUGE = 9_999_999_999_999_999_999_999n;

// remeda's `asBigInt`: eight random bytes become one 64-bit value.
function asBigInt(bytes: number[]): bigint {
  let result = 0n;
  for (const byte of bytes) {
    result = (result << 8n) + BigInt(byte);
  }
  return result;
}

function quotient(a: bigint, b: bigint): bigint {
  return a / b;
}

function power(base: bigint, exponent: bigint): bigint {
  return base ** exponent;
}

// A generic keeps the bigint's own type: `T` is `bigint` here, not erased.
function both<T>(first: T, second: T): T[] {
  return [first, second];
}

console.log("around 2^53:", TWO_53 - 1n, TWO_53, TWO_53 + 1n);
console.log("around 2^64:", TWO_64 - 1n, TWO_64, TWO_64 + 1n);
console.log("2^73 and up:", HUGE, HUGE + 1n, HUGE * HUGE);
console.log("bytes:", asBigInt([0xc3, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]));
console.log("ten bytes:", asBigInt([0x21, 0xe1, 0x9e, 0x0c, 0x9b, 0xab, 0x23, 0xff, 0xff, 0xf0]));

console.log("division:", -7n / 2n, 7n / -2n, -7n / -2n, HUGE / TWO_53);
console.log("remainder:", -7n % 2n, 7n % -2n, -7n % -2n, HUGE % TWO_53);
console.log("shift left:", 1n << 64n, 1n << 73n, -3n << 2n, 5n << -1n);
console.log("shift right:", -5n >> 1n, 5n >> 1n, -1n >> 100n, HUGE >> 13n, -HUGE >> 20n);
console.log("bitwise:", HUGE & 0xffffn, -HUGE & 0xffn, -6n | 3n, -6n ^ 3n, ~5n, ~-HUGE);
console.log("power:", power(2n, 100n), power(-3n, 3n), power(7n, 0n));

console.log("asIntN:", BigInt.asIntN(64, TWO_64 - 1n), BigInt.asIntN(8, 255n), BigInt.asIntN(64, HUGE));
console.log("asUintN:", BigInt.asUintN(64, -1n), BigInt.asUintN(8, -1n), BigInt.asUintN(80, HUGE));

console.log("compare:", 1n < 1.5, 2n == 2, 2n > 1.5, 3n <= 3, HUGE > 1e22, HUGE < 1e22);
console.log("compare NaN:", 1n < NaN, 1n > NaN, 1n == NaN);
console.log("strict:", 2n === 2n, TWO_53 + 1n === TWO_53 + 1n, HUGE !== HUGE + 1n);

console.log("toString:", HUGE.toString(), HUGE.toString(16), HUGE.toString(2).length, (-255n).toString(16));
console.log("String:", String(TWO_64), `${HUGE}`, "x" + 10n);
console.log("Number:", Number(TWO_53 + 1n), Number(HUGE), Number(-TWO_64));
console.log("BigInt():", BigInt("123456789012345678901234567890"), BigInt("  -42  "), BigInt("0x1f"), BigInt(2 ** 60), BigInt(true));
console.log("typeof:", typeof HUGE, typeof (HUGE + 1n), typeof Number(HUGE));
console.log("generic:", both(TWO_53, HUGE), both(1n, -1n).length);

let counter = TWO_53;
counter += 1n;
counter *= 3n;
counter -= TWO_53;
counter /= 2n;
counter **= 2n;
console.log("compound:", counter);

let step = TWO_64;
step++;
++step;
step--;
console.log("update:", step, step > TWO_64);

const keyed = new Map<bigint, string>([[TWO_64, "2^64"], [HUGE, "huge"]]);
const unique = new Set<bigint>([1n, 2n, 1n, TWO_64, TWO_64]);
console.log("collections:", keyed.get(TWO_64), keyed.has(HUGE + 1n), unique.size, unique.has(TWO_64));
const ordered = [HUGE, -1n, TWO_53, 0n].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
console.log("sorted:", ordered, ordered.reduce((sum, value) => sum + value, 0n));
console.log("truthy:", Boolean(0n), Boolean(-1n), 0n ? "yes" : "no", HUGE ? "yes" : "no");
