// Fixture: rest_callback_arguments_two_sites
// Area: callback_shape
// Guards: two callbacks supplied to one rest parameter at the call site.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], ...cbs: ((v: T) => boolean)[]): T[] {
  return xs;
}
export function use1(ns: number[]): number[] {
  return pick(ns, (v: number) => v > 1, (v: number) => v < 9);
}
