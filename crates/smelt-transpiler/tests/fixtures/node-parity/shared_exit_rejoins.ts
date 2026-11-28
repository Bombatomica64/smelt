// Branches whose arms share a `return`/`throw` before they meet, in sequence:
// a `switch` inside a `try` whose two calling `case`s throw into one `catch`
// that returns (or rethrows), in a function, a loop body and a closure. The
// shared exit is emitted inside each arm that reaches it and the code after
// the `switch` once; every path still runs the same code in the same order.

function risky(value: number, limit: number): number {
  if (value > limit) {
    throw new Error(`risky ${value}`);
  }
  return value + 1;
}

function returningCatches(seed: number): number {
  let total = seed;
  try { switch (total % 3) { case 0: total = risky(total, 30); break; case 1: total = risky(total + 1, 30); break; default: total -= 1; } } catch (e) { return -1; }
  try { switch (total % 3) { case 0: total = risky(total, 40); break; case 1: total = risky(total + 2, 40); break; default: total -= 2; } } catch (e) { return -2; }
  try { switch (total % 3) { case 0: total = risky(total * 2, 20); break; case 1: total = risky(total + 3, 20); break; default: total += 3; } } catch (e) { return -3; }
  return total * 7;
}

function rethrowingCatches(seed: number): number {
  let total = seed;
  try { switch (total % 2) { case 0: total = risky(total, 20); break; default: total = risky(total + 5, 20); } } catch (e) { throw new Error(`first ${(e as Error).message}`); }
  try { switch (total % 3) { case 0: total = risky(total, 12); break; case 1: total = risky(total, 12) + 1; break; default: total += 4; } } catch (e) { throw new Error(`second ${(e as Error).message}`); }
  return total + 100;
}

function inLoop(rounds: number, limit: number): string {
  const log: string[] = [];
  let total = 1;
  for (let round = 0; round < rounds; round++) {
    try { switch (total % 3) { case 0: total = risky(total, limit); break; case 1: total = risky(total + round, limit); break; default: total += 2; } } catch (e) { return `${log.join(",")}|caught ${round}`; }
    try { switch (total % 2) { case 0: total = risky(total, limit); break; default: total = risky(total + 1, limit); } } catch (e) { return `${log.join(",")}|second ${round}`; }
    log.push(`${round}:${total}`);
  }
  return log.join(",");
}

const viaClosure = (seed: number): number => {
  let total = seed;
  try { switch (total % 3) { case 0: total = risky(total, 35); break; case 1: total = risky(total + 4, 35); break; default: total -= 1; } } catch (e) { return -10; }
  try { switch (total % 3) { case 0: total = risky(total, 45); break; case 1: total = risky(total * 3, 45); break; default: total -= 3; } } catch (e) { return -20; }
  return total * 11;
};

function attempt(label: string, run: () => number | string): void {
  try {
    console.log(label, run());
  } catch (error) {
    console.log(label, "threw", (error as Error).message);
  }
}

for (const seed of [0, 1, 2, 3, 4, 5, 7, 9, 15, 16, 17, 31, 33, 34, 41, 44]) {
  attempt(`returningCatches(${seed})`, () => returningCatches(seed));
  attempt(`rethrowingCatches(${seed})`, () => rethrowingCatches(seed));
  attempt(`viaClosure(${seed})`, () => viaClosure(seed));
}
attempt("inLoop(6, 100)", () => inLoop(6, 100));
attempt("inLoop(8, 12)", () => inLoop(8, 12));
attempt("inLoop(8, 8)", () => inLoop(8, 8));
attempt("inLoop(8, 9)", () => inLoop(8, 9));
