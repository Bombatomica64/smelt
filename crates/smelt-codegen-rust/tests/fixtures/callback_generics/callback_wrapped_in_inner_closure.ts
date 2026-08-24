// Fixture: callback_wrapped_in_inner_closure
// Area: callback_shape
// Guards: the borrowed callback is only ever called from inside an inner closure.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function outer<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter((v: T) => cb(v));
}
export function use1(ns: number[]): number[] { return outer(ns, (v: number) => v > 1); }
