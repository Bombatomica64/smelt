function sum(...xs: number[]): number {
  let t = 0;
  for (const x of xs) t += x;
  return t;
}
function join(sep: string, ...rest: string[]): string {
  return rest.join(sep) + "|" + rest.length;
}
function sizes(...lists: number[][]): string {
  return lists.length + ":" + lists.map((l) => l.length).join(",");
}
function describe(...values: unknown[]): string {
  return values.length + ":" + values.map((v) => typeof v).join(",");
}
const zero: () => number = sum;
const one: (a: number) => number = sum;
const two: (a: number, b: number) => number = sum;
const restToRest: (...a: number[]) => number = sum;
const j: (s: string, a: string) => string = join;
const nested: (a: number[]) => string = sizes;
const nested2: (a: number[], b: number[]) => string = sizes;
const erased: (v: unknown) => string = describe;
console.log(zero(), one(5), two(2, 3), restToRest(1, 2, 3), j("-", "x"));
console.log(nested([1, 2, 3]), nested2([1], [2, 3]), erased("s"), erased([1, 2]));
function apply(f: (n: number) => number, n: number): number {
  return f(n);
}
console.log(apply(sum, 7));
console.log([1, 2, 3].map((x) => one(x)).join(","));
