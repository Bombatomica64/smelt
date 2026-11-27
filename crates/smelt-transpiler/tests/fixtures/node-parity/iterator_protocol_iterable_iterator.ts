// Classes that are their own iterator (`implements IterableIterator<T>`,
// `[Symbol.iterator]() { return this; }`), driven by every consumer, by user
// code through `next()`, and passed where `Iterator<T>` or `Iterable<T>` is
// expected. The element type stays the declared one, `T` included.
class Range implements IterableIterator<number> {
  private i: number;
  constructor(lo: number, private hi: number) {
    this.i = lo;
  }
  next(): IteratorResult<number> {
    if (this.i < this.hi) return { value: this.i++, done: false };
    return { value: undefined, done: true };
  }
  [Symbol.iterator](): IterableIterator<number> {
    return this;
  }
}

class ListIter<T> implements IterableIterator<T> {
  private i = 0;
  constructor(private items: T[]) {}
  next(): IteratorResult<T> {
    return this.i < this.items.length
      ? { value: this.items[this.i++], done: false }
      : { value: undefined, done: true };
  }
  return(): IteratorResult<T> {
    console.log("ListIter closed at", this.i);
    return { value: undefined, done: true };
  }
  [Symbol.iterator](): IterableIterator<T> {
    return this;
  }
}

function drain(it: Iterator<number>): number[] {
  const out: number[] = [];
  let step = it.next();
  while (!step.done) {
    out.push(step.value);
    step = it.next();
  }
  return out;
}

function sum(xs: Iterable<number>): number {
  let total = 0;
  for (const x of xs) total += x;
  return total;
}

function* numbers(): Generator<number> {
  try {
    yield 1;
    yield 2;
  } finally {
    console.log("generator finally");
  }
}

// A `Generator<number>` where an `IterableIterator<number>` is expected,
// left early: its `finally` still runs.
function firstOf(it: IterableIterator<number>): number {
  for (const v of it) return v;
  return -1;
}

function consumers(): void {
  console.log(Array.from(new Range(2, 6)).join(","));
  for (const w of new ListIter(["p", "q", "r"])) {
    if (w === "q") break;
    console.log(w);
  }
  console.log([...new ListIter([true, false])].join("/"));
}

function manual(): void {
  const r = new Range(0, 2);
  const s1 = r.next();
  console.log(s1.done, s1.value, r.next().value, r.next().done);
  const it: Iterator<number> = new Range(5, 7)[Symbol.iterator]();
  const s2: IteratorResult<number> = it.next();
  console.log(s2.done, s2.value);
  console.log(drain(new Range(3, 5)).join(","), drain(numbers()).join(","));
  console.log("first", firstOf(numbers()));
}

function collections(): void {
  console.log(new Set(new Range(1, 4)).size, sum(new Range(1, 5)));
  const m = new Map(new ListIter<[string, number]>([["a", 1], ["b", 2]]));
  console.log(m.get("b"));
  const [x, y] = new ListIter([10, 20, 30]);
  console.log(x, y);
}

consumers();
manual();
collections();
