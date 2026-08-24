// Fixture: optional_callback_parameter
// Area: callback_shape
// Guards: a `cb?:` optional callback, called both with and without it.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb?: (v: T) => boolean): T[] {
  if (cb === undefined) { return xs; }
  return xs.filter(cb);
}
export function useA(ns: number[]): number[] { return pick(ns); }
export function useB(ns: number[]): number[] { return pick(ns, (v: number) => v > 1); }
