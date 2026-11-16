// An object literal's `get`/`set` define an accessor property: every read
// runs the getter and every write the setter, each with the object as `this`,
// whether the literal has a record type or not.

// Getter-only literals, a computed key, and a getter reading `this`.
const a = { get x(): number { return 42; } };           console.log(a.x);
const b = { get ["k"](): number { return 7; } };        console.log(b.k);
const c = { n: 2, get double(): number { return this.n * 2; } };
console.log(c.double + 1);

// A getter that captures a local, read before and after the local changes.
function makeCounter() {
  let value = 0;
  return { bump: () => { value += 1; }, get current(): number { return value; } };
}
const counter = makeCounter();
console.log(counter.current);
counter.bump();
counter.bump();
console.log(counter.current);

// `this` sees later writes to the object.
const box = { n: 2, get double(): number { return this.n * 2; } };
box.n = 5;
console.log(box.double);

// A getter is a property: keys, JSON and spread see its current value.
const shape = { w: 3, h: 4, get area(): number { return this.w * this.h; } };
console.log(Object.keys(shape).join(","));
const copy = { ...shape };
shape.w = 10;
console.log(copy.area, shape.area);

// A string getter, a getter next to a method, a nested literal's getter.
const person = {
  first: "Ada",
  last: "Lovelace",
  get full(): string { return `${this.first} ${this.last}`; },
  greet(): string { return "hi " + this.full; },
  inner: { get answer(): number { return 42; } },
};
console.log(person.full, person.greet(), person.inner.answer);

// A getter runs on every read.
let reads = 0;
const tick = { get next(): number { reads += 1; return reads; } };
console.log(tick.next, tick.next, tick.next);

const shape2 = { w: 3, get area(): number { return this.w * 2; } };
console.log(shape2);
console.log(Object.values(shape2).join(","), Object.entries(shape2).length);
for (const k in shape2) console.log(k);
const { area } = shape2;
console.log(area);

const temp = {
  celsius: 0,
  get fahrenheit(): number { return this.celsius * 9 / 5 + 32; },
  set fahrenheit(value: number) { this.celsius = (value - 32) * 5 / 9; },
};
temp.fahrenheit = 212;
console.log(temp.celsius, temp.fahrenheit);

const log: string[] = [];
const acc = {
  _v: 1,
  get both(): number { return this._v; },
  set both(value: number) { log.push("set " + value); this._v = value; },
  get onlyGet(): number { return 7; },
  set onlySet(value: number) { log.push("onlySet " + value); },
};
console.log(acc);
acc.both = 5;
const key: string = "both";
(acc as Record<string, unknown>)[key] = 9;
console.log(acc.both, acc._v, String(acc.onlySet));
acc.onlySet = 3;
console.log(log.join("|"));
// A Set stored beside accessors is a Set, not an accessor: it reads back, has
// its size and prints as a Set.
const holder = {
  tags: new Set(["a", "b"]),
  get count(): number { return this.tags.size; },
};
console.log(holder.tags.size, holder.count, holder);
