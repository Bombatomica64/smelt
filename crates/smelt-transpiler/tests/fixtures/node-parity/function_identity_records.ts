// A function used at an object type is still that one function: the record
// Smelt builds from it carries the function's identity, so `===` and
// `Object.is` agree with Node between the function and every record built
// from it, and records built from different functions stay distinct.
interface Named {
  name: string;
  length: number;
}
interface Labelled {
  name: string;
  label?: string;
}
function hello(a: number, b: number): number {
  return a + b;
}
const arrow = (x: number): number => x;

const n: Named = hello;
const n2: Named = hello;
const m: Named = arrow;
console.log(n === hello, hello === n, n !== hello, Object.is(n, hello), Object.is(hello, n2));
console.log(n === n2, n !== n2, n === m, m === arrow, m === hello, arrow === m);

// Through a parameter, a return value and a list.
function same(record: Named, f: (a: number, b: number) => number): boolean {
  return record === f;
}
console.log(same(hello, hello), same(hello, (a, b) => a * b));
function asNamed(): Named {
  return hello;
}
console.log(asNamed() === hello, asNamed() === n, [m, n].indexOf(n2), [m, n].includes(n2));
const labelled: Labelled = hello;
console.log(labelled === hello, labelled.name, labelled.label === undefined);

// Closures made and dropped one after another: each record is its own function.
const handlers: Named[] = [(x: number) => x, (x: number) => x + 1, (x: number) => x + 2];
console.log(handlers[0] === handlers[1], handlers[1] === handlers[2], handlers[0] === handlers[0]);
const makers = [1, 2].map((k) => (x: number): number => x + k);
const first: Named = makers[0];
console.log(first === makers[0], first === makers[1], first.name, first.length);

// A record of the same shape built from an object literal is no function.
const plain: Named = { name: "hello", length: 2 };
console.log(plain === n, plain === hello, plain === plain, plain.name === n.name);
console.log(hello(1, 2), arrow(3));
