// A call through a function value fills its rest parameter on every route.
type S = (...args: never) => unknown;
function v(fn: S, args: readonly unknown[]) { return (fn as (...a: readonly unknown[]) => unknown)(...args); }
const count = (...xs: unknown[]) => xs.length;
console.log(v(count as S, [1, 2, 3]), v(count as S, []));
const k = (a: number, b: number): number => a + b;
console.log(v(k as unknown as S, [2, 3]));
function asserted(f: unknown) { return (f as (...a: number[]) => number)(1, 2, 3); }
console.log(asserted((...a: number[]) => a.length));
const table: Record<string, (...xs: string[]) => string> = { m: (...xs) => xs.join("-") };
const key: string = "m";
console.log(table[key]("a", "b"), table[key]());
function ctor(...xs: number[]) { return xs.length; }
const alias = ctor;
console.log(alias(1, 2), alias(), alias.call(null, 1, 2, 3), alias.apply(null, [1, 2]));
const lists = [[1, 2], [3]];
console.log(alias(...lists[0]));
const arr: ((...xs: number[]) => number)[] = [(...xs) => xs.length];
console.log(arr[0](4, 5, 6), arr[0]());
function mk() { return (...xs: number[]) => xs.length; }
console.log(mk()(1, 2), mk()());
setTimeout((...xs: number[]) => console.log("t", xs.length), 0, 1, 2);
