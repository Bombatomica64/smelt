// Fixture: callback_takes_list_of_type_param
// Area: callback_shape
// Guards: the callback parameter is `T[]`, a composite, not `T`.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (vs: T[]) => boolean): boolean {
  return cb(xs);
}
export function use1(ns: number[]): boolean {
  return pick(ns, (vs: number[]) => vs.length > 1);
}
