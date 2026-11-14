// `args.length` of a rest tuple with optional elements counts the arguments
// given: an erased element, a typed optional element (`second?: number`,
// `x?: string`), and the required elements before them.

function f(...args: [a?: unknown, b?: unknown]): string {
  const p: unknown = args.length === 2 ? args[1] : typeof args[0] === "string" ? undefined : args[0];
  return `${args.length} ${p === undefined} ${String(args[0])} ${String(args[1])}`;
}
function g(...args: [first: string, second?: number]): number {
  return args.length;
}
const h = (...args: [x?: string]): number => args.length;
// Read inside a closure, as a matcher built by the function reads it later.
function later(...args: [key?: unknown, value?: unknown]): () => number {
  return () => args.length;
}
console.log(f());
console.log(f("x"));
console.log(f(5));
console.log(f("k", 9));
console.log(g("a"), g("a", 2), h(), h("z"));
console.log(later()(), later(1)(), later(1, 2)());
