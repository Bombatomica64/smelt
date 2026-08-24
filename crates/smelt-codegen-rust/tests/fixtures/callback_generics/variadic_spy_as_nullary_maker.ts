// Fixture: variadic_spy_as_nullary_maker
// Area: site_pinning
// Guards: a variadic erased function supplied where `() => T` is declared.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function attempt<T>(make: () => T): T[] {
  return [make()];
}
export function runRest(spy: (...args: unknown[]) => unknown): unknown[] {
  return attempt(spy);
}
