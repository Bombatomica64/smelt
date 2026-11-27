// Branches whose arms meet again only transitively, in sequence: a `switch`
// whose cases call, throw or return, a `try` in a loop body, an `if`/`else`
// holding a call inside a `try`, an `if` whose arm may throw. Their shared
// continuation is emitted once; every path still runs the same code in the
// same order.

function risky(value: number): number {
  if (value > 50) {
    throw new Error(`risky ${value}`);
  }
  return value + 1;
}

function switches(seed: number): number {
  let total = seed;
  switch (total % 3) { case 0: total = risky(total); break; case 1: total += 10; break; default: total -= 1; }
  switch (total % 3) { case 0: total = risky(total); break; case 1: total += 20; break; default: total -= 2; }
  switch (total % 3) { case 0: total = risky(total); break; case 1: total += 30; break; default: total -= 3; }
  switch (total % 4) { case 0: total = risky(total) * 2; break; case 3: total += 7; default: total += 1; }
  return total * 3;
}

function switchesThatLeave(seed: number): number {
  let total = seed;
  switch (total % 3) { case 0: total = risky(total); break; case 2: return -1; default: total -= 1; }
  switch (total % 3) {
    case 0:
      if (total > 40) { throw new Error(`too big ${total}`); }
      total += 2;
      break;
    case 1: return -2;
    default: total -= 1;
  }
  switch (total % 2) { case 0: total = risky(total + 1); break; default: total += 5; }
  return total;
}

function switchesInLoop(rounds: number): string {
  const log: string[] = [];
  for (let round = 0; round < rounds; round++) {
    switch (round % 3) { case 0: log.push(`a${risky(round)}`); break; case 1: log.push("b"); break; default: continue; }
    switch (round % 2) { case 0: log.push(`c${risky(round * 2)}`); break; default: if (round > 6) { break; } log.push("d"); }
    log.push(`r${round}`);
  }
  return log.join(",");
}

function trysInLoop(values: number[]): string {
  const log: string[] = [];
  for (const value of values) {
    try { log.push(`t${risky(value)}`); } catch (error) { log.push("caught"); }
    try { log.push(`u${risky(value * 2)}`); } finally { console.log("finally", value, log.length); }
    log.push(`v${value}`);
  }
  return log.join(",");
}

function ifElseInTry(seed: number): number {
  let total = seed;
  try { if (total % 3 === 0) { total = risky(total); } else if (total % 3 === 1) { total = risky(total + 1); } else { total += 1; } } catch (error) { total -= 100; }
  try { if (total % 2 === 0) { total = risky(total * 2); } else { total += 3; } } catch (error) { total -= 1000; }
  return total;
}

function ifMayThrow(seed: number): number {
  let total = seed;
  if (total % 2 === 0) { if (total > 30) { throw new Error(`even ${total}`); } total += 1; } else { total -= 1; }
  if (total % 3 === 0) { if (total > 30) { throw new Error(`third ${total}`); } total += 4; } else { total += 2; }
  return total;
}

const viaClosure = (seed: number): number => {
  let total = seed;
  switch (total % 3) { case 0: total = risky(total); break; case 1: return -9; default: total += 1; }
  switch (total % 2) { case 0: total = risky(total); break; default: total -= 1; }
  return total * 5;
};

function attempt(label: string, run: () => number | string): void {
  try {
    console.log(label, run());
  } catch (error) {
    console.log(label, "threw", (error as Error).message);
  }
}

for (const seed of [0, 1, 2, 3, 5, 9, 40, 49, 60]) {
  attempt(`switches(${seed})`, () => switches(seed));
  attempt(`switchesThatLeave(${seed})`, () => switchesThatLeave(seed));
  attempt(`ifElseInTry(${seed})`, () => ifElseInTry(seed));
  attempt(`ifMayThrow(${seed})`, () => ifMayThrow(seed));
  attempt(`viaClosure(${seed})`, () => viaClosure(seed));
}
attempt("switchesInLoop(9)", () => switchesInLoop(9));
attempt("switchesInLoop(40)", () => switchesInLoop(40));
attempt("trysInLoop", () => trysInLoop([1, 20, 30, 60]));
