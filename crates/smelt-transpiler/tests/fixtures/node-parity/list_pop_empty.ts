// `Array.prototype.pop` on an empty array answers `undefined` for every element
// shape, and never panics.

function popGeneric<T>(items: T[]): T | undefined {
  return items.pop();
}

const numbers: number[] = [];
console.log(numbers.pop());
const strings: string[] = [];
console.log(strings.pop() ?? "absent");
const optionals: (number | undefined)[] = [];
console.log(optionals.pop());
const nullables: (string | null)[] = [];
console.log(nullables.pop());
const unknowns: unknown[] = [];
console.log(unknowns.pop());
const anys: any[] = [];
console.log(anys.pop());
const nested: number[][] = [];
console.log(nested.pop());
console.log(popGeneric<number>([]));
console.log(popGeneric<string>([]) ?? "generic absent");

const popped: number | undefined = numbers.pop();
console.log(popped === undefined);

const unions: (number | string)[] = [];
const unionPopped = unions.pop();
console.log(unionPopped);
console.log(typeof unionPopped);

const once: number[] = [7];
console.log(once.pop(), once.pop(), once.length);
const held: (number | undefined)[] = [undefined, 2];
console.log(held.pop(), held.pop(), held.pop(), held.length);

// `shift` takes the same rule from the front.
const shifted: (number | undefined)[] = [undefined, 2];
console.log(shifted.shift(), shifted.shift(), shifted.shift(), shifted.length);
const shiftedUnknown: unknown[] = [];
console.log(shiftedUnknown.shift());
const shiftedNumbers: number[] = [];
console.log(shiftedNumbers.shift());
