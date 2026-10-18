// Loop counters and indices that integer range analysis proves integral and
// bounded become Rust integers (`usize`, or `i64` when they can go negative);
// every other `number` stays an `f64`. The program prints identically either
// way, which is the point: the retyping is unobservable.

function sumPairs(xs: number[]): number {
  let total = 0;
  for (let i = 0; i + 1 < xs.length; i++) {
    total += xs[i] * xs[i + 1];
  }
  return total;
}

function lastIndexDown(xs: string[]): number {
  let i = xs.length - 1;
  while (i >= 0) {
    if (xs[i] === "stop") {
      break;
    }
    i--;
  }
  return i;
}

function stepTwo(xs: number[]): string {
  let i = 0;
  let out = "";
  for (; i < xs.length; i += 2) {
    out += `${i}:${xs[i]} `;
  }
  return out + "end=" + i;
}

function chars(s: string): string {
  let out = "";
  for (let i = 0; i < s.length; i++) {
    out = out + s[i] + i;
  }
  return out;
}

function grid(rows: number[][]): number {
  let acc = 0;
  for (let r = 0; r < rows.length; r++) {
    const row = rows[r];
    for (let c = 0; c < row.length; c++) {
      acc += row[c] * (r + 1) + c;
    }
  }
  return acc;
}

// Not provable, so these stay `f64`: an increment with no bound in sight, a
// multiplication by a negative (a `-0` producer), a division, a counter a
// closure captures, a counter started from an arbitrary number, and a bound
// that is itself an arbitrary number.
function unguarded(limit: number): number {
  let i = 0;
  while (true) {
    i++;
    if (i * 3 > limit) {
      break;
    }
  }
  return i;
}

function flip(times: number): number {
  let sign = 1;
  for (let k = 0; k < 4; k++) {
    sign *= -1;
  }
  return sign + times;
}

function halve(): number {
  let h = 64;
  while (h > 1) {
    h = h / 2;
  }
  return h;
}

function captured(xs: number[]): number {
  let seen = 0;
  for (let i = 0; i < xs.length; i++) {
    xs.forEach(() => {
      seen = seen + 1;
    });
  }
  return seen;
}

function fromStart(start: number, xs: number[]): number {
  let i = start;
  let acc = 0;
  while (i < xs.length) {
    acc += xs[i];
    i++;
  }
  return acc + i;
}

function belowBound(n: number): number {
  let i = 0;
  while (i < n) {
    i++;
  }
  return i;
}

console.log(sumPairs([1, 2, 3, 4]), sumPairs([]), sumPairs([5]));
console.log(lastIndexDown(["a", "stop", "b"]), lastIndexDown([]), lastIndexDown(["x"]));
console.log(stepTwo([10, 20, 30]), stepTwo([]), stepTwo([1, 2]));
console.log(chars("hello"), chars(""));
console.log(grid([[1, 2], [3], []]), grid([]));
console.log(unguarded(10), flip(0.5), halve(), captured([1, 2, 3]));
console.log(fromStart(1, [1, 2, 3]), belowBound(2.5), belowBound(-1));
for (let i = 0; i < 3; i++) {
  console.log(i, "i=" + i, `t${i}`, i - 1, String(i), i / 2);
}
let k = 5;
while (k > 0) {
  k -= 2;
}
console.log(k);
// `-1 * 0` is `-0`, which prints as `-0` and no integer can hold: the product
// stays a `number` even though both factors are proven counters.
let z = 0;
for (let j = 0; j < 2; j++) {
  const m = j - 1;
  z = m * j;
  console.log(z);
}
