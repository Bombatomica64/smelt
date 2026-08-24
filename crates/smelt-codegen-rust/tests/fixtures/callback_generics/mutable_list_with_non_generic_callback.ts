// Fixture: mutable_list_with_non_generic_callback
// Area: passthrough_ladder
// Guards: the mutated list monomorphizes but the callback does not; only the list may claim the mutable slot.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function sortIn<T>(xs: T[], flag: (a: number) => boolean): T[] {
  if (flag(1)) { xs.reverse(); }
  return xs;
}
export function use1(): number[] {
  const arr = [3, 1, 2];
  return sortIn(arr, (a: number) => a > 0);
}
