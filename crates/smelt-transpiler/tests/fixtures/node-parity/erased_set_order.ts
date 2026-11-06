// A `Set` that crosses `unknown` keeps its members and insertion order.
const s: unknown = new Set<number>([5, 1, 9, 3, 7, 2, 8]);
console.log([...(s as Set<number>)]);
const words: unknown = new Set(["z", "a", "m"]);
console.log([...(words as Set<string>)]);
function ids(x: Set<number>): number[] { return [...x]; }
console.log(ids(new Set([4, 2, 6])));
