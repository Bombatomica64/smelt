// Control flow and rest/default rules: each block prints the value the language defines.

function countDown(value: number): number {
  let current = value;
  do {
    current = current - 1;
  } while (current > 0);
  return current;
}
console.log(countDown(3), countDown(-2));

function collect(first: number, ...rest: number[]): number {
  return first * 100 + rest.length;
}
function call(values: number[]): number {
  return collect(1, 2, ...values);
}
console.log(call([5, 6, 7]), call([]));

function nullishAssign(value: number | undefined): number {
  const now = 1;
  value ??= now;
  return value;
}
console.log(nullishAssign(undefined), nullishAssign(0), nullishAssign(9));

function fallThrough(status: string): string {
  let result = "";
  switch (status) {
    case "pending":
      result = "waiting";
    case "approved":
      return "Approved:" + result;
    default:
      result = "unknown";
  }
  return result;
}
console.log(fallThrough("pending"), fallThrough("approved"), fallThrough("other"));

function stacked(kind: string): number {
  let value = 0;
  switch (kind) {
    case "relation":
    case "media": {
      value = 1;
      break;
    }
    default:
      break;
  }
  return value;
}
console.log(stacked("relation"), stacked("media"), stacked("x"));

function lastCase(value: string): string {
  let result = "";
  switch (value) {
    case "a":
      result += "a";
      break;
    default:
      result += value;
  }
  return result;
}
console.log(lastCase("a"), lastCase("zz"));

const stringTag = "[object String]";
const numberTag = "[object Number]";
function classify(tag: string): number {
  switch (tag) {
    case stringTag:
      return 1;
    case numberTag:
      return 2;
    default:
      return 0;
  }
}
console.log(classify("[object Number]"), classify("[object String]"), classify(""));

function maxValue(values: number[]): number {
  let result = 0;
  values.forEach((value, index) => {
    if (index < 0) return;
    if (result < value) result = value;
  });
  return result;
}
console.log(maxValue([3, 9, 2]));

function tally(values: number[]): number {
  let count = 0;
  values.forEach((value) => {
    if (value > 0) {
      count = count + 1;
    } else {
      count = count - 1;
    }
  });
  return count;
}
console.log(tally([1, -1, 2, 3]));

function tag(rows: string[][], suffix: string): string[][] {
  return rows.map((row) => {
    row[0] += suffix;
    return row;
  });
}
console.log(tag([["a"], ["b", "c"]], "!").map((row) => row.join(",")).join("|"));

function endOf(queryIndex: number, hashIndex: number): number | undefined {
  return queryIndex === -1
    ? hashIndex === -1
      ? undefined
      : hashIndex
    : hashIndex === -1
      ? queryIndex
      : Math.min(queryIndex, hashIndex);
}
console.log(endOf(-1, -1), endOf(-1, 4), endOf(2, -1), endOf(7, 3));

function commaLoop(size: number, length: number): number[] {
  const result: number[] = [];
  for (let step = length - size, resultIndex = 0; step < length; step++, resultIndex++) {
    result[resultIndex] = step;
  }
  return result;
}
console.log(commaLoop(2, 5).join(","));

function outerPair(n: number): number {
  if (n > 0) {
    class Pair {
      v: number;
      constructor(a: number, x: number = a++) {
        this.v = a * 10 + x;
      }
    }
    return new Pair(n).v;
  }
  return 0;
}
console.log(outerPair(2));

function outerField(n: number): number {
  if (n > 0) {
    class Counter {
      a: number = 3;
      b: number = this.a++;
    }
    const counter = new Counter();
    return counter.a * 10 + counter.b;
  }
  return 0;
}
console.log(outerField(1));

function outerLoop(n: number): number {
  let total = 0;
  for (let r = 0; r < n; r++) {
    class Scaled {
      get(a: number, x: number = a++): number {
        return a * 10 + x;
      }
    }
    total += new Scaled().get(r);
  }
  return total;
}
console.log(outerLoop(3));

function fillRange<T>(array: T[], value: T, start = 0, end = array.length): T[] {
  for (let i = start; i < end; i++) {
    array[i] = value;
  }
  return array;
}
console.log(fillRange([1, 2, 3], 9, 1).join(","));

class PrivateCounter {
  #count: number = 0;
  #bump(by: number): number {
    this.#count = this.#count + by;
    return this.#count;
  }
  add(by: number): number {
    return this.#bump(by);
  }
}
const privateCounter = new PrivateCounter();
privateCounter.add(3);
console.log(privateCounter.add(4));

function applyMaybe(mapper: ((value: number) => number) | undefined, value: number): number {
  const values = [value].map((item) => (mapper ? mapper(item) : item));
  return values[0];
}
console.log(applyMaybe(undefined, 4), applyMaybe((x) => x * 2, 4));
