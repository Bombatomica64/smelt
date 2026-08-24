// Fixture: named_local_and_inline_callback_sites
// Area: site_pinning
// Guards: the same callee reached once through a named local closure and once inline.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
export function useNamed(ns: number[]): number[] {
  const f = (v: number) => v > 1;
  return pick(ns, f);
}
export function useInline(ns: number[]): number[] {
  return pick(ns, (v: number) => v > 1);
}
