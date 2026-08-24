// Fixture: mutable_list_returned_and_filtered
// Area: passthrough_ladder
// Guards: mutable composite plus callback where the mutated list is also the return value.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function tap<T>(xs: T[], cb: (v: T) => boolean): T[] {
  xs.push(xs[0]);
  return xs.filter(cb);
}
export function use1(): number[] {
  const arr = [3, 1, 2];
  return tap(arr, (v: number) => v > 1);
}
