// `console.log` of a container of `T` prints what Node prints, erased or generic.
function wrap<T>(x: T): T[] { const out = [x]; console.log(out); return out; }
wrap(3); wrap("s"); wrap(true); wrap([1, "a"]); wrap({ a: 1 }); wrap(null); wrap(undefined);
function nest<T>(x: T): { v: T; l: T[] } { const o = { v: x, l: [x, x] }; console.log(o); return o; }
nest(1.5);
const u: unknown[] = [1, "a", [2]];
console.log(u);
function pairOf<T>(a: T, b: T): void { console.log([a, b], [[a]], new Set([a])); }
pairOf(1, 2); pairOf("x", "y");
class Box<T> { items: T[] = []; add(x: T): this { this.items.push(x); console.log(this.items); return this; } }
new Box<number>().add(1).add(2);
function keep<T extends number>(x: T): T[] { const l = [x, x]; console.log(l); return l; }
keep(2);
