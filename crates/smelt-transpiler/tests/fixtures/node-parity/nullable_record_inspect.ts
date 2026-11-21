// `console.log` of `T | null` values: a string-keyed record (an anonymous
// object type of one value type lowers to a record) next to other nullable
// scalars and containers, without any erased value in the program.
function find(xs: { a: number }[], k: number): { a: number } | null {
  for (const x of xs) if (x.a === k) return x;
  return null;
}
const xs = [{ a: 1 }, { a: 2 }];
console.log(find(xs, 2));
console.log(find(xs, 3));
function pick(n: number): string | null { return n > 0 ? "s" + n : null; }
function num(n: number): number | null { return n > 0 ? n * 2 : null; }
function flag(n: number): boolean | null { return n > 0 ? true : null; }
function list(n: number): number[] | null { return n > 0 ? [n] : null; }
console.log(pick(1), pick(0));
console.log(num(2), num(0));
console.log(flag(1), flag(0));
console.log(list(3), list(0));
console.log([find(xs, 1), find(xs, 9)]);
