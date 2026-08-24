// Fixture: mutable_list_and_callback_in_one_call
// Area: passthrough_ladder
// Guards: a parameter that is both a monomorphizing composite and in mutable_params: rendered by value against a `&mut` parameter.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function sortIn<T>(xs: T[], cmp: (a: T, b: T) => number): T[] {
  xs.sort(cmp);
  return xs;
}
export function use1(): number[] {
  const arr = [3, 1, 2];
  return sortIn(arr, (a: number, b: number) => a - b);
}
