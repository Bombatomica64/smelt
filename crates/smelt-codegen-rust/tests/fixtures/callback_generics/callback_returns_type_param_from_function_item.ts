// Fixture: callback_returns_type_param_from_function_item
// Area: adapter_substitution
// Guards: a named function item, not a closure, supplies the T-returning callback.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function mapAll<T>(xs: T[], cb: (v: T) => T): T[] {
  return xs.map(cb);
}
function inc(v: number): number { return v + 1; }
export function use1(ns: number[]): number[] {
  return mapAll(ns, inc);
}
