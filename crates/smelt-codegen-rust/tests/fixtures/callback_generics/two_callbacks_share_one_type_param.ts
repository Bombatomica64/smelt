// Fixture: two_callbacks_share_one_type_param
// Area: callback_shape
// Guards: two borrowed callbacks over one `T`, both captured by an inner closure.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function both<T>(xs: T[], a: (v: T) => boolean, b: (v: T) => boolean): T[] {
  return xs.filter((v: T) => a(v) && b(v));
}
export function use1(ns: number[]): number[] {
  return both(ns, (v: number) => v > 1, (v: number) => v < 9);
}
