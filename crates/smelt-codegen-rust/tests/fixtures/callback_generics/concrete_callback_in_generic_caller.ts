// Fixture: concrete_callback_in_generic_caller
// Area: dispatch
// Guards: a fully concrete callback threaded through a generic caller.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function plain(xs: number[], cb: (v: number) => boolean): number[] { return xs.filter(cb); }
export function outer<T>(xs: T[], ns: number[], cb: (v: number) => boolean): T[] {
  plain(ns, cb);
  return xs;
}
export function use1(ts: string[], ns: number[]): string[] { return outer(ts, ns, (v: number) => v > 1); }
