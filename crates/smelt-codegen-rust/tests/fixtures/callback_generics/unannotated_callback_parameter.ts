// Fixture: unannotated_callback_parameter
// Area: site_pinning
// Guards: the call-site arrow leaves its parameter unannotated, so the shape comes from the callee.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
export function use1(ns: number[]): number[] {
  return pick(ns, v => v > 1);
}
