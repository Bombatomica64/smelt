// Fixture: callback_reassigned_before_use
// Area: callback_shape
// Guards: the borrowed callback is reassigned to a new closure before use.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T) => boolean): T[] {
  let f = cb;
  f = (v: T) => !cb(v);
  return xs.filter(f);
}
export function use1(ns: number[]): number[] {
  return pick(ns, (v: number) => v > 1);
}
