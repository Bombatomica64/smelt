// Fixture: erased_callback_cast_at_site
// Area: site_pinning
// Guards: the callback arrives as an `unknown` cast to a function type at the call site.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
export function use1(ns: number[], raw: unknown): number[] {
  return pick(ns, raw as (v: number) => boolean);
}
