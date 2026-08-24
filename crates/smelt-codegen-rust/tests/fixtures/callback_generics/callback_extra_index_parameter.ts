// Fixture: callback_extra_index_parameter
// Area: callback_shape
// Guards: the callback takes a second, non-generic parameter.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T, i: number) => boolean): T[] {
  return xs.filter(cb);
}
export function use1(ns: number[]): number[] {
  return pick(ns, (v: number, i: number) => v > i);
}
