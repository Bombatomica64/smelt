// Fixture: throwing_callback_type_param_return
// Area: dispatch
// Guards: a throwing T-returning callback: fallibility and substitution at once.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function mapAll<T>(xs: T[], cb: (v: T) => T): T[] {
  return xs.map(cb);
}
export function use1(ns: number[]): number[] {
  return mapAll(ns, (v: number) => { if (v < 0) { throw new Error("x"); } return v + 1; });
}
