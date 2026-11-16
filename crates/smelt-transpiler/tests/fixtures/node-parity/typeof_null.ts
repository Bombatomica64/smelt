// `typeof null` is "object", including for a `T | null` holding `null`,
// while an absent optional is "undefined"; the `typeof` guards agree.
console.log(typeof null, typeof undefined);
function kind(x: string | null): string { return typeof x; }
console.log(kind(null), kind("a"));
function kindU(x?: string): string { return typeof x; }
console.log(kindU(), kindU("a"));
function isObj(x: { a: number } | null): string { return typeof x === "object" ? "obj" : "other"; }
console.log(isObj(null), isObj({ a: 1 }));
function nn(x: number | null): string { if (typeof x === "object") return "null!"; return "num " + x; }
console.log(nn(null), nn(3));
function undef(x: number | null): boolean { return typeof x === "undefined"; }
console.log(undef(null), undef(1));
function undefO(x?: number): boolean { return typeof x === "undefined"; }
console.log(undefO(), undefO(1));
const values: unknown[] = [null, undefined, 1, "s", {}, [], () => 1];
console.log(values.map((v) => typeof v).join(","));
console.log(typeof crypto === "undefined", typeof crypto !== "undefined", typeof crypto);
