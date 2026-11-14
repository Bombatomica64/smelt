// `new Map([..])` and `new Set([..])` passed straight to an `unknown` or a
// generic parameter are built from their entries, and the `size` of a map or
// set read through a generic is its entry count.

function describe(value: unknown): string {
  if (value instanceof Map) return `map ${value.size} ${JSON.stringify([...value.entries()])}`;
  if (value instanceof Set) return `set ${value.size} ${JSON.stringify([...value])}`;
  return "other";
}

function sizeOf<T>(value: T): number {
  if (value instanceof Map) return value.size;
  if (value instanceof Set) return value.size;
  return -1;
}

console.log(describe(new Map([["a", 1], ["b", 2]])));
console.log(describe(new Set(["x", "y", "x"])));
console.log(sizeOf(new Map([["k", true]])), sizeOf(new Set([1, 2, 3])), sizeOf("str"));
const bound = new Map([["one", 1]]);
console.log(sizeOf(bound), describe(bound));
