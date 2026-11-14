// A function is one value however it is held: erased, typed, in a list, a
// Set or a Map, compared with ===, indexOf, includes, has and get.
function sameFn() { return 1; }
function other() { return 2; }
function eq(a: unknown, b: unknown): boolean { return a === b; }
console.log(eq(sameFn, sameFn), eq(sameFn, other));
const fs: unknown[] = [sameFn, other];
console.log(fs[0] === sameFn, fs.indexOf(other), fs.includes(sameFn), fs.lastIndexOf(sameFn));
const g = (x: number) => x;
const h: any = g;
console.log(h === g, eq(g, h), [g].includes(h), [h].indexOf(g));
const seen = new Set<unknown>([sameFn]);
console.log(seen.has(sameFn), seen.has(other));
const handlers = new Map<unknown, string>([[sameFn, "same"], [g, "arrow"]]);
console.log(handlers.get(sameFn), handlers.get(g), handlers.get(other));
const back = h as (x: number) => number;
console.log(back === g, back(4));
const typed: Array<() => number> = [sameFn, other];
console.log(typed.indexOf(sameFn), typed.includes(other), eq(typed[0], sameFn));
