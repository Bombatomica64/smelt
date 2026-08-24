// Fixture: callback_is_only_type_param_source
// Area: callback_shape
// Guards: `T` appears only in the callback parameter, nowhere else in the signature.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function pick<T>(cb: (v: T) => boolean, n: number): number {
  return n;
}
export function use1(): number {
  return pick((v: number) => v > 1, 3);
}
