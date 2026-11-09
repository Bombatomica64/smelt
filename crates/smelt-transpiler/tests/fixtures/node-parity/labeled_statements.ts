// Labelled statements: `continue label` and `break label` across nested
// loops, a labelled block, a label crossing `finally`, and labels inside
// closure bodies.

function pairs(n: number): string {
  const out: string[] = [];
  outer: for (let a = 0; a < n; a++) {
    for (let b = 0; b < n; b++) {
      if (b > a) continue outer;
      out.push(`${a}${b}`);
    }
  }
  return out.join(",");
}
console.log(pairs(3));

function firstPair(grid: number[][], target: number): string {
  let found = "none";
  search: for (let row = 0; row < grid.length; row++) {
    for (let col = 0; col < grid[row].length; col++) {
      if (grid[row][col] === target) {
        found = `${row},${col}`;
        break search;
      }
    }
  }
  return found;
}
console.log(firstPair([[1, 2], [3, 4]], 3), firstPair([[1]], 9));

function labelledBlock(x: number): string {
  let note = "start";
  check: {
    if (x < 0) {
      note = "negative";
      break check;
    }
    note = "non-negative";
  }
  return note + "!";
}
console.log(labelledBlock(-1), labelledBlock(2));

function throughFinally(): string {
  const log: string[] = [];
  outer: while (true) {
    for (const x of [1, 2, 3]) {
      try {
        if (x === 2) break outer;
        log.push(`x${x}`);
      } finally {
        log.push(`f${x}`);
      }
    }
  }
  return log.join(" ");
}
console.log(throughFinally());

const inClosure = (rows: number[][]): number => {
  let total = 0;
  rows: for (const row of rows) {
    let i = 0;
    while (i < row.length) {
      if (row[i] < 0) continue rows;
      total += row[i];
      i++;
    }
  }
  return total;
};
console.log(inClosure([[1, 2], [3, -1, 100], [4]]));

let countdown = 0;
spin: do {
  countdown++;
  if (countdown < 3) continue spin;
  break spin;
} while (true);
console.log(countdown);

// `break outer` from two loops deep, and `continue outer` from inside a
// `switch` (whose own `break` only leaves the switch).
function grid(n: number): string {
  const out: string[] = [];
  outer: for (let a = 0; a < n; a++) {
    for (let b = 0; b < n; b++) {
      for (let c = 0; c < n; c++) {
        if (a + b + c === 4) break outer;
        switch (c) {
          case 1:
            continue outer;
          case 0:
            out.push(`${a}${b}`);
            break;
        }
      }
    }
  }
  return out.join(" ");
}
console.log(grid(3));

// `continue outer` that crosses a `finally`, and a labelled `for..of` over a
// Map whose iteration lowers through a prelude.
function scores(m: Map<string, number[]>): string {
  const log: string[] = [];
  people: for (const [name, xs] of m) {
    for (const x of xs) {
      try {
        if (x < 0) continue people;
        log.push(`${name}${x}`);
      } finally {
        log.push(".");
      }
    }
    log.push("|");
  }
  return log.join("");
}
console.log(scores(new Map([["a", [1, -1, 2]], ["b", [3]]])));

// An unlabelled `break` inside a labelled block leaves the enclosing loop,
// not the block; `break block` leaves only the block.
function blockInLoop(xs: number[]): string {
  const out: string[] = [];
  for (const x of xs) {
    block: {
      if (x === 2) break block;
      if (x === 4) break;
      out.push(String(x));
    }
    out.push(";");
  }
  return out.join("");
}
console.log(blockInLoop([1, 2, 3, 4, 5]));

// Labels inside a generator body.
function* evens(rows: number[][]): Generator<number, void, void> {
  row: for (const r of rows) {
    for (const v of r) {
      if (v < 0) continue row;
      if (v % 2 === 0) yield v;
    }
  }
}
console.log([...evens([[1, 2, -3, 4], [6, 7, 8]])].join(","));

// A labelled `while` whose body nests a `do`/`while`, left from the inner
// loop by `continue` of the outer label.
function nestedWhile(limit: number): number {
  let i = 0;
  let hits = 0;
  outer: while (i < limit) {
    i++;
    let j = 0;
    do {
      j++;
      if (j > i) continue outer;
      hits++;
    } while (j < 3);
  }
  return hits;
}
console.log(nestedWhile(5));
