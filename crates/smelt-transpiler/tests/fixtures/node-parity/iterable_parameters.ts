// A parameter typed `Iterable<T>` receives the iterable itself, iterated
// lazily: an infinite generator works, the producer's side effects interleave
// with the consumer's as in Node, and iterating the parameter again starts a
// new iteration (an array restarts, a generator stays exhausted). Arrays,
// sets, maps, strings, generators, classes with `[Symbol.iterator]()` and
// other iterables all pass, and every consumer reads it.
function take(xs: Iterable<number>, n: number): number[] {
  const out: number[] = [];
  if (n <= 0) return out;
  for (const x of xs) {
    console.log("take got", x);
    out.push(x);
    if (out.length >= n) break;
  }
  return out;
}

function* naturals(): Generator<number> {
  let i = 0;
  try {
    while (true) {
      console.log("produce", i);
      yield i++;
    }
  } finally {
    console.log("naturals closed");
  }
}

function twice(xs: Iterable<string>): string {
  let first = "";
  for (const x of xs) first += x;
  let second = "";
  for (const x of xs) second += x;
  return first + "|" + second;
}

function consumers(xs: Iterable<number>): void {
  console.log([...xs].join(","));
  console.log(Array.from(xs).length, new Set(xs).size);
  const [a, b] = xs;
  console.log(a, b);
}

class Countdown {
  constructor(private from: number) {}
  *[Symbol.iterator](): Iterator<number> {
    for (let i = this.from; i > 0; i--) yield i;
  }
}

function sumOf(xs: Iterable<number>): number {
  let total = 0;
  for (const x of xs) total += x;
  return total;
}

// An iterable returned, kept, and passed on.
function evens(limit: number): Iterable<number> {
  const out: number[] = [];
  for (let i = 0; i < limit; i += 2) out.push(i);
  return out;
}
function forward(xs: Iterable<number>): number {
  return sumOf(xs);
}

function* delegate(xs: Iterable<number>): Generator<number> {
  yield* xs;
}

function pairs(entries: Iterable<[string, number]>): string {
  const parts: string[] = [];
  for (const [k, v] of entries) parts.push(k + "=" + v);
  return parts.join("&");
}

console.log(take(naturals(), 3).join(","));
console.log(twice(["a", "b"]), twice(new Set(["x", "y"])), twice("hé"));
const gen = (function* (): Generator<string> {
  yield "g";
  yield "h";
})();
console.log(twice(gen));
consumers([4, 5, 6]);
consumers(new Set([7, 8]));
console.log(sumOf(new Countdown(4)), sumOf([1, 2, 3]));
const kept = evens(7);
console.log(sumOf(kept), sumOf(kept), forward(kept));
console.log([...delegate(new Set([9, 10]))].join(","));
console.log(pairs(new Map([["a", 1], ["b", 2]])), pairs([["c", 3]]));
const live = [1, 2];
let pushed = false;
for (const x of take(live, 0)) console.log(x);
console.log(
  take(
    (function* (): Generator<number> {
      for (const x of live) {
        if (!pushed) {
          pushed = true;
          live.push(3);
        }
        yield x;
      }
    })(),
    5,
  ).join(","),
);
console.log(sumOf(new Uint8Array([200, 55, 1])), sumOf(new Float64Array([0.5, 0.25])));
