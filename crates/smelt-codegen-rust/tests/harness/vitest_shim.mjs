// A `vitest` stand-in for the runtime tiers' Node oracle (tests/harness/mod.rs,
// `node_oracle`): the subset of the API the fixtures use, with vitest's
// semantics, so the expected values a fixture writes are checked against real
// Node. It prints one line per test (`##smelt-oracle## ok <name>` or `.. fail <name>:
// <message>`) after every test has run, then `.. summary <passed> <failed>`.
//
// Equality follows vitest's `equals`: `toBe` is `Object.is`; `toEqual`
// compares recursively, ignores `undefined`-valued properties and the object's
// class, and compares an array's own named properties too; `toStrictEqual`
// also compares classes, `undefined` properties and sparseness.

const tests = [];
const prefixes = [];

function register(name, fn) {
  tests.push({ name: [...prefixes, name].join(" > "), fn });
}

export function test(name, fn) {
  register(name, fn);
}
export const it = test;
export function describe(name, body) {
  prefixes.push(name);
  try {
    body();
  } finally {
    prefixes.pop();
  }
}
export function beforeEach() {}
export function afterEach() {}

function className(value) {
  return Object.prototype.toString.call(value);
}

function ownKeys(value, strict) {
  const keys = [
    ...Object.keys(value),
    ...Object.getOwnPropertySymbols(value).filter(
      (symbol) => Object.getOwnPropertyDescriptor(value, symbol).enumerable,
    ),
  ];
  return strict ? keys : keys.filter((key) => value[key] !== undefined);
}

// An asymmetric matcher (`expect.any(Number)`, `expect.arrayContaining([..])`)
// answers for itself wherever it sits in an expected value.
class Asymmetric {
  constructor(test) {
    this.test = test;
  }
}

function equals(a, b, strict, seen = []) {
  if (b instanceof Asymmetric) return b.test(a);
  if (a instanceof Asymmetric) return a.test(b);
  if (Object.is(a, b)) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) {
    return false;
  }
  if (className(a) !== className(b)) return false;
  if (strict && Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return false;
  if (a instanceof Error && b instanceof Error) {
    return a.name === b.name && a.message === b.message && equals(a.cause, b.cause, strict, seen);
  }
  if (a instanceof Date) return Object.is(a.getTime(), b.getTime());
  if (a instanceof RegExp) return a.source === b.source && a.flags === b.flags;
  if (a instanceof Number || a instanceof String || a instanceof Boolean) {
    return Object.is(a.valueOf(), b.valueOf());
  }
  if (seen.some(([x, y]) => x === a && y === b)) return true;
  seen = [...seen, [a, b]];
  if (a instanceof Map) {
    if (a.size !== b.size) return false;
    for (const [key, value] of a) {
      if (!b.has(key) || !equals(value, b.get(key), strict, seen)) return false;
    }
    return true;
  }
  if (a instanceof Set) {
    if (a.size !== b.size) return false;
    for (const value of a) {
      if (!b.has(value) && ![...b].some((other) => equals(value, other, strict, seen))) {
        return false;
      }
    }
    return true;
  }
  if (ArrayBuffer.isView(a) && !(a instanceof DataView)) {
    if (a.length !== b.length) return false;
    for (let index = 0; index < a.length; index++) {
      if (!Object.is(a[index], b[index])) return false;
    }
    return true;
  }
  if (Array.isArray(a)) {
    if (a.length !== b.length) return false;
    for (let index = 0; index < a.length; index++) {
      if (strict && (index in a) !== (index in b)) return false;
      if (!equals(a[index], b[index], strict, seen)) return false;
    }
    const named = (value) => ownKeys(value, strict).filter((key) => !/^\d+$/.test(String(key)));
    const left = named(a);
    const right = named(b);
    return left.length === right.length && left.every((key) => equals(a[key], b[key], strict, seen));
  }
  const left = ownKeys(a, strict);
  const right = ownKeys(b, strict);
  if (left.length !== right.length) return false;
  return left.every(
    (key) => Object.prototype.hasOwnProperty.call(b, key) && equals(a[key], b[key], strict, seen),
  );
}

function matchesObject(actual, expected) {
  if (typeof expected !== "object" || expected === null) return equals(actual, expected, false);
  if (typeof actual !== "object" || actual === null) return false;
  if (Array.isArray(expected)) {
    return (
      Array.isArray(actual) &&
      actual.length === expected.length &&
      expected.every((item, index) => matchesObject(actual[index], item))
    );
  }
  return Object.keys(expected).every(
    (key) => key in actual && matchesObject(actual[key], expected[key]),
  );
}

