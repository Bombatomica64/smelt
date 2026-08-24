// Fixture: callback_returns_type_param_through_hop
// Area: adapter_substitution
// Guards: the T-returning callback crosses a generic hop before reaching the pinning site.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function inner<T>(xs: T[], cb: (v: T) => T): T[] {
  return xs.map(cb);
}
export function outer<T>(xs: T[], cb: (v: T) => T): T[] {
  return inner(xs, cb);
}
export function use1(ns: number[]): number[] {
  return outer(ns, (v: number) => v + 1);
}
