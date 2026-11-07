// A field read keeps the value it had where it was evaluated, when a later
// argument writes that field through a computed key or `Object.assign`.
interface Counter { n: number; m: number; }
function pair(a: number, b: number): string { return a + "," + b; }
function setIndexed(c: Counter, key: keyof Counter): number { c[key] = 10; return 1; }
function assignInto(r: Record<string, number>): number { Object.assign(r, { m: 20 }); return 2; }
const c: Counter = { n: 1, m: 2 };
console.log(pair(c.n, setIndexed(c, "n")), pair(c.m, setIndexed(c, "m")));
const r: Record<string, number> = { m: 3 };
console.log(pair(r.m, assignInto(r)), r.m);
console.log(c.n, c.m);
