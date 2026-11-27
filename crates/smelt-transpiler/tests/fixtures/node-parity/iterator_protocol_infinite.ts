// An infinite hand-written iterator: every consumer that stops early
// (`break`, a destructuring that binds a fixed count) asks only for the steps
// it uses, then closes the iterator.
class Naturals {
  [Symbol.iterator](): Iterator<number> {
    let k = 0;
    return {
      next: () => ({ value: k++, done: false }),
      return: () => {
        console.log("stopped after", k);
        return { value: undefined, done: true };
      },
    };
  }
}

function total(limit: number): number {
  let sum = 0;
  for (const k of new Naturals()) {
    if (k > limit) break;
    sum += k;
  }
  return sum;
}

function firstThree(): string {
  const [a, b, c] = new Naturals();
  return [a, b, c].join(",");
}

console.log("total", total(100));
console.log("first three", firstThree());
