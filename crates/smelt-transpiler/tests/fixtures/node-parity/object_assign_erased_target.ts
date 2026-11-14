// `Object.assign` onto a value of a type parameter writes into that object
// and returns it: the target keeps its own members (a symbol-keyed method
// included) and gains the sources'.

const key = Symbol.for("@demo/matcher");

interface Protocol {
  match: (value: unknown) => boolean;
}

interface Matcher {
  [key](): Protocol;
}

function when(predicate: (value: unknown) => boolean): Matcher {
  return {
    [key]() {
      return { match: (value: unknown) => predicate(value) };
    },
  };
}

function chainable<p extends Matcher>(pattern: p) {
  return Object.assign(pattern, { optional: () => pattern, extra: 1 });
}

function isMatcher(x: unknown): x is Matcher {
  const m = x as Matcher;
  return m && !!m[key];
}

function run(pattern: unknown, value: unknown): boolean {
  return isMatcher(pattern) ? pattern[key]().match(value) : pattern === value;
}

const isString = (x: unknown): x is string => typeof x === "string";
const base = when(isString);
const chained = chainable(base);
console.log(run(base, "a"), run(chained, "a"), run(chained, 3));
console.log(isMatcher(chained), typeof (chained as any).optional, (chained as any).extra);
