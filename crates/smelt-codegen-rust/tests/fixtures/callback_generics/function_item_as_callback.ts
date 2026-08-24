// Fixture: function_item_as_callback
// Area: site_pinning
// Guards: a named function item, not a closure, as the callback argument.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
function isBig(v: number): boolean { return v > 1; }
export function use1(ns: number[]): number[] {
  return pick(ns, isBig);
}
