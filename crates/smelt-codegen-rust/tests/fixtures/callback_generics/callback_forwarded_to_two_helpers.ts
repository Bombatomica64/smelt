// Fixture: callback_forwarded_to_two_helpers
// Area: site_pinning
// Guards: one borrowed callback forwarded into two generic helpers from one body.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function h1<T>(xs: T[], cb: (v: T) => boolean): T[] { return xs.filter(cb); }
export function h2<T>(xs: T[], cb: (v: T) => boolean): number { return xs.filter(cb).length; }
export function both<T>(xs: T[], cb: (v: T) => boolean): number {
  return h1(xs, cb).length + h2(xs, cb);
}
export function use1(ns: number[]): number { return both(ns, (v: number) => v > 1); }
