// A bigint operation that throws reaches a source `catch`.
//
// `/` and `%` by `0n`, `**` with a negative exponent, `BigInt(1.5)`,
// `BigInt("1.5")` and an out-of-range `BigInt.asIntN` width all throw in
// JavaScript, and the throw is catchable. Each lowers to a fallible
// `BuiltinFn::BigInt` call, whose unwind edge is what lets the `catch` below
// see the branded `RangeError` / `SyntaxError` with Node's own message.
//
// A fixture rather than an example: the `catch` bindings are typed `unknown`
// by TypeScript, an erasure the bigint example corpus does not carry.

function quotient(a: bigint, b: bigint): bigint {
  return a / b;
}

function remainder(a: bigint, b: bigint): bigint {
  return a % b;
}

function power(base: bigint, exponent: bigint): bigint {
  return base ** exponent;
}

function remainderOrError(a: bigint, b: bigint): string {
  try {
    return `${remainder(a, b)}`;
  } catch (error) {
    if (error instanceof RangeError) {
      return `${error.name}: ${error.message}`;
    }
    return "unexpected";
  }
}

function widthOrError(bits: number, value: bigint): string {
  try {
    return `${BigInt.asIntN(bits, value)}`;
  } catch (error) {
    if (error instanceof RangeError) {
      return error.name;
    }
    return "unexpected";
  }
}

// The catch guard is the runtime-narrowing boundary the thrown value crosses.
function quotientOrError(a: bigint, b: bigint): string {
  try {
    return `${quotient(a, b)}`;
  } catch (error) {
    if (error instanceof RangeError) {
      return `${error.name}: ${error.message}`;
    }
    return "unexpected";
  }
}

function fromNumberOrError(value: number): string {
  try {
    return `${BigInt(value)}`;
  } catch (error) {
    if (error instanceof RangeError) {
      return `${error.name}: ${error.message}`;
    }
    return "unexpected";
  }
}

function fromStringOrError(value: string): string {
  try {
    return `${BigInt(value)}`;
  } catch (error) {
    if (error instanceof SyntaxError) {
      return `${error.name}: ${error.message}`;
    }
    return "unexpected";
  }
}

function powerOrError(base: bigint, exponent: bigint): string {
  try {
    return `${power(base, exponent)}`;
  } catch (error) {
    // V8's message for this one is "undefined must be positive", so only the
    // class is compared.
    if (error instanceof RangeError) {
      return error.name;
    }
    return "unexpected";
  }
}

console.log(quotientOrError(7n, -2n), quotientOrError(1n, 0n));
console.log(remainderOrError(-7n, 2n), remainderOrError(1n, 0n));
console.log(powerOrError(2n, 10n), powerOrError(2n, -1n));
console.log(fromNumberOrError(2 ** 53), fromNumberOrError(1.5), fromNumberOrError(NaN));
console.log(fromStringOrError("-0b1"), fromStringOrError("0b101"), fromStringOrError("1.5"), fromStringOrError(""));
console.log(widthOrError(8, 255n), widthOrError(-1, 255n));
