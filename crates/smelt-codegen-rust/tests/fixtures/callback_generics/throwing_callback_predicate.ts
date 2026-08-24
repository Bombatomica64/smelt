// Fixture: throwing_callback_predicate
// Area: dispatch
// Guards: a throwing callback makes the borrowed call fallible.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
export function use1(ns: number[]): number[] {
  return pick(ns, (v: number) => { if (v < 0) { throw new Error("neg"); } return v > 1; });
}
