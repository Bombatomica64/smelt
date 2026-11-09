// `console.log` lays values out the way `util.inspect` does: string quoting,
// depth, array grouping, line breaking, shapes with absent optionals, classes,
// tuples and the collection containers.
interface Labelled { x: number; label?: string; }
const absent: Labelled = { x: 1 };
const present: Labelled = { x: 1, label: "here" };
console.log(absent);
console.log(present);

class Point { x: number; y: number; constructor(x: number, y: number) { this.x = x; this.y = y; } }
class Tree { value = 1; next: Tree | undefined = undefined; }
const tree = new Tree();
tree.next = new Tree();
console.log(new Point(1, 2));
console.log(tree);

console.log(["plain", "it's", 'say "hi"', "both ' and \"", "back`tick ' \""]);
const tuple: [number, string, boolean] = [1, "a", true];
console.log(tuple);
console.log([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27]);
console.log(["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa"]);
console.log({ a: { b: { c: { d: 1 } } } });
console.log({ first: "a fairly long string value", second: "another fairly long string value", third: 3 });
console.log([[1, [2, [3, [4]]]]]);

console.log(new Map([["a", 1], ["b", 2]]));
console.log(new Set([1, 2, 3]));
console.log(new Map([["k", [1, 2]]]), new Set(["x"]));
console.log([], {}, new Map(), new Set());
console.log("top level string", 1.5, true, null);
