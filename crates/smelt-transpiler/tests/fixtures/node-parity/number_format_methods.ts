// `Number.prototype`'s formatting methods, typed and through an erased receiver.

// Exact ties round away from zero (Rust's formatting rounds them to even).
const half: number = 2.5;
console.log(half.toFixed(0), (0.5).toFixed(0), (-2.5).toFixed(0), (1.25).toFixed(1), (1.005).toFixed(2));
// Large, non-finite and negative-zero receivers.
console.log((1e21).toFixed(2), Infinity.toFixed(1), NaN.toFixed(2), (-0).toFixed(2), (-0.0001).toFixed(2));
console.log(half.toPrecision(3), (25).toPrecision(1), (1e21).toPrecision(3), (0.00000123).toPrecision(2), (123.456).toPrecision());
console.log(half.toExponential(2), (123.456).toExponential(), (0).toExponential(2), (-1.5e-7).toExponential(1));
// Fractions in other radixes.
let radix = 36;
console.log((255).toString(16), (0.5).toString(2), (-3.75).toString(2), (123.456).toString(radix));

// A computed argument outside the range is a catchable `RangeError`.
function fixed(x: number, digits: number): string {
  try {
    return x.toFixed(digits);
  } catch (error) {
    return `${(error as Error).name}: ${(error as Error).message}`;
  }
}
console.log(fixed(1.005, 2), fixed(1, 101), fixed(NaN, -1));
try {
  (5).toString(radix + 1);
} catch (error) {
  console.log((error as Error).message);
}

// An erased number answers the same members, bound to the receiver.
const erased: any = 3.14159;
console.log(typeof erased.toFixed, erased.toFixed.length, erased.valueOf.length);
console.log(erased.toFixed(2), erased.toPrecision(3), erased.toExponential(), erased.toString(2).slice(0, 8), erased.valueOf());
try {
  erased.toPrecision(0);
} catch (error) {
  console.log((error as Error).name, (error as Error).message);
}
