// A call through a function value may leave out trailing optional or
// defaulted parameters, whatever path reaches the callee: JavaScript passes
// `undefined` for each, and a default replaces it.
function f1(a: number, b?: number): string { return `${a}:${b}`; }
function f2(a: number, b: number = 7): string { return `${a}:${b}`; }
const fns = [f1];
console.log(fns[0](1));
const gs = [f2];
console.log(gs[0](2), gs[0](2, 3));
const h: (a: number, b?: number) => string = f1;
console.log(h(3));
const o = { k: f2 };
console.log(o.k(4));
class C { m = f2; n: (a: number, b?: number) => string = f1; }
const c = new C();
console.log(c.m(5), c.n(6));
const tbl: Record<string, (a: number, b?: number) => string> = { x: f1 };
console.log(tbl["x"](7));
const mp = new Map([["y", f2]]);
console.log(mp.get("y")!(8));
const pick = (t: boolean) => t ? f1 : f2;
console.log(pick(true)(9), pick(false)(10));
const tup: [typeof f2] = [f2];
console.log(tup[0](12));
console.log(fns.map((f) => f(13)).join(","));
const arrow = (a: number, b = "d") => a + b;
const arr2 = [arrow];
console.log(arr2[0](14));
