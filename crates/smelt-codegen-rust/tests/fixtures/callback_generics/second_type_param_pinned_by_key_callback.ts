// Fixture: second_type_param_pinned_by_key_callback
// Area: adapter_substitution
// Guards: a second type parameter `K` reachable only through the callback's return.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function sortBy<T, K>(xs: T[], key: (item: T) => K): T[] {
  return xs;
}
export function a(): number[] { return sortBy([1, 2], (n: number) => n); }
export function b(): string[] { return sortBy(["a"], (s: string) => s.length); }
