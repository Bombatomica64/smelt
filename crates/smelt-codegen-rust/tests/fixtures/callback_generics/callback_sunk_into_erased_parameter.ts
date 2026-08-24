// Fixture: callback_sunk_into_erased_parameter
// Area: dispatch
// Guards: the borrowed callback is also passed to an `unknown` parameter.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function erased(cb: unknown): boolean { return true; }
export function outer<T>(xs: T[], cb: (v: T) => boolean): T[] {
  erased(cb);
  return xs.filter(cb);
}
export function use1(ns: number[]): number[] { return outer(ns, (v: number) => v > 1); }
