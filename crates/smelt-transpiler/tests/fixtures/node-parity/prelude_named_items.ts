// Source items named like Rust prelude items keep their own meaning, and the
// generated crate keeps the prelude's (`dyn Fn(..)`, `impl Clone for ..`).
interface Fn { input: unknown; output: unknown }
type Default = { label: string; count: number };
class Iterator {
  constructor(private items: number[], private at = 0) {}
  next(): number | undefined { return this.items[this.at++]; }
}
class Clone {
  constructor(public value: string) {}
  copy(): Clone { return new Clone(this.value + "'"); }
}
function drop(xs: number[], n: number): number[] { return xs.slice(n); }
const fn: Fn = { input: 1, output: "one" };
const defaults: Default = { label: "x", count: 2 };
const it = new Iterator([3, 4]);
const twice = (f: (x: number) => number) => (x: number) => f(f(x));
console.log(fn.output, defaults.label, defaults.count, it.next(), it.next(), it.next());
console.log(new Clone("a").copy().value, drop([1, 2, 3], 1), twice((x) => x + 1)(0));
const record = { drop: 1, Clone: "c", Fn: true };
console.log(record.drop, record.Clone, record.Fn, JSON.stringify(record));
