// Section E leftovers of docs/design/record-identity.md:
// - a typed function adapted to another function type is the SAME function
//   (`===`, its `prototype`);
// - `"prototype" in f` answers from the function value (a constructor owns
//   one, an arrow does not);
// - an erased class instance prints as Node prints it (`P { x: 1 }`).

function partial<A extends unknown[], R>(f: (...args: A) => R): (...args: A) => R { return f; }
function add(a: number, b: number): number { return a + b; }
const p = partial(add);
console.log(p === add, (p as any).prototype === (add as any).prototype, p(2, 3));
const h: (a: number, b: number) => number = add;
const u: unknown = add;
console.log(h === add, u === add, u === h);

function f(): void {}
const arrow = (): number => 1;
const expr = function (): number { return 2; };
class K {}
console.log("prototype" in f, "prototype" in arrow, "prototype" in expr, "prototype" in K);
console.log("name" in arrow, "length" in f, "missing" in f);
function hasPrototype(g: () => unknown): boolean { return "prototype" in g; }
console.log(hasPrototype(f), hasPrototype(arrow));

class P { constructor(public x: number) {} }
const erased: unknown = new P(1);
console.log(erased);
const mixed: unknown[] = [new P(2), { y: 1 }];
console.log(mixed);
