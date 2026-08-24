// Fixture: callback_rebound_to_local_then_forwarded
// Area: callback_shape
// Guards: the borrowed callback is rebound to a local and then forwarded.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function h1<T>(xs: T[], cb: (v: T) => boolean): T[] { return xs.filter(cb); }
export function owner<T>(xs: T[], cb: (v: T) => boolean): T[] {
  let g = cb;
  g = cb;
  return h1(xs, g);
}
export function use1(ns: number[]): number[] { return owner(ns, (v: number) => v > 1); }
