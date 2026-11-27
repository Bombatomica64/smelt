// Unannotated protocol methods take the iterator type the TypeScript checker
// infers for them: `[Symbol.iterator]()` answering an object literal
// `{ next, return }`, `next()` answering step literals, and
// `[Symbol.iterator]() { return this; }`.

class Countdown {
  from: number;
  constructor(from: number) {
    this.from = from;
  }
  [Symbol.iterator]() {
    let n = this.from;
    return {
      next: () => (n > 0 ? { value: n--, done: false } : { value: 0, done: true }),
      return: () => {
        console.log("countdown closed");
        return { value: -1, done: true };
      },
    };
  }
}

class Range {
  private i = 0;
  end: number;
  constructor(end: number) {
    this.end = end;
  }
  next() {
    if (this.i < this.end) {
      return { value: this.i++ * 10, done: false };
    }
    return { value: -1, done: true };
  }
  [Symbol.iterator]() {
    return this;
  }
}

class Words {
  text: string;
  constructor(text: string) {
    this.text = text;
  }
  [Symbol.iterator]() {
    const parts = this.text.split(" ");
    let index = 0;
    return {
      next: () =>
        index < parts.length
          ? { value: parts[index++].toUpperCase(), done: false }
          : { value: undefined, done: true },
    };
  }
}

for (const v of new Countdown(4)) {
  if (v < 2) break;
  console.log("countdown", v);
}
console.log([...new Countdown(2)]);
const [first, second] = new Countdown(9);
console.log(first, second);

console.log(Array.from(new Range(3)));
const range = new Range(1);
const step = range.next();
console.log(step.value, step.done);
const last = range.next();
console.log(last.value, last.done);
console.log(range[Symbol.iterator]() === range);

console.log([...new Words("lazy iterators work")].join("-"));
console.log(new Set(new Words("a b a")).size);
