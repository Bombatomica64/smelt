// Fixture: two_call_sites_pin_differently
// Area: site_pinning
// Guards: two call sites pinning one callee differently; a call site using the declared rather than the substituted return type.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
export function useNum(ns: number[]): number[] {
  return pick(ns, (v: number) => v > 1);
}
export function useStr(ss: string[]): string[] {
  return pick(ss, (v: string) => v.length > 1);
}
