// `JSON.stringify` and `JSON.parse` read as values are first-class functions:
// a call through the copy runs the builtin, and the copy reports the
// builtin's own name and length.
const s = JSON.stringify;
console.log(s.name, s.length);
console.log(s({ a: 1, b: [true, null] }));
console.log(s({ a: 1 }, null, 2));
console.log(s(undefined));
console.log(s([1, 2], (_k: string, v: unknown) => (typeof v === "number" ? v * 10 : v)));
const p = JSON.parse;
console.log(p.name, p.length);
const parsed = p('{"x":[1,2,3]}');
console.log(parsed.x.length);
try {
  p("{bad");
} catch (e) {
  console.log(e instanceof SyntaxError);
}
console.log(JSON.stringify.name, JSON.stringify.length, JSON.parse.name, JSON.parse.length);
// `Object.is` and `Object.hasOwn` as values run the same checks as their calls.
const is = Object.is;
console.log(is.name, is.length, is(1, 1), is(NaN, NaN), is("a", "b"));
const has = Object.hasOwn;
console.log(has.name, has.length, has({ a: 1 }, "a"), has({ a: 1 }, "b"));
