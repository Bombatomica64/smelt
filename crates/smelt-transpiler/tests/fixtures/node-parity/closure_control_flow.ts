// Control flow inside closure bodies that the closure emitter must structure
// as loops, never as a run-time abort in the generated program.

// A loop whose back edge is the condition's false arm (do/while).
const sumUntil = (limit: number): number => {
  let total = 0;
  let i = 0;
  do {
    total += i;
    i++;
  } while (i < limit);
  return total;
};
console.log(sumUntil(5));

// An inner loop left by `break` straight into the outer loop's next turn,
// inside a closure: the path leaves the inner `loop` for the outer one.
const pairs = (n: number): string[] => {
  const out: string[] = [];
  for (let a = 0; a < n; a++) {
    let b = 0;
    while (true) {
      if (b > a) break;
      out.push(`${a}${b}`);
      b++;
    }
  }
  return out;
};
console.log(pairs(3).join(","));

// A try/catch around a throwing call in one arm of a branch inside a closure
// loop, the arms rejoining afterwards.
function risky(x: number): number {
  if (x % 2 === 1) throw new Error("odd " + x);
  return x * 10;
}
const collect = (xs: number[]): string => {
  const parts: string[] = [];
  for (const x of xs) {
    if (x > 2) {
      try {
        parts.push(String(risky(x)));
      } catch (e) {
        parts.push("caught");
      }
    } else {
      parts.push("small");
    }
    parts.push("|");
  }
  return parts.join("");
};
console.log(collect([1, 2, 3, 4]));

// `while (true)` left by `break` and by `return`.
const firstOver = (xs: number[], t: number): number => {
  let i = 0;
  while (true) {
    if (i >= xs.length) return -1;
    if (xs[i] > t) break;
    i++;
  }
  return xs[i];
};
console.log(firstOver([1, 5, 9], 4), firstOver([1], 4));

// A loop headed by a call: the header block ends in a call terminator.
function step(n: number): number {
  return n + 3;
}
const countSteps = (limit: number): number => {
  let n = 0;
  let steps = 0;
  for (;;) {
    n = step(n);
    steps++;
    if (n > limit) return steps;
  }
};
console.log(countSteps(10));
