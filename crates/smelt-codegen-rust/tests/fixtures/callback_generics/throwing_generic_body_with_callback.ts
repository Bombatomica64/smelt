// Fixture: throwing_generic_body_with_callback
// Area: dispatch
// Guards: the generic body itself throws while holding a borrowed callback.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function h1<T>(xs: T[], cb: (v: T) => T): T {
  if (xs.length === 0) { throw new Error("empty"); }
  return cb(xs[0]);
}
export function use1(ns: number[]): number { return h1(ns, (v: number) => v + 1); }
