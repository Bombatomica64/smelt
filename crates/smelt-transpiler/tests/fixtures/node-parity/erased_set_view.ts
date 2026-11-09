// An erased Set is still a Set: its identity, members, tag and printed form match
// Node, for primitive and object members alike. Maps print the same way.
const s = new Set(["a", "b"]);
const e: unknown = s;
console.log(e instanceof Set, Object.prototype.toString.call(e));
console.log((e as Set<string>).size, [...(e as Set<string>)], (e as Set<string>).has("a"), e === s);
const f = (x: any) => x;
for (const v of f(s) as Set<string>) console.log(v);
console.log(new Set([s]).has(s), JSON.stringify(f(new Set([1]))));
console.log(f(new Map([["a", 1]])), f(new Set<string>()), f(new Set(["x"])), f(new Set([{ a: 1 }])));
console.log([f(new Set([1]))], { s: f(new Set([1])) });
