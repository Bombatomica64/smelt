// ToString is one rule for a template literal, `+` beside a string and
// `String(x)`: an array joins its elements' strings with `,` (nested arrays
// recursively, absent elements empty), a class or object with a `toString`
// of its own is converted by calling it with itself as `this`, a set, a map
// and any other object keep their tags, and `null`/`undefined` are words.
// ToNumber of an array reads that string.
class A { n: number; constructor(n: number) { this.n = n; } toString(): string { return "A" + this.n; } }
const xs = [1, 2]; const a = new A(3); const s = new Set([1]);
console.log(`${xs}`, String(xs), "" + xs, xs + "!");
console.log(`${a}`, String(a), "" + a);
console.log(`${[[1, 2], [3]]}`, `${{ k: 1 }}`, `${s}`);
class Base { n: number; constructor(n: number) { this.n = n; } toString(): string { return "B" + this.n; } }
class Derived extends Base { constructor(n: number) { super(n); } }
class Plain { x = 1; }
const d4 = new Derived(4);
console.log(`${d4}`, "" + d4, String(d4), d4 + "!");
console.log(`${new Plain()}`, String(new Plain()));
let maybe: Base | undefined = Math.random() > 2 ? new Base(1) : undefined;
console.log(`${maybe}`);
maybe = new Base(2);
console.log(`${maybe}`);
console.log(`${[null, undefined, 1, "a", true]}`, String([[1, [2, 3]], []]));
const tup: [number, string] = [1, "a"];
console.log(`${tup}`, String(tup), "" + tup);
console.log(`${new Error("boom")}`, String(new TypeError("t")));
console.log(String(null), `${null}`, "" + null, String(undefined), `${undefined}`);
const lit = { name: "n", toString() { return "lit " + this.name; } };
console.log(`${lit}`, "" + lit);
console.log(`${new Map([[1, 2]])}`, `${new Set([1])}`, `${{ a: 1 }}`, String(true));
console.log([1, 2] + "", [] + "x");
console.log(Number([5]), Number([]), Number([1, 2]), Number(["7"]));
const nested = [[1, 2], [3]];
const ys: (number | null)[] = [1, null];
console.log(nested.join(","), ys.join(","));
// A separator typed `string | RegExp` reaches the erased-separator split.
function splitBy(text: string, separator: string | RegExp): string[] {
  return text.split(separator);
}
console.log(splitBy("a,b;c", ","), splitBy("a,b;c", /[,;]/));
