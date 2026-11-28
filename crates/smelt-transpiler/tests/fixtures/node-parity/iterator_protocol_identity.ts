// A class that is its own iterator, adapted at its annotated
// `[Symbol.iterator](): IterableIterator<T>`, is still that object:
// `obj[Symbol.iterator]() === obj`, two adapters of it are `===`, and they
// share its state. A different instance and a generator are not it.
class Ticks implements IterableIterator<number> {
  private n = 0;
  constructor(private limit: number) {}
  next(): IteratorResult<number> {
    return this.n < this.limit ? { value: this.n++, done: false } : { value: undefined, done: true };
  }
  [Symbol.iterator](): IterableIterator<number> {
    return this;
  }
}

function* counter(): Generator<number> {
  yield 1;
}

const t = new Ticks(4);
const other = new Ticks(4);
const it = t[Symbol.iterator]();
console.log(it === t, t === it, it !== t);
console.log(it === t[Symbol.iterator](), it === other[Symbol.iterator]());
console.log(it.next().value, t.next().value, it.next().value);
for (const v of t) console.log("rest", v);
const g = counter();
console.log(g[Symbol.iterator]() === g, g === counter());
