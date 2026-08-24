// Fixture: two_call_sites_pin_number_and_string
// Area: site_pinning
// Guards: the passing counterpart of two_call_sites_pin_differently, with no string `.length` read.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
export function useNum(ns: number[]): number[] {
  return pick(ns, (v: number) => v > 1);
}
export function useStr(ss: string[]): string[] {
  return pick(ss, (v: string) => v !== "a");
}
