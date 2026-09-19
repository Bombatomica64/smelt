// A value asked for at `string` is its STRING CONVERSION — what JavaScript
// does wherever a string is expected, and the sibling of the truthiness rule a
// value asked for at `boolean` already takes.
//
// The shape that needs it is the one TypeScript itself permits: `buffer` is
// `never` after the `instanceof` guard, and `never` is assignable to `string`,
// so tsc accepts the fall-through return. Smelt handed the value back at its
// own type and the generated crate did not compile. The same function written
// with a closure body already emitted the conversion, so the two spellings
// disagreed about one source.
//
// Both spellings are covered here so they cannot drift apart again.

const describe = (buffer: ArrayBuffer): string => {
  if (buffer instanceof ArrayBuffer) {
    return `bytes:${buffer.byteLength}`
  }
  return buffer
}

function describeBlock(buffer: ArrayBuffer): string {
  if (buffer instanceof ArrayBuffer) {
    return `block:${buffer.byteLength}`
  }
  return buffer
}

console.log(describe(new ArrayBuffer(3)))
console.log(describeBlock(new ArrayBuffer(4)))
