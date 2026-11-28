// `Object.is` is SameValue on every value, erased ones included: `0` and
// `-0` differ, `NaN` equals `NaN`, objects compare by identity. Through the
// `Object.is` value too, and `a === b || Object.is(a, b)` stays SameValueZero.
const a: any = 0;
const b: any = -0;
const n: any = NaN;
const u: unknown = -0;
console.log(Object.is(a, b), Object.is(n, n), Object.is(a, a), Object.is(u, 0), Object.is(u, -0));
const is = Object.is;
console.log(is(a, b), is(n, NaN), is(0, -0), is(NaN, NaN));
console.log(Object.is(0, -0), Object.is(NaN, NaN), Object.is("x", "x" as any), Object.is(null, undefined as any));
const objs: any[] = [{}, {}];
console.log(Object.is(objs[0], objs[0]), Object.is(objs[0], objs[1]));
function sameValueZero(x: any, y: any): boolean { return x === y || Object.is(x, y); }
function sameNumber(x: number, y: number): boolean { return x === y || Object.is(x, y); }
console.log(sameValueZero(0, -0), sameValueZero(NaN, NaN), sameValueZero(1, 2), sameNumber(0, -0), sameNumber(NaN, NaN));
