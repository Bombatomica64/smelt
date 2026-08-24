// Fixture: concrete_and_generic_callbacks_two_sinks
// Area: dispatch
// Guards: one generic and one concrete callback in one signature, sunk into different callees.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function plain(xs: number[], cb: (v: number) => boolean): number[] { return xs.filter(cb); }
export function outer<T>(xs: T[], ns: number[], cb: (v: T) => boolean, cb2: (v: number) => boolean): T[] {
  plain(ns, cb2);
  return xs.filter(cb);
}
export function use1(ts: string[], ns: number[]): string[] { return outer(ts, ns, (v: string) => v.length > 1, (v: number) => v > 1); }
