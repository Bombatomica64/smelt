// Fixture: recursive_forward_of_nullary_maker
// Area: dispatch
// Guards: a self-recursive generic forwards a nullary maker that pins `T`.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function rec<T>(n: number, make: () => T): T[] {
  if (n <= 0) { return []; }
  return rec(n - 1, make);
}
export function top(): string[] {
  return rec(3, () => "a");
}
