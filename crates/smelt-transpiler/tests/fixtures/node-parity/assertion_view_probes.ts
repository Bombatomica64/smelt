// `const view = erased as T` keeps the erased value: a guard that asserts and
// then inspects answers for every input, and a read of the view as a value is
// still a `T`.

const matcherKey = Symbol.for("@demo/matcher");

interface Protocol {
  match: (value: unknown) => boolean;
}

interface Matcher {
  [matcherKey](): Protocol;
}

interface Named {
  name: string;
  size: number;
}

// The type-guard shape: assert, then probe a symbol-keyed member.
function isMatcher(x: unknown): x is Matcher {
  const pattern = x as Matcher;
  return pattern && !!pattern[matcherKey];
}

// A truthiness test and a static member read through a view.
function describe(x: unknown): string {
  const named = x as Named;
  if (!named) {
    return "nothing";
  }
  if (named.name) {
    return `named ${named.name}`;
  }
  return "anonymous";
}

// The view read as a value: it is a `Named` where the program uses it as one.
function sizeOf(named: Named): number {
  return named.size;
}

function measured(x: unknown): number {
  const named = x as Named;
  return sizeOf(named);
}

const isString: Matcher = {
  [matcherKey]() {
    return { match: (value: unknown) => typeof value === "string" };
  },
};

function matches(pattern: unknown, value: unknown): boolean {
  if (isMatcher(pattern)) {
    return pattern[matcherKey]().match(value);
  }
  return pattern === value;
}

// The same guard as arrow consts: an arrow body numbers its locals afresh, so
// the view (`pattern`, the guard's second local) must not stand for the
// caller's second local (`value`).
const isMatcherArrow = (x: unknown): x is Matcher => {
  const pattern = x as Matcher;
  return pattern && !!pattern[matcherKey];
};

const matchesArrow = (pattern: any, value: any): boolean => {
  if (isMatcherArrow(pattern)) {
    return pattern[matcherKey]().match(value);
  }
  return pattern === value;
};

const candidates: unknown[] = [isString, [1, 2], "a", 3, 0, null, undefined, true, { other: 1 }];
for (const candidate of candidates) {
  console.log(
    isMatcher(candidate) ? "matcher" : "plain",
    matches(candidate, "a"),
    matchesArrow(candidate, "a"),
  );
}
const records: unknown[] = [{ name: "box", size: 3 }, { name: "", size: 1 }, null, 0];
for (const record of records) {
  console.log(describe(record));
}
console.log(measured({ name: "crate", size: 7 }));
