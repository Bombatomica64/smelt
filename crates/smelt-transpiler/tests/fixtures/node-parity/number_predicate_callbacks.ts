// A `Number.*` predicate passed as a callback tests each value as it is: the
// predicates never coerce, so a non-number answers `false`, as Number.isNaN
// called directly does.

const mixed: unknown[] = [NaN, "NaN", undefined, 1, null, "5"];
console.log(mixed.map(Number.isNaN));
console.log(mixed.map(Number.isFinite));
console.log(mixed.filter(Number.isInteger).length);
console.log(mixed.some(Number.isSafeInteger), ["1"].every(Number.isSafeInteger));

// Over a number array every element is a number.
const numbers: number[] = [1, NaN, 2.5, Infinity];
console.log(numbers.map(Number.isNaN), numbers.filter(Number.isFinite));

// A callback slot of a user function takes the slot's parameter type.
function count(values: unknown[], test: (value: unknown) => boolean): number {
  let n = 0;
  for (const value of values) {
    if (test(value)) n += 1;
  }
  return n;
}
console.log(count(mixed, Number.isNaN), count([NaN, NaN, "x"], Number.isNaN));
