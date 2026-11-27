// Loops whose body leaves them other than through their exit, in sequence:
// every path still runs the same code in the same order once their shared
// continuation is emitted once.

function* counted(label: string, limit: number): Generator<number> {
  try {
    for (let n = 1; n <= limit; n++) {
      yield n;
    }
  } finally {
    console.log(`${label} closed`);
  }
}

function* failing(after: number): Generator<number> {
  for (let n = 1; ; n++) {
    if (n > after) {
      throw new Error(`failed after ${after}`);
    }
    yield n;
  }
}

function risky(value: number): number {
  if (value > 4) {
    throw new Error(`risky ${value}`);
  }
  return value;
}

function sequentialForOf(): number {
  let total = 0;
  for (const n of counted("a", 2)) { total += n; }
  for (const n of counted("b", 3)) { if (n === 2) { break; } total += n * 10; }
  for (const n of counted("c", 2)) { total += n * 100; }
  for (const n of counted("d", 1)) { total += n * 1000; }
  return total;
}

function throwingNext(): string {
  let seen = "";
  for (const n of counted("e", 1)) { seen += n; }
  try {
    for (const n of failing(2)) { seen += n; }
  } catch (error) {
    seen += "!";
  }
  for (const n of counted("f", 2)) { seen += n; }
  return seen;
}

function earlyReturn(stop: number): string {
  let seen = "";
  for (const n of counted("g", 3)) { seen += n; }
  for (const n of counted("h", 3)) { if (n === stop) { return `${seen} returned at ${n}`; } seen += n; }
  for (const n of counted("i", 1)) { seen += n; }
  return `${seen} done`;
}

function whileInTryFinally(start: number): string {
  let log = "";
  let i = start;
  for (let round = 0; round < 3; round++) {
    try {
      while (i < 6) { i++; log += risky(i); }
      log += "+";
    } catch (error) {
      log += "x";
    } finally {
      log += ";";
    }
    i = start + round + 2;
  }
  return log;
}

function nestedInOuterLoop(): number {
  let total = 0;
  for (let round = 1; round <= 2; round++) {
    for (const n of counted("j", 2)) { total += n * round; }
    for (const n of counted("k", 2)) { if (n === 2) { break; } total += 10 * round; }
  }
  return total;
}

function inClosure(): number {
  const sum = (limit: number): number => {
    let total = 0;
    for (const n of counted("l", limit)) { total += n; }
    for (const n of counted("m", limit)) { total += n * 2; }
    for (const n of counted("n", limit)) { total += n * 3; }
    return total;
  };
  return sum(2) + sum(1);
}

function conditionForms(limit: number): string {
  let log = "";
  let i = 0;
  for (let round = 0; round < 2; round++) {
    try {
      while (i < limit && log.length < 50) { i++; log += risky(i); }
      log += "+";
    } catch (error) {
      log += "x";
    } finally {
      log += ";";
    }
    try {
      while (risky(i) < limit) { i++; log += i; }
      log += "+";
    } catch (error) {
      log += "x";
    } finally {
      log += ";";
    }
    i = round;
  }
  return log;
}

console.log(sequentialForOf());
console.log(throwingNext());
console.log(earlyReturn(2));
console.log(earlyReturn(9));
console.log(whileInTryFinally(0));
console.log(whileInTryFinally(6));
console.log(nestedInOuterLoop());
console.log(inClosure());
console.log(conditionForms(3));
console.log(conditionForms(9));
