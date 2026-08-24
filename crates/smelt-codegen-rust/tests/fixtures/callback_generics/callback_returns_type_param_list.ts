// Fixture: callback_returns_type_param_list
// Area: adapter_substitution
// Guards: adapter return is a composite `T[]`; substitution must reach inside the composite.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function expand<T>(xs: T[], cb: (v: T) => T[]): T[][] {
  return xs.map(cb);
}
export function use1(ns: number[]): number[][] {
  return expand(ns, (v: number) => [v, v]);
}
