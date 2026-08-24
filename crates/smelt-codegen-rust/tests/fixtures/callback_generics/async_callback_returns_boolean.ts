// Fixture: async_callback_returns_boolean
// Area: adapter_substitution
// Guards: an `async` arrow supplies a callback whose return is not generic.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T) => Promise<boolean>): T[] {
  return xs;
}
export function use1(ns: number[]): number[] {
  return pick(ns, async (v: number) => v > 1);
}
