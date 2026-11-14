// A method returning an anonymous object type of functions (`{ match: fn }`)
// erases with its function values, so an erased matcher is still callable.

const key = Symbol.for("@demo/matcher");
interface Matcher { [key](): { match: (value: unknown) => boolean } }
const isString = (x: unknown): x is string => typeof x === "string";
function when(predicate: (value: unknown) => boolean): Matcher {
  return { [key]() { return { match: (value: unknown) => predicate(value) }; } };
}
function isMatcher(x: unknown): x is Matcher {
  const m = x as Matcher;
  return m && !!m[key];
}
function run(pattern: unknown, value: unknown): boolean {
  return isMatcher(pattern) ? pattern[key]().match(value) : pattern === value;
}
const erased: unknown = when(isString);
console.log(run(erased, "a"), run(erased, 3), run(5, 5));
