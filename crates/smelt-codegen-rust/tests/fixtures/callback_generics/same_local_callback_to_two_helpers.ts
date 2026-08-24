// Fixture: same_local_callback_to_two_helpers
// Area: site_pinning
// Guards: one local closure passed to two different generic helpers.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function h1<T>(xs: T[], cb: (v: T) => boolean): T[] { return xs.filter(cb); }
export function h2<T>(xs: T[], cb: (v: T) => boolean): number { return xs.filter(cb).length; }
export function use1(ns: number[]): number {
  const f = (v: number) => v > 1;
  return h1(ns, f).length + h2(ns, f);
}
