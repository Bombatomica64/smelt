// Fixture: callback_returns_type_param
// Area: adapter_substitution
// Guards: adapter return left unsubstituted while its parameters were substituted.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function mapAll<T>(xs: T[], cb: (v: T) => T): T[] {
  return xs.map(cb);
}
export function use1(ns: number[]): number[] {
  return mapAll(ns, (v: number) => v + 1);
}
