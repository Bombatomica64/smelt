// A guard proving the global `Object` type leaves an erased value erased, as
// tsc does: an array is an `Object` too, so `Array.isArray` still answers for
// it. A union still narrows. And in a condition, the right operand of `&&`
// is tested for truthiness: an absent optional `boolean` member is false.

const isObject = (value: unknown): value is Object => Boolean(value && typeof value === "object");

const matchPattern = (pattern: any, value: any): boolean => {
  if (isObject(pattern)) {
    if (!isObject(value)) return false;
    if (Array.isArray(pattern)) {
      if (!Array.isArray(value)) return false;
      return pattern.length === value.length
        ? pattern.every((subPattern, i) => matchPattern(subPattern, value[i]))
        : false;
    }
    return Reflect.ownKeys(pattern).every((k): boolean => k in value && matchPattern(pattern[k], value[k]));
  }
  return Object.is(value, pattern);
};

function shape(value: unknown): string {
  if (isObject(value)) {
    return Array.isArray(value) ? `array of ${value.length}` : "object";
  }
  return typeof value;
}

class Box {
  size = 3;
}

function boxSize(value: Box | null): number {
  return isObject(value) ? value.size : 0;
}

const flag = Symbol.for("@demo/flag");

interface Tagged {
  [flag]?: boolean;
  label?: boolean;
}

function kind(x: Tagged | number): string {
  if (typeof x !== "number" && x[flag]) return "flagged";
  if (typeof x !== "number" && x.label) return "labelled";
  return typeof x === "number" ? "number" : "plain";
}

console.log(matchPattern([1, 2], [1, 2]), matchPattern([1, 2], [1, 3]), matchPattern([1], [1, 2]));
console.log(matchPattern({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }], c: 0 }), matchPattern({ a: 1 }, { a: 2 }));
console.log(shape([1, 2, 3]), shape({ a: 1 }), shape("s"), shape(null));
console.log(boxSize(new Box()), boxSize(null));
const flagged: Tagged = { [flag]: true };
console.log(kind({}), kind(flagged), kind({ label: true }), kind(5));
