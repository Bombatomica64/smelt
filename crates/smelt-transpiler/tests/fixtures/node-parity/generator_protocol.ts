// Generators: a finished generator answers `{ value: undefined, done: true }`,
// leaving a for...of early (break, return, throw) runs the generator's
// pending `finally`, `.next(v)` sends `v` at the type its `yield` is used at,
// and an object iterates through its `*[Symbol.iterator]()` generator.
function* gen(): Generator<number, string | undefined, number> { const x = yield 1; yield x; return "done"; }
const g = gen(); console.log(g.next(), g.next(42), g.next(), g.next());
function* plain() { yield 1; }
const p = plain(); console.log(p.next(), p.next(), p.next());
function* g2(): Generator<number> { try { yield 1; yield 2; } finally { console.log("g2 cleanup"); } }
for (const v of g2()) { console.log("v", v); break; }
function find(): number { for (const v of g2()) { if (v === 2) return v; } return -1; }
console.log("found", find());
try { for (const v of g2()) { console.log("v", v); throw new Error("stop"); } } catch (e) { console.log("caught", (e as Error).message); }
for (const v of g2()) { console.log("all", v); }
function* counted(): Generator<number> { let i = 0; try { while (true) yield i++; } finally { console.log("closed at", i); } }
const firstThree: number[] = [];
for (const n of counted()) { if (n >= 3) break; firstThree.push(n); }
console.log(firstThree.join(","));
function* genNext(n: number) { const r: number | undefined = yield n; return r ?? 0; }
const gn = genNext(3); console.log(gn.next().value, gn.next(5).value);
function* echo() { const got: string | undefined = yield 1; console.log("got", got); }
const e = echo(); e.next(); e.next("hello");
function* acc() { let total = 0; while (true) { const add: number = yield total; total += add; } }
const a = acc(); a.next(); a.next(2); console.log(a.next(3).value);
class Range implements Iterable<number> {
  lo: number; hi: number;
  constructor(lo: number, hi: number) { this.lo = lo; this.hi = hi; }
  *[Symbol.iterator](): Iterator<number> { for (let i = this.lo; i < this.hi; i++) yield i; }
}
console.log([...new Range(2, 5)].join(","));
for (const v of new Range(0, 2)) console.log(v);
console.log(Array.from(new Range(1, 4)).join("|"), [0, ...new Range(5, 7)].join(","));
