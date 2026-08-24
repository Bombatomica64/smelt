// Fixture: constrained_callee_receives_object_callback
// Area: dispatch
// Guards: a `T extends { id: number }` callee receiving an object-typed callback.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function narrow<T extends { id: number }>(xs: T[], cb: (v: T) => boolean): T[] { return xs.filter(cb); }
export function outer<T>(xs: T[], ys: { id: number }[], cb: (v: { id: number }) => boolean): T[] {
  narrow(ys, cb);
  return xs;
}
export function use1(ts: string[], ys: { id: number }[]): string[] { return outer(ts, ys, (v) => v.id > 1); }
