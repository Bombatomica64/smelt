// Fixture: concrete_and_erased_sites_same_callee
// Area: site_pinning
// Guards: one concrete and one `unknown` site of the same generic callee.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
export function useConcrete(ns: number[]): number[] {
  return pick(ns, (v: number) => v > 1);
}
export function useErased(xs: unknown[]): unknown[] {
  return pick(xs, (v: unknown) => v !== null);
}
