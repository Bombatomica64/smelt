// Fixture: variadic_type_param_callback
// Area: callback_shape
// Guards: the callback itself is variadic over `T`.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function rested<T>(cb: (...vals: T[]) => boolean): boolean {
  return cb();
}
export function call1(): boolean {
  return rested((...vals: number[]) => true);
}
