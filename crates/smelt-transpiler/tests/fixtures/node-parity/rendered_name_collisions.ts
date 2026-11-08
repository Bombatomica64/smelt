// Names that render alike stay distinct: a static and an instance member of
// one name, a module whose body function is named after it next to its own
// `isEven`, and a property named `_`.
import { isEven } from "./is-even.ts";
const key = Symbol.for("demo/key");
class Counter {
  constructor(public n: number) {}
  static describe(): string { return "Counter"; }
  describe(): string { return `Counter(${this.n})`; }
  static [key](): string { return "static key"; }
  [key](): number { return this.n; }
}
console.log(Counter.describe(), new Counter(2).describe(), new Counter(3)[key]());
console.log(isEven(4), isEven(5));
const wild = { _: 1, a: 2 };
const nested: { _: { _: number } } = { _: { _: 7 } };
console.log(wild._, wild.a, nested._._, JSON.stringify(wild), JSON.stringify(nested));
interface Deep { _: number; label: string }
const deep: Deep = { _: 4, label: "deep" };
const bump = (value: Deep): Deep => ({ _: value._ + 1, label: value.label });
console.log(bump(deep)._, bump(deep).label, JSON.stringify(bump(deep)));
