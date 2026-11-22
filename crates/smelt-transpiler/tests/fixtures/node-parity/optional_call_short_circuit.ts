// An optional call short-circuits before its arguments: when the callee is
// nullish, `f?.(args)` is `undefined` and no argument expression runs.

const log: string[] = [];
function arg(label: string, value: number): number {
  log.push(label);
  return value;
}

interface Hooks {
  onValue?: (n: number) => number;
  onPair?: (a: number, b: number) => string;
}

const none: Hooks = {};
const some: Hooks = {
  onValue: (n) => n * 2,
  onPair: (a, b) => a + ":" + b,
};

console.log(none.onValue?.(arg("absent value", 1)));
console.log(none.onPair?.(arg("absent a", 1), arg("absent b", 2)));
console.log(log.length);

console.log(some.onValue?.(arg("present value", 21)));
console.log(some.onPair?.(arg("present a", 3), arg("present b", 4)));
console.log(log.join(","));

// A callee held in a local, and arguments that are plain reads.
let maybe: ((s: string) => string) | undefined;
const word = "x";
console.log(maybe?.(word), maybe?.(word + arg("concat", 0)));
maybe = (s) => s.toUpperCase();
console.log(maybe?.(word), log.length);
