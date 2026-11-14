// An absent iterable builds an empty collection: `new Set(undefined)`,
// `new Map(null)`, for an optional array, map, set or string argument.
function mk(xs?: number[]): number { return new Set(xs).size; }
console.log(mk(), mk([1, 1, 2]));
function mkMap(es?: [string, number][]): number { return new Map(es).size; }
console.log(mkMap(), mkMap([["a", 1], ["a", 2], ["b", 3]]));
function chars(s?: string): number { return new Set(s).size; }
console.log(chars(), chars("abca"));
function copy(s?: Set<string>): string[] { return [...new Set(s)]; }
console.log(copy(), copy(new Set(["x", "y"])));
const n: string[] | null = Math.random() > 2 ? ["q"] : null;
console.log(new Set(n).size);
const literalSet: Set<number> = new Set(undefined);
const literalMap: Map<string, number> = new Map(null);
console.log(literalSet.size, literalMap.size);
