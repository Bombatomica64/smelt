// Fixture: callback_forwarded_one_hop
// Area: dispatch
// Guards: the simplest forwarding hop: one generic calls another with the same callback.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function inner<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
export function outer<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return inner(xs, cb);
}
export function use1(ns: number[]): number[] {
  return outer(ns, (v: number) => v > 1);
}
