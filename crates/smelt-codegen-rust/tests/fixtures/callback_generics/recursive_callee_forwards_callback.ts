// Fixture: recursive_callee_forwards_callback
// Area: dispatch
// Guards: a self-recursive generic forwards its own borrowed callback.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function rec<T>(xs: T[], cb: (v: T) => boolean): number {
  if (xs.length === 0) { return 0; }
  const head = xs[0];
  const rest = xs.slice(1);
  return (cb(head) ? 1 : 0) + rec(rest, cb);
}
export function use1(ns: number[]): number { return rec(ns, (v: number) => v > 1); }
