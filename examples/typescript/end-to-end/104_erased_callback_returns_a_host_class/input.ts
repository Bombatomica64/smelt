// Erasing a function value builds a forwarding closure that answers
// `SmeltUnknown`, so the wrapped call's RESULT has to be erased too — and a
// value's Rust representation is what decides whether it already is one.
//
// A MODELED host class (`ArrayBuffer` here) declares no fields of its own yet
// renders as a concrete Rust struct, so it needs the erasure step exactly like
// a user class does. Reading "does this class declare fields?" instead of
// "does this class render as the erased carrier?" handed the concrete struct
// straight to the erased callable's `Ok(..)` and the generated crate did not
// compile.
//
// The generated union below is what forces the seam: its `IntoSmeltUnknown`
// erases the function arm whether or not the program ever takes that path.

type Maker = boolean | ((size: number) => ArrayBuffer)

function describe(maker: Maker): string {
  if (typeof maker === 'function') {
    const buffer = maker(4)
    return `bytes=${buffer.byteLength}`
  }
  return `flag=${maker}`
}

function label(maker: Maker): string {
  return `${typeof maker}`
}

const make = (size: number): ArrayBuffer => new ArrayBuffer(size)

console.log(describe(make))
console.log(describe(true))
console.log(label(make))
