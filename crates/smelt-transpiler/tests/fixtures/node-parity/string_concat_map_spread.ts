// `String.prototype.concat` converts each argument with ToString and appends
// it; spreading a Map yields its `[key, value]` entries in insertion order.
const base = "50%";
console.log(base.concat(" off", "!"), "n=".concat(String(1)), "x".concat());
const parts = ["a", "b"];
console.log("list: ".concat(parts.join("+")), base.concat(" ", "of", " ", "10"));
const counts = new Map<string, number>([["b", 2], ["a", 1]]);
counts.set("c", 3);
const pairs = [...counts];
console.log(pairs, pairs.length);
console.log([...counts].map(([k, v]) => k.repeat(v)).join("|"));
console.log([...counts].sort((x, y) => x[1] - y[1])[0], [0, ...[...counts].map(([, v]) => v)]);
const empty = new Map<number, string>();
console.log([...empty].length);
