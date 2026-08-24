// Fixture: async_callback_returns_type_param
// Area: adapter_substitution
// Guards: an `async` arrow supplies a `(v: T) => Promise<T>` callback.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function mapAsync<T>(xs: T[], cb: (v: T) => Promise<T>): T[] {
  return xs;
}
export function use1(ns: number[]): number[] {
  return mapAsync(ns, async (v: number) => v + 1);
}
