// A loop condition that is a call puts a `Call` terminator at the loop header.
let calls = 0;
function same(a: number, b: number): boolean { calls++; return a === b; }
function weekend(x: number): boolean { calls++; return x % 3 === 0; }
function risky(x: number): boolean { if (x > 100) throw new Error("big"); return x < 5; }

function ternaryBody(later: number, earlier: number, sign: number): number {
  let result = 0;
  let moving = earlier;
  while (!same(later, moving)) {
    result += weekend(moving) ? 0 : sign;
    moving = moving + sign;
  }
  return result === 0 ? 0 : result;
}
function bareCall(limit: number): number {
  let n = 0;
  while (risky2(n, limit)) { n += 2; }
  return n;
}
function risky2(n: number, limit: number): boolean { return n < limit; }
function withIf(limit: number): number {
  let i = 0; let odd = 0;
  while (risky2(i, limit)) {
    if (i % 2 === 1) { odd += i; } else { odd -= 1; }
    i++;
  }
  return odd;
}
function withBreakContinue(limit: number): number {
  let i = 0; let total = 0;
  while (risky2(i, limit)) {
    i++;
    if (i === 2) continue;
    if (i === 7) break;
    total += weekend(i) ? 10 : 1;
  }
  return total;
}
function forCallCond(limit: number): number {
  let total = 0;
  for (let i = 0; risky2(i, limit); i++) { total += i % 2 === 0 ? i : -i; }
  return total;
}
function nested(limit: number): number {
  let total = 0;
  for (let i = 0; i < 3; i++) {
    let j = 0;
    while (risky2(j, limit)) {
      total += j === i ? 100 : 1;
      j++;
    }
  }
  return total;
}
function doCall(limit: number): number {
  let n = 0;
  do { n += n % 2 === 0 ? 1 : 3; } while (risky2(n, limit));
  return n;
}
function tryInBody(): number {
  let i = 90; let caught = 0;
  while (risky2(i, 104)) {
    try { caught += risky(i + 8) ? 1 : 2; } catch (e) { caught += 1000; }
    i += 3;
  }
  return caught;
}
function throwingCond(): string {
  let i = 0;
  try {
    while (risky(i * 60)) { i += 1; }
    return "done " + String(i);
  } catch (e) { return "threw at " + String(i); }
}
const inClosure = (limit: number): number => {
  let i = 0; let sum = 0;
  while (risky2(i, limit)) { sum += i % 2 === 0 ? 2 : 5; i++; }
  return sum;
};
function nestedDo(limit: number): number {
  let total = 0;
  for (let i = 0; i < 2; i++) {
    let n = i;
    do { n += n % 2 === 0 ? 1 : 3; total++; } while (risky2(n, limit));
  }
  return total;
}
function nestedWithBreaks(limit: number): number {
  let total = 0;
  outer: for (let i = 0; i < 4; i++) {
    let j = 0;
    while (risky2(j, limit)) {
      j++;
      if (j === 2) continue;
      if (i === 1 && j === 3) continue outer;
      if (i === 3) break outer;
      total += weekend(j) ? 50 : 1;
    }
    total += 1000;
  }
  return total;
}
function nestedInIf(flag: boolean, limit: number): number {
  let total = 0;
  for (let i = 0; i < 3; i++) {
    if (flag) {
      let j = 0;
      while (risky2(j, limit)) { total += j % 2 === 0 ? 2 : 3; j++; }
    } else { total -= 1; }
    total += 10;
  }
  return total;
}
function nestedTry(): number {
  let hits = 0;
  for (let i = 0; i < 3; i++) {
    let n = 0;
    try {
      while (risky(n * (i + 1) * 40)) { hits += n % 2 === 0 ? 1 : 2; n++; }
    } catch (e) { hits += 100; }
  }
  return hits;
}
const nestedClosure = (limit: number): number => {
  let t = 0;
  for (let i = 0; i < 2; i++) {
    let j = 0;
    while (risky2(j, limit)) { t += j === i ? 9 : 1; j++; }
  }
  return t;
};
function callTestedIf(limit: number): number {
  let total = 0;
  for (let i = 0; i < limit; i++) {
    if (i % 2 === 0) {
      if (risky2(i, 3)) { total += 10; continue; }
      total += 1;
    }
    total += 100;
  }
  return total;
}
function callTestedInFinally(limit: number): number {
  let total = 0;
  for (let i = 0; i < limit; i++) {
    try {
      if (risky2(i, 2)) { total += 10; continue; }
      total += 1;
    } finally {
      total += 100;
    }
  }
  return total;
}
async function ready(n: number, limit: number): Promise<boolean> { return n < limit; }
async function awaitCond(limit: number): Promise<number> {
  let n = 0; let t = 0;
  while (await ready(n, limit)) { t += n % 2 === 0 ? 7 : 1; n++; }
  return t;
}
console.log(ternaryBody(10, 1, 1), calls);
console.log(ternaryBody(1, 10, -1), calls);
console.log(ternaryBody(4, 4, 1), calls);
console.log(bareCall(7));
console.log(withIf(6));
console.log(withBreakContinue(12));
console.log(forCallCond(9));
console.log(nested(4));
console.log(doCall(20));
console.log(tryInBody());
console.log(throwingCond());
console.log(inClosure(8));
console.log(nestedDo(9));
console.log(nestedWithBreaks(5));
console.log(nestedInIf(true, 4), nestedInIf(false, 4));
console.log(nestedTry());
console.log(nestedClosure(3));
console.log(callTestedIf(6));
console.log(callTestedInFinally(4));
awaitCond(5).then((v) => console.log(v));
