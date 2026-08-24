// Fixture: overloaded_generic_with_callback
// Area: dispatch
// Guards: overload signatures over a generic callback callee.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T) => boolean): T[];
export function pick<T>(xs: T[], cb: (v: T) => boolean, n: number): T[];
export function pick<T>(xs: T[], cb: (v: T) => boolean, n?: number): T[] {
  return xs.filter(cb);
}
export function use1(ns: number[]): number[] {
  return pick(ns, (v: number) => v > 1);
}
export function use2(ns: number[]): number[] {
  return pick(ns, (v: number) => v > 1, 2);
}
