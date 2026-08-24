// Fixture: throwing_callback_pins_return_type
// Area: adapter_substitution
// Guards: a throwing callback body makes the adapter return fallible while `U` is still pinned from it.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function unionBy<T, U>(arr1: T[], arr2: T[], mapper: (item: T) => U): T[] {
  return arr1.concat(arr2);
}

export function pinned(xs: number[], ys: number[]): number[] {
  return unionBy(xs, ys, (item: number) => {
    if (item < 0) { throw new Error("neg"); }
    return item.toString();
  });
}
