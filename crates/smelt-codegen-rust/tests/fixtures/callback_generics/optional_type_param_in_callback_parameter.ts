// Fixture: optional_type_param_in_callback_parameter
// Area: callback_shape
// Guards: the callback parameter is `T | undefined`.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(xs: T[], cb: (v: T | undefined) => boolean): T[] {
  return xs.filter(cb);
}
export function use1(ns: number[]): number[] {
  return pick(ns, (v: number | undefined) => v !== undefined);
}
