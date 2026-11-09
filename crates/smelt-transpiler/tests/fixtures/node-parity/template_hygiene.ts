// Program locals named like the emitter's template temporaries, read from
// inside the code those templates splice. Each must keep its own value.

const value = 100;
const item = 7;
const key = "k";
const index = 1000;
const smelt_needle = 3;
const acc = 5;

const xs = [1, 2, 3];
console.log(xs.map((x) => x + value).join(","));
console.log(xs.filter((x) => x * item > 10).join(","));
console.log(xs.some((x) => x === smelt_needle), xs.indexOf(smelt_needle));
console.log(xs.reduce((total, x) => total + x * acc, index));
console.log(xs.findIndex((x) => x + index === 1002));

const m = new Map<string, number>([["a", 1], ["k", 2]]);
console.log(m.get(key), m.has(key));
const entries: string[] = [];
m.forEach((v, k) => entries.push(`${k}=${v + value}`));
console.log(entries.join(","));

const words = ["b", "a", "c"];
console.log(words.includes(key), [...words].sort().join(""));

const maybe: number | undefined = xs.length > 2 ? 4 : undefined;
console.log((maybe ?? 0) + value);

function* gen(co: number): Generator<number, void, void> {
  yield co;
  yield co + 1;
}
console.log([...gen(41)].join(","));
