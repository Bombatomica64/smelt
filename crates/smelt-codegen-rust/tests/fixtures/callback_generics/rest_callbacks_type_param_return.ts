// Fixture: rest_callbacks_type_param_return
// Area: callback_shape
// Guards: a rest parameter of T-returning callbacks.
// Rescued from the callback-generics repro suite (PRs #202/#203).
export function restCb<T>(...cbs: ((v: number) => T)[]): T[] {
  return cbs.map((c) => c(1));
}
export function call1(): string[] {
  return restCb((v: number) => "a");
}
