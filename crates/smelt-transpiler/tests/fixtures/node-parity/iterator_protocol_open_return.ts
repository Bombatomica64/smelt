// An annotated `next(): IteratorResult<T>` leaves `TReturn` at `any`; the
// value the body's `done: true` steps carry is kept, as Node keeps it: read
// back from `next()` and printed with the step.
class Countdown implements Iterator<number> {
  constructor(private n: number) {}
  next(): IteratorResult<number> {
    if (this.n > 0) return { value: this.n--, done: false };
    return { value: -1, done: true };
  }
}

class Words implements IterableIterator<string> {
  private i = 0;
  constructor(private words: string[]) {}
  next(): IteratorResult<string> {
    return this.i < this.words.length
      ? { value: this.words[this.i++], done: false }
      : { value: this.words.length, done: true };
  }
  [Symbol.iterator](): IterableIterator<string> {
    return this;
  }
}

// All completions `undefined`: the open `TReturn` stays `undefined`.
class Plain implements Iterator<number> {
  private done = false;
  next(): IteratorResult<number> {
    if (this.done) return { value: undefined, done: true };
    this.done = true;
    return { value: 7, done: false };
  }
}

const c = new Countdown(2);
console.log(c.next(), c.next(), c.next());
const end = c.next();
if (end.done) console.log("returned", end.value);

const w = new Words(["a", "b"]);
const steps: string[] = [];
let step = w.next();
while (!step.done) {
  steps.push(step.value);
  step = w.next();
}
console.log(steps.join(","), step);
console.log([...new Words(["x", "y", "z"])].join("|"));

const p = new Plain();
console.log(p.next(), p.next());
