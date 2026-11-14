// A destructuring default applies to `undefined` only, the binding keeps the
// declared type (`unknown` stays `unknown`), and the default is evaluated only
// when it applies.
let evaluated = 0;
function side(): string { evaluated++; return "side"; }
const rec: Record<string, unknown> = { x: "t", n: 0, z: null };
const { x = true, y = 5, n = 9, z = "zz" } = rec;
console.log(x, y, n, z);
function opt(bag: { a?: unknown }): unknown { const { a = "dflt" } = bag; return a; }
console.log(opt({}), opt({ a: 3 }), opt({ a: undefined }));
function f({ a = 5, b = "d" }: { a?: number | string; b?: string } = {}) { console.log(a, b); }
f({ a: "s" }); f();
const [p = 1, q = "w"] = [undefined, "v"] as [number | undefined, string | undefined];
console.log(p, q);
const bag: Record<string, unknown> = { k: false };
const { k = side(), m = side() } = bag;
console.log(k, m, evaluated);
function typed({ count = 1, label = "none" }: { count?: number; label?: string }): string { return `${label}:${count + 1}`; }
console.log(typed({}), typed({ count: 0, label: "" }));
