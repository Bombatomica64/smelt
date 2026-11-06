// An assignment is an expression wherever it stands, a call argument included.
function pair(a: number, b: number): number[] { return [a, b]; }
let a = 1;
console.log(pair(a, (a = 2)), a);
function h(x: number): number { let y = 0; console.log(y = x + 1); return y; }
console.log(h(1));
let t = 0;
const arr = [1, 2];
console.log(arr.map((x) => (t += x)), t);
const o = { v: 1 };
function id(x: number): number { return x; }
console.log(id((o.v = 5)), o.v, id(t++), t, id(t ? 1 : 2), id(((t))));
