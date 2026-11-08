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
