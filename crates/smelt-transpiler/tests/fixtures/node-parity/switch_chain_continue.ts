// `continue` inside a `switch` lowered as a `===` chain (fallthrough, or
// labels that are not literals) belongs to the enclosing loop, at any depth.

const Kind = { A: 1, B: 2, C: 3 } as const;

function nestedContinueInFallthrough(values: number[]): string {
  const out: string[] = [];
  for (let i = 0; i < values.length; i++) {
    const v = values[i];
    switch (v % 4) {
      case 0:
        if (v > 10) continue;
        out.push("zero");
      case 1:
        out.push("one");
        break;
      case 2:
        out.push("two");
        if (v === 6) {
          continue;
        }
      default:
        out.push("other");
    }
    out.push(`end${i}`);
  }
  return out.join(",");
}

function topLevelContinueInFallthrough(limit: number): string {
  const out: string[] = [];
  let i = 0;
  while (i < limit) {
    i++;
    switch (i % 3) {
      case 0:
        out.push("a");
      case 1:
        out.push("b");
        continue;
      default:
        out.push("c");
    }
    out.push(`after${i}`);
  }
  return out.join(",");
}

function continueInTryInsideChain(kinds: number[]): string {
  const out: string[] = [];
  let k = 0;
  do {
    const kind = kinds[k];
    k++;
    switch (kind) {
      case Kind.A:
        try {
          if (k % 2 === 0) continue;
          out.push("A");
        } finally {
          out.push("fin");
        }
        break;
      case Kind.B:
        out.push("B");
      case Kind.C:
        out.push("C");
        break;
    }
    out.push(`k${k}`);
  } while (k < kinds.length);
  return out.join(",");
}

function labelledAndInnerLoops(rows: number[][]): string {
  const out: string[] = [];
  outer: for (const row of rows) {
    for (const cell of row) {
      switch (cell) {
        case 0:
          out.push("z");
        case 1:
          for (let j = 0; j < 3; j++) {
            if (j === 1) continue;
            if (j === 2) break;
            out.push(`j${j}`);
          }
          if (cell === 1) continue;
          out.push("one");
          break;
        case 2:
          if (row.length > 2) continue outer;
          out.push("two");
        case 3:
          if (row.length === 1) break outer;
          out.push("three");
          break;
        default:
          switch (cell % 2) {
            case 0:
              out.push("even");
            case 1:
              if (cell > 8) continue;
              out.push("odd");
          }
      }
      out.push(`c${cell}`);
    }
    out.push("row");
  }
  return out.join(",");
}

function nestedBreakStaysInSwitch(values: number[]): string {
  const out: string[] = [];
  for (const v of values) {
    switch (v) {
      case 1:
        if (v > 0) {
          out.push("first");
          break;
        }
      case 2:
        out.push("second");
      default:
        out.push("default");
    }
    out.push(`v${v}`);
  }
  return out.join(",");
}

console.log(nestedContinueInFallthrough([0, 12, 1, 2, 6, 3, 5]));
console.log(topLevelContinueInFallthrough(6));
console.log(continueInTryInsideChain([1, 1, 2, 3, 1]));
console.log(labelledAndInnerLoops([[0, 1, 4, 9], [2, 3, 5], [3, 2, 1], [3]]));
console.log(labelledAndInnerLoops([[2, 3]]));
console.log(nestedBreakStaysInSwitch([1, 2, 3]));
