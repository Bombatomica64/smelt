// Fixture: conflicting_nullary_maker_arms
// Area: site_pinning
// Guards: two nullary makers at one site returning different concrete types.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function twoCb<T>(a: () => T, b: () => T): T[] {
  return [a(), b()];
}
export function call1(): unknown[] {
  return twoCb(() => 1, () => "a") as unknown[];
}
