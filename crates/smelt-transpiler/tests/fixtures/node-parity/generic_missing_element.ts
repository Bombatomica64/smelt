// A generic body reads a missing element as `undefined` in the carrier of
// whatever `T` is instantiated at, exactly as the typed read of the same slot:
// at `number` the hole (`undefined`, NaN in arithmetic, `null` in JSON), at an
// optional `undefined`, behind `unknown` `undefined`.
function pick<T>(values: readonly T[], indices: number[]): T[] {
  const result = new Array<T>(indices.length);
  for (let i = 0; i < indices.length; i++) {
    result[i] = values[indices[i]];
  }
  return result;
}
function holes<T>(n: number): T[] {
  return new Array<T>(n);
}
function last<T>(values: T[]): T {
  return values[values.length];
}

const numbers: number[] = [10, 20, 30];
const picked = pick(numbers, [0, 2, 7]);
console.log(picked.length, picked[0], picked[1], picked[2], picked[2] === undefined);
console.log(JSON.stringify(picked), picked[2] + 1, String(picked[2]));
const typed: number = numbers[7];
console.log(typed === picked[2], typed, JSON.stringify([typed]));

const allocated = holes<number>(2);
console.log(allocated.length, allocated[0], allocated[1] === undefined, JSON.stringify(allocated));
console.log(last(numbers), last(numbers) === undefined, last(numbers) + 1);

const maybe: (number | undefined)[] = [1, undefined];
const pickedMaybe = pick(maybe, [1, 5, 0]);
console.log(pickedMaybe[0], pickedMaybe[1], pickedMaybe[2], pickedMaybe[1] === undefined);

const erased: unknown[] = ["a", 2];
const pickedErased = pick(erased, [3, 0]);
console.log(pickedErased[0], pickedErased[0] === undefined, typeof pickedErased[0], pickedErased[1]);
