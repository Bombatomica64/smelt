// JSON.stringify serializes each property as ECMA-262's SerializeJSONProperty
// does (a Date's ISO text, an object's toJSON, a getter's value, then the
// replacer function or key list), and JSON.parse reads any nesting depth and
// any number literal, rejecting text that is not JSON with a SyntaxError.
console.log(JSON.stringify({ f: { toJSON() { return "custom"; } } }));
console.log(JSON.stringify([new Date(0)]));
console.log(JSON.stringify({ a: 1, b: 2, c: 3 }, ["a", "c"]));
console.log(JSON.stringify({ d: new Date(86400000) }));
const deep200: unknown = JSON.parse("[".repeat(200) + "]".repeat(200));
console.log(Array.isArray(deep200));
console.log(JSON.parse("1e400"));
const data = { a: 1, b: "two", c: [1, 2], d: { e: true } };
console.log(JSON.stringify(data, (key: string, value: unknown) => (typeof value === "number" ? value * 10 : value)));
console.log(JSON.stringify(data, ["d", "a", "e"]));
console.log(JSON.stringify(data, null, 2));
console.log(JSON.stringify(data, ["a", "b"], "-"));
console.log(JSON.stringify({ when: new Date(0), list: [new Date(86400000)] }));
console.log(JSON.stringify(new Date(0)), JSON.stringify(new Date(NaN)));
class Money { cents: number; constructor(cents: number) { this.cents = cents; } toJSON(): string { return (this.cents / 100).toFixed(2); } }
console.log(JSON.stringify({ price: new Money(1234) }), JSON.stringify([new Money(5)]));
const shape = { w: 3, h: 4, get area(): number { return this.w * this.h; } };
console.log(JSON.stringify(shape));
const nested = { inner: { toJSON(key: string) { return "key=" + key; } } };
console.log(JSON.stringify(nested));
for (const text of ["01", "1.", ".5", "-", "1e", "[1,]", "{\"a\":1,}", "-0", "1E+2", "[]", "{}"]) {
  try { const v: unknown = JSON.parse(text); console.log(text, "->", Object.is(v, -0) ? "-0" : JSON.stringify(v)); } catch (e) { console.log(text, "->", (e as Error).name); }
}
const deep: unknown = JSON.parse("[".repeat(3000) + "]".repeat(3000));
console.log(Array.isArray(deep), JSON.parse("1e400"), JSON.parse("-1e400"), JSON.parse('"\\ud800"').length);