function show(value) {
  try {
    if (typeof value === "string") return JSON.stringify(value);
    if (typeof value === "function") return `[Function ${value.name}]`;
    if (typeof value === "bigint") return `${value}n`;
    if (Object.is(value, -0)) return "-0";
    return String(JSON.stringify(value) ?? value);
  } catch {
    return String(value);
  }
}

function thrownBy(fn) {
  try {
    fn();
  } catch (error) {
    return { threw: true, error };
  }
  return { threw: false };
}

function throwMatches(error, expected) {
  if (expected === undefined) return true;
  const message = error instanceof Error ? error.message : String(error);
  if (typeof expected === "string") return message.includes(expected);
  if (expected instanceof RegExp) return expected.test(message);
  if (typeof expected === "function") return error instanceof expected;
  if (expected instanceof Error) return message === expected.message;
  return false;
}

function calls(mock) {
  if (!mock || !mock.mock) throw new Error(`${show(mock)} is not a vi.fn() mock`);
  return mock.mock.calls;
}

const matchers = {
  toBe: (actual, expected) => [Object.is(actual, expected), `expected ${show(actual)} to be ${show(expected)}`],
  toEqual: (actual, expected) => [equals(actual, expected, false), `expected ${show(actual)} to equal ${show(expected)}`],
  toStrictEqual: (actual, expected) => [equals(actual, expected, true), `expected ${show(actual)} to strictly equal ${show(expected)}`],
  toMatchObject: (actual, expected) => [matchesObject(actual, expected), `expected ${show(actual)} to match ${show(expected)}`],
  toBeUndefined: (actual) => [actual === undefined, `expected ${show(actual)} to be undefined`],
  toBeDefined: (actual) => [actual !== undefined, `expected ${show(actual)} to be defined`],
  toBeNull: (actual) => [actual === null, `expected ${show(actual)} to be null`],
  toBeTruthy: (actual) => [Boolean(actual), `expected ${show(actual)} to be truthy`],
  toBeFalsy: (actual) => [!actual, `expected ${show(actual)} to be falsy`],
  toBeNaN: (actual) => [Number.isNaN(actual), `expected ${show(actual)} to be NaN`],
  toBeTypeOf: (actual, expected) => [typeof actual === expected, `expected ${show(actual)} to be of type ${expected}`],
  toBeInstanceOf: (actual, expected) => [actual instanceof expected, `expected ${show(actual)} to be an instance of ${expected?.name}`],
  toBeGreaterThan: (actual, expected) => [actual > expected, `expected ${show(actual)} > ${show(expected)}`],
  toBeGreaterThanOrEqual: (actual, expected) => [actual >= expected, `expected ${show(actual)} >= ${show(expected)}`],
  toBeLessThan: (actual, expected) => [actual < expected, `expected ${show(actual)} < ${show(expected)}`],
  toBeLessThanOrEqual: (actual, expected) => [actual <= expected, `expected ${show(actual)} <= ${show(expected)}`],
  toBeCloseTo: (actual, expected, digits = 2) => [Math.abs(actual - expected) < 10 ** -digits / 2, `expected ${show(actual)} to be close to ${show(expected)}`],
  toContain: (actual, expected) => [
    typeof actual === "string" ? actual.includes(expected) : [...actual].some((item) => Object.is(item, expected)),
    `expected ${show(actual)} to contain ${show(expected)}`,
  ],
  toContainEqual: (actual, expected) => [[...actual].some((item) => equals(item, expected, false)), `expected ${show(actual)} to contain ${show(expected)}`],
  toHaveLength: (actual, expected) => [actual?.length === expected, `expected length ${show(actual?.length)} to be ${expected}`],
  toHaveProperty: (actual, key, ...value) => [
    actual != null && key in Object(actual) && (value.length === 0 || equals(actual[key], value[0], false)),
    `expected ${show(actual)} to have property ${show(key)}`,
  ],
  toMatch: (actual, expected) => [
    typeof expected === "string" ? actual.includes(expected) : expected.test(actual),
    `expected ${show(actual)} to match ${expected}`,
  ],
  toThrow: (fn, expected) => {
    const outcome = thrownBy(fn);
    return [outcome.threw && throwMatches(outcome.error, expected), `expected the callback to throw ${expected ?? ""}`];
  },
  toHaveBeenCalled: (mock) => [calls(mock).length > 0, "expected the mock to be called"],
  toHaveBeenCalledOnce: (mock) => [calls(mock).length === 1, "expected the mock to be called once"],
  toHaveBeenCalledTimes: (mock, times) => [calls(mock).length === times, `expected ${times} calls, got ${calls(mock).length}`],
  toHaveBeenCalledWith: (mock, ...args) => [calls(mock).some((call) => equals(call, args, false)), `expected a call with ${show(args)}`],
  toHaveBeenNthCalledWith: (mock, nth, ...args) => [equals(calls(mock)[nth - 1], args, false), `expected call ${nth} with ${show(args)}`],
  toHaveBeenLastCalledWith: (mock, ...args) => [equals(calls(mock).at(-1), args, false), `expected the last call with ${show(args)}`],
};
matchers.toThrowError = matchers.toThrow;
matchers.toBeCalled = matchers.toHaveBeenCalled;
matchers.toBeCalledTimes = matchers.toHaveBeenCalledTimes;
matchers.toBeCalledWith = matchers.toHaveBeenCalledWith;

