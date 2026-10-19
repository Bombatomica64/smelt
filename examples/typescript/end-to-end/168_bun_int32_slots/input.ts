// Bun's `int32` (`import type { int32 } from "bun"`): a field, parameter,
// result or variable of that type is a Rust `i32`, every value stored into it
// is proven to be a whole number in range, and every read is a `number`
// again. To node it is just `number`; the program prints the same either way.
import type { int32 } from "bun";

// Bun's own example.
class Cursor {
  line: int32 = 0;
  column: int32 = 0;
}

function advance(cursor: Cursor, by: int32): int32 {
  cursor.column = (cursor.column + by) | 0;
  return cursor.column;
}

interface Pixel {
  x: int32;
  y: int32;
  alpha?: int32;
}

function mix(a: int32, b: int32): int32 {
  // Arithmetic on two int32 values is `number` arithmetic: no wrapping here.
  const sum = a + b;
  console.log("sum as number", sum, a * b, a / b);
  return Math.imul(a, b);
}

function bits(n: number): int32 {
  const masked: int32 = (n & 255) * 3 + 1;
  const shifted: int32 = n >> 2;
  const left: int32 = 1 << 31;
  const zeros: int32 = Math.clz32(n);
  const flipped: int32 = ~n;
  return (masked ^ shifted ^ left ^ zeros ^ flipped) | 0;
}

function count(text: string, needle: string): int32 {
  let hits: int32 = 0;
  for (let i: int32 = 0; i < text.length; i++) {
    if (text[i] === needle) {
      hits = (hits + 1) | 0;
    }
  }
  return hits;
}

function wrapAround(n: number): int32 {
  return n as int32;
}

const c = new Cursor();
console.log(advance(c, 3), advance(c, 4), c.column, c.line);
console.log(advance(c, 2147483647), c.column);
const p: Pixel = { x: 3, y: -4 };
console.log(p.x + p.y, p.alpha === undefined, `${p.x},${p.y}`);
const q: Pixel = { x: 1, y: 2, alpha: 255 };
console.log(q.alpha, (q.alpha ?? 0) + 1);
console.log(mix(65536, 65536), mix(-7, 3));
console.log(bits(1000), bits(-1), bits(2 ** 40 + 5));
console.log(count("banana", "a"), count("", "a"));
// `as int32` is `| 0` under Bun --aot and a no-op in node; they agree on int32 values.
console.log(wrapAround(7), wrapAround(-2147483648));
