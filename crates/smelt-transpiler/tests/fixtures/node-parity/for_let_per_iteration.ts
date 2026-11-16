// Every evaluation of a `let` declaration is a binding of its own: a
// `for (let ..)` head gets one per iteration (CreatePerIterationEnvironment),
// and a `let` in a loop body or a `for..of`/`for..in` head one per run, so a
// closure keeps the value of the iteration that created it, including writes
// that iteration makes after the closure exists.

// Each iteration of a `for (let ..)` loop has its own binding.
function basic(): string {
  const fns: Array<() => number> = [];
  for (let i = 0; i < 3; i++) fns.push(() => i);
  return fns.map((f) => f()).join(",");
}
console.log(basic());

// A write in the body is seen by that iteration's closure and carried on.
function bodyWrite(): string {
  const fns: Array<() => number> = [];
  for (let i = 0; i < 6; i++) {
    i += 1;
    fns.push(() => i * 10);
  }
  return fns.map((f) => f()).join(",");
}
console.log(bodyWrite());

// `continue` still runs the update, and the skipped iterations capture nothing.
function withContinue(): string {
  const fns: Array<() => number> = [];
  for (let i = 0; i < 5; i++) {
    if (i % 2 === 1) continue;
    fns.push(() => i);
  }
  return fns.map((f) => f()).join(",");
}
console.log(withContinue());

// A closure that writes its own iteration's binding does not disturb the loop.
function closureWrite(): string {
  const bumps: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    bumps.push(() => {
      i += 100;
      return i;
    });
  }
  const first = bumps.map((f) => f()).join(",");
  const second = bumps.map((f) => f()).join(",");
  return first + " " + second;
}
console.log(closureWrite());

// No update clause: the body advances the binding itself.
function noUpdate(): string {
  const fns: Array<() => string> = [];
  for (let s = "a"; s.length < 4; ) {
    fns.push(() => s);
    s = s + "b";
  }
  return fns.map((f) => f()).join(",");
}
console.log(noUpdate());

// Two head bindings, one captured; a nested loop captures both levels.
function nested(): string {
  const fns: Array<() => string> = [];
  for (let i = 0, n = 2; i < n; i++) {
    for (let j = 0; j < 2; j++) {
      fns.push(() => `${i}${j}`);
    }
  }
  return fns.map((f) => f()).join(",");
}
console.log(nested());

// `break` leaves the last iteration's closure with its own value.
function withBreak(): string {
  const fns: Array<() => number> = [];
  for (let i = 0; ; i++) {
    fns.push(() => i);
    if (i === 2) break;
  }
  return fns.map((f) => f()).join(",");
}
console.log(withBreak());

// The value after the loop is the carrier's, and an outer `i` is untouched.
function afterLoop(): string {
  const i = "outer";
  let seen = 0;
  const fns: Array<() => number> = [];
  for (let i = 10; i < 13; i++) {
    fns.push(() => i);
    seen = i;
  }
  return `${i} ${seen} ${fns.map((f) => f()).join(",")}`;
}
console.log(afterLoop());

// A method callback (not an arrow) captures per iteration too.
function functionExpression(): string {
  const fns: Array<() => number> = [];
  for (let k = 1; k <= 3; k++) {
    fns.push(function () {
      return k * k;
    });
  }
  return fns.map((f) => f()).join(",");
}
console.log(functionExpression());

// A plain `let` in a loop body is a new binding each time it runs.
const fns: Array<() => number> = [];
for (const v of [1, 2, 3]) { let c = v; fns.push(() => c); c = c * 10; }
console.log(fns.map((f) => f()).join(","));
let k = 0;
const gs: Array<() => number> = [];
while (k < 3) { let c = k; gs.push(() => c); c += 100; k++; }
console.log(gs.map((f) => f()).join(","));

// for-of head binding written after the capture.
const ofHead: Array<() => number> = [];
for (let x of [1, 2, 3]) { ofHead.push(() => x); x = x * 10; }
console.log(ofHead.map((f) => f()).join(","));

// A loop inside an arrow body.
const make = (): string => {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) { let sq = i * i; fs.push(() => sq + i); sq = -1; }
  return fs.map((f) => f()).join(",");
};
console.log(make());

// A generator body.
function* gen(): Generator<() => number> {
  for (let i = 0; i < 3; i++) yield () => i;
}
console.log([...gen()].map((f) => f()).join(","));

// An async body.
async function run(): Promise<string> {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) { await Promise.resolve(); fs.push(() => i); }
  return fs.map((f) => f()).join(",");
}
run().then((s) => console.log(s));

// for-in head
const keys: Array<() => string> = [];
for (let key in { p: 1, q: 2 }) { keys.push(() => key); key = key + "!"; }
console.log(keys.map((f) => f()).join(","));

// while with a let captured and written by the closure.
let n = 0;
const counters: Array<() => number> = [];
while (n < 2) { let c = n * 100; counters.push(() => ++c); n++; }
console.log(counters.map((f) => f()).join(","), counters.map((f) => f()).join(","));

// A for-of over a generator.
function* nums(): Generator<number> { yield 1; yield 2; yield 3; }
const genFns: Array<() => number> = [];
for (let v of nums()) { genFns.push(() => v); v = v * 10; }
console.log(genFns.map((f) => f()).join(","));
