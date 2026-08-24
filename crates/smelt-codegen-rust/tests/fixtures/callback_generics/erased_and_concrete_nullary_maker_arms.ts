// Fixture: erased_and_concrete_nullary_maker_arms
// Area: site_pinning
// Guards: two nullary makers at one site, one erased and one concrete.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function twoCb<T>(a: () => T, b: () => T): T[] {
  return [a(), b()];
}
export function call1(spy: () => unknown): unknown[] {
  return twoCb(() => "a" as unknown, spy);
}
