// Fixture: constrained_outer_forwards_to_unconstrained
// Area: dispatch
// Guards: an `extends object` caller forwards into an unconstrained callee.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function inner<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
export function outer<T extends object>(xs: T[], cb: (v: T) => boolean): T[] {
  return inner(xs, cb);
}
export function use1(ns: number[]): number[] {
  return inner(ns, (v: number) => v > 1);
}
