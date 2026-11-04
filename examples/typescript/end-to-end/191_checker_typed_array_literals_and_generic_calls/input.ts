// A generic callee's own `T` in a parameter type is filled from an array
// literal argument's elements, so the call is instantiated at `number` and
// `string` (`chunk::<f64>`), not erased.
function chunk<T>(arr: readonly T[], size: number): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < arr.length; i += size) {
    out.push(arr.slice(i, i + size));
  }
  return out;
}

function swap<A, B>(pair: [A, B]): [B, A] {
  return [pair[1], pair[0]];
}

// An unannotated `= []` takes the type TypeScript evolves it to through its
// pushes (`T[]`), as if that were its annotation.
function keep<T>(items: T[], pred: (v: T) => boolean): T[] {
  const kept = [];
  for (const item of items) {
    if (pred(item)) {
      kept.push(item);
    }
  }
  return kept;
}

const numbers = chunk([1, 2, 3, 4, 5], 2);
console.log(numbers.length, numbers[2][0] + 1);
const words = chunk(["a", "b", "c"], 2);
console.log(words[0].join("+"), words[1].join("+"));
const [label, count] = swap([7, "seven"]);
console.log(label.toUpperCase(), count + 1);
console.log(keep([5, 12, 8, 20], (v) => v > 9).join(","));

// A heterogeneous literal takes the union element type TypeScript proved.
const mixed = [1, "two", true];
console.log(mixed.length, mixed.includes("two"), mixed.includes(2), mixed.indexOf(true), mixed.join("|"));

// A typeof guard on a union-typed callback parameter narrows each branch.
const bumped = [1, "a", 3].map((v) => typeof v === "number" ? v + 1 : v + "!");
console.log(bumped.join(","));

// `flat` of a union element unwraps one array level per arm.
const nested = [1, [2, [3, [4]]]];
console.log(JSON.stringify(nested.flat()), JSON.stringify(nested.flat(2)), nested.flat(Infinity).length);
