// Fixture: mutually_recursive_callees_forward_callback
// Area: dispatch
// Guards: two mutually recursive generics forward one callback.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function even<T>(xs: T[], cb: (v: T) => boolean): number {
  if (xs.length === 0) { return 0; }
  return odd(xs.slice(1), cb);
}
export function odd<T>(xs: T[], cb: (v: T) => boolean): number {
  if (xs.length === 0) { return 1; }
  return even(xs.slice(1), cb) + (cb(xs[0]) ? 1 : 0);
}
export function use1(ns: number[]): number { return even(ns, (v: number) => v > 1); }
