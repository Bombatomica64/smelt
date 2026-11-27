// A class whose `[Symbol.iterator]()` answers a hand-written iterator object
// literal: `next` and `return` closures over local state. Every consumer
// drives `next()` lazily and calls `return()` exactly when it leaves early.
class Countdown {
  constructor(private from: number) {}
  [Symbol.iterator](): Iterator<number> {
    let n = this.from;
    return {
      next: () => (n > 0 ? { value: n--, done: false } : { value: 0, done: true }),
      return: () => {
        console.log("closed at", n);
        return { value: 0, done: true };
      },
    };
  }
}

function loops(): void {
  for (const v of new Countdown(3)) console.log("for", v);
  for (const v of new Countdown(5)) {
    if (v < 4) break;
    console.log("break", v);
  }
}

function firstOf(c: Countdown): number {
  for (const v of c) return v;
  return -1;
}

function throwing(): void {
  try {
    for (const v of new Countdown(2)) throw new Error("boom " + v);
  } catch (e) {
    console.log((e as Error).message);
  }
}

function collected(): void {
  console.log([...new Countdown(4)].join(","));
  console.log(Array.from(new Countdown(2)).join(","));
  const [a, b] = new Countdown(9);
  console.log("destructured", a, b);
  const [p, ...rest] = new Countdown(3);
  console.log(p, rest.join("|"));
}

// A `next()` that throws does not close the iterator.
class Faulty {
  [Symbol.iterator](): Iterator<number> {
    let k = 0;
    return {
      next: () => {
        if (k === 2) throw new Error("next failed");
        return { value: k++, done: false };
      },
      return: () => {
        console.log("faulty closed");
        return { value: undefined, done: true };
      },
    };
  }
}

function faulty(): void {
  try {
    for (const v of new Faulty()) console.log("faulty", v);
  } catch (e) {
    console.log((e as Error).message);
  }
}

// A step whose `done` is computed.
class Evens {
  constructor(private limit: number) {}
  [Symbol.iterator](): Iterator<number> {
    let k = 0;
    const limit = this.limit;
    return {
      next: () => {
        k += 2;
        return { value: k, done: k > limit };
      },
    };
  }
}

loops();
console.log("first", firstOf(new Countdown(7)));
throwing();
collected();
faulty();
console.log([...new Evens(6)].join(" "));