function assertion(actual, negated) {
  const handlers = {};
  for (const [name, matcher] of Object.entries(matchers)) {
    handlers[name] = (...args) => {
      const [pass, message] = matcher(actual, ...args);
      if (pass === negated) throw new Error(negated ? `not: ${message}` : message);
    };
  }
  return handlers;
}

function settled(promise, wantResolved, negated) {
  const handlers = {};
  for (const name of Object.keys(matchers)) {
    handlers[name] = async (...args) => {
      let value;
      let resolved;
      try {
        value = await promise;
        resolved = true;
      } catch (error) {
        value = error;
        resolved = false;
      }
      if (resolved !== wantResolved) {
        throw new Error(`expected the promise to ${wantResolved ? "resolve" : "reject"}`);
      }
      assertion(value, negated)[name](...args);
    };
  }
  return handlers;
}

const asymmetric = (negated) => ({
  any: (type) =>
    new Asymmetric((value) => {
      const pass =
        type === Number ? typeof value === "number" || value instanceof Number
        : type === String ? typeof value === "string" || value instanceof String
        : type === Boolean ? typeof value === "boolean" || value instanceof Boolean
        : type === Function ? typeof value === "function"
        : type === Object ? typeof value === "object" && value !== null
        : value instanceof type;
      return pass !== negated;
    }),
  anything: () => new Asymmetric((value) => (value != null) !== negated),
  arrayContaining: (items) =>
    new Asymmetric(
      (value) =>
        (Array.isArray(value) &&
          items.every((item) => value.some((own) => equals(own, item, false)))) !== negated,
    ),
  objectContaining: (shape) =>
    new Asymmetric(
      (value) =>
        (typeof value === "object" &&
          value !== null &&
          Object.keys(shape).every((key) => key in value && equals(value[key], shape[key], false))) !==
        negated,
    ),
  stringContaining: (part) =>
    new Asymmetric((value) => (typeof value === "string" && value.includes(part)) !== negated),
  stringMatching: (pattern) =>
    new Asymmetric(
      (value) =>
        (typeof value === "string" &&
          (typeof pattern === "string" ? value.includes(pattern) : pattern.test(value))) !==
        negated,
    ),
  closeTo: (expected, digits = 2) =>
    new Asymmetric(
      (value) =>
        (typeof value === "number" && Math.abs(value - expected) < 10 ** -digits / 2) !== negated,
    ),
});

export function expect(actual) {
  const positive = assertion(actual, false);
  positive.not = assertion(actual, true);
  positive.resolves = settled(actual, true, false);
  positive.resolves.not = settled(actual, true, true);
  positive.rejects = settled(actual, false, false);
  positive.rejects.not = settled(actual, false, true);
  return positive;
}

Object.assign(expect, asymmetric(false));
expect.not = asymmetric(true);

const spies = [];

export const vi = {
  spyOn(target, key) {
    const original = target[key];
    const spy = vi.fn(function (...args) {
      return original.apply(this, args);
    });
    spy.mockRestore = () => {
      target[key] = original;
    };
    target[key] = spy;
    spies.push(spy);
    return spy;
  },
  restoreAllMocks() {
    for (const spy of spies.splice(0)) spy.mockRestore();
  },
  fn(implementation = () => undefined) {
    const mock = function (...args) {
      mock.mock.calls.push(args);
      const result = implementation.apply(this, args);
      mock.mock.results.push({ type: "return", value: result });
      return result;
    };
    mock.mock = { calls: [], results: [] };
    return mock;
  },
};

// Run once the fixture module (and every module it imports) has registered its
// tests: a microtask queued at import time would run before the importing
// module's body, so wait for the next macrotask.
setTimeout(async () => {
  let passed = 0;
  let failed = 0;
  const lines = [];
  for (const { name, fn } of tests) {
    try {
      await fn();
      passed++;
      lines.push(`##smelt-oracle## ok ${name}`);
    } catch (error) {
      failed++;
      const message = error instanceof Error ? error.message : String(error);
      lines.push(`##smelt-oracle## fail ${name}: ${message.split("\n")[0]}`);
    }
  }
  console.log(lines.join("\n"));
  console.log(`##smelt-oracle## summary ${passed} ${failed}`);
}, 0);
